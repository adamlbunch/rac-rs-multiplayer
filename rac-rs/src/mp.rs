//! Online multiplayer, the game side (this fork's own; upstream rac-rs has none). The network side is the rac-mp crate (mp/
//! beside rac-rs/: wire format, sessions, interpolation). Here: read the local hero out of the machine's RAM once per field,
//! hand it to the session, and take the other players' heroes back.
//!
//! Off unless RAC_MP is set, and then it only reads RAM (phase 0): without RAC_MP no socket is opened and nothing is read, so
//! the game runs exactly as upstream's.
//! - `RAC_MP=host` hosts on UDP port 2410, `RAC_MP=host:PORT` on another; `RAC_MP=join:ADDR` or `join:ADDR:PORT` joins
//!   (ADDR an IP address or a host name).
//! - `RAC_MP_NAME`: the name the others see (default "Ratchet").
//!
//! Phase 0 (now): connect, send the local hero every second field, receive and smooth the others', write who is where to
//! the run log. Phase 1 shows them as puppets. docs/MULTIPLAYER.md has the plan.
//!
//! Threads: `init` on the main thread in `Retail::load` (after `omni::init`); `note_front` and `tick` on the retail worker
//! thread, `tick` after each field the EE ran (after `omni::tick`).
//!
//! Addresses: both our discs (PAL and USA v1.00) have the data layout of the OmniLoader binding's "ntsc" column (omni/layout.rs):
//! hero base 0x13f350 (position +0x80, facing +0x98, state +0x2084), health 0x1415f8, the first item slot's id 0x140408,
//! level word 0x15ed84. The mode and level-loop exit words are overlay data, read through `disc::va` as omni/events.rs does.

use crate::retail::disc;
use ee_oracle::cpu::Cpu;
use rac_mp::{Event, HeroState, PlayerId, Session, Track};
use std::collections::BTreeMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Send the local hero every this many fields (25 or 30 times a second).
const SEND_EVERY: u64 = 2;
/// Show the others this many fields in the past (about 100 ms), so a late or lost state does not show.
const DELAY: f64 = 6.0;
/// Write where the others are to the run log every this many fields.
const LOG_EVERY: u64 = 300;

const HERO: u32 = 0x13_F350;
const HEALTH: u32 = 0x14_15F8;
const ITEM: u32 = 0x14_0408;
const LEVEL_WORD: u32 = 0x15_ED84;
const EXIT_WORD: u32 = 0x15_F550;
const MODE_WORD: u32 = 0x15_F5A4;
const MASK: u32 = 0x01ff_ffff;

struct Mp {
	session: Session,
	tracks: BTreeMap<PlayerId, Track>,
	fields: u64,
}

static MP: Mutex<Option<Mp>> = Mutex::new(None);
/// `init` ran (a later `Retail::load`, after Load Game or travel, keeps the session).
static STARTED: AtomicBool = AtomicBool::new(false);
/// A session is open: `tick` does nothing (not even take the lock) until then.
static ON: AtomicBool = AtomicBool::new(false);
/// The title screen / front end is running (field_front.rs, every field).
static FRONT: AtomicBool = AtomicBool::new(false);

/// `Retail::load`: opens the session RAC_MP asks for, once per process.
pub fn init() {
	if STARTED.swap(true, Ordering::Relaxed) {
		return;
	}
	let Ok(spec) = std::env::var("RAC_MP") else { return };
	let name = std::env::var("RAC_MP_NAME").unwrap_or_else(|_| "Ratchet".into());
	let hz = disc::field_rate().round() as u8;
	match open(&spec, &name, hz) {
		Ok(session) => {
			let what = if session.is_host() { "hosting" } else { "joining" };
			eprintln!("mp: {what} as {name:?} at {hz} ticks a second, local address {:?}", session.local_addr().ok());
			*lock() = Some(Mp { session, tracks: BTreeMap::new(), fields: 0 });
			ON.store(true, Ordering::Relaxed);
		}
		Err(e) => eprintln!("mp: RAC_MP={spec:?} not started: {e}"),
	}
}

fn open(spec: &str, name: &str, hz: u8) -> Result<Session, String> {
	let spec = spec.trim();
	if spec == "host" || spec.starts_with("host:") {
		let port = match spec.strip_prefix("host:") {
			Some(p) => p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?,
			None => rac_mp::DEFAULT_PORT,
		};
		return Session::host(([0, 0, 0, 0], port).into(), name, hz).map_err(|e| e.to_string());
	}
	if let Some(at) = spec.strip_prefix("join:") {
		let at = if at.contains(':') { at.to_string() } else { format!("{at}:{}", rac_mp::DEFAULT_PORT) };
		let addr: SocketAddr = at.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or_else(|| format!("no address for {at:?}"))?;
		return Session::join(addr, name, hz, Instant::now()).map_err(|e| e.to_string());
	}
	Err("expected host, host:PORT, join:ADDR or join:ADDR:PORT".into())
}

fn lock() -> std::sync::MutexGuard<'static, Option<Mp>> {
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
	if mp.fields % SEND_EVERY == 0 {
		mp.session.send_state(&read_hero(&c.mem.ram, mp.fields as u32));
	}
	for ev in mp.session.poll(Instant::now()) {
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
	let log = mp.fields % LOG_EVERY == 0;
	for (id, track) in mp.tracks.iter_mut() {
		// Phase 1 places a puppet here; for now the smoothed state is only logged.
		let Some(h) = track.sample(DELAY) else { continue };
		if log {
			let who = mp.session.name_of(*id).unwrap_or("?");
			eprintln!("mp: {who} (player {id}) level {} at ({:.1}, {:.1}, {:.1}) health {}", h.level, h.pos[0], h.pos[1], h.pos[2], h.health);
		}
	}
}

fn rd(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The local hero out of the machine's RAM (all 32 MB).
fn read_hero(ram: &[u8], tick: u32) -> HeroState {
	// The PS2 has no NaN or infinity: a word that reads as one here is sent as 0, since the others refuse non-finite floats.
	let f = |a: u32| Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0);
	HeroState {
		tick,
		level: level(ram),
		mode: rd(ram, disc::va(MODE_WORD)).min(255) as u8,
		pos: [f(HERO + 0x80), f(HERO + 0x84), f(HERO + 0x88)],
		facing: f(HERO + 0x98),
		anim: rd(ram, HERO + 0x2084) as i32,
		health: (rd(ram, HEALTH) as i32).clamp(0, 127) as i8,
		item: rd(ram, ITEM) as i32,
	}
}

/// The level running (0..=18), or -1: title screen, level loop ending, or no level word.
fn level(ram: &[u8]) -> i8 {
	let l = rd(ram, LEVEL_WORD) as i32;
	if FRONT.load(Ordering::Relaxed) || rd(ram, disc::va(EXIT_WORD)) != 0 || !(0..=18).contains(&l) {
		return -1;
	}
	l as i8
}
