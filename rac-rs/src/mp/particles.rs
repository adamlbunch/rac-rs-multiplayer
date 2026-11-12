//! Particles of the enemies another game runs. Most of what an enemy throws off (dust, sparks of its blows, a robot's glints, a
//! creature's spit) is a particle of the game's effect pool: 0x800 slots of 0x40 bytes and a bitmap of those in use, filled by some
//! forty functions of the game and moved on by the pool itself. An enemy whose update another game runs makes none here.
//!
//! So the game that runs an enemy looks at the slots that came into use since the last field (`take`): one near an enemy it runs that
//! another player is near goes to the others as it is, its 16 words, with the word that holds the enemy's address (a particle that
//! follows it) named, and none with another address in it (a particle on something else, which the others have elsewhere). The others
//! put it in a free slot of their own pool (`put`), the enemy's address made their copy's, and their pool moves it on and ends it.
//! A particle much like one this game just made there itself (the same type, as near, as soon) is not put twice, nor one of an enemy
//! this game is finishing (`dying`: both games make those).
//!
//! The pool's own bookkeeping is kept: the bit of the slot is set, the highest slot in use (+8 of the cell) raised to it, and the next
//! free slot the game hands out (+4) is never taken. Slots this game put are not taken for its own the next field.

use super::*;
use rac_mp::{Particle, NO_WORD};

/// Slots and their size.
const SLOTS: u32 = 0x800;
const SLOT: u32 = 0x40;
/// A particle this near an enemy we run (metres) is the enemy's.
const NEAR: f32 = 4.0;
/// Sent a field, at most.
const PER_FIELD: usize = 8;
/// One of ours of the same type this near and this recent is the same particle.
const SAME_NEAR: f32 = 0.5;
const SAME_FIELDS: u64 = 6;

#[derive(Default)]
pub(super) struct Particles {
	/// The bitmap as it was after the last field, and the level it is of.
	was: Vec<u8>,
	level: i32,
	/// Slots we put last field (not ours to send).
	put: Vec<u32>,
	/// Particles this game made, recently: type, place, field.
	mine: Vec<(u8, [f32; 3], u64)>,
}

/// Sent and put, for the run log.
pub(super) static SENT: AtomicU64 = AtomicU64::new(0);
pub(super) static PUT: AtomicU64 = AtomicU64::new(0);
/// Not put: one of ours was the same, the enemy has no copy here, an address in it, no free slot.
pub(super) static NOT_PUT: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// A word that can be an address in RAM: the high bits of a kseg address, or none, and a place past the boot area.
fn pointer_like(v: u32) -> bool {
	v & 0x0e00_0000 == 0 && v & MASK >= 0x10_0000 && matches!(v >> 28, 0 | 2 | 3 | 8 | 0xa | 0xb)
}

fn bit(ram: &[u8], bm: u32, i: u32) -> bool {
	rd8(ram, bm + (i >> 3)) >> (i & 7) & 1 != 0
}

fn put_word(ram: &mut [u8], a: u32, v: u32) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

/// The pool of `level`: its base, the cell (next free +4, highest in use +8) and the bitmap. None while the level has none.
fn pool(ram: &[u8], level: i8) -> Option<(u32, u32, u32)> {
	let (cell, bm) = crate::retail::effect_pool_cells(u8::try_from(level).ok()? as u32)?;
	let base = rd(ram, cell) & MASK;
	(base != 0 && base as usize + (SLOTS * SLOT) as usize <= ram.len()).then_some((base, cell, bm))
}

impl Particles {
	/// The level changed (or we are not in play): nothing of the last one counts.
	pub(super) fn forget(&mut self) {
		*self = Self::default();
	}

	/// Each field, the game that runs enemies: the particles that came since the last field near those of `ours` (moby, id) that
	/// another player sees (`seen`), at most `PER_FIELD`; and which particles this game made (for `put`). `dying`: enemies finishing.
	pub(super) fn take(&mut self, ram: &[u8], level: i8, field: u64, ours: &BTreeMap<u32, u16>, seen: impl Fn([f32; 3]) -> bool, dying: &BTreeMap<u32, u64>) -> Vec<Particle> {
		let Some((base, _, bm)) = pool(ram, level) else {
			self.forget();
			return Vec::new();
		};
		let now: Vec<u8> = (0..SLOTS / 8).map(|k| rd8(ram, bm + k)).collect();
		if self.level != level as i32 || self.was.len() != now.len() {
			self.level = level as i32;
			self.was = now;
			self.mine.clear();
			return Vec::new();
		}
		self.mine.retain(|m| field.saturating_sub(m.2) <= SAME_FIELDS);
		let foes: Vec<(u32, u16, [f32; 3])> = ours
			.iter()
			.filter(|(m, _)| !dying.contains_key(*m))
			.map(|(&m, &id)| (m, id, [rd_f(ram, m + 0x10), rd_f(ram, m + 0x14), rd_f(ram, m + 0x18)]))
			.filter(|f| seen(f.2))
			.collect();
		let mut out = Vec::new();
		for i in 0..SLOTS {
			let (k, b) = ((i >> 3) as usize, 1u8 << (i & 7));
			if now[k] & b == 0 || self.was[k] & b != 0 || self.put.contains(&i) {
				continue;
			}
			let s = base + SLOT * i;
			let words: [u32; 16] = std::array::from_fn(|w| rd(ram, s + 4 * w as u32));
			let pos = [f32::from_bits(words[4]), f32::from_bits(words[5]), f32::from_bits(words[6])];
			if pos.iter().any(|v| !v.is_finite()) || rd8(ram, s + 1) & 0x80 != 0 {
				continue;
			}
			self.mine.push((words[0] as u8, pos, field));
			if out.len() >= PER_FIELD {
				continue;
			}
			// The enemy's: the one whose address it holds, else the nearest. Another address: not ours to send.
			let mut owner = None;
			let mut other = false;
			for (w, &v) in words.iter().enumerate() {
				if pointer_like(v) {
					match foes.iter().find(|f| f.0 == v & MASK) {
						Some(f) if owner.is_none() => owner = Some((w as u8, f.1)),
						_ => other = true,
					}
				}
			}
			if other {
				continue;
			}
			let slot = match owner {
				Some((_, id)) => Some(id),
				None => foes.iter().filter(|f| dist2(f.2, pos) < NEAR * NEAR).min_by(|a, b| dist2(a.2, pos).total_cmp(&dist2(b.2, pos))).map(|f| f.1),
			};
			if let Some(slot) = slot {
				out.push(Particle { slot, owner: owner.map_or(NO_WORD, |o| o.0), words });
			}
		}
		self.was = now;
		self.put.clear();
		SENT.fetch_add(out.len() as u64, Ordering::Relaxed);
		out
	}

	/// Between two fields: another game's particles put in free slots of this game's pool, `local` naming this game's copy of an
	/// enemy by its id. How many were put.
	pub(super) fn put(&mut self, ram: &mut [u8], level: i8, field: u64, ps: &[Particle], local: impl Fn(u16) -> Option<u32>) -> usize {
		let Some((base, cell, bm)) = pool(ram, level) else { return 0 };
		let next_free = rd(ram, cell + 4);
		let mut high = rd(ram, cell + 8);
		let mut made = 0;
		let mut from = 1;
		for p in ps {
			let pos = [f32::from_bits(p.words[4]), f32::from_bits(p.words[5]), f32::from_bits(p.words[6])];
			let kind = p.words[0] as u8;
			if self.mine.iter().any(|m| m.0 == kind && field.saturating_sub(m.2) <= SAME_FIELDS && dist2(m.1, pos) < SAME_NEAR * SAME_NEAR) {
				NOT_PUT[0].fetch_add(1, Ordering::Relaxed);
				continue;
			}
			let mut words = p.words;
			if p.owner != NO_WORD {
				let Some(moby) = local(p.slot) else {
					NOT_PUT[1].fetch_add(1, Ordering::Relaxed);
					continue;
				};
				words[p.owner as usize] = moby;
			}
			if words.iter().enumerate().any(|(w, &v)| w != p.owner as usize && pointer_like(v)) {
				NOT_PUT[2].fetch_add(1, Ordering::Relaxed);
				continue;
			}
			let Some(i) = (from..SLOTS).find(|&i| i != next_free && !bit(ram, bm, i)) else {
				NOT_PUT[3].fetch_add(1, Ordering::Relaxed);
				break;
			};
			from = i + 1;
			let s = base + SLOT * i;
			for (w, &v) in words.iter().enumerate() {
				put_word(ram, s + 4 * w as u32, v);
			}
			let at = (bm + (i >> 3)) as usize & MASK as usize;
			ram[at] |= 1 << (i & 7);
			if (high as i32) < i as i32 {
				high = i;
			}
			self.put.push(i);
			made += 1;
		}
		put_word(ram, cell + 8, high);
		PUT.fetch_add(made as u64, Ordering::Relaxed);
		// What we put is no news next field.
		for &i in &self.put {
			if let Some(b) = self.was.get_mut((i >> 3) as usize) {
				*b |= 1 << (i & 7);
			}
		}
		made
	}
}
