//! Enemy sounds and effects. An enemy whose update another game runs makes no sound and no effect here, so they travel: hands at the
//! entry of the game's own functions catch the calls made in the game that runs it, and the other games make the same calls on their
//! copy, a moment later so they match the mirrored animation.
//!
//! - Sounds: `moby_cue(cue, arg, moby)` plays a sound cue of the moby's class on it, which is how enemies (and the shots they fire)
//!   make theirs. A cue on something this game runs goes out with the object's slot or id, and is played on the copy there.
//! - Explosions: `fx.explode`, whatever made it (an enemy's death, a lobbed bomb, a crate, a player's weapon), in play. Its arguments
//!   go out as they were, but for the object it was made for and the position, and the other games run it the same way. One that
//!   happened here anyway, at that place a moment before, is not shown twice.
//!
//! A hand only reads, and lets the call run, but for one thing: a sound cue or explosion this game makes itself just after making the same
//! for another game (an enemy finished here as it was there) is not made twice. Calls made while replaying are not caught again.

use super::MASK;
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use rac_mp::{FxEvent, NO_SLOT};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The NTSC number of `moby_cue` (omni/rac1_funcs.txt).
const MOBY_CUE: u32 = 0x28_D590;
/// An explosion shown here is not shown again for another game within this many metres and fields.
const SAME_BLAST_NEAR: f32 = 2.0;
const SAME_BLAST_FIELDS: u64 = 90;
/// The stack area a replayed explosion is given, this far below the game's stack pointer, and where its position goes in it. The game
/// can be stopped on the scratchpad's small stack (0x7000_0000): an explosion waits for a field on the main one (`MAIN_STACK`).
const MAIN_STACK: std::ops::Range<u32> = 0x10_0000 + SCRATCH_BELOW..0x200_0000;
const SCRATCH_BELOW: u32 = 0x2000;
const SCRATCH_POS: u32 = 0x200;

static CUE: OnceLock<&'static Reloc> = OnceLock::new();
static EXPLODE: OnceLock<&'static Reloc> = OnceLock::new();

fn cue_reloc() -> &'static Reloc {
	CUE.get().expect("cue hand without a spec")
}
fn explode_reloc() -> &'static Reloc {
	EXPLODE.get().expect("explode hand without a spec")
}

/// The local hero's moby and the ones he carries: their cues are his own sounds, for the others' voices of him.
static PLAYER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// What this game runs that others mirror, by moby address: its placed slot or spawned id.
static OURS: Mutex<BTreeMap<u32, u16>> = Mutex::new(BTreeMap::new());
/// Others are in this level, in play: catch calls. Placed objects lie in `PLACED` (start, end).
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PLACED: Mutex<(u32, u32)> = Mutex::new((0, 0));
/// Set while this game makes another game's call: a hand lets it be.
static REPLAYING: AtomicBool = AtomicBool::new(false);
/// What the hands caught this field, with the moby each was for.
static CAUGHT: Mutex<Vec<(u32, FxEvent)>> = Mutex::new(Vec::new());
/// Explosions this game made, and when (in fields): what another game sends near one is not shown again.
static BLASTS: Mutex<Vec<([f32; 3], u64)>> = Mutex::new(Vec::new());
/// Explosions and cues made here for other games, and when: this game's own of the same, just after, are not made.
static MADE_BLASTS: Mutex<Vec<([f32; 3], u64)>> = Mutex::new(Vec::new());
static MADE_CUES: Mutex<Vec<(u32, i32, u64)>> = Mutex::new(Vec::new());
static FIELD: AtomicU64 = AtomicU64::new(0);
/// Where `fx.explode` starts (it is found by its first call here, or a scan).
static EXPLODE_AT: AtomicU32 = AtomicU32::new(0);
/// Sounds and explosions caught, and made for other games, for the run log.
pub static SENT: AtomicU64 = AtomicU64::new(0);
pub static PLAYED: AtomicU64 = AtomicU64::new(0);

fn word(ram: &[u8], a: u32) -> u32 {
	let a = (a & MASK) as usize;
	u32::from_le_bytes([ram[a], ram[a + 1], ram[a + 2], ram[a + 3]])
}

fn put_word(ram: &mut [u8], a: u32, v: u32) {
	let a = (a & MASK) as usize;
	ram[a..a + 4].copy_from_slice(&v.to_le_bytes());
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The name another game knows the moby at `moby` by: what this game runs, or a placed object.
fn slot_of(moby: u32) -> u16 {
	if let Some(s) = lock(&OURS).get(&moby) {
		return *s;
	}
	let (start, end) = *lock(&PLACED);
	if (start..end).contains(&moby) && (moby - start) % 0x100 == 0 {
		((moby - start) / 0x100) as u16
	} else {
		NO_SLOT
	}
}

fn cue_hand(c: &mut Cpu) -> bool {
	if REPLAYING.load(Ordering::Relaxed) {
		return false;
	}
	let moby = c.g(6) as u32 & MASK;
	let (cue, now) = (c.g(4) as i32, FIELD.load(Ordering::Relaxed));
	if lock(&MADE_CUES).iter().any(|&(m, q, f)| m == moby && q == cue && now.saturating_sub(f) < SAME_BLAST_FIELDS) {
		c.s(2, 0);
		skip(c);
		return true;
	}
	if !ACTIVE.load(Ordering::Relaxed) {
		return false;
	}
	if let Some(&slot) = lock(&OURS).get(&moby) {
		lock(&CAUGHT).push((moby, FxEvent::Cue { slot, cue: c.g(4) as i32, arg: c.g(5) as u32 }));
	} else if lock(&PLAYER).contains(&moby) {
		let class = (word(&c.mem.ram, moby + 0xa4) >> 16) as u16;
		lock(&CAUGHT).push((moby, FxEvent::PlayerCue { class, cue: c.g(4) as i32, arg: c.g(5) as u32 }));
	}
	false
}

fn explode_hand(c: &mut Cpu) -> bool {
	EXPLODE_AT.store(c.pc, Ordering::Relaxed);
	let ram = &c.mem.ram;
	let at = c.g(6) as u32;
	let pos = [0, 4, 8, 12].map(|k| f32::from_bits(word(ram, at + k)));
	if REPLAYING.load(Ordering::Relaxed) {
		return false;
	}
	let now = FIELD.load(Ordering::Relaxed);
	let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
	if lock(&MADE_BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p)) {
		skip(c);
		return true;
	}
	let mut b = lock(&BLASTS);
	b.retain(|(_, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
	b.push(([pos[0], pos[1], pos[2]], now));
	drop(b);
	if !ACTIVE.load(Ordering::Relaxed) || pos.iter().any(|v| !v.is_finite()) {
		return false;
	}
	let sp = c.g(29) as u32;
	let ints = [5, 7, 8, 9, 10, 11].map(|r| c.g(r) as u32);
	let floats: [u32; 8] = std::array::from_fn(|k| c.f[12 + k]);
	let stack: [u32; 6] = std::array::from_fn(|k| word(ram, sp + k as u32 * 4));
	let moby = c.g(4) as u32 & MASK;
	lock(&CAUGHT).push((moby, FxEvent::Explode { slot: slot_of(moby), pos, ints, floats, stack }));
	false
}

/// Back to the caller without running the function.
fn skip(c: &mut Cpu) {
	let ra = c.g(31) as u32;
	c.pc = ra;
	c.npc = ra.wrapping_add(4);
	c.mem.now = c.icount;
}

/// Registers the hands. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	let mut n = 0;
	if let Some(r) = crate::omni::func_reloc("moby_cue") {
		let _ = CUE.set(r);
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: "moby_cue", reloc: cue_reloc, est: ee_oracle::hs::EXACT, f: cue_hand })));
		n += 1;
	}
	if let Some((_, r)) = crate::omni::hook_specs(|id| id == "rac1.omni.fx.explode").into_iter().next() {
		let _ = EXPLODE.set(r);
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: "rac1.omni.fx.explode", reloc: explode_reloc, est: ee_oracle::hs::EXACT, f: explode_hand })));
		n += 1;
	}
	eprintln!("mp: {n} hands for sounds and effects");
}

/// Each field, from `enemy_tick`: what this game runs that others mirror, whether anyone else is here, and where the placed objects are.
pub fn set_scene(ours: BTreeMap<u32, u16>, player: Vec<u32>, active: bool, placed: (u32, u32), field: u64) {
	*lock(&OURS) = ours;
	*lock(&PLAYER) = player;
	*lock(&PLACED) = placed;
	ACTIVE.store(active, Ordering::Relaxed);
	FIELD.store(field, Ordering::Relaxed);
	if !active {
		lock(&CAUGHT).clear();
	}
}

/// What the hands caught since the last call, with the moby each was for.
pub fn take() -> Vec<(u32, FxEvent)> {
	std::mem::take(&mut *lock(&CAUGHT))
}

/// Makes another game's sound or effect here. `local` names this game's moby for a slot or id. False: not now, the next field.
pub fn play(c: &mut Cpu, e: &FxEvent, local: impl Fn(u16) -> Option<u32>, voice: Option<u32>, hero: u32) -> bool {
	if matches!(e, FxEvent::Explode { .. }) && !MAIN_STACK.contains(&(c.g(29) as u32)) {
		return false;
	}
	REPLAYING.store(true, Ordering::Relaxed);
	let done = match *e {
		FxEvent::PlayerCue { cue, arg, .. } => voice.map(|moby| crate::omni::call_game(c, MOBY_CUE, |c| {
			c.s(4, cue as i64 as u64);
			c.s(5, arg as u64);
			c.s(6, moby as u64);
		})),
		FxEvent::Cue { slot, cue, arg } => local(slot).map(|moby| {
			let now = FIELD.load(Ordering::Relaxed);
			let mut made = lock(&MADE_CUES);
			made.retain(|(_, _, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
			made.push((moby, cue, now));
			drop(made);
			crate::omni::call_game(c, MOBY_CUE, |c| {
				c.s(4, cue as i64 as u64);
				c.s(5, arg as u64);
				c.s(6, moby as u64);
			})
		}),
		FxEvent::Explode { slot, pos, ints, floats, stack } => {
			let now = FIELD.load(Ordering::Relaxed);
			let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
			let seen = lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p));
			match explode_entry(c) {
				Some(entry) if !seen => {
					let mut made = lock(&MADE_BLASTS);
					made.retain(|(_, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
					made.push(([pos[0], pos[1], pos[2]], now));
					drop(made);
					let moby = if slot == NO_SLOT { hero } else { local(slot).unwrap_or(hero) };
					let scratch = (c.g(29) as u32).wrapping_sub(SCRATCH_BELOW) & !0xf;
					let ram = &mut c.mem.ram[..];
					stack.iter().enumerate().for_each(|(k, v)| put_word(ram, scratch + k as u32 * 4, *v));
					pos.iter().enumerate().for_each(|(k, v)| put_word(ram, scratch + SCRATCH_POS + k as u32 * 4, v.to_bits()));
					Some(crate::omni::run_game(c, entry, |c| {
						c.s(29, scratch as u64);
						c.s(4, moby as u64);
						c.s(6, (scratch + SCRATCH_POS) as u64);
						for (r, v) in [5, 7, 8, 9, 10, 11].into_iter().zip(ints) {
							c.s(r, v as i32 as i64 as u64);
						}
						c.f[12..20].copy_from_slice(&floats);
					}))
				}
				_ => None,
			}
		}
	};
	REPLAYING.store(false, Ordering::Relaxed);
	match done {
		Some(Ok(())) => {
			PLAYED.fetch_add(1, Ordering::Relaxed);
		}
		Some(Err(e)) => eprintln!("mp: could not make another game's effect: {e}"),
		None => {}
	}
	true
}

/// Where `fx.explode` starts in the code loaded now.
fn explode_entry(c: &Cpu) -> Option<u32> {
	let r = *EXPLODE.get()?;
	let at = EXPLODE_AT.load(Ordering::Relaxed);
	if at != 0 && crate::omni::entry_holds(c, r, at) {
		return Some(at);
	}
	let found = crate::omni::find_entry(c, r)?;
	EXPLODE_AT.store(found, Ordering::Relaxed);
	Some(found)
}
