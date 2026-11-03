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

/// The NTSC numbers of `moby_cue`, `spark_spawn` and `glow_flare_spawn` (omni/rac1_funcs.txt).
const MOBY_CUE: u32 = 0x28_D590;
const SPARK_SPAWN: u32 = 0x2A_EDF8;
const GLOW_SPAWN: u32 = 0x2D_F1B0;
/// An explosion shown here is not shown again for another game within this many metres and fields.
const SAME_BLAST_NEAR: f32 = 2.0;
const SAME_BLAST_FIELDS: u64 = 90;
/// The stack area a replayed explosion is given, just below the game's stack pointer (where the game's own calls go each field: further
/// down is another thread's, and writing there broke the game), and where its position goes in it. The game can be stopped on the
/// scratchpad's small stack (0x7000_0000): an explosion waits for a field on the main one (`MAIN_STACK`).
const MAIN_STACK: std::ops::Range<u32> = 0x10_0000 + SCRATCH_BELOW..0x200_0000;
const SCRATCH_BELOW: u32 = 0x300;
const SCRATCH_POS: u32 = 0x200;

static CUE: OnceLock<&'static Reloc> = OnceLock::new();
static SPARK: OnceLock<&'static Reloc> = OnceLock::new();
static GLOW: OnceLock<&'static Reloc> = OnceLock::new();
static EXPLODE: OnceLock<&'static Reloc> = OnceLock::new();

fn cue_reloc() -> &'static Reloc {
	CUE.get().expect("cue hand without a spec")
}
fn spark_reloc() -> &'static Reloc {
	SPARK.get().expect("spark hand without a spec")
}
fn glow_reloc() -> &'static Reloc {
	GLOW.get().expect("glow hand without a spec")
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
/// The same by kind (`kind`): enemy cues, explosions, player cues, sparks, glows.
pub static SENT_KINDS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
pub static PLAYED_KINDS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

/// The index of `e`'s kind in `SENT_KINDS` and `PLAYED_KINDS`.
pub fn kind(e: &FxEvent) -> usize {
	match e {
		FxEvent::Cue { .. } => 0,
		FxEvent::Explode { .. } => 1,
		FxEvent::PlayerCue { .. } => 2,
		FxEvent::Spark { .. } => 3,
		FxEvent::Glow { .. } => 4,
	}
}

/// Counts by kind, for the run log.
pub fn kinds(k: &[AtomicU64; 5]) -> [u64; 5] {
	std::array::from_fn(|i| k[i].load(Ordering::Relaxed))
}

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
/// The class of the moby at `moby`, `NO_SLOT` for none.
fn class_of(ram: &[u8], moby: u32) -> u16 {
	if moby == 0 { NO_SLOT } else { (word(ram, moby + 0xa4) >> 16) as u16 }
}

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
	lock(&CAUGHT).push((moby, FxEvent::Explode { slot: slot_of(moby), owner: class_of(ram, moby), pos, ints, floats, stack }));
	false
}

fn vec4(ram: &[u8], a: u32) -> [f32; 4] {
	[0, 4, 8, 12].map(|k| f32::from_bits(word(ram, a + k)))
}

/// Sparks and glow flares are made like explosions: in play, sent with their vectors, and not made twice at one place (`seen_near`).
fn spark_hand(c: &mut Cpu) -> bool {
	if REPLAYING.load(Ordering::Relaxed) {
		return false;
	}
	let at = vec4(&c.mem.ram, c.g(4) as u32);
	lock(&BLASTS).push(([at[0], at[1], at[2]], FIELD.load(Ordering::Relaxed)));
	if !ACTIVE.load(Ordering::Relaxed) {
		return false;
	}
	let ram = &c.mem.ram;
	let (pos, vel) = (vec4(ram, c.g(4) as u32), vec4(ram, c.g(5) as u32));
	if pos.iter().chain(&vel).all(|v| v.is_finite()) {
		lock(&CAUGHT).push((0, FxEvent::Spark { pos, vel, a2: c.g(6) as u32, a3: c.g(7), t0: c.g(8) as u8 }));
	}
	false
}

fn glow_hand(c: &mut Cpu) -> bool {
	if REPLAYING.load(Ordering::Relaxed) {
		return false;
	}
	let at = vec4(&c.mem.ram, c.g(5) as u32);
	lock(&BLASTS).push(([at[0], at[1], at[2]], FIELD.load(Ordering::Relaxed)));
	if !ACTIVE.load(Ordering::Relaxed) {
		return false;
	}
	let ram = &c.mem.ram;
	let (size, owner) = (f32::from_bits(c.f[12]), c.g(4) as u32 & MASK);
	let (pos, base) = (vec4(ram, c.g(5) as u32), vec4(ram, c.g(6) as u32));
	if size.is_finite() && pos.iter().chain(&base).all(|v| v.is_finite()) {
		let rgba = [8, 9, 10, 11].map(|r| c.g(r) as u32);
		lock(&CAUGHT).push((owner, FxEvent::Glow { slot: slot_of(owner), owner: class_of(ram, owner), size, pos, base, life: c.g(7) as u32, rgba }));
	}
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
	for (name, slot, reloc, f) in [
		("spark_spawn", &SPARK, spark_reloc as fn() -> &'static Reloc, spark_hand as fn(&mut Cpu) -> bool),
		("glow_flare_spawn", &GLOW, glow_reloc, glow_hand),
	] {
		if let Some(r) = crate::omni::func_reloc(name) {
			let _ = slot.set(r);
			ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name, reloc, est: ee_oracle::hs::EXACT, f })));
			n += 1;
		}
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

/// The object an explosion or glow is made for here: this game's copy of the one it was made for, none if it was made for none, else
/// `voice`, a hidden object of the same class. Never the local hero: the game reads what the object is (a bomb's numbers from a
/// hero's memory broke it). None: not made.
fn owner_of(local: &impl Fn(u16) -> Option<u32>, slot: u16, owner: u16, voice: Option<u32>) -> Option<u32> {
	match local(slot) {
		Some(a) => Some(a),
		None if owner == NO_SLOT => Some(0),
		None => voice,
	}
}

/// Makes another game's sound or effect here. `local` names this game's moby for a slot or id; `voice` is a hidden object of the
/// player's: of the class of his sound, or of what his explosion or glow was made for. False: not now, the next field.
pub fn play(c: &mut Cpu, e: &FxEvent, local: impl Fn(u16) -> Option<u32>, voice: Option<u32>) -> bool {
	if matches!(e, FxEvent::Explode { .. } | FxEvent::Spark { .. } | FxEvent::Glow { .. }) && !MAIN_STACK.contains(&(c.g(29) as u32)) {
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
		FxEvent::Spark { pos, vel, a2, a3, t0 } => (!seen_near(&pos)).then(|| {
			let scratch = (c.g(29) as u32).wrapping_sub(SCRATCH_BELOW) & !0xf;
			let ram = &mut c.mem.ram[..];
			pos.iter().chain(&vel).enumerate().for_each(|(k, v)| put_word(ram, scratch + SCRATCH_POS + k as u32 * 4, v.to_bits()));
			crate::omni::call_game(c, SPARK_SPAWN, |c| {
				c.s(29, scratch as u64);
				c.s(4, (scratch + SCRATCH_POS) as u64);
				c.s(5, (scratch + SCRATCH_POS + 16) as u64);
				c.s(6, a2 as u64);
				c.s(7, a3);
				c.s(8, t0 as u64);
			})
		}),
		FxEvent::Glow { slot, owner, size, pos, base, life, rgba } => owner_of(&local, slot, owner, voice).filter(|_| !seen_near(&pos)).map(|owner| {
			let scratch = (c.g(29) as u32).wrapping_sub(SCRATCH_BELOW) & !0xf;
			let ram = &mut c.mem.ram[..];
			pos.iter().chain(&base).enumerate().for_each(|(k, v)| put_word(ram, scratch + SCRATCH_POS + k as u32 * 4, v.to_bits()));
			crate::omni::call_game(c, GLOW_SPAWN, |c| {
				c.s(29, scratch as u64);
				c.f[12] = size.to_bits();
				c.s(4, owner as u64);
				c.s(5, (scratch + SCRATCH_POS) as u64);
				c.s(6, (scratch + SCRATCH_POS + 16) as u64);
				c.s(7, life as u64);
				for (r, v) in [8, 9, 10, 11].into_iter().zip(rgba) {
					c.s(r, v as u64);
				}
			})
		}),
		FxEvent::Explode { slot, owner, pos, ints, floats, stack } => {
			let now = FIELD.load(Ordering::Relaxed);
			let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
			let seen = lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p));
			match (explode_entry(c), owner_of(&local, slot, owner, voice)) {
				(Some(entry), Some(moby)) if !seen => {
					let mut made = lock(&MADE_BLASTS);
					made.retain(|(_, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
					made.push(([pos[0], pos[1], pos[2]], now));
					drop(made);
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
			PLAYED_KINDS[kind(e)].fetch_add(1, Ordering::Relaxed);
		}
		Some(Err(e)) => eprintln!("mp: could not make another game's effect: {e}"),
		None => {}
	}
	true
}

/// A spark or flare at `pos` this game made itself a moment ago, as an explosion would be (`BLASTS`): not made again.
fn seen_near(pos: &[f32; 4]) -> bool {
	let now = FIELD.load(Ordering::Relaxed);
	lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR)
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
