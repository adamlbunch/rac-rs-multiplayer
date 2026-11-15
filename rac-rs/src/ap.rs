//! [AP] This fork's Archipelago randomizer, the game side. The world (what is shuffled, the logic) and the client that talks to an
//! Archipelago server are Python, in `archipelago/rac1/` (an apworld, modelled on evilwb/APRac2, MIT, the randomizer of Ratchet &
//! Clank 2 for PCSX2). The client reaches this game over a local TCP port (`bridge.rs`, `RAC_AP=port`), not through the emulator's
//! memory as APRac2 does: what APRac2 patches into its disc image and pokes into PCSX2's RAM is done here, in the game's own
//! functions, through hands at the mods' hook points.
//!
//! What the game gives the player in vanilla becomes a "location": an item (`item.give`, every gadget, the R.Y.N.O., the Suck
//! Cannon, the Morph-o-Ray ...), an infobot (`planet.unlock` of another planet), a gold bolt (its byte at 0x14bec0), a special item
//! (Zoomerator, Raritanium, Codebot, Premium and Ultra Nanotech: bytes at 0x13d388) and, with the vendor option, a weapon bought at a
//! Gadgetron vendor. The game does not get it: the hand blocks the give or the unlock, the byte goes back. The client reports the
//! location and hands this game the items the multiworld sent it, which are given with the same functions (`granting`): one a
//! field, at a safe point in play. Gold bolts are counted from what was received (`gold_bolt.count_collected`); the bytes stay the
//! places' own. The goal is Drek's defeat on Veldin (class 1422 on level 18).
//!
//! With multiplayer on (mp/story.rs shares the team's items and planets) the hands at `item.give` and `planet.unlock` are story's,
//! which ask here first; what the team shares is the team's received items (both players run the client on one slot).
//!
//! Threads: the hands on the game thread during a field; `tick` on the worker between fields; the bridge on its own ("ap-bridge").

use crate::retail::disc;
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

mod bridge;
mod notice;
mod text;

/// Ids shared with the apworld (archipelago/rac1/data/Ids.py). Items: the game's item id, a planet, a special item, a gold bolt, bolts.
pub const ITEM_EQUIP: u32 = 1000;
pub const ITEM_COORDS: u32 = 2000;
pub const ITEM_SPECIAL: u32 = 3000;
pub const ITEM_GOLD_BOLT: u32 = 4000;
pub const ITEM_BOLT_PACK: u32 = 4001;
/// Locations: the item the game gives there, the planet an infobot unlocks, a special item (level * 16 + index), a gold bolt
/// (level * 4 + index), a vendor's weapon.
pub const LOC_ITEM: u32 = 10000;
pub const LOC_INFOBOT: u32 = 20000;
pub const LOC_SPECIAL: u32 = 30000;
pub const LOC_GOLD: u32 = 40000;
pub const LOC_VENDOR: u32 = 50000;

/// Game memory: both discs have the binding's "ntsc" data layout (mp.rs); the overlay words go through `disc::va`.
const OWNED: u32 = 0x13_D4C0;
const SPECIAL: u32 = 0x13_D388;
const UNLOCKED: u32 = 0x13_DD40;
const GOLD: u32 = 0x14_BEC0;
const BOLTS: u32 = 0x15_ED98;
const MAX_HEALTH: u32 = 0x15_EDA0;
const HEALTH: u32 = 0x14_15F8;
const HERO_STATE: u32 = 0x13_F350 + 0x2084;
const LEVEL_WORD: u32 = 0x15_ED84;
/// PAL addresses (through `disc::va`): the game mode, the level loop's exit, and the level request (devtools.rs).
const MODE_WORD: u32 = 0x15_F5A4;
const EXIT_WORD: u32 = 0x15_F550;
const REQ_LEVEL: u32 = 0x15_F5A0;
const REQ_NO_PRESENT: u32 = 0x15_F5B8;
const ITEMS: u32 = crate::inventory::GADGETS as u32;
const PLANETS: u32 = 19;
/// The special items' bytes (0x13d388 + index): Zoomerator, Raritanium, Codebot, Premium Nanotech, Ultra Nanotech.
const SPECIALS: [u32; 5] = [0, 1, 2, 4, 5];
const PREMIUM: u32 = 4;
const ULTRA: u32 = 5;
const GOLD_PER_LEVEL: u32 = 4;
const GAMEPLAY: u32 = 0;
const VENDOR: u32 = 5;
const BOMB_GLOVE: u32 = 10;
/// The weapons a Gadgetron vendor sells (one a planet adds when first visited: retail's table at 0x1c4318).
const VENDOR_WEAPONS: [u32; 11] = [16, 15, 20, 17, 14, 11, 18, 13, 25, 24, 19];
/// Drek's mech (`rac1.moby.final_boss.update`) on Veldin's return, level 18.
const FINAL_BOSS: u32 = 1422;
const FINAL_LEVEL: i32 = 18;
/// L1 + L2 + R1 + R2 + Select, as APRac2's: back to the ship (the planet restarts at its landing pad).
const ESCAPE: u16 = 0x010F;
/// Bolts in a "Bolt Pack" (the filler item) when the client does not say (option `bolt_pack_size`).
const BOLT_PACK: u32 = 1000;
/// A received death (death link): the hero takes this much.
const KILL_DAMAGE: u64 = 99;
const MASK: u32 = 0x01ff_ffff;
/// The main thread's stack: a game function is only run while the game waits there (mp/story.rs).
const MAIN_STACK: std::ops::Range<u32> = 0x10_2000..0x200_0000;

const GIVE: &str = "rac1.item.give";
const UNLOCK: &str = "rac1.planet.unlock";
const GOLD_COUNT: &str = "rac1.gold_bolt.count_collected";
const TAKE_DAMAGE: &str = "rac1.player.take_damage";

/// What the client said about the seed (`Msg seed`).
#[derive(Clone)]
struct Options {
	death_link: bool,
	vendor: bool,
	/// The Bomb Glove is the starting weapon (the game grants it at a level load).
	vanilla_start: bool,
	/// Bolts a Bolt Pack gives.
	bolt_pack: u32,
}

/// Before the client has said: the game's own start (the Bomb Glove), no death link, the vendor as it is.
impl Default for Options {
	fn default() -> Options {
		Options { death_link: false, vendor: false, vanilla_start: true, bolt_pack: BOLT_PACK }
	}
}

#[derive(Default)]
struct State {
	/// The seed and slot (None: no client has told us yet).
	seed: Option<String>,
	slot: String,
	options: Options,
	/// The items received, in the server's order.
	received: Vec<u32>,
	/// Every location found (this seed), and those the client has not been told yet.
	found: BTreeSet<u32>,
	unsent: Vec<u32>,
	/// Bolt packs already paid out and special items the player gave away (a trade): kept per seed.
	packs_paid: u32,
	consumed: BTreeSet<u32>,
	/// Bytes as last seen (owned items, special items, gold bolts), None: not in play yet.
	seen_owned: Option<Vec<u8>>,
	seen_special: Option<Vec<u8>>,
	seen_gold: Option<Vec<u8>>,
	/// Items the hand let through this field (granted, shared by the team, the starting weapon): not taken back.
	allowed: BTreeSet<u32>,
	/// Death link: a death to deal, and whether the one we are in was dealt.
	kill: Option<String>,
	dead: bool,
	dealt: bool,
	/// Drek seen alive, the goal told.
	boss_seen: bool,
	goal: bool,
	escape_held: bool,
	/// Vendor weapons shown as owned while the shop is open (found already, not received).
	vendor_shown: Vec<u32>,
	level: i32,
	mode: u32,
	last_state: Option<Instant>,
	/// Planets landed on this session, with the escape's line shown once (APRac2's "unstuck" message).
	hinted: BTreeSet<i32>,
}

static ON: AtomicBool = AtomicBool::new(false);
/// The pad buttons of the field running (field_run.rs `field_begin`), for the escape.
static BUTTONS: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
static GRANTING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);
static SPECS: OnceLock<Specs> = OnceLock::new();

struct Specs {
	give: Option<&'static Reloc>,
	unlock: Option<&'static Reloc>,
	damage: Option<&'static Reloc>,
}

fn state() -> MutexGuard<'static, Option<State>> {
	STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// RAC_AP is set (the port the client connects to, or `on` for the default): decided once.
fn wanted() -> Option<u16> {
	static W: OnceLock<Option<u16>> = OnceLock::new();
	*W.get_or_init(|| {
		let v = std::env::var("RAC_AP").ok()?;
		let v = v.trim();
		if v.is_empty() || v == "0" || v.eq_ignore_ascii_case("off") {
			return None;
		}
		Some(v.parse().unwrap_or(bridge::DEFAULT_PORT))
	})
}

/// Whether the randomizer runs (RAC_AP).
pub fn on() -> bool {
	ON.load(Ordering::Relaxed)
}

fn byte(ram: &[u8], a: u32) -> u8 {
	ram.get((a & MASK) as usize).copied().unwrap_or(0)
}

fn word(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn put_byte(ram: &mut [u8], a: u32, v: u8) {
	if let Some(b) = ram.get_mut((a & MASK) as usize) {
		*b = v;
	}
}

fn put_word(ram: &mut [u8], a: u32, v: u32) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

/// The level running (0..=18), or -1 (the front end, a level loop ending).
fn level(ram: &[u8]) -> i32 {
	let l = word(ram, LEVEL_WORD) as i32;
	if crate::mp::in_front() || word(ram, disc::va(EXIT_WORD)) != 0 || !(0..=18).contains(&l) {
		return -1;
	}
	l
}

/// `Retail::load`: the bridge and the hands, once, with RAC_AP.
pub fn init() {
	static STARTED: AtomicBool = AtomicBool::new(false);
	let Some(port) = wanted() else { return };
	if STARTED.swap(true, Ordering::Relaxed) {
		return;
	}
	*state() = Some(State { level: -1, ..State::default() });
	let spec = |id: &str| crate::omni::hook_spec(id);
	let _ = SPECS.set(Specs { give: spec(GIVE), unlock: spec(UNLOCK), damage: spec(TAKE_DAMAGE) });
	install();
	ON.store(true, Ordering::Relaxed);
	bridge::start(port);
	eprintln!("ap: Archipelago randomizer on, the client connects to 127.0.0.1:{port}");
}

/// The hands: the gold bolt count always; `item.give` and `planet.unlock` only without multiplayer (story.rs has them then and asks
/// here). A hook point a mod has taken stays the mod's.
fn install() {
	let mp = crate::mp::wanted_now();
	let want = |id: &str| id == GOLD_COUNT || (!mp && (id == GIVE || id == UNLOCK));
	let specs = crate::omni::hook_specs(want);
	for (id, reloc) in specs {
		let (f, r): (fn(&mut Cpu) -> bool, fn() -> &'static Reloc) = match id.as_str() {
			GIVE => (give_hand, || SPECS.get().and_then(|s| s.give).expect("give spec")),
			UNLOCK => (unlock_hand, || SPECS.get().and_then(|s| s.unlock).expect("unlock spec")),
			_ => {
				let _ = GOLD_SPEC.set(reloc);
				(gold_hand, || GOLD_SPEC.get().copied().expect("gold spec"))
			}
		};
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: id.leak(), reloc: r, est: ee_oracle::hs::EXACT, f })));
	}
	crate::omni::allow_hook_hands();
}

static GOLD_SPEC: OnceLock<&'static Reloc> = OnceLock::new();

fn back_to_caller(c: &mut Cpu) {
	let ra = c.g(31) as u32;
	c.pc = ra;
	c.npc = ra.wrapping_add(4);
	c.mem.now = c.icount;
}

fn give_hand(c: &mut Cpu) -> bool {
	if give_hook(c) {
		back_to_caller(c);
		return true;
	}
	false
}

fn unlock_hand(c: &mut Cpu) -> bool {
	if unlock_hook(c) {
		back_to_caller(c);
		return true;
	}
	false
}

/// `gold_bolt.count_collected()`: the gold bolts received from the multiworld, once a seed is known.
fn gold_hand(c: &mut Cpu) -> bool {
	if !on() {
		return false;
	}
	let n = match state().as_ref() {
		Some(s) if s.seed.is_some() => s.received.iter().filter(|&&i| i == ITEM_GOLD_BOLT).count() as u64,
		_ => return false,
	};
	c.s(2, n);
	back_to_caller(c);
	true
}

/// `item.give(id, equip)` (this module's hand, or story.rs's): true = the game must not give it, it is a location.
pub(crate) fn give_hook(c: &Cpu) -> bool {
	if !on() || GRANTING.load(Ordering::Relaxed) {
		return false;
	}
	let id = c.g(4) as u32;
	let mut g = state();
	let Some(s) = g.as_mut() else { return false };
	if crate::mp::story_replaying() || !(2..ITEMS).contains(&id) {
		s.allowed.insert(id);
		return false;
	}
	let ram = &c.mem.ram[..];
	if word(ram, disc::va(MODE_WORD)) == VENDOR {
		if s.options.vendor && VENDOR_WEAPONS.contains(&id) {
			found(s, LOC_VENDOR + id);
			return true;
		}
		s.allowed.insert(id);
		return false;
	}
	if id == BOMB_GLOVE && s.options.vanilla_start {
		s.allowed.insert(id);
		return false;
	}
	// Owned already (the multiworld sent it) or not: the place is found either way, and the game gives nothing.
	found(s, LOC_ITEM + id);
	true
}

/// `planet.unlock(planet)`: true = an infobot's planet, a location (found whether the planet is open already or not). The planet
/// being landed on unlocks as the game does (the level load's `planet.unlock(level)`).
pub(crate) fn unlock_hook(c: &Cpu) -> bool {
	if !on() || GRANTING.load(Ordering::Relaxed) || crate::mp::story_replaying() {
		return false;
	}
	let p = c.g(4) as u32;
	let ram = &c.mem.ram[..];
	if p >= PLANETS || p as i32 == word(ram, LEVEL_WORD) as i32 {
		return false;
	}
	let mut g = state();
	let Some(s) = g.as_mut() else { return false };
	found(s, LOC_INFOBOT + p);
	true
}

/// A location found: told to the client (and kept for the seed), with a line on the screen.
fn found(s: &mut State, loc: u32) {
	if s.found.insert(loc) {
		s.unsent.push(loc);
		save(s);
		eprintln!("ap: location {loc} found");
	}
}

/// The item the multiworld sent the player, as the game has it: owned (equipment), unlocked (a planet), its byte (special).
fn received_equip(s: &State, id: u32) -> bool {
	s.received.contains(&(ITEM_EQUIP + id))
}

/// retail/worker/field_run.rs: the pad buttons of the field about to run.
pub fn note_buttons(b: u16) {
	BUTTONS.store(b, Ordering::Relaxed);
}

/// After each field the EE ran, on the worker thread: the locations the bytes say were found, what the multiworld sent, death link,
/// the goal, the escape, and the client's lines on the screen.
pub fn tick(c: &mut Cpu) {
	if !on() {
		return;
	}
	let buttons = BUTTONS.load(Ordering::Relaxed);
	let ram = &c.mem.ram[..];
	let lvl = level(ram);
	let mode = word(ram, disc::va(MODE_WORD));
	let mut g = state();
	let Some(s) = g.as_mut() else { return };
	if (lvl, mode) != (s.level, s.mode) || s.last_state.is_none_or(|t| t.elapsed().as_secs() >= 2) {
		(s.level, s.mode, s.last_state) = (lvl, mode, Some(Instant::now()));
		bridge::send(format!("{{\"t\":\"state\",\"level\":{lvl},\"mode\":{mode}}}"));
	}
	let in_play = lvl >= 0 && matches!(mode, GAMEPLAY | 3 | 4 | VENDOR);
	watch(c, s, lvl, in_play);
	if !s.unsent.is_empty() {
		let list = s.unsent.drain(..).map(|l| l.to_string()).collect::<Vec<_>>().join(",");
		bridge::send(format!("{{\"t\":\"found\",\"loc\":[{list}]}}"));
	}
	vendor_view(c, s, mode);
	if lvl >= 0 && mode == GAMEPLAY {
		if lvl > 0 && s.seed.is_some() && s.hinted.insert(lvl) {
			notice::push("Stuck? Hold L1+L2+R1+R2+Select: back to the ship".into());
		}
		escape(c, s, lvl, buttons);
		death(c, s);
		goal(c, s, lvl);
		if s.seed.is_some() && MAIN_STACK.contains(&(c.g(29) as u32)) {
			grant(c, s);
		}
	}
	s.allowed.clear();
	drop(g);
	if lvl >= 0 {
		text::tick(&mut c.mem.ram[..], lvl);
	}
	notice::draw(lvl >= 0 && mode == GAMEPLAY);
}

/// The bytes the game set itself since the last field: a special item, an item it stored without `item.give` (the Persuader), a gold
/// bolt. Found; the first two go back unless the multiworld sent them.
fn watch(c: &mut Cpu, s: &mut State, lvl: i32, in_play: bool) {
	let ram = &mut c.mem.ram[..];
	let owned: Vec<u8> = (0..ITEMS).map(|i| byte(ram, OWNED + i)).collect();
	let special: Vec<u8> = (0..8).map(|i| byte(ram, SPECIAL + i)).collect();
	let gold: Vec<u8> = (0..PLANETS * GOLD_PER_LEVEL).map(|i| byte(ram, GOLD + i)).collect();
	let (n_found, n_consumed) = (s.found.len(), s.consumed.len());
	if in_play {
		if let Some(was) = s.seen_owned.as_ref() {
			for id in 2..ITEMS {
				let i = id as usize;
				if owned[i] != 0 && was[i] == 0 && !s.allowed.contains(&id) && !received_equip(s, id) && !(id == BOMB_GLOVE && s.options.vanilla_start) {
					let loc = LOC_ITEM + id;
					if s.found.insert(loc) {
						s.unsent.push(loc);
						eprintln!("ap: location {loc} found (item {id} stored)");
					}
					put_byte(ram, OWNED + id, 0);
				}
			}
		}
		if let Some(was) = s.seen_special.as_ref() {
			for &k in &SPECIALS {
				let i = k as usize;
				let ap = s.received.contains(&(ITEM_SPECIAL + k));
				if special[i] != 0 && was[i] == 0 && !s.allowed.contains(&(ITEM_SPECIAL + k)) {
					let loc = LOC_SPECIAL + lvl as u32 * 16 + k;
					if s.found.insert(loc) {
						s.unsent.push(loc);
						eprintln!("ap: location {loc} found (special {k})");
					}
					if !ap || s.consumed.contains(&k) {
						put_byte(ram, SPECIAL + k, 0);
					}
					nanotech_health(ram, s);
				} else if special[i] == 0 && was[i] != 0 && ap && s.consumed.insert(k) {
					eprintln!("ap: special item {k} traded away");
				}
			}
		}
		if let Some(was) = s.seen_gold.as_ref() {
			for i in 0..gold.len() {
				if gold[i] != 0 && was[i] == 0 {
					let loc = LOC_GOLD + i as u32;
					if s.found.insert(loc) {
						s.unsent.push(loc);
						eprintln!("ap: location {loc} found (gold bolt)");
					}
				}
			}
		}
		if (s.found.len(), s.consumed.len()) != (n_found, n_consumed) {
			save(s);
		}
	}
	let ram = &c.mem.ram[..];
	s.seen_owned = Some((0..ITEMS).map(|i| byte(ram, OWNED + i)).collect());
	s.seen_special = Some((0..8).map(|i| byte(ram, SPECIAL + i)).collect());
	s.seen_gold = Some(gold);
}

/// Max health as the nanotech received gives it (retail sets 5 for the Premium, 8 for the Ultra when it sells them).
fn nanotech_health(ram: &mut [u8], s: &State) {
	let has = |k: u32| s.received.contains(&(ITEM_SPECIAL + k));
	let want = if has(ULTRA) { 8 } else if has(PREMIUM) { 5 } else { 4 };
	if word(ram, MAX_HEALTH) != want {
		put_word(ram, MAX_HEALTH, want);
	}
}

/// What the multiworld sent, made in the game: one item or planet a field through the game's own function, the special items and
/// bolts at once.
fn grant(c: &mut Cpu, s: &mut State) {
	let ram = &mut c.mem.ram[..];
	for &k in &SPECIALS {
		if s.received.contains(&(ITEM_SPECIAL + k)) && !s.consumed.contains(&k) && byte(ram, SPECIAL + k) == 0 {
			put_byte(ram, SPECIAL + k, 1);
			s.allowed.insert(ITEM_SPECIAL + k);
			if let Some(v) = s.seen_special.as_mut() {
				v[k as usize] = 1;
			}
			nanotech_health(ram, s);
		}
	}
	let packs = s.received.iter().filter(|&&i| i == ITEM_BOLT_PACK).count() as u32;
	if packs > s.packs_paid {
		let n = packs - s.packs_paid;
		put_word(ram, BOLTS, word(ram, BOLTS).saturating_add(n * s.options.bolt_pack));
		s.packs_paid = packs;
		save(s);
	}
	let planet = s.received.iter().filter_map(|&i| i.checked_sub(ITEM_COORDS).filter(|&p| p < PLANETS)).find(|&p| byte(ram, UNLOCKED + p) == 0);
	let equip = s.received.iter().filter_map(|&i| i.checked_sub(ITEM_EQUIP).filter(|&id| (2..ITEMS).contains(&id))).find(|&id| byte(ram, OWNED + id) == 0);
	let specs = SPECS.get();
	let (spec, a0, what) = match (planet, equip) {
		(Some(p), _) => (specs.and_then(|s| s.unlock), p, ITEM_COORDS + p),
		(None, Some(id)) => (specs.and_then(|s| s.give), id, ITEM_EQUIP + id),
		_ => return,
	};
	let Some(entry) = spec.and_then(|r| crate::omni::find_entry(c, r)) else { return };
	if planet.is_none() {
		s.allowed.insert(a0);
	}
	GRANTING.store(true, Ordering::Relaxed);
	let r = crate::omni::run_game(c, entry, |c| {
		c.s(4, a0 as u64);
		c.s(5, 0);
	});
	GRANTING.store(false, Ordering::Relaxed);
	match r {
		Ok(()) => eprintln!("ap: gave item {what}"),
		Err(e) => eprintln!("ap: could not give item {what}: {e}"),
	}
	if let Some(v) = s.seen_owned.as_mut() {
		let ram = &c.mem.ram[..];
		for (i, b) in v.iter_mut().enumerate() {
			*b = byte(ram, OWNED + i as u32);
		}
	}
}

/// The vendor's weapons already found there show as owned while the shop is open (an ammo row, not one to buy again), and go back
/// after.
fn vendor_view(c: &mut Cpu, s: &mut State, mode: u32) {
	let ram = &mut c.mem.ram[..];
	if mode == VENDOR && s.options.vendor && s.seed.is_some() {
		for &id in &VENDOR_WEAPONS {
			if s.found.contains(&(LOC_VENDOR + id)) && byte(ram, OWNED + id) == 0 {
				put_byte(ram, OWNED + id, 1);
				s.vendor_shown.push(id);
				s.allowed.insert(id);
			}
		}
	} else if !s.vendor_shown.is_empty() {
		for id in std::mem::take(&mut s.vendor_shown) {
			if !received_equip(s, id) {
				put_byte(ram, OWNED + id, 0);
			}
		}
		if let Some(v) = s.seen_owned.as_mut() {
			for (i, b) in v.iter_mut().enumerate() {
				*b = byte(ram, OWNED + i as u32);
			}
		}
	}
}

/// L1 + L2 + R1 + R2 + Select: the planet restarts at its landing pad (the level request retail's own Load Game makes).
fn escape(c: &mut Cpu, s: &mut State, lvl: i32, buttons: u16) {
	let held = buttons & ESCAPE == ESCAPE;
	if held && !s.escape_held {
		let ram = &mut c.mem.ram[..];
		put_word(ram, disc::va(REQ_LEVEL), lvl as u32);
		put_word(ram, disc::va(REQ_NO_PRESENT), 1);
		put_word(ram, disc::va(EXIT_WORD), 1);
		notice::push("Back to the ship".into());
		eprintln!("ap: escape: level {lvl} restarts");
	}
	s.escape_held = held;
}

/// Death link: the hero's death goes out (unless it was dealt), and a death that came in is dealt (the game's `take_damage`).
fn death(c: &mut Cpu, s: &mut State) {
	let ram = &c.mem.ram[..];
	let state = word(ram, HERO_STATE) as i32;
	let dead = (word(ram, HEALTH) as i32) <= 0 || crate::mp::death_state(state);
	if dead && !s.dead {
		if s.options.death_link && !s.dealt {
			bridge::send("{\"t\":\"death\"}".into());
		}
		s.dealt = false;
	}
	s.dead = dead;
	if dead || !s.options.death_link {
		s.kill = None;
		return;
	}
	let Some(why) = s.kill.take() else { return };
	if !MAIN_STACK.contains(&(c.g(29) as u32)) {
		s.kill = Some(why);
		return;
	}
	let Some(entry) = SPECS.get().and_then(|s| s.damage).and_then(|r| crate::omni::find_entry(c, r)) else { return };
	notice::push(why);
	s.dealt = true;
	if let Err(e) = crate::omni::run_game(c, entry, |c| c.s(4, KILL_DAMAGE)) {
		eprintln!("ap: could not deal the death: {e}");
	}
}

/// Drek on Veldin's return: seen alive, then gone or out of health, is the goal.
fn goal(c: &Cpu, s: &mut State, lvl: i32) {
	if s.goal || lvl != FINAL_LEVEL {
		s.boss_seen = s.boss_seen && lvl == FINAL_LEVEL;
		return;
	}
	let ram = &c.mem.ram[..];
	let (start, end) = crate::retail::table(lvl as u32);
	let (start, end) = (word(ram, start) & MASK, word(ram, end) & MASK);
	let mut alive = None;
	if start != 0 && end > start && end - start < 0x10_0000 {
		for m in (start..end).step_by(0x100) {
			if word(ram, m + 0xa4) >> 16 == FINAL_BOSS && byte(ram, m + 0x20) < 0x7f {
				let pvar = word(ram, m + 0x78) & MASK;
				let health = f32::from_bits(word(ram, pvar + 0x20));
				alive = Some(pvar == 0 || health > 0.0);
			}
		}
	}
	match alive {
		Some(true) => s.boss_seen = true,
		Some(false) | None if s.boss_seen => {
			s.goal = true;
			bridge::send("{\"t\":\"goal\"}".into());
			notice::push("Drek is defeated!".into());
			eprintln!("ap: the goal: Drek defeated");
		}
		_ => {}
	}
}

/// A message from the client (bridge thread).
fn from_client(m: &crate::omni::json::Json) {
	let Some(t) = m.get("t").and_then(|t| t.str()) else { return };
	let mut g = state();
	let Some(s) = g.as_mut() else { return };
	match t {
		"seed" => {
			let seed = m.get("seed").and_then(|v| v.str()).unwrap_or("").to_string();
			let o = m.get("options");
			let flag = |k: &str| o.and_then(|o| o.get(k)).and_then(|v| v.as_num()).is_some_and(|n| n != 0.0);
			let pack = o.and_then(|o| o.get("bolt_pack")).and_then(|v| v.as_num()).map_or(BOLT_PACK, |n| n.clamp(0.0, 1e6) as u32);
			s.options = Options { death_link: flag("death_link"), vendor: flag("randomize_vendor"), vanilla_start: !flag("random_start_weapon"), bolt_pack: pack };
			s.slot = m.get("slot").and_then(|v| v.str()).unwrap_or("").to_string();
			if s.seed.as_deref() != Some(seed.as_str()) {
				load(s, &seed);
			}
			eprintln!("ap: seed {seed} for {} (death link {}, vendor {})", s.slot, s.options.death_link, s.options.vendor);
			// Every location found goes to the client again: the server keeps the first of each.
			s.unsent = s.found.iter().copied().collect();
		}
		"items" => {
			s.received = m.get("items").map(|a| a.arr().iter().filter_map(|v| v.as_num()).map(|n| n as u32).collect()).unwrap_or_default();
		}
		"names" => {
			let names = m.get("names").map(|o| o.entries().iter().filter_map(|(k, v)| Some((k.parse::<u32>().ok()?, v.str()?.to_string()))).collect()).unwrap_or_default();
			text::set_names(names);
		}
		"msg" => {
			if let Some(text) = m.get("text").and_then(|v| v.str()) {
				notice::push(text.to_string());
			}
		}
		"kill" => {
			if s.options.death_link {
				s.kill = Some(m.get("text").and_then(|v| v.str()).unwrap_or("Death link").to_string());
			}
		}
		_ => {}
	}
}

/// A client connected: what it needs first.
fn on_connect() {
	let mut g = state();
	let Some(s) = g.as_mut() else { return };
	bridge::send(format!("{{\"t\":\"hello\",\"game\":\"Ratchet & Clank\",\"version\":\"{}\"}}", bridge::esc(env!("CARGO_PKG_VERSION"))));
	s.unsent = s.found.iter().copied().collect();
	s.last_state = None;
	if s.goal {
		bridge::send("{\"t\":\"goal\"}".into());
	}
}

fn seed_file(seed: &str) -> std::path::PathBuf {
	let clean: String = seed.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
	crate::retail::memcard::player_dir().join("ap").join(format!("{clean}.txt"))
}

/// What is kept for a seed: the locations found, the bolt packs paid out, the special items traded away.
fn load(s: &mut State, seed: &str) {
	let old: Vec<u32> = s.found.iter().copied().collect();
	s.found.clear();
	s.consumed.clear();
	s.packs_paid = 0;
	s.goal = false;
	if let Ok(t) = std::fs::read_to_string(seed_file(seed)) {
		for l in t.lines() {
			let mut w = l.split_whitespace();
			match (w.next(), w.next().and_then(|v| v.parse::<u32>().ok())) {
				(Some("found"), Some(v)) => {
					s.found.insert(v);
				}
				(Some("packs"), Some(v)) => s.packs_paid = v,
				(Some("consumed"), Some(v)) => {
					s.consumed.insert(v);
				}
				(Some("goal"), _) => s.goal = true,
				_ => {}
			}
		}
	}
	// Found before the client named the seed: this seed's.
	s.found.extend(old);
	s.seed = Some(seed.to_string());
	save(s);
}

fn save(s: &State) {
	let Some(seed) = s.seed.as_deref() else { return };
	let mut t = String::new();
	for l in &s.found {
		t += &format!("found {l}\n");
	}
	t += &format!("packs {}\n", s.packs_paid);
	for k in &s.consumed {
		t += &format!("consumed {k}\n");
	}
	if s.goal {
		t += "goal\n";
	}
	let p = seed_file(seed);
	if let Some(d) = p.parent() {
		let _ = std::fs::create_dir_all(d);
	}
	let tmp = p.with_extension("tmp");
	if std::fs::write(&tmp, t).is_ok() {
		let _ = std::fs::rename(&tmp, &p);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ids_match_the_apworld() {
		// archipelago/rac1/data/Ids.py has the same numbers.
		assert_eq!((ITEM_EQUIP, ITEM_COORDS, ITEM_SPECIAL, ITEM_GOLD_BOLT, ITEM_BOLT_PACK), (1000, 2000, 3000, 4000, 4001));
		assert_eq!((LOC_ITEM, LOC_INFOBOT, LOC_SPECIAL, LOC_GOLD, LOC_VENDOR), (10000, 20000, 30000, 40000, 50000));
		assert_eq!(ESCAPE, 0x010F);
	}
}
