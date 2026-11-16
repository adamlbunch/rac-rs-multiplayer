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
mod givers;
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
/// The unlock bytes retail counts (19 planets and one more), and the planets in the order they were unlocked (a word each).
const UNLOCK_BYTES: u32 = 20;
const GALAXY_ORDER: u32 = 0x13_D510;
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
const SCENE: u32 = 2;
const PAUSE: u32 = 3;
const VENDOR: u32 = 5;
/// The vendor's stock (an item id a byte, 0xff ends it; retail's `vendor.init_level_stock` keeps it) and its owned mark.
const STOCK: u32 = 0x15_EDD0;
const STOCK_SIZE: u32 = 12;
const STOCK_OWNED: u8 = 0x40;
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
const MASK: u32 = 0x01ff_ffff;
/// The main thread's stack: a game function is only run while the game waits there (mp/story.rs).
const MAIN_STACK: std::ops::Range<u32> = 0x10_2000..0x200_0000;

const GIVE: &str = "rac1.item.give";
const UNLOCK: &str = "rac1.planet.unlock";
const GOLD_COUNT: &str = "rac1.gold_bolt.count_collected";
const TAKE_DAMAGE: &str = "rac1.player.take_damage";
const SHOP_LIST: &str = "rac1.vendor.build_item_list";

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
	/// The vendor's weapons shown as their places stand while the shop is open, with their owned byte from before.
	shop_saved: Vec<(u32, u8)>,
	/// The level whose givers' bytes show their places (`ap/givers.rs`).
	view: Option<usize>,
	level: i32,
	mode: u32,
	last_state: Option<Instant>,
	/// Planets landed on this session, with the escape's line shown once (APRac2's "unstuck" message).
	hinted: BTreeSet<i32>,
	/// Fields since the start, and the test aids done (`test_aids`).
	fields: u64,
	aids_done: BTreeSet<&'static str>,
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
	/// The vendor menu's list from the stock and the items owned (`vendor.build_item_list(skip_unflagged)`).
	shop_list: Option<&'static Reloc>,
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
	let _ = SPECS.set(Specs { give: spec(GIVE), unlock: spec(UNLOCK), damage: spec(TAKE_DAMAGE), shop_list: spec(SHOP_LIST) });
	install();
	ON.store(true, Ordering::Relaxed);
	bridge::start(port);
	eprintln!("ap: Archipelago randomizer on, the client connects to 127.0.0.1:{port}");
}

/// `Retail::load`, after the multiplayer's hands (fork_pack.rs): the givers' hands, which must win at the updates both take.
pub fn init_givers() {
	if on() {
		givers::install();
	}
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
	s.fields += 1;
	if (lvl, mode) != (s.level, s.mode) || s.last_state.is_none_or(|t| t.elapsed().as_secs() >= 2) {
		if (lvl, mode) != (s.level, s.mode) {
			eprintln!("ap: f{} level {lvl} mode {mode}", s.fields);
		}
		(s.level, s.mode, s.last_state) = (lvl, mode, Some(Instant::now()));
		bridge::send(format!("{{\"t\":\"state\",\"level\":{lvl},\"mode\":{mode}}}"));
	}
	// Scenes too: Pokitaru's trade is made in its scene.
	let in_play = lvl >= 0 && matches!(mode, GAMEPLAY | SCENE | PAUSE | 4 | VENDOR);
	watch(c, s, lvl, in_play);
	if s.seed.is_some() {
		givers::keep(&mut c.mem.ram, s);
	}
	if in_play && s.seed.is_some() {
		nanotech_health(&mut c.mem.ram, s);
	}
	if !s.unsent.is_empty() {
		let list = s.unsent.drain(..).map(|l| l.to_string()).collect::<Vec<_>>().join(",");
		bridge::send(format!("{{\"t\":\"found\",\"loc\":[{list}]}}"));
	}
	vendor_view(c, s, lvl, mode);
	if lvl == FINAL_LEVEL {
		goal(c, s, lvl);
	}
	if lvl >= 0 && mode == GAMEPLAY {
		test_aids(c, s, lvl);
		if lvl > 0 && s.seed.is_some() && s.hinted.insert(lvl) {
			notice::push("Stuck? Hold L1+L2+R1+R2+Select: back to the ship".into());
		}
		death(c, s);
		if s.seed.is_some() && MAIN_STACK.contains(&(c.g(29) as u32)) {
			grant(c, s);
		}
	}
	// The combo's Select opens the pause menu in the same field: there too (retail's own Load Game runs from it).
	if lvl >= 0 && matches!(mode, GAMEPLAY | PAUSE) {
		escape(c, s, lvl, buttons);
	} else {
		s.escape_held = buttons & ESCAPE == ESCAPE;
	}
	s.allowed.clear();
	let fields = s.fields;
	drop(g);
	if lvl >= 0 && mode == GAMEPLAY {
		test_unlock(c, fields);
	}
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
				let counts = !received_equip(s, id) || givers::viewed(s, OWNED + id);
				if owned[i] != 0 && was[i] == 0 && !s.allowed.contains(&id) && counts && !(id == BOMB_GLOVE && s.options.vanilla_start) {
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
/// Kept so in play: Orxon's seller sets the most it sells (and fills the health to it) whether or not the multiworld sent it, and
/// a saved game's load sets it from the nanotech bytes. A nanotech received fills the health as the seller does; one taken back
/// leaves no more health than the most.
fn nanotech_health(ram: &mut [u8], s: &State) {
	let has = |k: u32| s.received.contains(&(ITEM_SPECIAL + k));
	let want = if has(ULTRA) { 8 } else if has(PREMIUM) { 5 } else { 4 };
	let max = word(ram, MAX_HEALTH);
	if max != want {
		put_word(ram, MAX_HEALTH, want);
		if want > max && (word(ram, HEALTH) as i32) > 0 {
			put_word(ram, HEALTH, want);
		}
	}
	if (word(ram, HEALTH) as i32) > want as i32 {
		put_word(ram, HEALTH, want);
	}
	if max != want {
		eprintln!("ap: max health {max} -> {want}, health {}", word(ram, HEALTH) as i32);
	}
}

/// What the multiworld sent, made in the game: one item or planet a field through the game's own function, the special items and
/// bolts at once.
fn grant(c: &mut Cpu, s: &mut State) {
	let ram = &mut c.mem.ram[..];
	for &k in &SPECIALS {
		if s.received.contains(&(ITEM_SPECIAL + k)) && !s.consumed.contains(&k) && byte(ram, SPECIAL + k) == 0 && !givers::viewed(s, SPECIAL + k) {
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
	if let Some(p) = s.received.iter().filter_map(|&i| i.checked_sub(ITEM_COORDS).filter(|&p| p < PLANETS)).find(|&p| byte(ram, UNLOCKED + p) == 0) {
		unlock_planet(ram, p);
		let set = (0..UNLOCK_BYTES).filter(|&i| byte(ram, UNLOCKED + i) != 0).count() as u32;
		let order: Vec<u32> = (0..set).map(|i| word(ram, GALAXY_ORDER + 4 * i)).collect();
		eprintln!("ap: gave item {} (galaxy order {order:?})", ITEM_COORDS + p);
		return;
	}
	let Some(id) = s.received.iter().filter_map(|&i| i.checked_sub(ITEM_EQUIP).filter(|&id| (2..ITEMS).contains(&id))).find(|&id| byte(ram, OWNED + id) == 0 && !givers::viewed(s, OWNED + id)) else { return };
	let Some(entry) = SPECS.get().and_then(|s| s.give).and_then(|r| crate::omni::find_entry(c, r)) else { return };
	s.allowed.insert(id);
	GRANTING.store(true, Ordering::Relaxed);
	let r = crate::omni::run_game(c, entry, |c| {
		c.s(4, id as u64);
		c.s(5, 0);
	});
	GRANTING.store(false, Ordering::Relaxed);
	match r {
		Ok(()) => eprintln!("ap: gave item {}", ITEM_EQUIP + id),
		Err(e) => eprintln!("ap: could not give item {}: {e}", ITEM_EQUIP + id),
	}
	if let Some(v) = s.seen_owned.as_mut() {
		let ram = &c.mem.ram[..];
		for (i, b) in v.iter_mut().enumerate() {
			*b = byte(ram, OWNED + i as u32);
		}
	}
}

/// Coordinates received: what retail's `planet.unlock` writes (USA Veldin 0x2607d0: the planet's unlock byte, and the planet
/// appended to the galaxy order at the count of the 20 unlock bytes set), without its banner. The banner's text is the infobot's,
/// in the text of the level whose infobot gives the planet: on any other level the game shows its "This message does not exist"
/// (Raj, on Veldin, for the Kerwan and Rilgar coordinates of the start).
fn unlock_planet(ram: &mut [u8], p: u32) {
	let n = (0..UNLOCK_BYTES).filter(|&i| byte(ram, UNLOCKED + i) != 0).count() as u32;
	put_word(ram, GALAXY_ORDER + 4 * n, p);
	put_byte(ram, UNLOCKED + p, 1);
}

/// While the shop is open, each of the vendor's weapons shows as its location stands: bought there already, as owned (an ammo row);
/// not bought there yet, as for sale, even when the multiworld sent the weapon (or its place could never be bought). The shop's
/// rows come from its stock (`STOCK`: an item id a byte, `STOCK_OWNED` set for an ammo row) and, for what is not in stock, the
/// owned bytes: both are set, and a list the shop built already is built again (the game's own `vendor.build_item_list`, as after
/// a purchase). When the shop closes the owned bytes go back, and the stock's marks follow them as the game keeps them.
fn vendor_view(c: &mut Cpu, s: &mut State, lvl: i32, mode: u32) {
	if mode == VENDOR && s.options.vendor && s.seed.is_some() && lvl >= 0 {
		let change: Vec<(u32, bool)> = VENDOR_WEAPONS
			.iter()
			.map(|&id| (id, s.found.contains(&(LOC_VENDOR + id))))
			.filter(|&(id, bought)| (byte(&c.mem.ram, OWNED + id) != 0) != bought || stock_mark(&c.mem.ram, id).is_some_and(|m| m != bought))
			.collect();
		if change.is_empty() {
			return;
		}
		let built = crate::retail::vendor::rows_built(c, lvl as usize);
		if built && !MAIN_STACK.contains(&(c.g(29) as u32)) {
			return;
		}
		for (id, bought) in change {
			if !s.shop_saved.iter().any(|&(i, _)| i == id) {
				s.shop_saved.push((id, byte(&c.mem.ram, OWNED + id)));
				if bought {
					eprintln!("ap: the vendor shows item {id} as owned (bought here already)");
				} else {
					eprintln!("ap: the vendor offers item {id} (not bought here yet)");
				}
			}
			put_byte(&mut c.mem.ram, OWNED + id, bought as u8);
			set_stock_mark(&mut c.mem.ram, id, bought);
			// Seen as it is now: the next field's `watch` must not take it for an item the game stored.
			if let Some(v) = s.seen_owned.as_mut() {
				v[id as usize] = bought as u8;
			}
		}
		if built {
			let r = SPECS.get().and_then(|s| s.shop_list).and_then(|r| crate::omni::find_entry(c, r)).ok_or_else(|| "no build_item_list in the code".to_string());
			match r.and_then(|entry| crate::omni::run_game(c, entry, |c| c.s(4, 0))) {
				Ok(()) => eprintln!("ap: the vendor's list built again: {:?}", crate::retail::vendor::rows(c, lvl as usize)),
				Err(e) => eprintln!("ap: the vendor's list could not be built again: {e}"),
			}
		}
	} else if !s.shop_saved.is_empty() {
		let ram = &mut c.mem.ram[..];
		for (id, owned) in std::mem::take(&mut s.shop_saved) {
			// Received in the meantime (a co-op partner's purchase replayed): owned.
			let owned = owned != 0 || received_equip(s, id);
			put_byte(ram, OWNED + id, owned as u8);
			set_stock_mark(ram, id, owned);
		}
		if let Some(v) = s.seen_owned.as_mut() {
			for (i, b) in v.iter_mut().enumerate() {
				*b = byte(ram, OWNED + i as u32);
			}
		}
	}
}

/// The stock's entry of `id`, if it is in stock (`STOCK_SIZE` bytes, 0xff ends the list).
fn stock_slot(ram: &[u8], id: u32) -> Option<u32> {
	(0..STOCK_SIZE).map(|k| (k, byte(ram, STOCK + k))).take_while(|&(_, b)| b != 0xff).find(|&(_, b)| u32::from(b & 0x3f) == id).map(|(k, _)| k)
}

/// Whether the stock marks `id` as owned (an ammo row), if it is in stock.
fn stock_mark(ram: &[u8], id: u32) -> Option<bool> {
	stock_slot(ram, id).map(|k| byte(ram, STOCK + k) & STOCK_OWNED != 0)
}

fn set_stock_mark(ram: &mut [u8], id: u32, owned: bool) {
	if let Some(k) = stock_slot(ram, id) {
		let b = byte(ram, STOCK + k);
		put_byte(ram, STOCK + k, if owned { b | STOCK_OWNED } else { b & !STOCK_OWNED });
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
			eprintln!("ap: death link: the hero died, the others are told");
		} else if s.dealt {
			eprintln!("ap: death link: the hero died of it (not sent back)");
		}
		s.dealt = false;
	}
	s.dead = dead;
	if dead || !s.options.death_link {
		s.kill = None;
		return;
	}
	let Some(why) = s.kill.clone() else { return };
	if !MAIN_STACK.contains(&(c.g(29) as u32)) {
		return;
	}
	if !s.dealt {
		eprintln!("ap: death link: {why}");
		notice::push(why);
		s.dealt = true;
	}
	// Tried again each field while the hero lives (a blow just taken leaves him unhurt for a moment).
	if let Err(e) = kill_hero(c) {
		eprintln!("ap: could not deal the death: {e}");
		s.kill = None;
	}
}

/// The hero's death the game's way: one hit point left, then the game's own `take_damage` (it takes at most one a call), so his
/// death plays as an enemy's last blow would make it.
fn kill_hero(c: &mut Cpu) -> Result<(), String> {
	let entry = SPECS.get().and_then(|s| s.damage).and_then(|r| crate::omni::find_entry(c, r)).ok_or("no take_damage in the code")?;
	if (word(&c.mem.ram, HEALTH) as i32) > 1 {
		put_word(&mut c.mem.ram, HEALTH, 1);
	}
	crate::omni::run_game(c, entry, |c| c.s(4, 1))
}

/// Drek on Veldin's return: seen alive, then seen out of health or dying, is the goal (in any game mode: his defeat goes into a
/// scene at once). A table without him (the level reloading after a death) is not his defeat.
fn goal(c: &Cpu, s: &mut State, lvl: i32) {
	if s.goal || lvl != FINAL_LEVEL {
		return;
	}
	let ram = &c.mem.ram[..];
	let (start, end) = crate::retail::table(lvl as u32);
	let (start, end) = (word(ram, start) & MASK, word(ram, end) & MASK);
	let mut alive = None;
	if start != 0 && end > start && end - start < 0x10_0000 {
		for m in (start..end).step_by(0x100) {
			// State 0: just placed (after a death the level loads again), his init has not set his health yet.
			if word(ram, m + 0xa4) >> 16 == FINAL_BOSS && word(ram, m + 0x24) != 0 && byte(ram, m + 0x20) != 0 {
				let pvar = word(ram, m + 0x78) & MASK;
				let health = f32::from_bits(word(ram, pvar + 0x20));
				alive = Some(byte(ram, m + 0x20) < 0x7f && (pvar == 0 || health > 0.0));
			}
		}
	}
	match alive {
		Some(true) => {
			if !s.boss_seen {
				eprintln!("ap: Drek is here");
			}
			s.boss_seen = true;
		}
		Some(false) if s.boss_seen => {
			s.goal = true;
			bridge::send("{\"t\":\"goal\"}".into());
			notice::push("Drek is defeated!".into());
			eprintln!("ap: the goal: Drek defeated");
		}
		_ => {}
	}
}

/// Test aids for the end-to-end runs, from the environment; without them nothing happens. `RAC_AP_TEST_DIE=FIELD`: from that field
/// (in play) the hero takes his last blow the game's way (`kill_hero`), not as a death link (his death goes out). `RAC_AP_TEST_DREK=FIELD`: at that field on Veldin's return, Drek's health is set to 0 (the goal's check, not his defeat scene).
fn test_aids(c: &mut Cpu, s: &mut State, lvl: i32) {
	let due = |name: &str| std::env::var(name).ok().and_then(|v| v.trim().parse::<u64>().ok());
	if let Some(at) = due("RAC_AP_TEST_DIE").filter(|&f| s.fields >= f && !s.aids_done.contains("die")) {
		if MAIN_STACK.contains(&(c.g(29) as u32)) {
			if s.aids_done.insert("die said") {
				eprintln!("ap test: f{} (asked for {at}) the hero takes his last blow (health {})", s.fields, word(&c.mem.ram, HEALTH) as i32);
			}
			let r = kill_hero(c);
			if (word(&c.mem.ram, HEALTH) as i32) <= 0 || r.is_err() {
				s.aids_done.insert("die");
				eprintln!("ap test: health now {} ({r:?})", word(&c.mem.ram, HEALTH) as i32);
			}
		}
	}
	// `RAC_AP_TEST_POKE=FIELD:ADDR=BYTE[,ADDR=wWORD]`: at that field the bytes (or words) a giver's code writes, written once.
	if let Some((at, pokes)) = std::env::var("RAC_AP_TEST_POKE").ok().and_then(|v| v.split_once(':').and_then(|(f, p)| Some((f.trim().parse::<u64>().ok()?, p.to_string())))) {
		if s.fields >= at && s.aids_done.insert("poke") {
			for (a, v) in pokes.split(',').filter_map(|e| e.split_once('=')) {
				let Ok(addr) = u32::from_str_radix(a.trim().trim_start_matches("0x"), 16) else { continue };
				let ram = &mut c.mem.ram[..];
				match v.trim().strip_prefix('w') {
					Some(w) => put_word(ram, addr, w.parse::<i64>().unwrap_or(0) as u32),
					None => put_byte(ram, addr, v.trim().parse().unwrap_or(0)),
				}
			}
			eprintln!("ap test: f{} (asked for {at}) poked {pokes}", s.fields);
		}
	}
	if let Some(at) = due("RAC_AP_TEST_DREK").filter(|&f| s.fields >= f && lvl == FINAL_LEVEL && !s.aids_done.contains("drek")) {
		let ram = &mut c.mem.ram[..];
		let (start, end) = crate::retail::table(lvl as u32);
		let (start, end) = (word(ram, start) & MASK, word(ram, end) & MASK);
		if start != 0 && end > start && end - start < 0x10_0000 {
			for m in (start..end).step_by(0x100) {
				if word(ram, m + 0xa4) >> 16 == FINAL_BOSS && word(ram, m + 0x24) != 0 {
					let pvar = word(ram, m + 0x78) & MASK;
					eprintln!("ap test: f{} (asked for {at}) Drek's health {} -> 0", s.fields, f32::from_bits(word(ram, pvar + 0x20)));
					put_word(ram, pvar + 0x20, 0f32.to_bits());
					s.aids_done.insert("drek");
				}
			}
		}
	}
}

/// Test aid `RAC_AP_TEST_UNLOCK=FIELD:PLANET`: from that field (in play), the game's own `planet.unlock(PLANET)` as an infobot calls
/// it, once. Run with the state let go (the hand asks `unlock_hook`, which takes it).
fn test_unlock(c: &mut Cpu, fields: u64) {
	static DONE: AtomicBool = AtomicBool::new(false);
	let Some((at, p)) = std::env::var("RAC_AP_TEST_UNLOCK").ok().and_then(|v| {
		let (f, p) = v.trim().split_once(':')?;
		Some((f.parse::<u64>().ok()?, p.parse::<u32>().ok()?))
	}) else {
		return;
	};
	if fields < at || DONE.load(Ordering::Relaxed) || !MAIN_STACK.contains(&(c.g(29) as u32)) || p >= PLANETS {
		return;
	}
	let Some(entry) = SPECS.get().and_then(|s| s.unlock).and_then(|r| crate::omni::find_entry(c, r)) else { return };
	DONE.store(true, Ordering::Relaxed);
	eprintln!("ap test: f{fields} (asked for {at}) planet.unlock({p}), unlocked before: {}", byte(&c.mem.ram, UNLOCKED + p));
	let r = crate::omni::run_game(c, entry, |c| c.s(4, p as u64));
	eprintln!("ap test: planet.unlock returned {r:?}, unlocked now: {}", byte(&c.mem.ram, UNLOCKED + p));
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
		"checked" => {
			// What the server has as found (a co-op partner on the same slot, an earlier save): known here too, not sent again.
			if s.seed.is_some() {
				let before = s.found.len();
				s.found.extend(m.get("loc").map(|a| a.arr().iter().filter_map(|v| v.as_num()).map(|n| n as u32).collect::<Vec<_>>()).unwrap_or_default());
				if s.found.len() != before {
					eprintln!("ap: the server has {} more locations as found", s.found.len() - before);
					save(s);
				}
			}
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
