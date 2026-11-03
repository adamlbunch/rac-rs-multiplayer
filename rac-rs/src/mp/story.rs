//! The team's progress. A game made for two keeps one story: what a player is given (a gadget from a quest, a weapon found) the
//! other has too, a planet one of them unlocked is open to both, and a gold bolt one of them collected counts for both. Hands at the
//! game's own `item.give` and `planet.unlock` (the mods' hook points, omni/rac1_reloc.txt) see the local game do it, and its gold
//! bytes (0x14bec0, 4 a level) are watched in play; the session takes it to every other game, and a late arrival gets all of it.
//! Each game then does the same, in play, one a field, unless it has it already: the same function for an item it doesn't own (its
//! byte at 0x13d4c0) or a planet whose unlock byte (0x13dd40 + level) is clear, the byte for a gold bolt. What a player buys at a
//! vendor stays his.

use super::{disc, Mp, MODE_WORD};
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use rac_mp::Progress;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const GIVE: &str = "rac1.item.give";
const UNLOCK: &str = "rac1.planet.unlock";
/// The owned byte of each item, and the unlock byte of each planet (both discs, inventory.rs and kofi.rs).
const OWNED: u32 = 0x13_D4C0;
const UNLOCKED: u32 = 0x13_DD40;
/// The collected byte of each gold bolt, 4 a level (save.rs, B3003).
const GOLD: u32 = 0x14_BEC0;
const ITEMS: u32 = crate::inventory::GADGETS as u32;
const PLANETS: u32 = 19;
/// The game mode at a vendor: what is bought there is the buyer's.
const VENDOR: u32 = 5;
/// The game is between fields on its main stack (as mp/fx.rs `play` checks): a call made now returns.
const MAIN_STACK: std::ops::Range<u32> = 0x10_2000..0x200_0000;

static GIVE_SPEC: OnceLock<&'static Reloc> = OnceLock::new();
static UNLOCK_SPEC: OnceLock<&'static Reloc> = OnceLock::new();
/// What the hands saw the local game do, and whether a call is ours (another game's progress made here).
static CAUGHT: Mutex<Vec<Progress>> = Mutex::new(Vec::new());
static REPLAYING: AtomicBool = AtomicBool::new(false);
/// The gold bytes as last seen (None: not yet).
static GOLD_SEEN: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn give_spec() -> &'static Reloc {
	GIVE_SPEC.get().expect("give hand without a spec")
}

fn unlock_spec() -> &'static Reloc {
	UNLOCK_SPEC.get().expect("unlock hand without a spec")
}

fn byte(ram: &[u8], a: u32) -> u8 {
	ram.get(a as usize).copied().unwrap_or(0)
}

fn word(ram: &[u8], a: u32) -> u32 {
	let a = a as usize;
	ram.get(a..a + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn caught(p: Progress) {
	CAUGHT.lock().unwrap_or_else(|e| e.into_inner()).push(p);
}

/// `item.give(id, equip)`: an item the player did not have, not bought.
fn give_hand(c: &mut Cpu) -> bool {
	let id = c.g(4) as u32;
	if !REPLAYING.load(Ordering::Relaxed) && id < ITEMS && byte(&c.mem.ram, OWNED + id) == 0 && word(&c.mem.ram, disc::va(MODE_WORD)) != VENDOR {
		caught(Progress::Item(id as u8));
	}
	false
}

/// `planet.unlock(planet)`: a planet that was locked.
fn unlock_hand(c: &mut Cpu) -> bool {
	let planet = c.g(4) as u32;
	if !REPLAYING.load(Ordering::Relaxed) && planet < PLANETS && byte(&c.mem.ram, UNLOCKED + planet) == 0 {
		caught(Progress::Planet(planet as u8));
	}
	false
}

/// Registers the hands. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	let mut n = 0;
	for (id, slot, reloc, f) in [(GIVE, &GIVE_SPEC, give_spec as fn() -> &'static Reloc, give_hand as fn(&mut Cpu) -> bool), (UNLOCK, &UNLOCK_SPEC, unlock_spec, unlock_hand)] {
		if let Some((_, r)) = crate::omni::hook_specs(|h| h == id).into_iter().next() {
			let _ = slot.set(r);
			ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: id, reloc, est: ee_oracle::hs::EXACT, f })));
			n += 1;
		}
	}
	eprintln!("mp: {n} hands for the team's progress");
}

/// Whether this game has it already.
fn has(ram: &[u8], p: Progress) -> bool {
	match p {
		Progress::Item(id) => (id as u32) >= ITEMS || byte(ram, OWNED + id as u32) != 0,
		Progress::Planet(l) => (l as u32) >= PLANETS || byte(ram, UNLOCKED + l as u32) != 0,
		Progress::Gold(g) => g >= rac_mp::proto::GOLD_BOLTS || byte(ram, GOLD + g as u32) != 0,
	}
}

/// The gold bolts collected since the last look, in play in a level (a save loaded, or another game's, isn't the player's doing).
fn gold_collected(ram: &[u8], in_play: bool) -> Vec<Progress> {
	let now: Vec<u8> = (0..rac_mp::proto::GOLD_BOLTS as u32).map(|g| byte(ram, GOLD + g)).collect();
	let mut seen = GOLD_SEEN.lock().unwrap_or_else(|e| e.into_inner());
	let new = match seen.as_ref() {
		Some(was) if in_play => (0..now.len()).filter(|&g| now[g] != 0 && was[g] == 0).map(|g| Progress::Gold(g as u8)).collect(),
		_ => Vec::new(),
	};
	*seen = Some(now);
	new
}

/// One field: what the local game got goes to the others; what they got is made here, in play (`in_play`: and not dying), one a field.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, in_play: bool) {
	let mut got = std::mem::take(&mut *CAUGHT.lock().unwrap_or_else(|e| e.into_inner()));
	got.extend(gold_collected(&c.mem.ram, in_play && mp.level >= 0));
	for p in got {
		eprintln!("mp: the team gets {p:?}");
		super::wire().story.push(p);
	}
	mp.story_in.retain(|p| !has(&c.mem.ram, *p));
	// A gold bolt is only its byte, which the game counts (`gold_bolt.count_collected`): written at once. The bolt itself, where it
	// lies, is the shared world's.
	for p in mp.story_in.extract_if(.., |p| matches!(p, Progress::Gold(_))) {
		if let Progress::Gold(g) = p {
			c.mem.ram[(GOLD + g as u32) as usize] = 1;
			if let Some(seen) = GOLD_SEEN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
				seen[g as usize] = 1;
			}
			eprintln!("mp: from the team: {p:?}");
		}
	}
	let Some(&p) = mp.story_in.first() else { return };
	if !in_play || !MAIN_STACK.contains(&(c.g(29) as u32)) {
		return;
	}
	let (spec, a0, a1) = match p {
		Progress::Item(id) => (GIVE_SPEC.get(), id as u64, 0),
		Progress::Planet(l) => (UNLOCK_SPEC.get(), l as u64, 0),
		Progress::Gold(_) => return,
	};
	let Some(entry) = spec.and_then(|r| crate::omni::find_entry(c, r)) else {
		mp.story_in.remove(0);
		return;
	};
	REPLAYING.store(true, Ordering::Relaxed);
	let r = crate::omni::run_game(c, entry, |c| {
		c.s(4, a0);
		c.s(5, a1);
	});
	REPLAYING.store(false, Ordering::Relaxed);
	mp.story_in.remove(0);
	match r {
		Ok(()) => eprintln!("mp: from the team: {p:?}"),
		Err(e) => eprintln!("mp: could not take {p:?} from the team: {e}"),
	}
}
