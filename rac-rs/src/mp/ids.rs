//! What the games call a placed object: the id rac-mp gives it (mp/src/ids.rs) from its class, its UID (the halfword at +0xb2, the
//! number the game's own "gone for good" bitmap uses) and where the level places it. The ids are built when the table is: at a level
//! start, and again when a respawn rebuilds it without what is gone for good (the slots after that move down: Novalis' bolt cranks
//! went from 296 and 297 to 294 and 295), which is when every object stands where the level put it. A table is taken for rebuilt when
//! its size or the class and UID in any slot changed; otherwise the ids stay as they were, wherever the objects went since.

use super::{rd, rd_f, MASK};
use std::collections::HashMap;
use std::sync::Arc;

/// The ids of the level's placed objects, both ways, and the table they were built for.
#[derive(Default)]
pub(super) struct Ids {
	by_id: HashMap<u16, u32>,
	by_addr: Arc<HashMap<u32, u16>>,
	start: u32,
	built: Vec<(u16, i16)>,
}

impl Ids {
	/// Each field: the placed objects from `start`, `slots` of them (slot 0, the hero, has none). Built again only for a new table.
	pub(super) fn update(&mut self, ram: &[u8], start: u32, slots: u32) {
		let addrs: Vec<u32> = (1..slots).map(|s| (start + s * 0x100) & MASK).collect();
		let now: Vec<(u16, i16)> = addrs.iter().map(|&a| ((rd(ram, a + 0xa4) >> 16) as u16, (rd(ram, a + 0xb0) >> 16) as i16)).collect();
		if start == self.start && now == self.built {
			return;
		}
		let objs: Vec<(u16, i16, [f32; 3])> = now.iter().zip(&addrs).map(|(&(class, uid), &a)| (class, uid, [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)])).collect();
		let pairs: Vec<(u16, u32)> = rac_mp::ids::placed_ids(&objs).into_iter().zip(addrs).filter_map(|(id, a)| Some((id?, a))).collect();
		*self = Ids { by_id: pairs.iter().copied().collect(), by_addr: Arc::new(pairs.iter().map(|&(id, a)| (a, id)).collect()), start, built: now };
	}

	/// The address of the object with id `id` in this game.
	pub(super) fn addr(&self, id: u16) -> Option<u32> {
		self.by_id.get(&id).copied()
	}

	/// The id of the placed object at `a`.
	pub(super) fn id(&self, a: u32) -> Option<u16> {
		self.by_addr.get(&(a & MASK)).copied()
	}

	/// Address to id, for the hands on the game thread.
	pub(super) fn by_addr(&self) -> Arc<HashMap<u32, u16>> {
		self.by_addr.clone()
	}
}
