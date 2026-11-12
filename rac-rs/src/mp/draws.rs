//! What the enemies another game runs draw besides their model: a beam, a glint, a trail, a charge. An enemy's update puts a draw
//! callback on one of the game's draw queues (`draw_queue_add(callback, arg)`, two to four queues a level, emptied every field) with
//! the enemy as its argument, every field it wants it drawn, and the callback draws from the enemy's variables (pvars) what the update
//! just worked out. An enemy whose update another game runs puts none here, and its pvars stand still.
//!
//! The game that runs an enemy catches its callbacks (a hand at every queue's add, the spec `draw_queue_add` of omni/rac1_funcs.txt)
//! and sends them with the enemy's pvar words, `MAX_DRAW_VARS` at most, every other field (`Msg::Draws`). The others, for their copy:
//! - write the words in between two fields, all but those that are, there or here, an address (a pointer of the enemy's is this game's
//!   own, and a number written over one would be read as one), keeping what they wrote over (`backup`) and putting it back as soon as
//!   the copy is no longer shown from the other game (it runs here again, or dies here): its update goes on from its own state;
//! - put the same callbacks on the same queues with the copy as the argument, in the game's own frame (the hero's update,
//!   `in_frame`), as the enemy's update would. Only a callback whose code here is the code there (`DrawCall::code`), and not while a
//!   word the callback may read as an address is one there and not here.
//!
//! Draws are shown `DELAY` fields after they came, as the poses are, and each for `LASTS` fields: when the enemy stops drawing, so
//! does the copy.

use super::*;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use rac_mp::proto::{MAX_DRAW_CALLS, MAX_DRAW_VARS};
use rac_mp::{DrawCall, Draws};

/// Fields a draw is shown after it came (as the poses are, mp.rs `DELAY`), and how long it is shown for.
const SHOW_AFTER: u64 = 6;
const LASTS: u64 = 2;
/// Draws of a copy kept, waiting to be shown.
const KEEP: usize = 6;
/// A queue holds this many.
const QUEUE_MAX: u32 = 0x40;

static SPEC: OnceLock<&'static Reloc> = OnceLock::new();

fn spec() -> &'static Reloc {
	SPEC.get().expect("draw queue hand without a spec")
}

/// The enemies this game runs that others see, by moby address (their id); what their updates put on the queues this field.
static WATCH: Mutex<BTreeMap<u32, u16>> = Mutex::new(BTreeMap::new());
static ON: AtomicBool = AtomicBool::new(false);
static CAUGHT: Mutex<Vec<(u32, u32, u32, bool)>> = Mutex::new(Vec::new());
/// The callbacks to put on the queues this field, in the game's frame: (count cell, fn array, arg array, callback, argument), and the
/// field they are for.
static DUE: Mutex<(u64, Vec<(u32, u32, u32, u32, u32)>)> = Mutex::new((0, Vec::new()));
static FIELD: AtomicU64 = AtomicU64::new(0);
/// Callbacks caught and sent, and put here for others, for the run log.
pub(super) static SENT: AtomicU64 = AtomicU64::new(0);
pub(super) static SHOWN: AtomicU64 = AtomicU64::new(0);

/// The first time a callback is seen (which 0: caught here, 1: shown here), for the run log.
fn first(func: u32, which: u8) -> bool {
	static SEEN: Mutex<BTreeSet<(u32, u8)>> = Mutex::new(BTreeSet::new());
	SEEN.lock().unwrap_or_else(|e| e.into_inner()).insert((func, which))
}

/// Registers the hand. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	if let Some(r) = crate::omni::func_reloc("draw_queue_add") {
		let _ = SPEC.set(r);
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: "draw_queue_add", reloc: spec, est: ee_oracle::hs::EXACT, f: add_hand })));
		eprintln!("mp: a hand for the draw queues");
	}
}

/// `draw_queue_add(callback, arg)`: one of ours put a callback on a queue (the arg the enemy, or its pvars).
fn add_hand(c: &mut Cpu) -> bool {
	if !ON.load(Ordering::Relaxed) {
		return false;
	}
	let arg = c.g(5) as u32 & MASK;
	let watch = WATCH.lock().unwrap_or_else(|e| e.into_inner());
	let found = watch.keys().copied().find(|&m| m == arg).map(|m| (m, false)).or_else(|| watch.keys().copied().find(|&m| rd(&c.mem.ram, m + 0x78) & MASK == arg).map(|m| (m, true)));
	drop(watch);
	if let Some((moby, pvars)) = found {
		CAUGHT.lock().unwrap_or_else(|e| e.into_inner()).push((moby, c.pc, c.g(4) as u32 & MASK, pvars));
	}
	false
}

/// A queue's add function: where it is, and its count cell and arrays (from its own `lui`/`lw` and `lui`/`addiu`: count first, then
/// the arg array, then the fn array, as the game's code builds them).
#[derive(Clone, Copy, Debug)]
struct Queue {
	add: u32,
	count: u32,
	fns: u32,
	args: u32,
}

fn queue_at(ram: &[u8], add: u32) -> Option<Queue> {
	let mut hi = [None::<u32>; 32];
	let mut count = None;
	let mut built = Vec::new();
	for k in 0..20 {
		let w = rd(ram, add + 4 * k);
		let (op, rs, rt, imm) = (w >> 26, (w >> 21 & 31) as usize, (w >> 16 & 31) as usize, w & 0xffff);
		let simm = imm as u16 as i16 as i32 as u32;
		match op {
			0x0f => hi[rt] = Some(imm << 16),
			0x23 => {
				if count.is_none() {
					count = hi[rs].map(|h| h.wrapping_add(simm));
				}
				hi[rt] = None;
			}
			0x09 if rs == rt && hi[rs].is_some() => {
				built.push(hi[rs].map_or(0, |h| h.wrapping_add(simm)));
				hi[rt] = None;
			}
			0x09 => hi[rt] = None,
			_ => {}
		}
	}
	let [args, fns] = built[..] else { return None };
	Some(Queue { add, count: count?, fns, args })
}

/// Where the game's code is (omni/funcs.rs `CODE_LO`..`CODE_HI`).
const CODE: std::ops::Range<u32> = 0x10_0000..0x40_0000;
const JR_RA: u32 = 0x03e0_0008;

/// Each queue's add in the code loaded now, by address (the same order in every copy of the disc). A function starts after a `jr ra`
/// and its delay slot (and up to three words of padding).
fn queues(c: &Cpu) -> Vec<Queue> {
	let Some(r) = SPEC.get() else { return Vec::new() };
	let ram = &c.mem.ram[..];
	let at = |a: u32| rd(ram, a);
	let mut out = Vec::new();
	let mut a = CODE.start + 8;
	while a + r.len <= CODE.end {
		if (2..=5).any(|k| rd(ram, a - 4 * k) == JR_RA) && r.masked_hash_by(a, at) == r.mhash {
			if let Some(q) = queue_at(ram, a) {
				out.push(q);
			}
			a += r.len;
		} else {
			a += 4;
		}
	}
	out
}

/// A copy's draws: the newest few, with the field each came; the words written over and what they were.
#[derive(Default)]
struct Shown {
	got: Vec<(u64, Draws)>,
	backup: BTreeMap<u32, u32>,
}

#[derive(Default)]
pub(super) struct DrawState {
	level: i32,
	queues: Vec<Queue>,
	/// Another game's draws by its enemy's id, and the copies written to.
	shown: BTreeMap<u16, Shown>,
	/// The copy each id was written to (its moby), for putting things back.
	at: BTreeMap<u16, u32>,
	/// Fields since the level changed (the queues are looked for again every `LOOK_AGAIN` while none were found).
	looked: u64,
}

/// Fields between two looks for a level's queues while it has none.
const LOOK_AGAIN: u64 = 60;

fn put_word(ram: &mut [u8], a: u32, v: u32) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

/// A word that can be an address in RAM (mp/particles.rs has the same test).
fn pointer_like(v: u32) -> bool {
	v & 0x0e00_0000 == 0 && v & MASK >= 0x10_0000 && matches!(v >> 28, 0 | 2 | 3 | 8 | 0xa | 0xb)
}

/// How many pvar words of the moby at `moby` to send: up to the next pvars of the table's objects, `MAX_DRAW_VARS` at most.
fn vars_len(ram: &[u8], table: u32, end: u32, moby: u32) -> usize {
	let pv = rd(ram, moby + 0x78) & MASK;
	let mut next = pv + 4 * MAX_DRAW_VARS as u32;
	let mut m = table;
	while m < end {
		let p = rd(ram, m + 0x78) & MASK;
		if rd(ram, m + 0x24) != 0 && p > pv && p < next {
			next = p;
		}
		m += 0x100;
	}
	((next - pv) / 4) as usize
}

impl DrawState {
	/// The queues of the level loaded now: looked for when it changes, and again while none were found (its code may not be in yet)
	/// or one of ours used an add not among them.
	fn level_is(&mut self, c: &Cpu, level: i8, unknown_add: bool) {
		if self.level != level as i32 {
			self.level = level as i32;
			self.queues.clear();
			self.shown.clear();
			self.at.clear();
			self.looked = 0;
		}
		self.looked += 1;
		if (self.queues.is_empty() && self.looked % LOOK_AGAIN == 1) || unknown_add {
			self.queues = queues(c);
			eprintln!("mp: level {level} has {} draw queues", self.queues.len());
		}
	}

	/// Each field, the game that runs enemies: which of ours others see (`ours`: moby to id), and what they drew this field, to send.
	pub(super) fn take(&mut self, c: &Cpu, level: i8, table: u32, end: u32, ours: BTreeMap<u32, u16>, send: bool) -> Vec<Draws> {
		let caught = std::mem::take(&mut *CAUGHT.lock().unwrap_or_else(|e| e.into_inner()));
		let unknown = caught.iter().any(|k| !self.queues.iter().any(|q| q.add == k.1));
		self.level_is(c, level, unknown);
		ON.store(!ours.is_empty(), Ordering::Relaxed);
		*WATCH.lock().unwrap_or_else(|e| e.into_inner()) = ours.clone();
		if !send || caught.is_empty() {
			return Vec::new();
		}
		let ram = &c.mem.ram[..];
		let mut by: BTreeMap<u32, Vec<DrawCall>> = BTreeMap::new();
		for (moby, add, func, pvars) in caught {
			let Some(q) = self.queues.iter().position(|q| q.add == add) else { continue };
			let code: Vec<u32> = (0..rac_mp::proto::DRAW_CODE_WORDS as u32).map(|k| rd(ram, func + 4 * k)).collect();
			let call = DrawCall { queue: q as u8 | if pvars { 0x80 } else { 0 }, func, code: DrawCall::code_of(&code) };
			if first(func, 0) {
				eprintln!("mp: an enemy we run (class {}) draws with {func:#x} on queue {q}{}", rd(ram, moby + 0xa4) >> 16, if pvars { ", given its pvars" } else { "" });
			}
			let v = by.entry(moby).or_default();
			if v.len() < MAX_DRAW_CALLS && !v.contains(&call) {
				v.push(call);
			}
		}
		let out: Vec<Draws> = by
			.into_iter()
			.filter_map(|(moby, calls)| {
				let slot = *ours.get(&moby)?;
				let pv = rd(ram, moby + 0x78) & MASK;
				let n = vars_len(ram, table, end, moby);
				Some(Draws { slot, calls, vars: (0..n as u32).map(|k| rd(ram, pv + 4 * k)).collect() })
			})
			.collect();
		SENT.fetch_add(out.iter().map(|d| d.calls.len() as u64).sum(), Ordering::Relaxed);
		out
	}

	/// Another game's draws of its enemy `slot`, come at field `now`.
	pub(super) fn got(&mut self, d: Draws, now: u64) {
		let s = self.shown.entry(d.slot).or_default();
		s.got.push((now, d));
		if s.got.len() > KEEP {
			s.got.remove(0);
		}
	}

	/// Between two fields: for each copy shown from another game (`shown`: id to moby) whose draws are due, its words written and its
	/// callbacks made ready for the frame; every copy no longer shown (or with nothing due) given back what was written over.
	pub(super) fn show(&mut self, c: &mut Cpu, level: i8, now: u64, shown: &BTreeMap<u16, u32>) {
		let mut due = Vec::new();
		let ids: Vec<u16> = self.shown.keys().copied().collect();
		for id in ids {
			let moby = shown.get(&id).copied();
			let s = self.shown.get_mut(&id).expect("listed");
			s.got.retain(|(f, _)| now.saturating_sub(*f) <= SHOW_AFTER + LASTS + 8);
			let draw = s.got.iter().rev().find(|(f, _)| now >= f + SHOW_AFTER && now < f + SHOW_AFTER + LASTS).map(|(_, d)| d.clone());
			let (Some(moby), Some(d)) = (moby, draw) else {
				// Nothing to show: what was written over goes back (to the copy it was written to).
				if let Some(at) = self.at.remove(&id) {
					let ram = &mut c.mem.ram[..];
					let pv = rd(ram, at + 0x78) & MASK;
					for (o, v) in std::mem::take(&mut s.backup) {
						put_word(ram, pv + o, v);
					}
				}
				if s.got.is_empty() {
					self.shown.remove(&id);
				}
				continue;
			};
			let ram = &mut c.mem.ram[..];
			let pv = rd(ram, moby + 0x78) & MASK;
			if pv == 0 || self.at.get(&id).is_some_and(|&a| a != moby) {
				continue;
			}
			self.at.insert(id, moby);
			// An address there that is a number here: the callbacks are not put on (they may follow it).
			let mut safe = true;
			for (k, &v) in d.vars.iter().enumerate() {
				let o = 4 * k as u32;
				let here = rd(ram, pv + o);
				match (pointer_like(v), pointer_like(here)) {
					(false, false) => {
						s.backup.entry(o).or_insert(here);
						put_word(ram, pv + o, v);
					}
					(true, false) => safe = false,
					_ => {}
				}
			}
			if !safe {
				continue;
			}
			for call in &d.calls {
				let Some(q) = self.queues.get((call.queue & 0x7f) as usize) else { continue };
				let code: Vec<u32> = (0..rac_mp::proto::DRAW_CODE_WORDS as u32).map(|k| rd(ram, call.func + 4 * k)).collect();
				if DrawCall::code_of(&code) != call.code {
					continue;
				}
				let arg = if call.queue & 0x80 != 0 { pv } else { moby };
				if first(call.func, 1) {
					eprintln!("mp: another game's enemy (class {}) is drawn here with {:#x}", rd(ram, moby + 0xa4) >> 16, call.func);
				}
				due.push((q.count, q.fns, q.args, call.func, arg));
			}
		}
		FIELD.store(now, Ordering::Relaxed);
		*DUE.lock().unwrap_or_else(|e| e.into_inner()) = (now, due);
	}

	/// The level is gone, or this game no longer shows anything: nothing is put back (the copies are gone with it).
	pub(super) fn forget(&mut self) {
		self.level = -1;
		self.queues.clear();
		self.shown.clear();
		self.at.clear();
		ON.store(false, Ordering::Relaxed);
		DUE.lock().unwrap_or_else(|e| e.into_inner()).1.clear();
	}
}

/// In the game's own frame (the hero's update, scenes.rs): the callbacks due this field go on their queues, once a field.
pub(super) fn in_frame(c: &mut Cpu) {
	let mut due = DUE.lock().unwrap_or_else(|e| e.into_inner());
	if due.1.is_empty() || due.0 != FIELD.load(Ordering::Relaxed) {
		return;
	}
	for (count, fns, args, func, arg) in std::mem::take(&mut due.1) {
		let n = rd(&c.mem.ram, count);
		if n >= QUEUE_MAX {
			continue;
		}
		c.mem.wr32(fns + 4 * n, func);
		c.mem.wr32(args + 4 * n, arg);
		c.mem.wr32(count, n + 1);
		SHOWN.fetch_add(1, Ordering::Relaxed);
	}
}
