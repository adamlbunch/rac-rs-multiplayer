//! Online multiplayer, the game side (this fork's own; upstream rac-rs has none). The network side is the rac-mp crate (mp/
//! beside rac-rs/: wire format, sessions, interpolation). Here: read the local hero out of the machine's RAM once per field,
//! hand it to the session, take the other players' heroes back and show them as puppets.
//!
//! Off unless RAC_MP is set: without it no socket is opened, nothing is read and no puppet prop exists, so the game runs exactly
//! as upstream's.
//! - `RAC_MP=host` hosts on UDP port 2410, `RAC_MP=host:PORT` on another, `RAC_MP=host:ADDR:PORT` on one address only
//!   (127.0.0.1: this PC); `RAC_MP=join:ADDR` or `join:ADDR:PORT` joins (ADDR an IP address or a host name).
//! - `RAC_MP=mirror` (or `mirror:FIELDS`): no network; one puppet plays the local hero a second (FIELDS) behind. The
//!   offline check of the puppets.
//! - `RAC_MP_NAME`: the name the others see (default "Ratchet").
//! - `RAC_MP_FOLLOW=0`: a client stays where it is when the host travels.
//!
//! Puppets are picture only: each other player in the same level is one more class-0 (Ratchet) moby in the field's snapshot
//! (`puppets`, retail/snapshot.rs), placed, turned, lit and posed by his own animation clock like any moby the renderer
//! mirrors, with the mobys the game keeps on him (Clank, a pack, the held item) as his game placed them (`read_part`). Nothing
//! is written to game RAM, so the game itself does not know they are there (enemies, collision: later phases). The class-0 prop
//! pool they draw with (`pool`) is added to the level's pools (retail/prop_pools.rs) and its model to the level's prop models
//! (viewer/app.rs) only while RAC_MP is set. The wrench draws under a class of its own (`PUPPET_WRENCH`). Names go to the mods'
//! last 2D layer as a list of their own (`draw_names`, omni/draw.rs `submit_mp`).
//!
//! Enemies fight everyone (`stand`, mp/stand.rs): around an enemy's update, the nearest other player stands in for the hero.
//!
//! The shared world (`world_tick`): each game watches the objects the level placed that matter (enemies, crates, pickups) in the
//! moby table, where a placed object keeps its slot. One that goes from a player's game near that player is reported; the host
//! takes the first report and tells the others, and each game removes it with the game's own `delete_moby`, again if it comes
//! back (a player's death restores a checkpoint) or if the player arrives later. This is the only thing that changes a game's
//! RAM, and only when another player removed something. The rewards stay with the player who earned them.
//!
//! Staying in step: clients follow the host to another level (`follow_host`), by the request the game's own level ends make. A
//! player in a scene, a movie, a ship or a sprite screen is not drawn, and nobody is drawn over a local scene (`on_stage`). A
//! name says when its player is paused, in a dialog or shopping. A client that lost the host keeps trying to get back in
//! (`run_net`), and tells the host again what it knows is gone.
//!
//! Threads: `init` on the main thread in `Retail::load` (after `omni::init`); `note_front`, `tick` and `puppets` on the retail
//! worker thread: `tick` after each field the EE ran (after `omni::tick`), `puppets` when that field's snapshot is taken. The
//! session runs on a thread of its own ("mp-net", `run_net`): the game does not tick in parts of the front end or while a
//! level loads, and the players must keep answering each other meanwhile. `WIRE` is what the two hand each other.
//!
//! Addresses: both our discs (PAL and USA v1.00) have the data layout of the OmniLoader binding's "ntsc" column (omni/layout.rs):
//! the hero moby pointer 0x1413d0 (as retail/snapshot.rs reads it), hero base 0x13f350 (position +0x80, state +0x2084), health
//! 0x1415f8, the first item slot's id 0x140408, level word 0x15ed84. The mode and level-loop exit words are overlay data, read
//! through `disc::va` as omni/events.rs does.

mod stand;

use crate::fmath::Fm;
use crate::retail::{disc, euler_from_rows, table, Clock, Culled, Moby};
use ee_oracle::cpu::Cpu;
use rac_mp::{AnimClock, Event, HeroState, Part, PlayerId, Session, Track, WorldKey};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Send the local hero every this many fields (25 or 30 times a second).
const SEND_EVERY: u64 = 2;
/// Show the others this many fields in the past (about 100 ms), so a late or lost state does not show.
const DELAY: f64 = 6.0;
/// The offline mirror's default delay: one second.
const MIRROR_DELAY: f64 = 60.0;
/// Write where the others are to the run log every this many fields.
const LOG_EVERY: u64 = 300;
/// Class types (class record +0x46) the shared world follows: enemies and crates.
const WORLD_TYPES: [i16; 2] = [5, 9];
/// Classes it follows besides those: the gold bolt.
const WORLD_CLASSES: [u16; 1] = [1134];
/// Fields after a level starts before anything counts as gone: the game is still setting the level up.
const WORLD_SETTLE: u32 = 120;
/// Only something this near the local hero (metres) can be his doing.
const WORLD_NEAR: f32 = 80.0;
/// More than this many going at once in one field is the game clearing the level, not a player.
const WORLD_BURST: usize = 8;
/// The most slots a moby table can have that we believe.
const WORLD_SLOTS: u32 = 4096;
/// How long a client waits between tries to get back to a lost host.
const REJOIN_EVERY: Duration = Duration::from_secs(3);
/// Fields between travel requests while a client has not reached the host's level.
const FOLLOW_EVERY: u64 = 600;
/// How far above his feet (metres) a player's name sits, and its text size (1 = the game's 16-pixel line).
const NAME_HEIGHT: f32 = 1.5;
const NAME_SIZE: f32 = 0.75;
/// Puppet props: one per other player.
const PUPPETS: u32 = rac_mp::proto::MAX_PLAYERS as u32 - 1;
/// The puppets' made-up moby addresses (above the 32 MB of RAM, so no real moby has one): the props bind by address.
const PUPPET_ADDR: u32 = 0xF000_0000;

/// The back objects that ride on every hero (607 and Clank, 601), so a level's pools need room for the puppets' too
/// (retail/prop_pools.rs).
pub const PART_CLASSES: [u32; 2] = [607, 601];
/// The class the puppets' wrenches draw as (`puppet_wrench`): no real class has it, and it stays under the pool code's limits.
pub const PUPPET_WRENCH: u32 = 0xFF0;
/// The cells that point at the parts: the back objects, then the held item (retail/snapshot.rs reads the same).
const PART_CELLS: [u32; rac_mp::MAX_PARTS] = [0x14_04D0, 0x14_04D4, crate::retail::HELD_MOBY];

const HERO_MOBY: u32 = 0x14_13D0;
const HERO: u32 = 0x13_F350;
const HEALTH: u32 = 0x14_15F8;
const ITEM: u32 = 0x14_0408;
const LEVEL_WORD: u32 = 0x15_ED84;
const EXIT_WORD: u32 = 0x15_F550;
const MODE_WORD: u32 = 0x15_F5A4;
const MASK: u32 = 0x01ff_ffff;

struct Mp {
	/// The offline mirror's delay in fields (None: a session runs on the network thread).
	mirror: Option<f64>,
	tracks: BTreeMap<PlayerId, Track>,
	/// This field's smoothed heroes of the others, for `puppets`.
	shown: Vec<(PlayerId, HeroState)>,
	/// The local hero's level (-1: none) and game mode this field.
	level: i8,
	mode: u8,
	/// The local hero moby's draw distance (+0x32), which the puppets take.
	draw: i16,
	fields: u64,
	/// What the shared world watches in the level now, by slot: its class and class type. `watched_level` is that level, `settle` the fields left
	/// before something going counts.
	watched: BTreeMap<u16, (u16, i16)>,
	watched_level: i8,
	settle: u32,
	/// What is gone from the shared world, in every level.
	removed: BTreeSet<WorldKey>,
	/// The level last told to the session (-1: none).
	announced: i8,
	/// The host's level while this client is not there yet, and the field of the next request.
	follow: Option<u8>,
	next_follow: u64,
}

static MP: Mutex<Option<Mp>> = Mutex::new(None);

/// What the game and the network thread hand each other.
struct Wire {
	/// The local hero to send (the newest replaces one not sent yet).
	out: Option<HeroState>,
	/// What the session reported since the game last looked.
	events: Vec<Event>,
	/// Everyone in, us included (names for the log).
	roster: Vec<(PlayerId, String)>,
	/// What went from our game, for the session to report.
	gone: Vec<WorldKey>,
	/// The level our game is in, for the session to announce.
	level: Option<u8>,
}

static WIRE: Mutex<Wire> = Mutex::new(Wire { out: None, events: Vec::new(), roster: Vec::new(), gone: Vec::new(), level: None });
/// Hero states held for a game that does not tick (front end, level load): the older ones are dropped past this many events.
const WIRE_KEEP: usize = 2048;
/// `init` ran (a later `Retail::load`, after Load Game or travel, keeps the session).
static STARTED: AtomicBool = AtomicBool::new(false);
/// A session is open: `tick` and `puppets` do nothing (not even take the lock) until then.
static ON: AtomicBool = AtomicBool::new(false);
/// The title screen / front end is running (field_front.rs, every field).
static FRONT: AtomicBool = AtomicBool::new(false);

/// RAC_MP is set: decided once, before the first level loads (the puppet pool and model are part of the level's props).
fn wanted() -> bool {
	static W: OnceLock<bool> = OnceLock::new();
	*W.get_or_init(|| std::env::var("RAC_MP").is_ok_and(|v| !v.trim().is_empty()))
}

/// The class-0 props to set aside for puppets (retail/prop_pools.rs `count_pools`, viewer/app.rs): 0 without RAC_MP.
pub fn pool() -> u32 {
	if wanted() { PUPPETS } else { 0 }
}

/// viewer/app.rs, at a level load (after Ratchet's class is taken out of the props): Ratchet's model and animation as the
/// class-0 prop model the puppets draw with. Nothing without RAC_MP, on the title, or outside retail mode.
pub fn puppet_model(
	l: &crate::level::Level,
	hero: Option<&crate::mobies::MobyClass>,
	front: bool,
	meshes: &mut std::collections::HashMap<u32, crate::mesh::Mesh>,
	anims: &mut std::collections::HashMap<u32, (crate::mobies::MobyClass, crate::anim::Animated)>,
) {
	if pool() == 0 || front || !crate::retail::active() {
		return;
	}
	let Some(c) = hero else { return };
	match crate::anim::animation(l, 0) {
		Ok(Some(a)) if a.sequences.iter().any(|s| !s.frames.is_empty()) => {
			let m = &c.mesh;
			let mesh = crate::mesh::Mesh { name: m.name.clone(), positions: m.positions.clone(), uvs: m.uvs.clone(), faces: m.faces.clone(), colours: Vec::new(), lit: None, strip_phase: m.strip_phase.clone(), normals: m.normals.clone() };
			meshes.insert(0, mesh);
			anims.insert(0, (c.clone(), a));
		}
		r => eprintln!("mp: no puppet model: Ratchet's animation {:?}", r.err()),
	}
}

/// viewer/app.rs, at a level load: the wrench as a prop model under `PUPPET_WRENCH`. Class 71 itself stays out of the props, as
/// upstream keeps it: the local hero's loose wrenches are drawn their own way and must not find a prop.
pub fn puppet_wrench(
	l: &crate::level::Level,
	gadgets: &std::collections::HashMap<u32, crate::mobies::MobyClass>,
	meshes: &mut std::collections::HashMap<u32, crate::mesh::Mesh>,
	anims: &mut std::collections::HashMap<u32, (crate::mobies::MobyClass, crate::anim::Animated)>,
) {
	if pool() == 0 {
		return;
	}
	let Some(c) = gadgets.get(&crate::mobies::WRENCH_CLASS) else { return };
	if let Ok(Some(a)) = crate::anim::gadget_animation(l, crate::mobies::WRENCH_CLASS) {
		let m = &c.mesh;
		meshes.insert(PUPPET_WRENCH, crate::mesh::Mesh { name: m.name.clone(), positions: m.positions.clone(), uvs: m.uvs.clone(), faces: m.faces.clone(), colours: Vec::new(), lit: None, strip_phase: m.strip_phase.clone(), normals: m.normals.clone() });
		anims.insert(PUPPET_WRENCH, (c.clone(), a));
	}
}

/// `Retail::load`: opens the session RAC_MP asks for, once per process.
pub fn init() {
	if STARTED.swap(true, Ordering::Relaxed) || !wanted() {
		return;
	}
	let spec = std::env::var("RAC_MP").unwrap_or_default();
	let name = std::env::var("RAC_MP_NAME").unwrap_or_else(|_| "Ratchet".into());
	let hz = disc::field_rate().round() as u8;
	stand::install();
	let mp = |mirror| Mp { mirror, tracks: BTreeMap::new(), shown: Vec::new(), level: -1, mode: 0, draw: 0, fields: 0, watched: BTreeMap::new(), watched_level: -1, settle: 0, removed: BTreeSet::new(), announced: -1, follow: None, next_follow: 0 };
	let started = match spec.trim().strip_prefix("mirror") {
		Some(rest) => match rest.strip_prefix(':').map(str::parse::<f64>) {
			None if rest.is_empty() => Ok(mp(Some(MIRROR_DELAY))),
			Some(Ok(d)) if d >= 1.0 => Ok(mp(Some(d))),
			_ => Err(format!("bad mirror delay {rest:?}")),
		},
		None => open(&spec, &name, hz).and_then(|(s, host)| {
			let what = if s.is_host() { "hosting" } else { "joining" };
			eprintln!("mp: {what} as {name:?} at {hz} ticks a second, local address {:?}", s.local_addr().ok());
			let again = host.map(|h| (h, name.clone(), hz));
			std::thread::Builder::new().name("mp-net".into()).spawn(move || run_net(s, again)).map_err(|e| e.to_string())?;
			Ok(mp(None))
		}),
	};
	match started {
		Ok(m) => {
			if let Some(d) = m.mirror {
				eprintln!("mp: offline mirror, a puppet {d} fields behind");
			}
			*lock() = Some(m);
			ON.store(true, Ordering::Relaxed);
		}
		Err(e) => eprintln!("mp: RAC_MP={spec:?} not started: {e}"),
	}
}

fn wire() -> MutexGuard<'static, Wire> {
	WIRE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The network thread: sends what the game left in `WIRE`, polls the session every few milliseconds whether the game ticks or
/// not, and leaves what came in for the game. A client that lost the host (`again`: the host, the name, the tick rate) keeps
/// joining again; one the host refused stops.
fn run_net(mut s: Session, again: Option<(SocketAddr, String, u8)>) {
	loop {
		let (out, gone, level) = {
			let mut w = wire();
			(w.out.take(), std::mem::take(&mut w.gone), w.level.take())
		};
		if let Some(h) = out {
			s.send_state(&h);
		}
		gone.into_iter().for_each(|k| s.report(k));
		level.into_iter().for_each(|l| s.travel(l));
		let events = s.poll(Instant::now());
		let refused = events.iter().any(|e| matches!(e, Event::Rejected(_)));
		{
			let mut w = wire();
			w.events.extend(events);
			if w.events.len() > WIRE_KEEP {
				// Nobody read them for a long while: the hero states are stale, the joins and leaves still count.
				w.events.retain(|e| !matches!(e, Event::State { .. }));
			}
			if w.roster.as_slice() != s.roster() {
				w.roster = s.roster().to_vec();
			}
		}
		if s.is_over() {
			let Some((host, name, hz)) = again.as_ref().filter(|_| !refused) else { return };
			std::thread::sleep(REJOIN_EVERY);
			if let Ok(n) = Session::join(*host, name, *hz, Instant::now()) {
				s = n;
			}
		}
		std::thread::sleep(Duration::from_millis(2));
	}
}

/// The session RAC_MP asks for, and the host's address when it is a join.
fn open(spec: &str, name: &str, hz: u8) -> Result<(Session, Option<SocketAddr>), String> {
	let spec = spec.trim();
	if spec == "host" || spec.starts_with("host:") {
		// host, host:PORT (every network), or host:ADDR:PORT (one address: 127.0.0.1 keeps it to this PC).
		let bind: SocketAddr = match spec.strip_prefix("host:") {
			None => ([0, 0, 0, 0], rac_mp::DEFAULT_PORT).into(),
			Some(p) if !p.contains(':') => ([0, 0, 0, 0], p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?).into(),
			Some(a) => a.parse().map_err(|_| format!("bad address {a:?}"))?,
		};
		return Session::host(bind, name, hz).map(|s| (s, None)).map_err(|e| e.to_string());
	}
	if let Some(at) = spec.strip_prefix("join:") {
		let at = if at.contains(':') { at.to_string() } else { format!("{at}:{}", rac_mp::DEFAULT_PORT) };
		let addr: SocketAddr = at.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or_else(|| format!("no address for {at:?}"))?;
		return Session::join(addr, name, hz, Instant::now()).map(|s| (s, Some(addr))).map_err(|e| e.to_string());
	}
	Err("expected host, host:PORT, host:ADDR:PORT, join:ADDR, join:ADDR:PORT, mirror or mirror:FIELDS".into())
}

fn lock() -> MutexGuard<'static, Option<Mp>> {
	MP.lock().unwrap_or_else(|e| e.into_inner())
}

/// `retail/worker/field_front.rs`, once per field: whether the front end is running (there is no level to be in).
pub fn note_front(front: bool) {
	FRONT.store(front, Ordering::Relaxed);
}

/// After each field the EE ran, on the worker thread. Changes the game only to remove what the shared world says is gone.
pub fn tick(c: &mut Cpu) {
	if !ON.load(Ordering::Relaxed) {
		return;
	}
	let mut g = lock();
	let Some(mp) = g.as_mut() else { return };
	mp.fields += 1;
	let me = read_hero(&c.mem.ram, mp.fields as u32);
	mp.level = me.level;
	mp.mode = me.mode;
	// +0x32 is the high half of the word at +0x30.
	mp.draw = hero_moby(&c.mem.ram).map_or(0, |a| (rd(&c.mem.ram, a + 0x30) >> 16) as i16);
	let events = if mp.mirror.is_some() {
		mp.tracks.entry(1).or_default().push(me);
		Vec::new()
	} else {
		let mut w = wire();
		if mp.fields % SEND_EVERY == 0 {
			w.out = Some(me);
		}
		std::mem::take(&mut w.events)
	};
	for ev in events {
		match ev {
			Event::State { id, hero } => mp.tracks.entry(id).or_default().push(hero),
			Event::Joined { id, name } => eprintln!("mp: {name:?} joined as player {id}"),
			Event::Left { id } => {
				mp.tracks.remove(&id);
				eprintln!("mp: player {id} left");
			}
			Event::Connected { id } => {
				eprintln!("mp: in, as player {id}");
				// Back after losing the host, perhaps a new one: it hears again what this game knows is gone.
				wire().gone.extend(mp.removed.iter().copied());
			}
			Event::Rejected(why) => eprintln!("mp: the host refused us: {why:?}"),
			Event::Lost => {
				mp.tracks.clear();
				eprintln!("mp: the host is gone, trying to get back in");
			}
			Event::Removed(key) => {
				mp.removed.insert(key);
			}
			Event::Travel(level) if follows() => {
				mp.follow = Some(level);
				mp.next_follow = 0;
			}
			Event::Travel(_) => {}
		}
	}
	if mp.mirror.is_none() {
		if me.level >= 0 && me.level != mp.announced {
			mp.announced = me.level;
			wire().level = Some(me.level as u8);
		}
		follow_host(c, mp, &me);
		world_tick(c, mp, &me);
	}
	let delay = mp.mirror.unwrap_or(DELAY);
	mp.shown = mp.tracks.iter_mut().filter_map(|(id, t)| Some((*id, t.sample(delay)?))).collect();
	// Enemies fight whoever is nearest, among the players in this level, in play and alive.
	stand::set_targets(mp.shown.iter().filter(|(_, h)| h.level == mp.level && mp.level >= 0 && h.mode == 0 && h.health > 0).map(|(_, h)| h.pos).collect());
	let roster = wire().roster.clone();
	draw_names(c, mp, &roster);
	if mp.fields % LOG_EVERY == 0 {
		eprintln!("mp: enemy updates with a stand-in {}, blows dropped {}", stand::STOOD.load(Ordering::Relaxed), stand::DROPPED.load(Ordering::Relaxed));
		for (id, h) in &mp.shown {
			let who = roster.iter().find(|(i, _)| i == id).map_or("mirror", |(_, n)| n.as_str());
			let parts: Vec<u16> = h.parts.iter().map(|p| p.class).take_while(|&c| c != 0).collect();
			eprintln!("mp: {who} (player {id}) level {} at ({:.1}, {:.1}, {:.1}) seq {} health {} parts {parts:?}", h.level, h.pos[0], h.pos[1], h.pos[2], h.anim.new_seq, h.health);
		}
	}
}

/// RAC_MP_FOLLOW=0 turns following the host off.
fn follows() -> bool {
	std::env::var("RAC_MP_FOLLOW").map_or(true, |v| v != "0")
}

/// A client whose host is in another level asks the game to go there, in play only, by the request the game's own level ends make
/// (retail/worker/field_run.rs writes the same three words). The game then runs its own trip.
fn follow_host(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let Some(to) = mp.follow else { return };
	if me.level == to as i8 {
		mp.follow = None;
		return;
	}
	if me.level >= 0 && me.mode == 0 && mp.fields >= mp.next_follow && !disc::extras_off() {
		mp.next_follow = mp.fields + FOLLOW_EVERY;
		eprintln!("mp: following the host to level {to}");
		c.mem.wr32(disc::va(0x15_F5A0u32), to as u32);
		c.mem.wr32(disc::va(0x15_F5B8u32), 1);
		c.mem.wr32(disc::va(EXIT_WORD), 1);
	}
}

/// One field of the shared world, in the level the local hero is in: walks the placed slots of the moby table, reports what went
/// from this game (near the hero, in play, alive, not a whole level clearing) and removes what the host says is gone.
fn world_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if me.level < 0 {
		mp.watched.clear();
		mp.watched_level = -1;
		return;
	}
	if mp.watched_level != me.level {
		mp.watched.clear();
		mp.watched_level = me.level;
		mp.settle = WORLD_SETTLE;
	}
	mp.settle = mp.settle.saturating_sub(1);
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let (cell, _) = table(level as u32);
	// The table: the placed objects from `start`, the spawned ones from `spawned` (omni/mobys.rs reads the same cells).
	let (start, spawned) = (rd(ram, cell) & MASK, rd(ram, cell + 4) & MASK);
	let slots = spawned.wrapping_sub(start) / 0x100;
	if start == 0 || spawned <= start || (spawned - start) % 0x100 != 0 || slots > WORLD_SLOTS {
		return;
	}
	let (mut gone, mut delete) = (Vec::new(), Vec::new());
	// Slot 0 is the hero.
	for slot in 1..slots {
		let a = start + slot * 0x100;
		let (class, s) = ((rd(ram, a + 0xa4) >> 16) as u16, slot as u16);
		if rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 {
			if mp.removed.contains(&WorldKey { level, slot: s, class }) {
				delete.push((a, s));
			} else if !mp.watched.contains_key(&s) {
				let kind = (rd(ram, (rd(ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
				if WORLD_TYPES.contains(&kind) || WORLD_CLASSES.contains(&class) {
					mp.watched.insert(s, (class, kind));
				}
			}
		} else if let Some((class, kind)) = mp.watched.remove(&s) {
			let (x, y, z) = (rd_f(ram, a + 0x10) - me.pos[0], rd_f(ram, a + 0x14) - me.pos[1], rd_f(ram, a + 0x18) - me.pos[2]);
			if x * x + y * y + z * z < WORLD_NEAR * WORLD_NEAR {
				gone.push((WorldKey { level, slot: s, class }, kind));
			}
		}
	}
	// In play, alive and settled: anything else that takes objects away (a scene, a death, a level starting) is not a player's doing.
	if mp.settle == 0 && me.mode == 0 && me.health > 0 && gone.len() <= WORLD_BURST {
		for (k, kind) in gone {
			if mp.removed.insert(k) {
				eprintln!("mp: gone here: level {} slot {} class {} type {kind}", k.level, k.slot, k.class);
				wire().gone.push(k);
			}
		}
	}
	if me.mode == 0 {
		for (a, s) in delete {
			mp.watched.remove(&s);
			match crate::omni::delete_moby(c, a) {
				Ok(()) => eprintln!("mp: removed for another player: level {level} slot {s}"),
				Err(e) => eprintln!("mp: could not remove slot {s}: {e}"),
			}
		}
	}
}

/// Whether a player in game mode `mode` stands in the world for the others: in play, or paused, in a dialog or shopping, where his
/// hero waits where he is. Not in a movie, a scene, a ship or a sprite screen (mode 0 gameplay, 1 movie, 2 scene, 3 pause, 4 dialog,
/// 5 vendor, 6 ship, 7 sprite screen).
fn on_stage(mode: u8) -> bool {
	matches!(mode, 0 | 3 | 4 | 5)
}

/// The names above the puppets, as a text list for the last 2D layer (omni/draw.rs `submit_mp`). A list every field, an empty one
/// with nobody to name, so a name leaves with its player. Only in play (mode 0): a scene or a menu has the screen to itself.
fn draw_names(c: &Cpu, mp: &Mp, roster: &[(PlayerId, String)]) {
	let (mut cmds, mut pool) = (Vec::new(), Vec::new());
	for (id, h) in mp.shown.iter().filter(|(_, h)| h.level == mp.level && mp.level >= 0 && mp.mode == 0 && on_stage(h.mode)) {
		let name = roster.iter().find(|(i, _)| i == id).map_or("Mirror", |(_, n)| n.as_str());
		let name = format!("{name}{}", match h.mode {
			3 => " (paused)",
			4 => " (dialog)",
			5 => " (shopping)",
			_ => "",
		});
		let head = [h.pos[0], h.pos[1], h.pos[2] + NAME_HEIGHT];
		let Some(([x, y], true)) = crate::omni::screen_of(c, head) else { continue };
		let text_off = pool.len() as u32;
		pool.extend_from_slice(name.as_bytes());
		cmds.push(crate::omni::OmniDrawCmd { kind: crate::omni::draw_quads::KIND_TEXT, layer: 0, align: 1, mod_index: 0, x, y: y - NAME_SIZE * crate::omni::draw_quads::LINE, w: NAME_SIZE, h: 0.0, color: 0xFFFF_FFFF, color2: 0x0000_00C0, value: 0.0, icon: 0, text_off, text_len: name.len() as u32, reserved0: 0, reserved1: 0 });
	}
	crate::omni::draw::submit_mp(&cmds, &pool);
}

/// retail/snapshot.rs, when a field's snapshot is taken: the other players in the local hero's level, as class-0 mobys.
pub fn puppets() -> Vec<Moby> {
	if !ON.load(Ordering::Relaxed) {
		return Vec::new();
	}
	let g = lock();
	let Some(mp) = g.as_ref().filter(|mp| mp.level >= 0) else { return Vec::new() };
	let mut out = Vec::new();
	// Nobody walks through a scene of ours, and a player in a scene of theirs is not here.
	if !on_stage(mp.mode) {
		return out;
	}
	for (id, h) in mp.shown.iter().filter(|(_, h)| h.level == mp.level && on_stage(h.mode)).take(PUPPETS as usize) {
		let addr = PUPPET_ADDR + *id as u32 * 0x100;
		out.push(puppet_moby(addr, 0, h.pos, h.rot, 1.0, &h.anim, h, mp.draw));
		for (k, p) in h.parts.iter().take_while(|p| p.class != 0).enumerate() {
			let pos = [h.pos[0] + p.offset[0], h.pos[1] + p.offset[1], h.pos[2] + p.offset[2]];
			let class = if p.class as u32 == crate::mobies::WRENCH_CLASS { PUPPET_WRENCH } else { p.class as u32 };
			out.push(puppet_moby(addr + 1 + k as u32, class, pos, p.rot, p.scale, &p.anim, h, mp.draw));
		}
	}
	out
}

/// One moby of a puppet, lit as his hero `h`.
fn puppet_moby(addr: u32, class: u32, pos: [f32; 3], rot: [f32; 3], scale: f32, a: &AnimClock, h: &HeroState, draw: i16) -> Moby {
	Moby {
		addr,
		class,
		pos,
		rot,
		mirror: false,
		scale,
		shown: true,
		side_only: None,
		view_culled: Culled::No,
		#[cfg(test)]
		culls: None,
		#[cfg(test)]
		raw: (0, 0, 0),
		rgb: h.rgb,
		light: h.light,
		clock: Clock { cur: a.cur, next: a.next, old_seq: a.old_seq, new_seq: a.new_seq, p: a.p, m: a.m, r: a.r },
		fx: Vec::new(),
		alpha: 0x80,
		draw,
		ovr: None,
	}
}

fn rd(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_f(ram: &[u8], a: u32) -> f32 {
	Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0)
}

fn rd8(ram: &[u8], a: u32) -> u8 {
	ram.get((a & MASK) as usize).copied().unwrap_or(0)
}

/// The hero's moby, when the pointer holds one.
fn hero_moby(ram: &[u8]) -> Option<u32> {
	Some(rd(ram, HERO_MOBY) & MASK).filter(|&a| a != 0 && rd(ram, a + 0x24) != 0)
}

/// The local hero out of the machine's RAM (all 32 MB).
fn read_hero(ram: &[u8], tick: u32) -> HeroState {
	// The PS2 has no NaN or infinity: a word that reads as one here is sent as 0, since the others refuse non-finite floats.
	let f = |a: u32| Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0);
	let v3 = |a: u32| [f(a), f(a + 4), f(a + 8)];
	let mut h = HeroState {
		tick,
		level: level(ram),
		mode: rd(ram, disc::va(MODE_WORD)).min(255) as u8,
		pos: v3(HERO + 0x80),
		state: rd(ram, HERO + 0x2084) as i32,
		health: (rd(ram, HEALTH) as i32).clamp(0, 127) as i8,
		item: rd(ram, ITEM) as i32,
		..Default::default()
	};
	if let Some(m) = hero_moby(ram) {
		h.pos = v3(m + 0x10);
		h.rot = v3(m + 0x40);
		h.anim = AnimClock { cur: rd8(ram, m + 0x50), next: rd8(ram, m + 0x51), old_seq: rd8(ram, m + 0x52), new_seq: rd8(ram, m + 0x53), p: f(m + 0x54), m: f(m + 0x58), r: f(m + 0x5c) };
		h.rgb = [rd8(ram, m + 0x3c), rd8(ram, m + 0x3d), rd8(ram, m + 0x3e)];
		h.light = rd(ram, m + 0x38);
		// The game places the parts by the joints of his matrix (rows +0xc0), which with the modern camera lag the drawn yaw
		// +0x48 the puppet takes: they turn about him by the difference, as retail/strafe.rs `turn_parts` does for the local hero.
		let r0 = v3(m + 0xc0);
		let turn = if r0[0] != 0.0 || r0[1] != 0.0 { h.rot[2] - r0[1].fatan2(r0[0]) } else { 0.0 };
		let mut n = 0;
		for cell in PART_CELLS {
			if let Some(p) = read_part(ram, rd(ram, cell) & MASK, h.pos, turn) {
				h.parts[n] = p;
				n += 1;
			}
		}
	}
	h
}

/// The part in the moby at `a`: none when the cell is empty, the slot is free or the game hides it.
fn read_part(ram: &[u8], a: u32, hero: [f32; 3], turn: f32) -> Option<Part> {
	let f = |a: u32| Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0);
	let v3 = |a: u32| [f(a), f(a + 4), f(a + 8)];
	let header = rd(ram, a + 0x24) & MASK;
	let class = (rd(ram, a + 0xa4) >> 16) as u16;
	if a == 0 || header == 0 || class == 0 || rd8(ram, a + 0x20) >= 0xfd || rd(ram, a + 0x34) & 0x81 != 0 {
		return None;
	}
	let unit = |v: [f32; 3]| {
		let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
		if l > 0.0 { v.map(|x| x / l) } else { v }
	};
	let (r0, r1, r2) = (unit(v3(a + 0xc0)), unit(v3(a + 0xd0)), unit(v3(a + 0xe0)));
	let mut rot = if r0 == [0.0; 3] { v3(a + 0x40) } else { euler_from_rows(r0, r1, r2) };
	rot[2] += turn;
	let pos = v3(a + 0x10);
	let (sn, cs) = turn.fsin_cos();
	let (x, y) = (pos[0] - hero[0], pos[1] - hero[1]);
	let base = f(header + 0x24);
	Some(Part {
		class,
		offset: [cs * x - sn * y, sn * x + cs * y, pos[2] - hero[2]],
		rot,
		scale: if base != 0.0 { f(a + 0x2c) / base } else { 1.0 },
		anim: AnimClock { cur: rd8(ram, a + 0x50), next: rd8(ram, a + 0x51), old_seq: rd8(ram, a + 0x52), new_seq: rd8(ram, a + 0x53), p: f(a + 0x54), m: f(a + 0x58), r: f(a + 0x5c) },
	})
}

/// The level running (0..=18), or -1: title screen, level loop ending, or no level word.
fn level(ram: &[u8]) -> i8 {
	let l = rd(ram, LEVEL_WORD) as i32;
	if FRONT.load(Ordering::Relaxed) || rd(ram, disc::va(EXIT_WORD)) != 0 || !(0..=18).contains(&l) {
		return -1;
	}
	l as i8
}
