//! Enemy sounds and effects. An enemy whose update another game runs makes no sound and no effect here, so they travel: hands at the
//! entry of the game's own functions catch the calls made in the game that runs it, and the other games make the same calls on their
//! copy, a moment later so they match the mirrored animation.
//!
//! - Sounds: `moby_cue(cue, arg, moby)` plays a sound cue of the moby's class on it, which is how enemies (and the shots they fire)
//!   make theirs. A cue on something this game runs goes out with the object's slot or id, and is played on the copy there.
//! - Explosions: `fx.explode`, whatever made it (an enemy's death, a lobbed bomb, a crate, a player's weapon), in play. Its arguments
//!   go out as they were, but for the object it was made for and the position, and the other games run it the same way. One that
//!   happened here anyway, at that place a moment before, is not shown twice.
//! - The same way: the game's two small explosions (`burst_small`, `burst_large`: sparks, glows, smoke and a sound, a robot's or a
//!   creature's hit or blow), its second glow (`glow_ring_spawn`, an explosion's rings) and a hit's flash (`flash_spawn`, class 775,
//!   where an enemy's blow lands). Found by their code (omni/rac1_funcs.txt, the 0xf000000N numbers).
//! - What one of these makes inside it (the cue of a burst, the rings of an explosion) is not sent by itself: the other games make it
//!   when they run the outer one (`BODIES`, by the return address).
//!
//! Another game's sounds and effects are made inside this game's own frame, not between two fields: the game's functions for them
//! can wait for a field, and called between two fields (`omni::run_game`) they stopped halfway and left the game broken (pair runs
//! faulted so after an explosion, and after a cue, spark or glow, on Veldin and Novalis). The hand at the hero's `update_motion_input`
//! (scenes.rs) runs each first, as the game's own code calls it, on a stack area below the caller's, and has it return to that entry,
//! where the hand puts the call back as it came (`in_frame`, `resumed`), or runs the next.
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

/// The NTSC numbers of `moby_cue`, `spark_spawn` and `glow_flare_spawn` (omni/rac1_funcs.txt), and this port's own numbers of the
/// game's two small explosions, its second glow and a hit's flash.
const MOBY_CUE: u32 = 0x28_D590;
const SPARK_SPAWN: u32 = 0x2A_EDF8;
const GLOW_SPAWN: u32 = 0x2D_F1B0;
const BURST_SMALL: u32 = 0xF000_0001;
const BURST_LARGE: u32 = 0xF000_0002;
const GLOW_RING: u32 = 0xF000_0003;
const FLASH: u32 = 0xF000_0004;
/// An explosion shown here is not shown again for another game within this many metres and fields.
const SAME_BLAST_NEAR: f32 = 2.0;
const SAME_BLAST_FIELDS: u64 = 90;
/// The stack area a call made for another game is given, just below the stack pointer of the hero's update (where the game's own calls
/// go: further down is another thread's, and writing there broke the game), and where its vectors go in it.
const SCRATCH_BELOW: u32 = 0x300;
const SCRATCH_POS: u32 = 0x200;

static CUE: OnceLock<&'static Reloc> = OnceLock::new();
static SPARK: OnceLock<&'static Reloc> = OnceLock::new();
static GLOW: OnceLock<&'static Reloc> = OnceLock::new();
static EXPLODE: OnceLock<&'static Reloc> = OnceLock::new();
static BURST_S: OnceLock<&'static Reloc> = OnceLock::new();
static BURST_L: OnceLock<&'static Reloc> = OnceLock::new();
static RING: OnceLock<&'static Reloc> = OnceLock::new();
static FLASH_R: OnceLock<&'static Reloc> = OnceLock::new();

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
fn burst_s_reloc() -> &'static Reloc {
	BURST_S.get().expect("burst hand without a spec")
}
fn burst_l_reloc() -> &'static Reloc {
	BURST_L.get().expect("burst hand without a spec")
}
fn ring_reloc() -> &'static Reloc {
	RING.get().expect("glow ring hand without a spec")
}
fn flash_reloc() -> &'static Reloc {
	FLASH_R.get().expect("flash hand without a spec")
}

/// Where the functions whose calls go out whole (the explosion, the bursts) are in the code loaded now, as (entry, length): what they
/// make inside them goes with them. Noted at their entry, forgotten when the level changes (`forget`).
static BODIES: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

fn note_body(entry: u32, len: u32) {
	let mut b = lock(&BODIES);
	if !b.iter().any(|&(e, _)| e == entry) {
		b.push((entry, len));
	}
}

/// The call at this hand was made from inside one that goes out whole.
fn inside_sent(c: &Cpu) -> bool {
	let ra = c.g(31) as u32 & MASK;
	lock(&BODIES).iter().any(|&(e, l)| ra > e && ra < e + l)
}

/// The level changed: the functions are elsewhere now.
pub fn forget() {
	lock(&BODIES).clear();
}

/// The local hero's moby and the ones he carries: their cues are his own sounds, for the others' voices of him.
static PLAYER: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// What this game runs that others mirror, by moby address: its placed slot or spawned id.
static OURS: Mutex<BTreeMap<u32, u16>> = Mutex::new(BTreeMap::new());
/// Others are in this level, in play: catch calls. Placed objects and their ids are `PLACED` (address to id, mp/ids.rs).
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PLACED: Mutex<Option<std::sync::Arc<std::collections::HashMap<u32, u16>>>> = Mutex::new(None);
/// What the hands caught this field, with the moby each was for.
static CAUGHT: Mutex<Vec<(u32, FxEvent)>> = Mutex::new(Vec::new());
/// Explosions this game made, and when (in fields): what another game sends near one is not shown again.
static BLASTS: Mutex<Vec<([f32; 3], u64)>> = Mutex::new(Vec::new());
/// Explosions and cues made here for other games, and when: this game's own of the same, just after, are not made.
static MADE_BLASTS: Mutex<Vec<([f32; 3], u64)>> = Mutex::new(Vec::new());
static MADE_CUES: Mutex<Vec<(u32, i32, u64)>> = Mutex::new(Vec::new());
static FIELD: AtomicU64 = AtomicU64::new(0);
/// What a call made for another game is to: a game function by its NTSC number (omni/rac1_funcs.txt), or the explosion.
#[derive(Clone, Copy)]
enum Target {
	Func(u32),
	Explode,
}

/// Another game's sound or effect waiting for this game's frame: what to call; its integer registers; those that get an address in
/// the stack area (register, offset); its FPU registers; the stack area's words (offset, word); the moby it is on, which must still
/// be there (0: none); the field it came.
struct Call {
	to: Target,
	regs: Vec<(usize, u64)>,
	in_area: Vec<(usize, u32)>,
	fregs: Vec<(usize, u32)>,
	words: Vec<(u32, u32)>,
	moby: u32,
	at: u64,
}
static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());
/// At most this many a field, and how many this field has had.
const CALLS_PER_FIELD: u32 = 24;
static STARTED: Mutex<(u64, u32)> = Mutex::new((0, 0));
/// The call one made for another game was run ahead of: its entry, its registers to go on with, the stack area the explosion was given, the field.
struct Resume {
	entry: u32,
	sp: u64,
	low: u32,
	ra: u64,
	args: [u64; 8],
	floats: [u32; 8],
	at: u64,
}
static RESUME: Mutex<Option<Resume>> = Mutex::new(None);
/// A call made for another game is running in this game's frame: what it makes is not caught again.
static IN_FRAME: AtomicBool = AtomicBool::new(false);
/// A call not back after this many fields (the level was left while it waited) frees the hand again.
const RESUME_WITHIN: u64 = 600;
/// Where `fx.explode` starts (it is found by its first call here, or a scan).
static EXPLODE_AT: AtomicU32 = AtomicU32::new(0);
/// Sounds and explosions caught, and made for other games, for the run log.
pub static SENT: AtomicU64 = AtomicU64::new(0);
pub static PLAYED: AtomicU64 = AtomicU64::new(0);
/// The same by kind (`kind`): enemy cues, explosions, player cues, sparks, glows, bursts, flashes.
pub const KINDS: usize = 7;
pub static SENT_KINDS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];
pub static PLAYED_KINDS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];

/// The index of `e`'s kind in `SENT_KINDS` and `PLAYED_KINDS`.
pub fn kind(e: &FxEvent) -> usize {
	match e {
		FxEvent::Cue { .. } => 0,
		FxEvent::Explode { .. } => 1,
		FxEvent::PlayerCue { .. } => 2,
		FxEvent::Spark { .. } => 3,
		FxEvent::Glow { .. } => 4,
		FxEvent::Burst { .. } => 5,
		FxEvent::Flash { .. } => 6,
	}
}

/// Counts by kind, for the run log.
pub fn kinds(k: &[AtomicU64; KINDS]) -> [u64; KINDS] {
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
	lock(&PLACED).as_ref().and_then(|p| p.get(&moby).copied()).unwrap_or(NO_SLOT)
}

fn cue_hand(c: &mut Cpu) -> bool {
	if IN_FRAME.load(Ordering::Relaxed) {
		return false;
	}
	let moby = c.g(6) as u32 & MASK;
	let (cue, now) = (c.g(4) as i32, FIELD.load(Ordering::Relaxed));
	if lock(&MADE_CUES).iter().any(|&(m, q, f)| m == moby && q == cue && now.saturating_sub(f) < SAME_BLAST_FIELDS) {
		c.s(2, 0);
		skip(c);
		return true;
	}
	if !ACTIVE.load(Ordering::Relaxed) || inside_sent(c) {
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
	note_body(c.pc, explode_reloc().len);
	let ram = &c.mem.ram;
	let at = c.g(6) as u32;
	let pos = [0, 4, 8, 12].map(|k| f32::from_bits(word(ram, at + k)));
	if IN_FRAME.load(Ordering::Relaxed) {
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
	if IN_FRAME.load(Ordering::Relaxed) {
		return false;
	}
	let at = vec4(&c.mem.ram, c.g(4) as u32);
	lock(&BLASTS).push(([at[0], at[1], at[2]], FIELD.load(Ordering::Relaxed)));
	if !ACTIVE.load(Ordering::Relaxed) || inside_sent(c) {
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
	glow_or_ring(c, false)
}

fn ring_hand(c: &mut Cpu) -> bool {
	glow_or_ring(c, true)
}

/// `glow_flare_spawn` and `glow_ring_spawn` take the same arguments: owner, position, base, life, colour and size.
fn glow_or_ring(c: &mut Cpu, ring: bool) -> bool {
	if IN_FRAME.load(Ordering::Relaxed) {
		return false;
	}
	let at = vec4(&c.mem.ram, c.g(5) as u32);
	lock(&BLASTS).push(([at[0], at[1], at[2]], FIELD.load(Ordering::Relaxed)));
	if !ACTIVE.load(Ordering::Relaxed) || inside_sent(c) {
		return false;
	}
	let ram = &c.mem.ram;
	let (size, owner) = (f32::from_bits(c.f[12]), c.g(4) as u32 & MASK);
	let (pos, base) = (vec4(ram, c.g(5) as u32), vec4(ram, c.g(6) as u32));
	if size.is_finite() && pos.iter().chain(&base).all(|v| v.is_finite()) {
		let rgba = [8, 9, 10, 11].map(|r| c.g(r) as u32);
		lock(&CAUGHT).push((owner, FxEvent::Glow { ring, slot: slot_of(owner), owner: class_of(ram, owner), size, pos, base, life: c.g(7) as u32, rgba }));
	}
	false
}

fn burst_small_hand(c: &mut Cpu) -> bool {
	note_body(c.pc, burst_s_reloc().len);
	burst(c, false)
}

fn burst_large_hand(c: &mut Cpu) -> bool {
	note_body(c.pc, burst_l_reloc().len);
	burst(c, true)
}

/// A burst (`burst_small` / `burst_large`): for the object a0 (may be none), at the position a1 points at, with sound cue a2 and the
/// sizes f12, f13. Made here and there like an explosion: one this game makes just after making the same for another game is not made.
fn burst(c: &mut Cpu, large: bool) -> bool {
	if IN_FRAME.load(Ordering::Relaxed) {
		return false;
	}
	let pos = vec4(&c.mem.ram, c.g(5) as u32);
	let now = FIELD.load(Ordering::Relaxed);
	let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
	if lock(&MADE_BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p)) {
		c.s(2, 0);
		skip(c);
		return true;
	}
	lock(&BLASTS).push(([pos[0], pos[1], pos[2]], now));
	if !ACTIVE.load(Ordering::Relaxed) || inside_sent(c) || pos.iter().any(|v| !v.is_finite()) {
		return false;
	}
	let moby = c.g(4) as u32 & MASK;
	let ram = &c.mem.ram;
	let ev = FxEvent::Burst { large, slot: slot_of(moby), owner: class_of(ram, moby), pos, cue: c.g(6) as u32, sizes: [c.f[12], c.f[13]] };
	lock(&CAUGHT).push((moby, ev));
	false
}

/// A hit's flash (`flash_spawn`): at the position a0 points at, f12 its size.
fn flash_hand(c: &mut Cpu) -> bool {
	if IN_FRAME.load(Ordering::Relaxed) {
		return false;
	}
	let pos = vec4(&c.mem.ram, c.g(4) as u32);
	lock(&BLASTS).push(([pos[0], pos[1], pos[2]], FIELD.load(Ordering::Relaxed)));
	let scale = f32::from_bits(c.f[12]);
	if !ACTIVE.load(Ordering::Relaxed) || inside_sent(c) || !scale.is_finite() || pos.iter().any(|v| !v.is_finite()) {
		return false;
	}
	lock(&CAUGHT).push((0, FxEvent::Flash { pos, scale }));
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
	for (name, slot, reloc, f) in [
		("burst_small", &BURST_S, burst_s_reloc as fn() -> &'static Reloc, burst_small_hand as fn(&mut Cpu) -> bool),
		("burst_large", &BURST_L, burst_l_reloc, burst_large_hand),
		("glow_ring_spawn", &RING, ring_reloc, ring_hand),
		("flash_spawn", &FLASH_R, flash_reloc, flash_hand),
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
pub fn set_scene(ours: BTreeMap<u32, u16>, player: Vec<u32>, active: bool, placed: std::sync::Arc<std::collections::HashMap<u32, u16>>, field: u64) {
	*lock(&OURS) = ours;
	*lock(&PLAYER) = player;
	*lock(&PLACED) = Some(placed);
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

/// Makes another game's sound or effect here, in this game's next frame (`in_frame`). `local` names this game's moby for a slot or id;
/// `voice` is a hidden object of the player's: of the class of his sound, or of what his explosion or glow was made for. Always true
/// (false would be: not now, the next field).
pub fn play(_c: &mut Cpu, e: &FxEvent, local: impl Fn(u16) -> Option<u32>, voice: Option<u32>) -> bool {
	let now = FIELD.load(Ordering::Relaxed);
	let cue = |cue: i32, arg: u32, moby: u32| Call { to: Target::Func(MOBY_CUE), regs: vec![(4, cue as i64 as u64), (5, arg as u64), (6, moby as u64)], in_area: Vec::new(), fregs: Vec::new(), words: Vec::new(), moby, at: now };
	let vecs = |a: &[f32; 4], b: &[f32; 4]| a.iter().chain(b).enumerate().map(|(k, v)| (SCRATCH_POS + k as u32 * 4, v.to_bits())).collect::<Vec<_>>();
	let call = match *e {
		FxEvent::PlayerCue { cue: q, arg, .. } => voice.map(|moby| cue(q, arg, moby)),
		FxEvent::Cue { slot, cue: q, arg } => local(slot).map(|moby| {
			let mut made = lock(&MADE_CUES);
			made.retain(|(_, _, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
			made.push((moby, q, now));
			cue(q, arg, moby)
		}),
		FxEvent::Spark { pos, vel, a2, a3, t0 } => (!seen_near(&pos)).then(|| Call {
			to: Target::Func(SPARK_SPAWN),
			regs: vec![(6, a2 as u64), (7, a3), (8, t0 as u64)],
			in_area: vec![(4, SCRATCH_POS), (5, SCRATCH_POS + 16)],
			fregs: Vec::new(),
			words: vecs(&pos, &vel),
			moby: 0,
			at: now,
		}),
		FxEvent::Glow { ring, slot, owner, size, pos, base, life, rgba } => owner_of(&local, slot, owner, voice).filter(|_| !seen_near(&pos)).map(|owner| Call {
			to: Target::Func(if ring { GLOW_RING } else { GLOW_SPAWN }),
			regs: [(4, owner as u64), (7, life as u64)].into_iter().chain([8, 9, 10, 11].into_iter().zip(rgba.map(|v| v as u64))).collect(),
			in_area: vec![(5, SCRATCH_POS), (6, SCRATCH_POS + 16)],
			fregs: vec![(12, size.to_bits())],
			words: vecs(&pos, &base),
			moby: owner,
			at: now,
		}),
		FxEvent::Flash { pos, scale } => (!seen_near(&pos)).then(|| Call {
			to: Target::Func(FLASH),
			regs: Vec::new(),
			in_area: vec![(4, SCRATCH_POS)],
			fregs: vec![(12, scale.to_bits())],
			words: pos.iter().enumerate().map(|(k, v)| (SCRATCH_POS + k as u32 * 4, v.to_bits())).collect(),
			moby: 0,
			at: now,
		}),
		FxEvent::Burst { large, slot, owner, pos, cue, sizes } => {
			let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
			let seen = lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p));
			match owner_of(&local, slot, owner, voice) {
				Some(moby) if !seen => {
					let mut made = lock(&MADE_BLASTS);
					made.retain(|(_, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
					made.push(([pos[0], pos[1], pos[2]], now));
					drop(made);
					Some(Call {
						to: Target::Func(if large { BURST_LARGE } else { BURST_SMALL }),
						regs: vec![(4, moby as u64), (6, cue as i32 as i64 as u64)],
						in_area: vec![(5, SCRATCH_POS)],
						fregs: vec![(12, sizes[0]), (13, sizes[1])],
						words: pos.iter().enumerate().map(|(k, v)| (SCRATCH_POS + k as u32 * 4, v.to_bits())).collect(),
						moby,
						at: now,
					})
				}
				_ => None,
			}
		}
		FxEvent::Explode { slot, owner, pos, ints, floats, stack } => {
			let near = |p: &[f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR;
			let seen = lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && near(p));
			match owner_of(&local, slot, owner, voice) {
				Some(moby) if !seen => {
					let mut made = lock(&MADE_BLASTS);
					made.retain(|(_, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS);
					made.push(([pos[0], pos[1], pos[2]], now));
					drop(made);
					Some(Call {
						to: Target::Explode,
						regs: std::iter::once((4, moby as u64)).chain([5, 7, 8, 9, 10, 11].into_iter().zip(ints.map(|v| v as i32 as i64 as u64))).collect(),
						in_area: vec![(6, SCRATCH_POS)],
						fregs: (12..20).zip(floats).collect(),
						words: stack.iter().enumerate().map(|(k, v)| (k as u32 * 4, *v)).chain(pos.iter().enumerate().map(|(k, v)| (SCRATCH_POS + k as u32 * 4, v.to_bits()))).collect(),
						moby,
						at: now,
					})
				}
				_ => None,
			}
		}
	};
	if let Some(call) = call {
		lock(&CALLS).push(call);
		PLAYED.fetch_add(1, Ordering::Relaxed);
		PLAYED_KINDS[kind(e)].fetch_add(1, Ordering::Relaxed);
	}
	true
}

/// A spark or flare at `pos` this game made itself a moment ago, as an explosion would be (`BLASTS`): not made again.
fn seen_near(pos: &[f32; 4]) -> bool {
	let now = FIELD.load(Ordering::Relaxed);
	lock(&BLASTS).iter().any(|(p, f)| now.saturating_sub(*f) < SAME_BLAST_FIELDS && (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>() < SAME_BLAST_NEAR * SAME_BLAST_NEAR)
}

/// Where `fx.explode` starts in the code loaded now.
/// At the entry of the hero's `update_motion_input`, a call run ahead of this one for another game is back: this one goes on as it came.
pub(super) fn resumed(c: &mut Cpu) -> bool {
	let mut resume = lock(&RESUME);
	let Some(r) = resume.as_ref() else { return false };
	if c.pc == r.entry && c.g(29) as u32 == r.low {
		let r = resume.take().expect("checked");
		c.s(29, r.sp);
		c.s(31, r.ra);
		for (k, v) in r.args.iter().enumerate() {
			c.s(4 + k, *v);
		}
		c.f[12..20].copy_from_slice(&r.floats);
		IN_FRAME.store(false, Ordering::Relaxed);
		let n = PLAYED_IN_FRAME.fetch_add(1, Ordering::Relaxed) + 1;
		if n.is_power_of_two() {
			eprintln!("mp: {n} sounds and effects of other games made here, each in this game's own frame");
		}
		return true;
	}
	if FIELD.load(Ordering::Relaxed).saturating_sub(r.at) > RESUME_WITHIN {
		*resume = None;
		IN_FRAME.store(false, Ordering::Relaxed);
		eprintln!("mp: another game's sound or effect did not come back to the hero's update");
	}
	false
}

/// Sounds and effects of other games made in this game's frame and back (for the run log).
pub static PLAYED_IN_FRAME: AtomicU64 = AtomicU64::new(0);

/// At the entry of the hero's `update_motion_input` (once a field in play): another game's sound or effect, if one waits, runs first,
/// as the game's own code calls it, on a stack area below the caller's, and returns to this entry (`resumed`), where the next may run.
/// True: one was started.
pub(super) fn in_frame(c: &mut Cpu) -> bool {
	let mut resume = lock(&RESUME);
	if resume.is_some() {
		return false;
	}
	let now = FIELD.load(Ordering::Relaxed);
	{
		let mut n = lock(&STARTED);
		if n.0 != now {
			*n = (now, 0);
		}
		if n.1 >= CALLS_PER_FIELD {
			return false;
		}
	}
	let call = loop {
		let mut q = lock(&CALLS);
		q.retain(|k| now.saturating_sub(k.at) < SAME_BLAST_FIELDS);
		if q.is_empty() {
			return false;
		}
		let k = q.remove(0);
		drop(q);
		if k.moby == 0 || word(&c.mem.ram, k.moby + 0x24) != 0 {
			break k;
		}
	};
	let entry = match call.to {
		Target::Explode => explode_entry(c),
		Target::Func(n) => crate::omni::game_entry(c, n),
	};
	let Some(entry) = entry else { return false };
	lock(&STARTED).1 += 1;
	let sp = c.g(29);
	let low = (sp as u32).wrapping_sub(SCRATCH_BELOW) & !0xf;
	let ram = &mut c.mem.ram[..];
	call.words.iter().for_each(|&(o, w)| put_word(ram, low + o, w));
	let mut floats = [0u32; 8];
	floats.copy_from_slice(&c.f[12..20]);
	*resume = Some(Resume { entry: c.pc, sp, low, ra: c.g(31), args: std::array::from_fn(|k| c.g(4 + k)), floats, at: now });
	c.s(29, low as u64);
	c.s(31, c.pc as u64);
	call.regs.iter().for_each(|&(r, v)| c.s(r, v));
	call.in_area.iter().for_each(|&(r, o)| c.s(r, (low + o) as u64));
	call.fregs.iter().for_each(|&(r, v)| c.f[r] = v);
	IN_FRAME.store(true, Ordering::Relaxed);
	c.pc = entry;
	c.npc = entry.wrapping_add(4);
	c.mem.now = c.icount;
	true
}

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
