//! Online multiplayer, the game side (this fork's own; upstream rac-rs has none). The network side is the rac-mp crate (mp/
//! beside rac-rs/: wire format, sessions, interpolation). Here: read the local hero out of the machine's RAM once per field,
//! hand it to the session, take the other players' heroes back and show them as puppets.
//!
//! Off unless RAC_MP is set, and even then it only reads game RAM: without RAC_MP no socket is opened, nothing is read and no
//! puppet prop exists, so the game runs exactly as upstream's.
//! - `RAC_MP=host` hosts on UDP port 2410, `RAC_MP=host:PORT` on another, `RAC_MP=host:ADDR:PORT` on one address only
//!   (127.0.0.1: this PC); `RAC_MP=join:ADDR` or `join:ADDR:PORT` joins (ADDR an IP address or a host name).
//! - `RAC_MP=mirror` (or `mirror:FIELDS`): no network; one puppet plays the local hero a second (FIELDS) behind. The
//!   offline check of the puppets.
//! - `RAC_MP_NAME`: the name the others see (default "Ratchet").
//!
//! Puppets are picture only: each other player in the same level is one more class-0 (Ratchet) moby in the field's snapshot
//! (`puppets`, retail/snapshot.rs), placed, turned, lit and posed by his own animation clock like any moby the renderer
//! mirrors. Nothing is written to game RAM, so the game itself does not know they are there (enemies, collision: later
//! phases). The class-0 prop pool they draw with (`pool`) is added to the level's pools (retail/prop_pools.rs) and its model
//! to the level's prop models (viewer/app.rs) only while RAC_MP is set.
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

use crate::retail::{disc, Clock, Culled, Moby};
use ee_oracle::cpu::Cpu;
use rac_mp::{AnimClock, Event, HeroState, PlayerId, Session, Track};
use std::collections::BTreeMap;
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
/// Puppet props: one per other player.
const PUPPETS: u32 = rac_mp::proto::MAX_PLAYERS as u32 - 1;
/// The puppets' made-up moby addresses (above the 32 MB of RAM, so no real moby has one): the props bind by address.
const PUPPET_ADDR: u32 = 0xF000_0000;

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
	/// The local hero's level this field (-1: none).
	level: i8,
	/// The local hero moby's draw distance (+0x32), which the puppets take.
	draw: i16,
	fields: u64,
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
}

static WIRE: Mutex<Wire> = Mutex::new(Wire { out: None, events: Vec::new(), roster: Vec::new() });
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

/// `Retail::load`: opens the session RAC_MP asks for, once per process.
pub fn init() {
	if STARTED.swap(true, Ordering::Relaxed) || !wanted() {
		return;
	}
	let spec = std::env::var("RAC_MP").unwrap_or_default();
	let name = std::env::var("RAC_MP_NAME").unwrap_or_else(|_| "Ratchet".into());
	let hz = disc::field_rate().round() as u8;
	let mp = |mirror| Mp { mirror, tracks: BTreeMap::new(), shown: Vec::new(), level: -1, draw: 0, fields: 0 };
	let started = match spec.trim().strip_prefix("mirror") {
		Some(rest) => match rest.strip_prefix(':').map(str::parse::<f64>) {
			None if rest.is_empty() => Ok(mp(Some(MIRROR_DELAY))),
			Some(Ok(d)) if d >= 1.0 => Ok(mp(Some(d))),
			_ => Err(format!("bad mirror delay {rest:?}")),
		},
		None => open(&spec, &name, hz).and_then(|s| {
			let what = if s.is_host() { "hosting" } else { "joining" };
			eprintln!("mp: {what} as {name:?} at {hz} ticks a second, local address {:?}", s.local_addr().ok());
			std::thread::Builder::new().name("mp-net".into()).spawn(move || run_net(s)).map_err(|e| e.to_string())?;
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
/// not, and leaves what came in for the game. Ends when a client's session is over (refused, or the host gone).
fn run_net(mut s: Session) {
	loop {
		let out = wire().out.take();
		if let Some(h) = out {
			s.send_state(&h);
		}
		let events = s.poll(Instant::now());
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
			return;
		}
		std::thread::sleep(Duration::from_millis(2));
	}
}

fn open(spec: &str, name: &str, hz: u8) -> Result<Session, String> {
	let spec = spec.trim();
	if spec == "host" || spec.starts_with("host:") {
		// host, host:PORT (every network), or host:ADDR:PORT (one address: 127.0.0.1 keeps it to this PC).
		let bind: SocketAddr = match spec.strip_prefix("host:") {
			None => ([0, 0, 0, 0], rac_mp::DEFAULT_PORT).into(),
			Some(p) if !p.contains(':') => ([0, 0, 0, 0], p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?).into(),
			Some(a) => a.parse().map_err(|_| format!("bad address {a:?}"))?,
		};
		return Session::host(bind, name, hz).map_err(|e| e.to_string());
	}
	if let Some(at) = spec.strip_prefix("join:") {
		let at = if at.contains(':') { at.to_string() } else { format!("{at}:{}", rac_mp::DEFAULT_PORT) };
		let addr: SocketAddr = at.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or_else(|| format!("no address for {at:?}"))?;
		return Session::join(addr, name, hz, Instant::now()).map_err(|e| e.to_string());
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

/// After each field the EE ran, on the worker thread. Reads RAM only.
pub fn tick(c: &Cpu) {
	if !ON.load(Ordering::Relaxed) {
		return;
	}
	let mut g = lock();
	let Some(mp) = g.as_mut() else { return };
	mp.fields += 1;
	let ram = &c.mem.ram[..];
	let me = read_hero(ram, mp.fields as u32);
	mp.level = me.level;
	// +0x32 is the high half of the word at +0x30.
	mp.draw = hero_moby(ram).map_or(0, |a| (rd(ram, a + 0x30) >> 16) as i16);
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
			Event::Connected { id } => eprintln!("mp: in, as player {id}"),
			Event::Rejected(why) => eprintln!("mp: the host refused us: {why:?}"),
			Event::Lost => eprintln!("mp: the host is gone"),
		}
	}
	let delay = mp.mirror.unwrap_or(DELAY);
	mp.shown = mp.tracks.iter_mut().filter_map(|(id, t)| Some((*id, t.sample(delay)?))).collect();
	if mp.fields % LOG_EVERY == 0 {
		let roster = wire().roster.clone();
		for (id, h) in &mp.shown {
			let who = roster.iter().find(|(i, _)| i == id).map_or("mirror", |(_, n)| n.as_str());
			eprintln!("mp: {who} (player {id}) level {} at ({:.1}, {:.1}, {:.1}) seq {} health {}", h.level, h.pos[0], h.pos[1], h.pos[2], h.anim.new_seq, h.health);
		}
	}
}

/// retail/snapshot.rs, when a field's snapshot is taken: the other players in the local hero's level, as class-0 mobys.
pub fn puppets() -> Vec<Moby> {
	if !ON.load(Ordering::Relaxed) {
		return Vec::new();
	}
	let g = lock();
	let Some(mp) = g.as_ref().filter(|mp| mp.level >= 0) else { return Vec::new() };
	mp.shown
		.iter()
		.filter(|(_, h)| h.level == mp.level)
		.take(PUPPETS as usize)
		.map(|(id, h)| {
			let a = h.anim;
			Moby {
				addr: PUPPET_ADDR + *id as u32 * 0x100,
				class: 0,
				pos: h.pos,
				rot: h.rot,
				mirror: false,
				scale: 1.0,
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
				draw: mp.draw,
				ovr: None,
			}
		})
		.collect()
}

fn rd(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
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
	}
	h
}

/// The level running (0..=18), or -1: title screen, level loop ending, or no level word.
fn level(ram: &[u8]) -> i8 {
	let l = rd(ram, LEVEL_WORD) as i32;
	if FRONT.load(Ordering::Relaxed) || rd(ram, disc::va(EXIT_WORD)) != 0 || !(0..=18).contains(&l) {
		return -1;
	}
	l as i8
}
