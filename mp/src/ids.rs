//! What the games call a placed object. It has a slot in its game's moby table, but the slot is not the same in every game: a respawn
//! rebuilds the table without what is gone for good, and every slot after it moves down; neither is a count among objects of the
//! same class and UID (Kerwan has two crates of class 500 with UID 62, and after one is gone for good the other is the first). What
//! does not change is where the level places an object: a game builds the ids when its table is built or rebuilt, when every
//! object stands where the level put it, from the object's UID and that place (or its class and place for one with no UID). The game
//! side reads the table (rac-rs/src/mp/ids.rs).
//!
//! Ids are below `SPAWNED_ID` (spawned objects have their own): below 0x4000 a UID (11 bits) and 3 bits of its place, 0x4000 and up
//! 14 bits of the class and place of one with no UID. Two objects of a table with the same id have none, and are not shared.

/// UIDs are 0..0x7ff; anything else (-1, -16) is none.
const UIDS: i32 = 0x7ff;
const NO_UID: u16 = 0x4000;
/// Places are taken to this many metres.
const GRID: f32 = 1.0;

/// FNV-1a over the words.
fn hash(words: &[u32]) -> u32 {
	words.iter().flat_map(|w| w.to_le_bytes()).fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// The id of each object of a freshly built table from its class, UID and place.
pub fn placed_ids(objs: &[(u16, i16, [f32; 3])]) -> Vec<Option<u16>> {
	let ids: Vec<Option<u16>> = objs
		.iter()
		.map(|&(class, uid, pos)| {
			let place = pos.map(|v| if v.is_finite() { (v / GRID).round() as i32 as u32 } else { 0 });
			let id = if (0..UIDS).contains(&(uid as i32)) {
				uid as u16 | ((hash(&place) & 7) as u16) << 11
			} else {
				NO_UID | (hash(&[class as u32, place[0], place[1], place[2]]) & 0x3fff) as u16
			};
			Some(id)
		})
		.collect();
	let mut seen = std::collections::HashMap::<u16, usize>::new();
	ids.iter().flatten().for_each(|&i| *seen.entry(i).or_default() += 1);
	ids.into_iter().map(|i| i.filter(|i| seen[i] == 1)).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_object_keeps_its_id_when_the_table_is_rebuilt_without_another() {
		// Two crates gone for good before the cranks: the rebuilt table has the cranks two slots lower, with the same ids.
		let crane = |uid, x| (280, uid, [x, 90.6, 62.3]);
		let before = placed_ids(&[(501, 10, [1.0, 2.0, 3.0]), (501, 11, [4.0, 2.0, 3.0]), (13, -1, [9.0, 9.0, 9.0]), crane(52, 171.6), crane(53, 108.6)]);
		let after = placed_ids(&[(13, -1, [9.0, 9.0, 9.0]), crane(52, 171.6), crane(53, 108.6)]);
		assert_eq!(&before[2..], &after[..]);
		assert_eq!(before[3].map(|i| i & 0x7ff), Some(52));
		assert!(before[2].unwrap() >= NO_UID);
	}

	#[test]
	fn two_of_one_class_and_uid_are_told_apart_by_place_and_keep_their_ids_alone() {
		let (a, b) = ((500, 62, [261.0, 412.0, 80.0]), (500, 62, [266.0, 83.0, 58.0]));
		let both = placed_ids(&[a, b]);
		assert!(both.iter().all(Option::is_some));
		assert_ne!(both[0], both[1]);
		// The first gone for good: the second keeps its id.
		assert_eq!(placed_ids(&[b])[0], both[1]);
	}

	#[test]
	fn the_same_id_twice_is_none_and_ids_stay_below_spawned_ones() {
		let same = placed_ids(&[(9, 5, [1.0; 3]), (9, 5, [1.2; 3]), (13, -16, [7.0; 3])]);
		assert_eq!(&same[..2], &[None, None]);
		assert!(same[2].unwrap() < crate::SPAWNED_ID);
		assert!(placed_ids(&[(9, 0x7fe, [1.0e6, -1.0e6, 3.0])])[0].unwrap() < crate::SPAWNED_ID);
	}
}
