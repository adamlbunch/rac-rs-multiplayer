//! [AP] The game's givers decide what they offer from bytes the multiworld writes too, so an item received before its place was
//! visited could take that place away:
//! - Each item's "had it" mark (`HAD`, beside the owned bytes; retail `item.give` sets it) is read by givers when their level
//!   loads: Eudora's Suck Cannon (class 1120), Orxon's Magneboots (18), Pokitaru's trader (298, the Persuader), Quartu's Bolt
//!   Grabber (1388), Rilgar's R.Y.N.O. seller (925) and more leave or stay silent once it is set. For every item that is a place,
//!   the mark says whether its place was found.
//! - Some read the item's own byte: Orxon's nanotech seller (1326) sells the Premium while its byte is clear, then the Ultra;
//!   Pokitaru's trader plays its trade while the Raritanium's or the Persuader's byte is set; Hoven's Raritanium (282) and the
//!   Fleet's Codebot (1428) leave when the level loads if theirs is set. On those levels the bytes say whether the places were
//!   found (none of them is needed to get around there); elsewhere, what was received.

use super::{byte, put_byte, received_equip, word, State, ITEM_SPECIAL, LOC_ITEM, LOC_SPECIAL, LEVEL_WORD, OWNED, SPECIAL};

/// Each item's "had it" mark.
const HAD: u32 = 0x13_D4E8;
/// The items that are places of their own (data/Locations.py `item_loc`): their mark says whether the place was found.
const ITEM_PLACES: [u32; 21] = [2, 3, 4, 5, 6, 7, 9, 12, 21, 22, 23, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35];

const fn special(level: u32, k: u32) -> (u32, u32) {
	(SPECIAL + k, LOC_SPECIAL + level * 16 + k)
}

/// (level, the bytes its givers read with the places they stand for).
const VIEWS: [(i32, &[(u32, u32)]); 4] = [
	(10, &[special(10, 4), special(10, 5)]),
	(11, &[(OWNED + 35, LOC_ITEM + 35)]),
	(12, &[special(12, 1)]),
	(17, &[special(17, 2)]),
];

/// What the byte says away from its level: received (and, for a special item, not traded away).
fn received(s: &State, addr: u32) -> bool {
	if addr >= OWNED {
		received_equip(s, addr - OWNED)
	} else {
		let k = addr - SPECIAL;
		s.received.contains(&(ITEM_SPECIAL + k)) && !s.consumed.contains(&k)
	}
}

/// Whether the byte at `addr` shows its place now (the watch counts its change as a find even when it was received, and the
/// grant leaves it alone).
pub(super) fn viewed(s: &State, addr: u32) -> bool {
	s.view.is_some_and(|i| VIEWS[i].1.iter().any(|&(a, _)| a == addr))
}

/// Every field once the seed is known, in any game mode (givers read the bytes when their level loads, before play): the marks
/// say whether the places were found; on a level with givers that read an item's byte, its byte says so too, and once the level
/// is left it says what was received again.
pub(super) fn keep(ram: &mut [u8], s: &mut State) {
	for &id in &ITEM_PLACES {
		let found = s.found.contains(&(LOC_ITEM + id));
		if (byte(ram, HAD + id) != 0) != found {
			put_byte(ram, HAD + id, found as u8);
		}
	}
	let level = word(ram, LEVEL_WORD) as i32;
	let now = VIEWS.iter().position(|v| v.0 == level);
	if s.view != now {
		if let Some(i) = s.view.take() {
			for &(addr, _) in VIEWS[i].1 {
				let v = received(s, addr);
				set(ram, s, addr, v);
			}
		}
		if now.is_some() {
			eprintln!("ap: level {level}: its givers' bytes show their places");
		}
		s.view = now;
	}
	if let Some(i) = now {
		for &(addr, loc) in VIEWS[i].1 {
			let v = s.found.contains(&loc);
			set(ram, s, addr, v);
		}
	}
}

/// The byte set, and seen as it is now (the next field's watch must not take it for a find).
fn set(ram: &mut [u8], s: &mut State, addr: u32, v: bool) {
	if (byte(ram, addr) != 0) == v {
		return;
	}
	put_byte(ram, addr, v as u8);
	let seen = if addr >= OWNED { s.seen_owned.as_mut().map(|x| (x, addr - OWNED)) } else { s.seen_special.as_mut().map(|x| (x, addr - SPECIAL)) };
	if let Some((x, i)) = seen {
		if let Some(b) = x.get_mut(i as usize) {
			*b = v as u8;
		}
	}
}
