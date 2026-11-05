//! Switches, levers, targets and whatever else of the level a player's blow sets off. Each game has its own of them, so a lever pulled
//! in one game would stay up in the others and the door behind it shut. A hand at the game's `get_hit`, where every object reads the
//! hit pending on it, sees a placed object that is no enemy and no crate (those are shared their own way) take a blow from a player
//! (not from an enemy), and the hit goes to every other game, which posts the same hit on its own: the object does there what it did
//! here. A hit posted here for another game is not sent back. An object far from the local hero doesn't update, so it doesn't read
//! the hit; the hit waits for it (`keep_up`), posted again whenever the queue has gone round over it, so a lever another player pulled
//! is pulled when this player gets there.

use super::enemies::{hit_ring, HIT_ENTRY};
use super::MASK;
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Class types that are shared otherwise: the hero, enemies, crates.
const HERO: i16 = 1;
const ENEMY: i16 = 5;
const CRATE: i16 = 9;
/// A hit posted here for another game is not sent back for this many fields.
const OURS_FOR: u64 = 60;

static SPEC: OnceLock<&'static Reloc> = OnceLock::new();

fn spec() -> &'static Reloc {
	SPEC.get().expect("touch hand without a spec")
}

/// Others are here, in play; the placed objects and their ids are `PLACED` (address to id, mp/ids.rs).
static ACTIVE: AtomicBool = AtomicBool::new(false);
static PLACED: Mutex<Option<std::sync::Arc<std::collections::HashMap<u32, u16>>>> = Mutex::new(None);
static FIELD: AtomicU64 = AtomicU64::new(0);
/// Blows caught (slot, class, damage, mask), the queue entry each was last caught at, and the objects hit here for other games.
static CAUGHT: Mutex<Vec<(u16, u16, f32, u32)>> = Mutex::new(Vec::new());
static SEEN: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());
static POSTED: Mutex<Vec<(u32, u64)>> = Mutex::new(Vec::new());
/// Other games' blows not read yet: the object, its class, damage, mask.
static WAITING: Mutex<Vec<(u32, u16, f32, u32)>> = Mutex::new(Vec::new());
/// Blows sent and taken, for the run log.
pub static SENT: AtomicU64 = AtomicU64::new(0);
pub static TAKEN: AtomicU64 = AtomicU64::new(0);

fn word(ram: &[u8], a: u32) -> u32 {
	let a = (a & MASK) as usize;
	u32::from_le_bytes([ram[a], ram[a + 1], ram[a + 2], ram[a + 3]])
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The class type of the moby at `moby` (class record +0x46; -1 is a type too: Novalis' breakable classes 1042 and 1813 have it),
/// or None without a class record.
fn kind(ram: &[u8], moby: u32) -> Option<i16> {
	let h = word(ram, moby + 0x24) & MASK;
	(h != 0).then(|| (word(ram, h + 0x44) >> 16) as i16)
}

fn get_hit_hand(c: &mut Cpu) -> bool {
	if !ACTIVE.load(Ordering::Relaxed) {
		return false;
	}
	let Some((queue, _)) = hit_ring(c) else { return false };
	let ram = &c.mem.ram;
	let moby = c.g(4) as u32 & MASK;
	let Some(id) = lock(&PLACED).as_ref().and_then(|p| p.get(&moby).copied()) else { return false };
	if matches!(kind(ram, moby), None | Some(HERO | ENEMY | CRATE)) {
		return false;
	}
	let held = ram[((moby + 0xa4) & MASK) as usize];
	if held == 0xff {
		return false;
	}
	let e = queue + held as u32 * HIT_ENTRY;
	let attacker = word(ram, e + 0x20) & MASK;
	if word(ram, e + 0x34) & MASK != moby || attacker == 0 || kind(ram, attacker) == Some(ENEMY) {
		return false;
	}
	// Read more than once a tick: once per hit.
	let stamp = e ^ (word(ram, e + 0x2c) << 8) ^ attacker;
	if lock(&SEEN).insert(moby, stamp) == Some(stamp) {
		return false;
	}
	let now = FIELD.load(Ordering::Relaxed);
	if lock(&POSTED).iter().any(|&(m, f)| m == moby && now.saturating_sub(f) < OURS_FOR) {
		return false;
	}
	let class = (word(ram, moby + 0xa4) >> 16) as u16;
	lock(&CAUGHT).push((id, class, f32::from_bits(word(ram, e + 0x2c)), word(ram, e + 0x24)));
	false
}

/// Each field: the blows still waiting. One the object read is done (its queue byte is clear again); one the queue went round over is
/// posted again, a few a field; one whose object is gone, or another object now, is dropped.
pub fn keep_up(c: &mut Cpu, hero: u32, budget: usize) {
	let Some((queue, _)) = hit_ring(c) else { return };
	let mut again = Vec::new();
	lock(&WAITING).retain(|&(moby, class, damage, mask)| {
		let ram = &c.mem.ram;
		if word(ram, moby + 0x24) == 0 || ram[((moby + 0x20) & MASK) as usize] >= 0x80 || (word(ram, moby + 0xa4) >> 16) as u16 != class {
			return false;
		}
		let held = ram[((moby + 0xa4) & MASK) as usize];
		if held == 0xff {
			return false;
		}
		if word(ram, queue + held as u32 * HIT_ENTRY + 0x34) & MASK != moby && again.len() < budget {
			again.push((moby, damage, mask));
		}
		true
	});
	for (moby, damage, mask) in again {
		lock(&POSTED).push((moby, FIELD.load(Ordering::Relaxed)));
		let _ = super::enemies::post_hit(c, moby, hero, damage, mask);
	}
}

/// Another level: nothing waits any more.
pub fn forget() {
	lock(&WAITING).clear();
}

/// Registers the hand. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	if let Some((_, r)) = crate::omni::hook_specs(|id| id == "rac1.combat.get_hit").into_iter().next() {
		let _ = SPEC.set(r);
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: "rac1.combat.get_hit", reloc: spec, est: ee_oracle::hs::EXACT, f: get_hit_hand })));
		eprintln!("mp: a hand for blows on the level's objects");
	}
}

/// Each field: whether anyone else is here in play, and where the placed objects are.
pub fn set_scene(active: bool, placed: std::sync::Arc<std::collections::HashMap<u32, u16>>, field: u64) {
	ACTIVE.store(active, Ordering::Relaxed);
	*lock(&PLACED) = Some(placed);
	FIELD.store(field, Ordering::Relaxed);
	lock(&POSTED).retain(|(_, f)| field.saturating_sub(*f) < OURS_FOR);
	if !active {
		lock(&CAUGHT).clear();
	}
}

/// The blows caught since the last call: (slot, class, damage, mask).
pub fn take() -> Vec<(u16, u16, f32, u32)> {
	let v = std::mem::take(&mut *lock(&CAUGHT));
	SENT.fetch_add(v.len() as u64, Ordering::Relaxed);
	v
}

/// Another game's blow on the placed object at `moby`, posted here from the local hero.
pub fn take_blow(c: &mut Cpu, moby: u32, hero: u32, damage: f32, mask: u32) -> Result<(), String> {
	lock(&POSTED).push((moby, FIELD.load(Ordering::Relaxed)));
	TAKEN.fetch_add(1, Ordering::Relaxed);
	let class = (word(&c.mem.ram, moby + 0xa4) >> 16) as u16;
	lock(&WAITING).push((moby, class, damage, mask));
	super::enemies::post_hit(c, moby, hero, damage, mask)
}
