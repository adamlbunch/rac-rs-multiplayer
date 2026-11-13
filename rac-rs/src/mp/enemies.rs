//! Enemies are synced by giving each one a single authority: it runs in the game of the player nearest to it, as the host decides.
//! That game's AI, attacks and collision are the game's own, against a hero who is really there. Every other game mirrors it: the
//! hands at the entry of every class's update function (found by code like the mods' hook points, the specs of
//! `rac1.moby.*.update` in omni/rac1_reloc.txt) skip the update of an enemy another game runs and put it where that game says
//! instead, with its rotation, animation and health. When the nearest player changes, the new owner's game simply lets the
//! update run again from there, and the enemy goes for that player.
//!
//! A blow from a game that does not run the enemy would land on a copy nobody updates: `seg_hand` tests a weapon's segment against
//! where the mirrored enemies are, `hit_hand` catches a hit posted on one, and either way it travels to the owner (`take_hits`),
//! where `post_hit` posts it on the real one.
//!
//! The hands exist only with RAC_MP, and do nothing but one atomic load while no enemy is mirrored. A hook point a mod has taken
//! stays the mod's (`omni::hook_specs`): that class then runs in every game.
//!
//! A mod that scores blows (Arsenal Mastery takes `damage.hit_victim` and `damage.resolve`: XP for the weapon, the blow scaled by its
//! level and the enemy rebalance) gets this port's hand over its own at both (`omni::taken_hook`), which passes it every call it does
//! not take. A blow of this game's player on a mirrored enemy is scored HERE, for him (`resolve_here`), and what it came to goes to
//! the owner; a hit this port posts for another game (that blow, a finishing one) runs the game's own code there, so no mod scores
//! it for the player who didn't deal it (`resolve_hand`).
//!
//! Threads: the hands run on the game thread "ee" during a field; everything else on the worker between fields. Never concurrent.

use crate::fmath::Fm;
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use rac_mp::AnimClock;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The cell that holds the hero's moby (retail/snapshot.rs reads the same).
const HERO_MOBY: u32 = 0x14_13D0;
const MASK: u32 = 0x01ff_ffff;

const UPDATE_PREFIX: &str = "rac1.moby.";
const UPDATE_SUFFIX: &str = ".update";
/// The specs of omni/rac1_funcs.txt for enemy updates that have no hook point (`mp_enemy_update_<class>`).
const MP_UPDATE_PREFIX: &str = "mp_enemy_update_";
const HIT_VICTIM: &str = "rac1.omni.damage.hit_victim";
/// Where an object reads the hit pending on it (`resolve(victim, entry, record, 0, &status, &strength, &class, 4)`, after `get_hit`).
const RESOLVE: &str = "rac1.omni.damage.resolve";
const SEGMENT: &str = "rac1.omni.collide.segment";
/// The class type of an enemy (class record +0x46).
const ENEMY: i16 = 5;
/// The game's hit queue has 64 entries of 0x40 bytes.
pub(super) const HIT_ENTRY: u32 = 0x40;
/// `hit_ring_write` (omni/rac1_funcs.txt, NTSC number): its code says where the hit queue is.
const HIT_RING_WRITE: u32 = 0x25_9698;
/// The hit queue and its index word as last read from code, with the code words that said so (address, word).
static RING: Mutex<Option<([(u32, u32); 4], u32, u32)>> = Mutex::new(None);
/// After a miss (the level's code not in yet), the next look at the code (a scan of it) waits this long.
const RING_RETRY: std::time::Duration = std::time::Duration::from_millis(500);
static RING_MISSED: Mutex<Option<std::time::Instant>> = Mutex::new(None);
/// The flag (+0x34) that lets weapons hit a moby.
const HITTABLE: u32 = 0x4000;
/// The least health that is health.
const HEALTH_LEAST: f32 = 1e-3;
/// A weapon's segment within this of an enemy's middle touches it (metres), and the middle is this far above its feet.
const HIT_REACH: f32 = 1.3;
const HIT_LIFT: f32 = 0.5;
/// One swing sweeps several segments: fields before the same enemy can be hit again.
const HIT_EVERY: u64 = 20;

/// Where another game has an enemy, for the update it skips here.
#[derive(Clone, Copy)]
pub struct Driven {
	pub pos: [f32; 3],
	pub rot: [f32; 3],
	pub anim: AnimClock,
	pub health: f32,
	pub scale: f32,
	/// How opaque it is drawn (+0x23), and the colour it is drawn in instead of its own (+0x90, bit 0x10 of +0x34): None leaves
	/// both as they are (machinery), `Some(None)` clears the override.
	pub look: Option<(u8, Option<u32>)>,
}

/// A hit that landed on a mirrored enemy here.
#[derive(Clone, Copy)]
pub struct Hit {
	pub moby: u32,
	pub damage: f32,
	pub mask: u32,
	/// What dealt it (the hero, his wrench's or a shot's moby).
	pub attacker: u32,
}

/// The enemies mirrored this field, by moby address.
static DRIVEN: Mutex<BTreeMap<u32, Driven>> = Mutex::new(BTreeMap::new());
/// `DRIVEN` holds something: without, the hands are one atomic load.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static HITS: Mutex<Vec<Hit>> = Mutex::new(Vec::new());
/// Fields so far, and the field each mirrored enemy was last hit.
static FIELD: AtomicU64 = AtomicU64::new(0);
static LAST_HIT: Mutex<BTreeMap<u32, u64>> = Mutex::new(BTreeMap::new());
/// Updates skipped and hits sent on, for the run log.
pub static MIRRORED: AtomicU64 = AtomicU64::new(0);
pub static FORWARDED: AtomicU64 = AtomicU64::new(0);
/// Blows on mirrored enemies a mod scored here (`resolve_here`), for the run log.
pub static SCORED: AtomicU64 = AtomicU64::new(0);

/// The hands of the mods that took the hit points (`omni::taken_hook`), under this port's, and the resolver's spec.
static MOD_HIT: OnceLock<fn(&mut Cpu) -> bool> = OnceLock::new();
static MOD_RESOLVE: OnceLock<fn(&mut Cpu) -> bool> = OnceLock::new();
/// The mod's hand runs for a blast on a mirrored enemy (`hit_hand`): the call that comes back from it is the one that goes.
static IN_MOD_HIT: AtomicBool = AtomicBool::new(false);
static RESOLVE_SPEC: OnceLock<&'static Reloc> = OnceLock::new();
/// Where the resolver is in the level loaded now, and when it was last looked for in vain.
static RESOLVER: Mutex<(Option<u32>, Option<std::time::Instant>)> = Mutex::new((None, None));
/// Hits this port posted (entry, victim, attacker, strength bits), which the resolver runs without the mod; the newest of them.
static POSTED: Mutex<Vec<(u32, u32, u32, u32)>> = Mutex::new(Vec::new());
const POSTED_KEEP: usize = 64;
/// The object has a damage record (flag of +0x34): its pvars' first word points at it (`target_component`), health first.
const HAS_RECORD: u32 = 0x20;

static SPECS: OnceLock<Vec<&'static Reloc>> = OnceLock::new();

fn reloc_n<const N: usize>() -> &'static Reloc {
	SPECS.get().expect("an enemy hand registered without a spec")[N]
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
	let mut specs = crate::omni::hook_specs(|id| (id.starts_with(UPDATE_PREFIX) && id.ends_with(UPDATE_SUFFIX)) || id == HIT_VICTIM || id == SEGMENT);
	// The hit points a mod has taken (Arsenal Mastery): this port's hand over the mod's, first, so the cap never drops them.
	for id in [HIT_VICTIM, RESOLVE] {
		if let Some((reloc, f)) = crate::omni::taken_hook(id) {
			specs.insert(0, (id.to_string(), reloc));
			if id == HIT_VICTIM {
				let _ = MOD_HIT.set(f);
			} else {
				let _ = (MOD_RESOLVE.set(f), RESOLVE_SPEC.set(reloc));
			}
		}
	}
	// The enemy updates no mod hook point names (16 classes on 12 levels ran in every game before v0.4.1.9): this port's own specs.
	specs.extend(crate::omni::func_relocs(MP_UPDATE_PREFIX));
	let specs = &specs[..specs.len().min(MAX_HANDS)];
	let _ = SPECS.set(specs.iter().map(|(_, r)| *r).collect());
	for (i, (id, _)) in specs.iter().enumerate() {
		let f: fn(&mut Cpu) -> bool = match id.as_str() {
			HIT_VICTIM => hit_hand,
			SEGMENT => seg_hand,
			RESOLVE => resolve_hand,
			_ => update_hand,
		};
		let hand: &'static RelocHand = Box::leak(Box::new(RelocHand { name: id.clone().leak(), reloc: RELOCS[i], est: ee_oracle::hs::EXACT, f }));
		ee_oracle::rec::register_reloc_hand(hand);
	}
	crate::omni::allow_hook_hands();
	eprintln!("mp: {} hook points for synced enemies", specs.len());
	if MOD_HIT.get().is_some() || MOD_RESOLVE.get().is_some() {
		eprintln!("mp: a mod scores blows: hits on enemies another game runs are scored here, for this player, and not there");
	}
}

/// The worker, each field: which enemies another game runs, and where they are.
pub fn set_driven(d: BTreeMap<u32, Driven>) {
	FIELD.fetch_add(1, Ordering::Relaxed);
	ACTIVE.store(!d.is_empty(), Ordering::Relaxed);
	*DRIVEN.lock().unwrap_or_else(|e| e.into_inner()) = d;
}

/// The enemy at `moby` is no longer mirrored, from now: its update runs, and a hit on it lands here.
pub fn release(moby: u32) {
	DRIVEN.lock().unwrap_or_else(|e| e.into_inner()).remove(&moby);
}

/// The worker, each field: the hits that landed on mirrored enemies since the last one.
pub fn take_hits() -> Vec<Hit> {
	std::mem::take(&mut *HITS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn word(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_v3(ram: &[u8], a: u32) -> [f32; 3] {
	[0, 4, 8].map(|k| f32::from_bits(word(ram, a + k)))
}

fn put_word(ram: &mut [u8], a: u32, v: u32) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

fn put_v3(ram: &mut [u8], a: u32, v: [f32; 3]) {
	for (k, x) in v.iter().enumerate() {
		put_word(ram, a + k as u32 * 4, x.to_bits());
	}
}

/// The length of the row at `a` (the rows carry the moby's scale).
fn row_len(ram: &[u8], a: u32) -> f32 {
	let v = [0, 4, 8].map(|k| f32::from_bits(word(ram, a + k)));
	(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The basis rows of Euler angles `r`, each as long as the row it replaces (retail/moby_state.rs `euler_from_rows` is the way back).
fn put_rows(ram: &mut [u8], moby: u32, r: [f32; 3]) {
	let (sx, cx, sy, cy, sz, cz) = (r[0].fsin(), r[0].fcos(), r[1].fsin(), r[1].fcos(), r[2].fsin(), r[2].fcos());
	let rows = [[cy * cz, cy * sz, -sy], [sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy], [cx * sy * cz + sx * sz, cx * sy * sz - sx * cz, cx * cy]];
	for (k, row) in rows.iter().enumerate() {
		let a = moby + 0xc0 + k as u32 * 0x10;
		let l = row_len(ram, a);
		put_v3(ram, a, row.map(|x| x * l));
	}
}

/// Puts the moby at `moby` where its owner has it, its health too.
pub fn apply(ram: &mut [u8], moby: u32, d: &Driven) {
	place(ram, moby, d);
	set_health(ram, moby, d.health);
}

/// Puts the moby at `moby` where another game has it: position, scale, rotation and animation.
pub fn place(ram: &mut [u8], moby: u32, d: &Driven) {
	put_v3(ram, moby + 0x10, d.pos);
	put_word(ram, moby + 0x2c, d.scale.to_bits());
	put_v3(ram, moby + 0x40, d.rot);
	put_rows(ram, moby, d.rot);
	let a = &d.anim;
	// A class can keep its own numbers in these bytes, which it can because its update says what they mean. The game's step after an
	// update, here with no update before it, would take them for sequences.
	if has_seq(ram, moby, a.old_seq) && has_seq(ram, moby, a.new_seq) {
		put_word(ram, moby + 0x50, u32::from_le_bytes([a.cur, a.next, a.old_seq, a.new_seq]));
		put_v3(ram, moby + 0x54, [a.p, a.m, a.r]);
	}
	// Its fade and a hit's flash, as its game draws it.
	if let Some((alpha, ovr)) = d.look {
		let i = (moby & MASK) as usize;
		if let Some(b) = ram.get_mut(i + 0x23) {
			*b = alpha;
		}
		let flags = word(ram, moby + 0x34);
		match ovr {
			Some(c) => {
				put_word(ram, moby + 0x90, c);
				put_word(ram, moby + 0x34, flags | OVERRIDE);
			}
			None if flags & OVERRIDE != 0 => put_word(ram, moby + 0x34, flags & !OVERRIDE),
			None => {}
		}
	}
}

/// The flag (+0x34) that has the game draw a moby in the colour of +0x90.
const OVERRIDE: u32 = 0x10;

fn back_to_caller(c: &mut Cpu, ra: u32) {
	c.pc = ra;
	c.npc = ra.wrapping_add(4);
	c.mem.now = c.icount;
}

/// At a class's update: an enemy another game runs is put where that game has it, and its update does not run.
fn update_hand(c: &mut Cpu) -> bool {
	if !ACTIVE.load(Ordering::Relaxed) {
		return false;
	}
	let moby = c.g(4) as u32 & MASK;
	let Some(d) = DRIVEN.lock().unwrap_or_else(|e| e.into_inner()).get(&moby).copied() else { return false };
	apply(&mut c.mem.ram, moby, &d);
	MIRRORED.fetch_add(1, Ordering::Relaxed);
	let ra = c.g(31) as u32;
	back_to_caller(c, ra);
	true
}

/// `seg_collide` with a hit descriptor is a blow on its way: a wrench's sweep or a shot. The game's own test looks through its collision
/// grid, which an enemy whose update is skipped here has left, so it is tested against where the mirrored enemies really are, and a
/// hit goes to the game that runs the enemy. The call itself then runs as it is.
fn seg_hand(c: &mut Cpu) -> bool {
	let desc = c.g(8) as u32 & MASK;
	if !ACTIVE.load(Ordering::Relaxed) || desc == 0 {
		return false;
	}
	let ram = &c.mem.ram;
	let attacker = word(ram, desc + 0x10) & MASK;
	let class = word(ram, attacker + 0x24) & MASK;
	// An enemy's own attack is no blow on another enemy.
	if attacker == 0 || class == 0 || (word(ram, class + 0x44) >> 16) as i16 == ENEMY {
		return false;
	}
	let (a, b) = (read_v3(ram, c.g(4) as u32), read_v3(ram, c.g(5) as u32));
	let (mask, damage) = (word(ram, desc + 0x14), f32::from_bits(word(ram, desc + 0x1c)));
	let now = FIELD.load(Ordering::Relaxed);
	let mut last = LAST_HIT.lock().unwrap_or_else(|e| e.into_inner());
	for (moby, d) in DRIVEN.lock().unwrap_or_else(|e| e.into_inner()).iter() {
		let mid = [d.pos[0], d.pos[1], d.pos[2] + HIT_LIFT];
		let due = last.get(moby).is_none_or(|t| now.saturating_sub(*t) >= HIT_EVERY);
		if due && seg_dist2(a, b, mid) < HIT_REACH * HIT_REACH {
			last.insert(*moby, now);
			HITS.lock().unwrap_or_else(|e| e.into_inner()).push(Hit { moby: *moby, damage, mask, attacker });
			FORWARDED.fetch_add(1, Ordering::Relaxed);
		}
	}
	false
}

/// The squared distance from `p` to the segment `a`..`b`.
fn seg_dist2(a: [f32; 3], b: [f32; 3], p: [f32; 3]) -> f32 {
	let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
	let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
	let len = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
	let t = if len > 0.0 { ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / len).clamp(0.0, 1.0) } else { 0.0 };
	let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t, ap[2] - ab[2] * t];
	d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// `hit_ring_write` is how a blast's hit lands: one on a mirrored enemy goes the same way. Every other call goes to the mod that took
/// it, if one did; one on a mirrored enemy goes to it first too (Arsenal Mastery's perks change the blow there), and its way on to
/// the game's code comes back here and goes.
fn hit_hand(c: &mut Cpu) -> bool {
	if !ACTIVE.load(Ordering::Relaxed) {
		return to_mod(&MOD_HIT, c);
	}
	let (moby, desc) = (c.g(4) as u32 & MASK, c.g(5) as u32 & MASK);
	if !DRIVEN.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&moby) {
		return to_mod(&MOD_HIT, c);
	}
	if MOD_HIT.get().is_some() && !IN_MOD_HIT.swap(true, Ordering::Relaxed) {
		let done = to_mod(&MOD_HIT, c);
		IN_MOD_HIT.store(false, Ordering::Relaxed);
		if done {
			return true;
		}
	}
	let ram = &c.mem.ram;
	let (attacker, mask, damage) = (word(ram, desc + 0x10) & MASK, word(ram, desc + 0x14), f32::from_bits(word(ram, desc + 0x1c)));
	HITS.lock().unwrap_or_else(|e| e.into_inner()).push(Hit { moby, damage, mask, attacker });
	FORWARDED.fetch_add(1, Ordering::Relaxed);
	let ra = c.g(31) as u32;
	back_to_caller(c, ra);
	true
}

/// The hand of the mod under this port's at a hit point (false without one: the game's own code runs). Its own `original` comes back
/// to this port's hand, and from there to the mod's again, whose guard sends it on to the game's code.
fn to_mod(hand: &OnceLock<fn(&mut Cpu) -> bool>, c: &mut Cpu) -> bool {
	hand.get().is_some_and(|f| f(c))
}

/// `damage.resolve` when a mod has taken it: a hit this port posted (another player's blow, scored in his game; a finishing blow)
/// runs the game's own code, so the mod here neither scores it for this player nor scales it twice. Every other call goes to the mod.
fn resolve_hand(c: &mut Cpu) -> bool {
	let (victim, entry) = (c.g(4) as u32 & MASK, c.g(5) as u32 & MASK);
	if entry != 0 && posted_here(&c.mem.ram, victim, entry) {
		return false;
	}
	to_mod(&MOD_RESOLVE, c)
}

/// The hit at `entry` on `victim` is one this port posted, still as it was: it is taken off the list.
fn posted_here(ram: &[u8], victim: u32, entry: u32) -> bool {
	let mut p = POSTED.lock().unwrap_or_else(|e| e.into_inner());
	if p.is_empty() {
		return false;
	}
	let now = (entry, victim, word(ram, entry + 0x20) & MASK, word(ram, entry + 0x2c));
	let Some(i) = p.iter().position(|x| *x == now) else { return false };
	p.remove(i);
	true
}

/// Where the resolver starts in the code loaded now (looked for again after a level's code changed it, at most twice a second).
fn resolver(c: &Cpu) -> Option<u32> {
	let reloc = *RESOLVE_SPEC.get()?;
	let mut g = RESOLVER.lock().unwrap_or_else(|e| e.into_inner());
	if let Some(e) = g.0.filter(|&e| crate::omni::entry_holds(c, reloc, e)) {
		return Some(e);
	}
	if g.1.is_some_and(|t| t.elapsed() < RING_RETRY) {
		return None;
	}
	let found = crate::omni::find_entry(c, reloc);
	*g = (found, found.is_none().then(std::time::Instant::now));
	found
}

/// A blow of this game's player on an enemy another game runs, scored here by the mod that took `damage.resolve`: Arsenal Mastery gives
/// the XP to this player's weapon and scales the blow by its level and the enemy rebalance. The hit is posted on the copy and resolved
/// by the game's own function between two fields, as the enemy's update would (the copy then forgets it; its owner's game takes the
/// health off). The answer is the strength it came to, for that game. None without such a mod, or when the copy can't take a hit:
/// the blow then goes as it was dealt.
pub fn resolve_here(c: &mut Cpu, h: &Hit) -> Option<f32> {
	MOD_RESOLVE.get()?;
	let ram = &c.mem.ram[..];
	if word(ram, h.moby + 0x34) & HAS_RECORD == 0 {
		return None;
	}
	let record = word(ram, word(ram, h.moby + 0x78) & MASK) & MASK;
	if record == 0 {
		return None;
	}
	let entry = resolver(c)?;
	let e = post(c, h.moby, h.attacker, h.damage, h.mask).ok()??;
	// Its status, strength and attacker class come back below the game's stack (the resolver's own frame is under `sp`).
	let sp = (c.g(29) as u32).wrapping_sub(0x100) & !0xf;
	let out = sp + 0x10;
	for k in 0..3 {
		put_word(&mut c.mem.ram, out + k * 4, 0);
	}
	let (moby, e) = (h.moby, e);
	let r = crate::omni::run_game(c, entry, |c| {
		for (reg, v) in [(4, moby), (5, e), (6, record), (7, 0), (8, out), (9, out + 4), (10, out + 8), (11, 4), (29, sp)] {
			c.s(reg, v as u64);
		}
	});
	if let Some(b) = c.mem.ram.get_mut(((moby + 0xa4) & MASK) as usize) {
		*b = 0xff;
	}
	if let Err(why) = r {
		eprintln!("mp: a blow on a mirrored enemy could not be scored here: {why}");
		return None;
	}
	SCORED.fetch_add(1, Ordering::Relaxed);
	Some(f32::from_bits(word(&c.mem.ram, out + 4)))
}

/// The moby's class has this animation sequence: the class record's list (+0x48) holds a pointer for it.
fn has_seq(ram: &[u8], moby: u32, seq: u8) -> bool {
	let p = word(ram, (word(ram, moby + 0x24) & MASK) + 0x48 + seq as u32 * 4);
	p != 0 && p & !MASK == 0
}

/// A stand-in's update distance (+0x30) and draw distance (+0x32), which the class's first update would have set.
pub fn show(ram: &mut [u8], moby: u32) {
	put_word(ram, moby + 0x30, word(ram, moby + 0x30) & 0xff00 | 0xff | 0x80 << 16);
}

/// The update pointer of the moby at `moby` to nothing, and what it was: the game no longer updates it. For a spawned object that
/// only mirrors another game's (its class's first update, which would set it up as a live one, doesn't run), and for a copy that
/// stood down.
pub fn no_update(ram: &mut [u8], moby: u32) -> u32 {
	let update = word(ram, moby + 0x74);
	set_update(ram, moby, 0);
	update
}

pub fn set_update(ram: &mut [u8], moby: u32, update: u32) {
	put_word(ram, moby + 0x74, update);
}

/// Takes the moby at `moby` off the picture and out of reach, and says what its flags (+0x34) were: the hide and hittable flags, and
/// the draw byte (+0x31).
pub fn hide(ram: &mut [u8], moby: u32) -> u32 {
	let flags = word(ram, moby + 0x34);
	put_word(ram, moby + 0x34, flags & !HITTABLE | 1);
	put_word(ram, moby + 0x30, word(ram, moby + 0x30) & !0xff00);
	flags
}

/// Whether the moby at `moby` is still one this game made of `class` and keeps without an update (a stand-in, a voice). The game can
/// take such a moby away and give its memory to another (a bomb over the limit, a respawn): a pose written into it then lands in
/// the game's free list or someone else's moby, and broke the game.
pub fn still_ours(ram: &[u8], moby: u32, class: u16) -> bool {
	let state = ram.get((moby & MASK) as usize + 0x20).copied().unwrap_or(0xff);
	word(ram, moby + 0x24) != 0 && state < 0x80 && (word(ram, moby + 0xa4) >> 16) as u16 == class && word(ram, moby + 0x74) == 0
}

/// Back from `no_update` and `hide`. The game sets the draw byte itself.
pub fn wake(ram: &mut [u8], moby: u32, update: u32, flags: u32) {
	set_update(ram, moby, update);
	put_word(ram, moby + 0x34, flags);
}

/// The enemy's health, at the usual place in its pvars. A class can keep something else there: a pointer reads as a tiny number, and
/// one from another game means nothing here.
pub fn set_health(ram: &mut [u8], moby: u32, health: f32) {
	let pvar = word(ram, moby + 0x78) & MASK;
	if pvar != 0 && (health == 0.0 || health.abs() >= HEALTH_LEAST) {
		put_word(ram, pvar + 0x20, health.to_bits());
	}
}

/// The game's hit queue and the word with its next entry's index in the level loaded now, read from `hit_ring_write`'s own code:
/// `lui v0 / addiu v0, v0` give the block whose +0x14 is the index, `lui s0 / addiu a0, s0` the queue (USA Veldin: 0x173e40 and
/// 0x178100). They are the level overlay's data, linked into each level apart, and neither disc keeps them at one place: on USA
/// Novalis the queue is at 0x178580, on Kerwan 0x178200 (a hit written at Veldin's place there was never read).
pub fn hit_ring(c: &Cpu) -> Option<(u32, u32)> {
	let ram = &c.mem.ram[..];
	let mut g = RING.lock().unwrap_or_else(|e| e.into_inner());
	if let Some((said, queue, index)) = *g {
		if said.iter().all(|&(a, w)| word(ram, a) == w) {
			return Some((queue, index));
		}
	}
	*g = None;
	let mut missed = RING_MISSED.lock().unwrap_or_else(|e| e.into_inner());
	if missed.is_some_and(|t| t.elapsed() < RING_RETRY) {
		return None;
	}
	*missed = Some(std::time::Instant::now());
	let entry = crate::omni::game_entry(c, HIT_RING_WRITE)?;
	let at = |op: u32| (entry..entry + 0x80).step_by(4).find(|&a| word(ram, a) >> 16 == op);
	let said = [at(0x3c02)?, at(0x2442)?, at(0x3c10)?, at(0x2604)?];
	let [lui_v0, block_lo, lui_s0, queue_lo] = said.map(|a| word(ram, a));
	let lo = |w: u32| (w & 0xffff) as i16 as i32 as u32;
	let index = (lui_v0 << 16).wrapping_add(lo(block_lo)) + 0x14;
	let queue = (lui_s0 << 16).wrapping_add(lo(queue_lo));
	*missed = None;
	*g = Some((said.map(|a| (a, word(ram, a))), queue, index));
	eprintln!("mp: the hit queue is at {queue:#x}, its next index at {index:#x}");
	Some((queue, index))
}

/// The hero's moby.
pub fn hero_moby(ram: &[u8]) -> u32 {
	word(ram, HERO_MOBY) & MASK
}

/// A hit on the moby at `moby`, from `attacker`, posted as the game's `hit_ring_write` does it (USA Veldin 0x259a88, the same code on every
/// level): into the next of the 64 entries of the hit queue, unless the moby already holds a stronger hit this tick. Only memory is
/// written, so it is safe at any time; the victim reads the hit at its next update. A call into the game here could stop at a frame
/// wait halfway between two fields and leave the game broken.
pub fn post_hit(c: &mut Cpu, moby: u32, attacker: u32, damage: f32, mask: u32) -> Result<(), String> {
	let Some(e) = post(c, moby, attacker, damage, mask)? else { return Ok(()) };
	// A mod that scores blows doesn't score this one (`resolve_hand`).
	if MOD_RESOLVE.get().is_some() {
		let mut p = POSTED.lock().unwrap_or_else(|e| e.into_inner());
		p.retain(|x| x.0 != e);
		if p.len() >= POSTED_KEEP {
			p.remove(0);
		}
		p.push((e, moby & MASK, attacker & MASK, damage.to_bits()));
	}
	Ok(())
}

/// `post_hit`'s writes: the entry the hit went to, or None when the moby holds a stronger one already.
fn post(c: &mut Cpu, moby: u32, attacker: u32, damage: f32, mask: u32) -> Result<Option<u32>, String> {
	let (queue, index_word) = hit_ring(c).ok_or("no hit queue found in this level's code")?;
	let ram = &mut c.mem.ram[..];
	let held = ram[((moby + 0xa4) & MASK) as usize];
	if held != 0xff {
		let e = queue + held as u32 * HIT_ENTRY;
		if word(ram, e + 0x34) & MASK == moby && damage < f32::from_bits(word(ram, e + 0x2c)) {
			return Ok(None);
		}
	}
	let index = word(ram, index_word) & 63;
	let e = queue + index * HIT_ENTRY;
	// The cleared head and the direction (none), then the descriptor's fields.
	for k in 0..8 {
		put_word(ram, e + k * 4, 0);
	}
	for (at, v) in [(0x20, attacker), (0x24, mask), (0x28, 0), (0x2c, damage.to_bits()), (0x30, 1), (0x34, moby), (0x38, 0)] {
		put_word(ram, e + at, v);
	}
	ram[((moby + 0xa4) & MASK) as usize] = index as u8;
	put_word(ram, index_word, (index + 1) & 63);
	Ok(Some(e))
}
