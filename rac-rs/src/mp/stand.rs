//! Enemies fight everyone: each enemy goes for the nearest player. The game knows one hero, and every enemy class reads his
//! position for its own AI, so nothing about that AI is rewritten: around an enemy's update, a player nearer to it than the local
//! hero stands in for him. The hero's position words are swapped for that player's for exactly that one update, the enemy
//! sees, chases and attacks there, and the words go back before anything else runs.
//!
//! The hands sit at the entry of every class's update function, found by code like the mods' hook points (the specs of
//! `rac1.moby.*.update` in omni/rac1_reloc.txt). They exist only with RAC_MP, and do nothing but one atomic load while no other
//! player is in the level. A hook point a mod has taken stays the mod's (`omni::hook_specs`): those enemies keep to the local hero.
//!
//! An attack made during a stand-in lands on the other player's picture, not on anyone: each game runs its own enemies, so the
//! other player is hurt by the copies in their own game. Two more hands keep it that way: while a stand-in runs, a hit posted
//! on the local hero is dropped (`hit_hand`) and so is health taken from him (`damage_hand`).
//!
//! Threads: the hands run on the game thread "ee" during a field; `set_targets` on the worker between fields. Never concurrent.

use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The hero's position words (binding `hero.position`).
const HERO_POS: u32 = 0x13_F3D0;
/// The cell that holds the hero's moby (retail/snapshot.rs reads the same), and that moby's position.
const HERO_MOBY: u32 = 0x14_13D0;
const MOBY_POS: u32 = 0x10;
const MASK: u32 = 0x01ff_ffff;
/// The class type of an enemy (class record +0x46).
const ENEMY: i16 = 5;
/// Bytes of a position: x, y, z.
const POS: usize = 12;

const UPDATE_PREFIX: &str = "rac1.moby.";
const UPDATE_SUFFIX: &str = ".update";
const TAKE_DAMAGE: &str = "rac1.player.take_damage";
const HIT_VICTIM: &str = "rac1.omni.damage.hit_victim";

/// Where the other players stand in the local hero's level, in play and alive.
static TARGETS: Mutex<Vec<[f32; 3]>> = Mutex::new(Vec::new());
/// `TARGETS` holds someone: without, the hands are one atomic load.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// A stand-in is running on the game thread.
static STANDING: AtomicBool = AtomicBool::new(false);
/// Updates that ran with a stand-in, and hits and damage dropped, for the run log.
pub static STOOD: AtomicU64 = AtomicU64::new(0);
pub static DROPPED: AtomicU64 = AtomicU64::new(0);

static SPECS: OnceLock<Vec<&'static Reloc>> = OnceLock::new();

fn reloc_n<const N: usize>() -> &'static Reloc {
	SPECS.get().expect("a stand-in hand registered without a spec")[N]
}

macro_rules! relocs {
	($($n:literal)*) => {
		const MAX_HANDS: usize = [$($n),*].len();
		static RELOCS: [fn() -> &'static Reloc; MAX_HANDS] = [$(reloc_n::<$n>),*];
	};
}
relocs!(
	0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
	32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63
	64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95
	96 97 98 99 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115 116 117 118 119 120 121 122 123 124 125 126 127
	128 129 130 131 132 133 134 135 136 137 138 139 140 141 142 143 144 145 146 147 148 149 150 151 152 153 154 155 156 157 158 159
	160 161 162 163 164 165 166 167 168 169 170 171 172 173 174 175 176 177 178 179 180 181 182 183 184 185 186 187 188 189 190 191
);

/// Registers the hands. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	let specs = crate::omni::hook_specs(|id| (id.starts_with(UPDATE_PREFIX) && id.ends_with(UPDATE_SUFFIX)) || id == TAKE_DAMAGE || id == HIT_VICTIM);
	let specs = &specs[..specs.len().min(MAX_HANDS)];
	let _ = SPECS.set(specs.iter().map(|(_, r)| *r).collect());
	for (i, (id, _)) in specs.iter().enumerate() {
		let f: fn(&mut Cpu) -> bool = match id.as_str() {
			TAKE_DAMAGE => damage_hand,
			HIT_VICTIM => hit_hand,
			_ => update_hand,
		};
		let hand: &'static RelocHand = Box::leak(Box::new(RelocHand { name: id.clone().leak(), reloc: RELOCS[i], est: ee_oracle::hs::EXACT, f }));
		ee_oracle::rec::register_reloc_hand(hand);
	}
	crate::omni::allow_hook_hands();
	eprintln!("mp: {} hook points for enemies that fight everyone", specs.len());
}

/// The worker, each field: where the other players are.
pub fn set_targets(t: Vec<[f32; 3]>) {
	ACTIVE.store(!t.is_empty(), Ordering::Relaxed);
	*TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = t;
}

fn word(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn pos(ram: &[u8], a: u32) -> [f32; 3] {
	[0, 4, 8].map(|k| f32::from_bits(word(ram, a + k)))
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
	(0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// The target nearer to `at` than `hero`, the nearest of them; None when the hero is the nearest.
fn nearest(targets: &[[f32; 3]], at: [f32; 3], hero: [f32; 3]) -> Option<[f32; 3]> {
	let mut best = (dist2(at, hero), None);
	for t in targets {
		let d = dist2(at, *t);
		if d < best.0 {
			best = (d, Some(*t));
		}
	}
	best.1
}

/// The player to stand in for the hero at the update of the moby at `moby`: only an enemy, only one nearer to someone else.
fn stand_in(ram: &[u8], moby: u32) -> Option<[f32; 3]> {
	let class = word(ram, moby + 0x24) & MASK;
	if class == 0 || (word(ram, class + 0x44) >> 16) as i16 != ENEMY {
		return None;
	}
	let t = TARGETS.lock().unwrap_or_else(|e| e.into_inner());
	nearest(&t, pos(ram, moby + MOBY_POS), pos(ram, HERO_POS))
}

fn take(ram: &[u8], a: u32) -> [u8; POS] {
	let i = (a & MASK) as usize;
	ram.get(i..i + POS).and_then(|b| b.try_into().ok()).unwrap_or([0; POS])
}

fn put(ram: &mut [u8], a: u32, v: [u8; POS]) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + POS) {
		b.copy_from_slice(&v);
	}
}

fn back_to_caller(c: &mut Cpu, ra: u32) {
	c.pc = ra;
	c.npc = ra.wrapping_add(4);
	c.mem.now = c.icount;
}

/// At an enemy's update: runs it with the nearest other player in the hero's place. False: the game runs it as it is.
fn update_hand(c: &mut Cpu) -> bool {
	// The entry again from inside `run_retail` below: that is the update itself.
	if !ACTIVE.load(Ordering::Relaxed) || STANDING.load(Ordering::Relaxed) {
		return false;
	}
	let moby = c.g(4) as u32 & MASK;
	let Some(target) = stand_in(&c.mem.ram, moby) else { return false };
	let hero_moby = word(&c.mem.ram, HERO_MOBY) & MASK;
	if hero_moby == 0 {
		return false;
	}
	let at = hero_moby + MOBY_POS;
	let saved = (take(&c.mem.ram, HERO_POS), take(&c.mem.ram, at));
	let there: [u8; POS] = std::array::from_fn(|i| target[i / 4].to_le_bytes()[i % 4]);
	put(&mut c.mem.ram, HERO_POS, there);
	put(&mut c.mem.ram, at, there);
	STANDING.store(true, Ordering::Relaxed);
	STOOD.fetch_add(1, Ordering::Relaxed);
	let (entry, ra) = (c.pc, c.g(31) as u32);
	let ran = catch_unwind(AssertUnwindSafe(|| crate::omni::run_retail(c, entry, ra)));
	STANDING.store(false, Ordering::Relaxed);
	put(&mut c.mem.ram, HERO_POS, saved.0);
	put(&mut c.mem.ram, at, saved.1);
	match ran {
		// The oracle's rebuild or quit, on its way to the game thread's loop.
		Err(p) => resume_unwind(p),
		Ok(true) => back_to_caller(c, ra),
		// It left at a frame wait: the machine stays there.
		Ok(false) => {}
	}
	true
}

/// `hit_ring_write(victim, descriptor)` during a stand-in, on the local hero: the blow was aimed at another player.
fn hit_hand(c: &mut Cpu) -> bool {
	if !STANDING.load(Ordering::Relaxed) || c.g(4) as u32 & MASK != word(&c.mem.ram, HERO_MOBY) & MASK {
		return false;
	}
	DROPPED.fetch_add(1, Ordering::Relaxed);
	let ra = c.g(31) as u32;
	back_to_caller(c, ra);
	true
}

/// `Hero_TakeDamage` during a stand-in, the same.
fn damage_hand(c: &mut Cpu) -> bool {
	if !STANDING.load(Ordering::Relaxed) {
		return false;
	}
	DROPPED.fetch_add(1, Ordering::Relaxed);
	let ra = c.g(31) as u32;
	back_to_caller(c, ra);
	true
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_nearest_of_the_others_stands_in_only_when_nearer_than_the_hero() {
		let (hero, a, b) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]);
		assert_eq!(nearest(&[a, b], [18.0, 0.0, 0.0], hero), Some(b));
		assert_eq!(nearest(&[a, b], [9.0, 0.0, 0.0], hero), Some(a));
		assert_eq!(nearest(&[a, b], [1.0, 0.0, 0.0], hero), None);
		assert_eq!(nearest(&[], [1.0, 0.0, 0.0], hero), None);
	}
}
