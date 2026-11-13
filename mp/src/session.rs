//! A listen server over one non-blocking UDP socket. The host is player 0 and keeps the list of who is in; a client sends
//! `Hello` until it is welcomed, then its hero state. The host relays each state to the others and sends the roster on every
//! change and every second (states and rosters are unreliable: a newer one replaces a lost one). A client that sent no state
//! for `PING_EVERY` sends a `Ping` (its game may be loading a level). A player silent for `TIMEOUT` is gone.
//!
//! The host also keeps the shared world: the list of what is gone from it. A player reports an object gone (`report`), over the
//! reliable stream of `reliable.rs`; the host takes the first report of each object, tells everyone else, and tells a player who
//! joins later the whole list. It also says which level it is in, and the clients follow (`travel`).
//!
//! Enemies run in one game each: the host says whose (`assign`), that game streams them (`send_enemies`, unreliable like hero
//! states), and a hit on one from another game travels to it (`send_hit`).
//!
//! Objects spawned during play have no slot in common, so each game reports them (`report_spawn`) and the host gives them an id. Two
//! games can spawn the same thing, from the same spawner: a report of the class that was just reported near the same place is a
//! duplicate, and that game's copy stands down.
//!
//! `poll` takes the time as an argument, so the timeouts can be tested without waiting.

use crate::find;
use crate::proto::{self, ArcPose, DecodeError, Draws, EnemyPose, FxEvent, FxQuad, HeroState, Keep, MAX_ARCS, MAX_FX, MAX_PARTICLES, MAX_QUADS, Particle, RopePose, MAX_PACKET, MAX_PLAYERS, MAX_POSES, MAX_VARS, Msg, Name, PlayerId, Progress, RejectReason, Reliable, SPAWNED_ID, WorldKey};
use crate::reliable::Link;
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// A player silent this long is gone. Long enough for a game that stalls a while: a level loading on a slow PC, a busy machine
/// (a Linux VM joiner, its whole machine stalling 3 s at a time while it loaded Novalis, was dropped four times at 5 s).
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// How long a client asks before it gives up on a host that never answered (its game may still be starting).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const HELLO_EVERY: Duration = Duration::from_millis(500);
const ROSTER_EVERY: Duration = Duration::from_secs(1);
const PING_EVERY: Duration = Duration::from_secs(1);
/// Objects the host remembers as gone: more than a whole game has, so a hostile client cannot grow the list for ever.
const MAX_WORLD: usize = 32_768;
/// A spawn report within this far (metres) and this long of one of the same class is the same object from another game.
const SAME_SPAWN_NEAR: f32 = 12.0;
const SAME_SPAWN_SINCE: Duration = Duration::from_secs(8);
/// Stranger addresses remembered (`Event::Stranger`), so a flood of them cannot grow the list.
const MAX_STRANGERS: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
	/// Client: the host let us in as `id`.
	Connected { id: PlayerId },
	/// Client: the host refused us. The session is over.
	Rejected(RejectReason),
	/// Client: the host left, went silent or never answered. The session is over.
	Lost,
	/// Host: packets from this address, which is not a player here (told so with a Bye). Said once an address.
	Stranger(SocketAddr),
	/// Another player came in.
	Joined { id: PlayerId, name: String },
	/// Another player left or went silent.
	Left { id: PlayerId },
	/// Another player's hero.
	State { id: PlayerId, hero: HeroState },
	/// An object is gone from the shared world, by another player's hand.
	Removed(WorldKey),
	/// An object gone from the shared world was brought back by another player's game: it is no longer gone.
	Back(WorldKey),
	/// Client: the host is in this level.
	Travel(u8),
	/// Host: this player took a ship to this level by itself. A team travels together, so the host's game goes there, and the rest
	/// follow the host.
	Went { id: PlayerId, level: u8 },
	/// Client: this player's game runs this enemy from now on.
	Owner { key: WorldKey, owner: PlayerId },
	/// Another player hit an enemy; the game that runs it takes the hit.
	Hit { key: WorldKey, damage: f32, mask: u32 },
	/// The enemies another player's game runs.
	Enemies { id: PlayerId, level: u8, tick: u32, poses: Vec<EnemyPose> },
	/// The machinery another player's game has moving.
	Machines { id: PlayerId, level: u8, tick: u32, poses: Vec<EnemyPose> },
	/// A spawned object has an id: `owner`'s game runs it, where it is called `local`.
	Spawned { id: u16, owner: PlayerId, local: u16, level: u8, class: u16 },
	/// Our spawned object `local` is a copy of another game's: it stands down.
	Duplicate { local: u16, of: u16 },
	/// Another player's blow set off this placed object: this game deals it too.
	Touch { key: WorldKey, damage: f32, mask: u32 },
	/// Another player's game got further: this one gets the same (once each).
	Story(Progress),
	/// Another player's game started a cutscene: this one plays it too, if it is in that level.
	Scene { level: u8, id: i32 },
	/// A placed object's variables as the game that moved it has them.
	Vars { key: WorldKey, state: u8, words: Vec<u32> },
	/// Sounds and effects another player's game made for what it runs.
	Fx { id: PlayerId, level: u8, events: Vec<FxEvent> },
	/// The arcs the enemies another player's game runs keep up.
	Arcs { id: PlayerId, level: u8, tick: u32, arcs: Vec<ArcPose> },
	/// Particles another player's game made near the enemies it runs.
	Particles { id: PlayerId, level: u8, particles: Vec<Particle> },
	/// What an enemy another player's game runs drew.
	Draws { id: PlayerId, level: u8, tick: u32, draws: Draws },
	/// Part of another player's weapon beams of one field.
	Quads { id: PlayerId, level: u8, tick: u32, part: u8, parts: u8, rope: Option<RopePose>, quads: Vec<FxQuad> },
	/// A player's things as the team keeps them (another's, or ours given back): `live`, his game has them now; else a stored copy.
	Keep { name: Name, keep: Keep, live: bool },
}

struct Peer {
	id: PlayerId,
	name: String,
	addr: SocketAddr,
	heard: Instant,
	link: Link,
}

/// A spawn report the host took: the object's id, the game that runs it, and the games that have since reported a copy of it (a bit
/// per player).
struct Report {
	id: u16,
	from: PlayerId,
	level: u8,
	class: u16,
	pos: [f32; 3],
	at: Instant,
	lasting: bool,
	copies: u32,
}

enum Role {
	Host {
		peers: Vec<Peer>,
		roster_sent: Option<Instant>,
		/// What is gone from the world, in the order it went, and the same as a set.
		world: Vec<WorldKey>,
		gone: HashSet<WorldKey>,
		/// The level the host's game is in.
		level: Option<u8>,
		/// Who runs which enemy, for whoever joins later.
		owners: HashMap<WorldKey, PlayerId>,
		/// Spawned objects: the next id, those alive (for whoever joins later), the recent reports, and our own answers for `poll`.
		next_id: u16,
		live: Vec<Reliable>,
		recent: Vec<Report>,
		pending: Vec<Event>,
		/// The team's progress so far, and the newest variables of each object, for whoever joins later.
		story: Vec<Progress>,
		vars: HashMap<WorldKey, Reliable>,
		/// Addresses already said to be strangers (`Event::Stranger`).
		strangers: Vec<SocketAddr>,
		/// Every player's things, the newest of each (by `same_player` name), for whoever joins later.
		keeps: Vec<(Name, Keep)>,
	},
	Client {
		link: Link,
		host: SocketAddr,
		me: Option<PlayerId>,
		hello_sent: Option<Instant>,
		heard: Instant,
		done: bool,
		/// When we last sent the host anything after the welcome; `state_out`: a state went out since the last `poll`.
		sent: Option<Instant>,
		state_out: bool,
	},
}

/// What a session has read, for the run log when a player goes: every datagram, those that would not decode (another version
/// included), those from strangers; and the last that would not decode (its size, its tag byte, why).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
	pub read: u64,
	pub undecodable: u64,
	pub from_strangers: u64,
	pub last_undecodable: Option<(usize, u8, DecodeError)>,
}

pub struct Session {
	sock: UdpSocket,
	role: Role,
	stats: Stats,
	name: String,
	tick_hz: u8,
	/// Everyone in, us included, as the host last said (the host's own list on the host).
	roster: Vec<(PlayerId, String)>,
}

impl Session {
	/// Host on `bind` (for example 0.0.0.0:2410). `tick_hz` is the disc's tick rate: players on the other region are refused.
	pub fn host(bind: SocketAddr, name: &str, tick_hz: u8) -> std::io::Result<Session> {
		let name = proto::clean_name(name);
		let role = Role::Host { peers: Vec::new(), roster_sent: None, world: Vec::new(), gone: HashSet::new(), level: None, owners: HashMap::new(), next_id: 0, live: Vec::new(), recent: Vec::new(), pending: Vec::new(), story: Vec::new(), vars: HashMap::new(), strangers: Vec::new(), keeps: Vec::new() };
		Session::open(bind, role, name.clone(), tick_hz, vec![(0, name)])
	}

	/// Join the host at `host`.
	pub fn join(host: SocketAddr, name: &str, tick_hz: u8, now: Instant) -> std::io::Result<Session> {
		let any: SocketAddr = if host.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { (std::net::Ipv6Addr::UNSPECIFIED, 0).into() };
		let role = Role::Client { link: Link::default(), host, me: None, hello_sent: None, heard: now, done: false, sent: None, state_out: false };
		Session::open(any, role, proto::clean_name(name), tick_hz, Vec::new())
	}

	fn open(bind: SocketAddr, role: Role, name: String, tick_hz: u8, roster: Vec<(PlayerId, String)>) -> std::io::Result<Session> {
		let sock = UdpSocket::bind(bind)?;
		sock.set_nonblocking(true)?;
		Ok(Session { sock, role, stats: Stats::default(), name, tick_hz, roster })
	}

	pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
		self.sock.local_addr()
	}

	/// Our player id: 0 on the host, the host's answer on a client (None until welcomed).
	pub fn id(&self) -> Option<PlayerId> {
		match &self.role {
			Role::Host { .. } => Some(0),
			Role::Client { me, .. } => *me,
		}
	}

	pub fn is_host(&self) -> bool {
		matches!(self.role, Role::Host { .. })
	}

	/// A client whose host refused it, left or went silent: nothing more will happen.
	pub fn is_over(&self) -> bool {
		matches!(self.role, Role::Client { done: true, .. })
	}

	/// Everyone in (id, name), us included.
	pub fn roster(&self) -> &[(PlayerId, String)] {
		&self.roster
	}

	pub fn name_of(&self, id: PlayerId) -> Option<&str> {
		self.roster.iter().find(|(i, _)| *i == id).map(|(_, n)| n.as_str())
	}

	/// Our hero, to everyone else.
	pub fn send_state(&mut self, hero: &HeroState) {
		match &mut self.role {
			Role::Host { peers, .. } => {
				let b = proto::encode(&Msg::State { id: 0, hero: *hero });
				for p in peers.iter() {
					let _ = self.sock.send_to(&b, p.addr);
				}
			}
			Role::Client { host, me: Some(id), done: false, state_out, .. } => {
				send_msg(&self.sock, &Msg::State { id: *id, hero: *hero }, *host);
				*state_out = true;
			}
			Role::Client { .. } => {}
		}
	}

	/// An object went from our game: on a client, tell the host; on the host, it is decided, tell everyone.
	pub fn report(&mut self, key: WorldKey) {
		match &mut self.role {
			Role::Host { peers, world, gone, live, owners, .. } => {
				if accept(key, None, peers, world, gone) {
					forget(key, live, owners);
				}
			}
			Role::Client { link, done: false, .. } => link.push(Reliable::Removed(key)),
			Role::Client { .. } => {}
		}
	}

	/// The host's game is in `to` now: the clients are told, and whoever joins later. Nothing on a client.
	pub fn travel(&mut self, to: u8) {
		if let Role::Host { peers, level, recent, .. } = &mut self.role {
			if *level != Some(to) {
				*level = Some(to);
				recent.retain(|r| r.level == to);
				peers.iter_mut().for_each(|p| p.link.push(Reliable::Travel(to)));
			}
		}
	}

	/// Client: our game went to `to` by itself (its player took a ship), not after the host. The host is told, and its game follows.
	/// Nothing on the host.
	pub fn went(&mut self, to: u8) {
		if let Role::Client { link, done: false, .. } = &mut self.role {
			link.push(Reliable::Travel(to));
		}
	}

	/// From now on `owner`'s game runs this enemy or spawned object. Everyone is told, through the host: the host decides in its level,
	/// the first player in a level the host isn't in decides there.
	pub fn assign(&mut self, key: WorldKey, owner: PlayerId) {
		match &mut self.role {
			Role::Host { peers, owners, live, recent, .. } => owns(peers, owners, live, recent, key, owner, None),
			Role::Client { link, done: false, .. } => link.push(Reliable::Owner { key, owner }),
			Role::Client { .. } => {}
		}
	}

	/// Our game spawned an object we follow, known here as `local`. The answer comes as `Event::Spawned` or `Event::Duplicate`.
	pub fn report_spawn(&mut self, local: u16, level: u8, class: u16, pos: [f32; 3], lasting: bool, solo: bool, now: Instant) {
		match &mut self.role {
			Role::Host { peers, next_id, live, recent, pending, .. } => match decide(0, level, class, pos, lasting, solo, now, next_id, recent) {
				Ok(id) => {
					let s = Reliable::Spawned { id, owner: 0, local, level, class };
					live.push(s);
					peers.iter_mut().for_each(|p| p.link.push(s));
					pending.push(Event::Spawned { id, owner: 0, local, level, class });
				}
				Err(of) => pending.push(Event::Duplicate { local, of }),
			},
			Role::Client { link, done: false, .. } => link.push(Reliable::Spawn { local, level, class, pos, lasting, solo }),
			Role::Client { .. } => {}
		}
	}

	/// We hit an enemy another game runs.
	pub fn send_hit(&mut self, key: WorldKey, damage: f32, mask: u32) {
		let hit = Reliable::Hit { key, damage, mask };
		match &mut self.role {
			Role::Host { peers, .. } => peers.iter_mut().for_each(|p| p.link.push(hit)),
			Role::Client { link, done: false, .. } => link.push(hit),
			Role::Client { .. } => {}
		}
	}

	/// Sounds and effects our game made for what it runs, in as many datagrams as they need. Lost ones stay lost.
	pub fn send_fx(&mut self, level: u8, events: &[FxEvent]) {
		for part in events.chunks(MAX_FX) {
			let m = Msg::Fx { id: self.id().unwrap_or(0), level, events: part.to_vec() };
			match &mut self.role {
				Role::Host { peers, .. } => peers.iter().for_each(|p| send_msg(&self.sock, &m, p.addr)),
				Role::Client { host, me: Some(_), done: false, .. } => send_msg(&self.sock, &m, *host),
				Role::Client { .. } => {}
			}
		}
	}

	/// Our player's things (`Keep`), sent when they changed: the team keeps them.
	pub fn keep(&mut self, keep: Keep) {
		let name = Name::new(&self.name);
		match &mut self.role {
			Role::Host { peers, keeps, .. } => {
				store_keep(keeps, name, keep, true);
				peers.iter_mut().for_each(|q| q.link.push(Reliable::Keep { name, keep, live: true }));
			}
			Role::Client { link, done: false, .. } => link.push(Reliable::Keep { name, keep, live: true }),
			Role::Client { .. } => {}
		}
	}

	/// Client, back in: a player's things this game remembers. The host keeps them if it has none of that player (it started again).
	pub fn remembered(&mut self, name: Name, keep: Keep) {
		if let Role::Client { link, done: false, .. } = &mut self.role {
			link.push(Reliable::Keep { name, keep, live: false });
		}
	}

	/// The arcs the enemies our game runs keep up.
	pub fn send_arcs(&mut self, level: u8, tick: u32, arcs: &[ArcPose]) {
		let m = Msg::Arcs { id: self.id().unwrap_or(0), level, tick, arcs: arcs[..arcs.len().min(MAX_ARCS)].to_vec() };
		match &mut self.role {
			Role::Host { peers, .. } => peers.iter().for_each(|p| send_msg(&self.sock, &m, p.addr)),
			Role::Client { host, me: Some(_), done: false, .. } => send_msg(&self.sock, &m, *host),
			Role::Client { .. } => {}
		}
	}

	/// Particles our game made near the enemies it runs.
	pub fn send_particles(&mut self, level: u8, particles: &[Particle]) {
		for chunk in particles.chunks(MAX_PARTICLES) {
			let m = Msg::Particles { id: self.id().unwrap_or(0), level, particles: chunk.to_vec() };
			self.send_unreliable(&m);
		}
	}

	/// What an enemy our game runs drew this field.
	pub fn send_draws(&mut self, level: u8, tick: u32, draws: &Draws) {
		let m = Msg::Draws { id: self.id().unwrap_or(0), level, tick, draws: draws.clone() };
		self.send_unreliable(&m);
	}

	/// Our player's weapon beams this field (and his rope), in as many datagrams as they take: 8 at most.
	pub fn send_quads(&mut self, level: u8, tick: u32, rope: Option<RopePose>, quads: &[FxQuad]) {
		let chunks: Vec<&[FxQuad]> = if quads.is_empty() { vec![&[][..]] } else { quads.chunks(MAX_QUADS).take(8).collect() };
		let parts = chunks.len() as u8;
		for (k, c) in chunks.into_iter().enumerate() {
			let m = Msg::Quads { id: self.id().unwrap_or(0), level, tick, part: k as u8, parts, rope: if k == 0 { rope } else { None }, quads: c.to_vec() };
			self.send_unreliable(&m);
		}
	}

	/// To every other player: the host sends to each, a client to the host (which passes it on).
	fn send_unreliable(&self, m: &Msg) {
		match &self.role {
			Role::Host { peers, .. } => peers.iter().for_each(|p| send_msg(&self.sock, m, p.addr)),
			Role::Client { host, me: Some(_), done: false, .. } => send_msg(&self.sock, m, *host),
			Role::Client { .. } => {}
		}
	}

	/// Our game got further: every other game gets the same, whoever joins later too.
	pub fn progress(&mut self, p: Progress) {
		match &mut self.role {
			Role::Host { peers, story, .. } => {
				if !story.contains(&p) {
					story.push(p);
					peers.iter_mut().for_each(|q| q.link.push(Reliable::Story(p)));
				}
			}
			Role::Client { link, done: false, .. } => link.push(Reliable::Story(p)),
			Role::Client { .. } => {}
		}
	}

	/// Our game brought back an object reported gone (an enemy the level sends again): it is no longer gone for anyone, and its
	/// next end counts again. The host forgets it, so whoever joins later is not told it is gone.
	pub fn back(&mut self, key: WorldKey) {
		match &mut self.role {
			Role::Host { peers, world, gone, .. } => {
				if gone.remove(&key) {
					world.retain(|k| *k != key);
				}
				peers.iter_mut().for_each(|p| p.link.push(Reliable::Back(key)));
			}
			Role::Client { link, done: false, .. } => link.push(Reliable::Back(key)),
			Role::Client { .. } => {}
		}
	}

	/// A cutscene our game started in `level`: every other game in that level plays it too.
	pub fn send_scene(&mut self, level: u8, id: i32) {
		let s = Reliable::Scene { level, id };
		match &mut self.role {
			Role::Host { peers, .. } => peers.iter_mut().for_each(|p| p.link.push(s)),
			Role::Client { link, done: false, .. } => link.push(s),
			Role::Client { .. } => {}
		}
	}

	/// A placed object's variables as our game has them (it moved it): every other game takes them, whoever joins later too.
	pub fn send_vars(&mut self, key: WorldKey, state: u8, vars: &[u32]) {
		let n = vars.len().min(MAX_VARS);
		let mut words = [0u32; MAX_VARS];
		words[..n].copy_from_slice(&vars[..n]);
		let v = Reliable::Vars { key, state, len: n as u8, words };
		match &mut self.role {
			Role::Host { peers, vars, .. } => {
				vars.insert(key, v);
				peers.iter_mut().for_each(|p| p.link.push(v));
			}
			Role::Client { link, done: false, .. } => link.push(v),
			Role::Client { .. } => {}
		}
	}

	/// A player's blow set off this placed object here: every other game deals it too.
	pub fn touch(&mut self, key: WorldKey, damage: f32, mask: u32) {
		let t = Reliable::Touch { key, damage, mask };
		match &mut self.role {
			Role::Host { peers, .. } => peers.iter_mut().for_each(|p| p.link.push(t)),
			Role::Client { link, done: false, .. } => link.push(t),
			Role::Client { .. } => {}
		}
	}

	/// The enemies our game runs, in as many datagrams as they need.
	pub fn send_enemies(&mut self, level: u8, tick: u32, poses: &[EnemyPose]) {
		self.send_poses(false, level, tick, poses);
	}

	/// The machinery our game has moving, in as many datagrams as it needs.
	pub fn send_machines(&mut self, level: u8, tick: u32, poses: &[EnemyPose]) {
		self.send_poses(true, level, tick, poses);
	}

	fn send_poses(&mut self, machines: bool, level: u8, tick: u32, poses: &[EnemyPose]) {
		for part in poses.chunks(MAX_POSES) {
			let (id, poses) = (self.id().unwrap_or(0), part.to_vec());
			let m = if machines { Msg::Machines { id, level, tick, poses } } else { Msg::Enemies { id, level, tick, poses } };
			match &mut self.role {
				Role::Host { peers, .. } => peers.iter().for_each(|p| send_msg(&self.sock, &m, p.addr)),
				Role::Client { host, me: Some(_), done: false, state_out, .. } => {
					send_msg(&self.sock, &m, *host);
					*state_out = true;
				}
				Role::Client { .. } => {}
			}
		}
	}

	/// Reads everything that arrived, resends what is due and drops the silent. Call it once per game tick.
	pub fn poll(&mut self, now: Instant) -> Vec<Event> {
		let mut out = Vec::new();
		let mut buf = [0u8; MAX_PACKET + 1];
		loop {
			match self.sock.recv_from(&mut buf) {
				Ok((n, from)) => self.on_packet(&buf[..n], from, now, &mut out),
				Err(e) if e.kind() == ErrorKind::WouldBlock => break,
				// Windows reports an earlier send that bounced (ICMP port unreachable) on the next receive: not fatal.
				Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
				Err(_) => break,
			}
		}
		self.on_time(now, &mut out);
		if let Role::Host { pending, .. } = &mut self.role {
			out.append(pending);
		}
		out
	}

	/// What this session has read so far.
	pub fn stats(&self) -> Stats {
		self.stats
	}

	/// Client: how long the host has been silent (0 on the host).
	pub fn silent_for(&self, now: Instant) -> Duration {
		match &self.role {
			Role::Client { heard, .. } => now.saturating_duration_since(*heard),
			Role::Host { .. } => Duration::ZERO,
		}
	}

	/// Leave: tell the others.
	pub fn close(&mut self) {
		match &mut self.role {
			Role::Host { peers, .. } => {
				let b = proto::encode(&Msg::Bye);
				for p in peers.drain(..) {
					let _ = self.sock.send_to(&b, p.addr);
				}
			}
			Role::Client { host, done, .. } => {
				if !*done {
					let _ = self.sock.send_to(&proto::encode(&Msg::Bye), *host);
				}
				*done = true;
			}
		}
	}

	fn send(&self, m: &Msg, to: SocketAddr) {
		send_msg(&self.sock, m, to);
	}

	fn on_packet(&mut self, b: &[u8], from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		// The setup window looking for hosts (`find`): a host says who it is; nothing else happens.
		if find::is_query(b) {
			if let Role::Host { peers, level, .. } = &self.role {
				let i = find::Info { version: proto::VERSION, tick_hz: self.tick_hz, players: (peers.len() + 1) as u8, max: MAX_PLAYERS as u8, level: level.map_or(-1, |l| l as i8), name: self.name.clone() };
				let _ = self.sock.send_to(&find::info(&i), from);
			}
			return;
		}
		let msg = proto::decode(b);
		self.stats.read += 1;
		// (the window's version probe, answered below, is not a broken datagram)
		if let Err(e) = msg
			&& e != DecodeError::Version(find::PROBE_VERSION)
		{
			self.stats.undecodable += 1;
			self.stats.last_undecodable = Some((b.len(), b.get(6).copied().unwrap_or(0), e));
		}
		if self.is_host() {
			match msg {
				Ok(m) => self.host_msg(m, from, now, out),
				Err(DecodeError::Version(_)) => self.send(&Msg::Reject(RejectReason::Version), from),
				// A player's packet that would not decode still says the player is there (a client takes any packet from the host
				// so): one bad message must not have it dropped as silent.
				Err(DecodeError::Malformed) => {
					if let Role::Host { peers, .. } = &mut self.role {
						peers.iter_mut().filter(|p| p.addr == from).for_each(|p| p.heard = now);
					}
				}
				Err(DecodeError::Magic) => {}
			}
		} else {
			self.client_msg(msg, from, now, out);
		}
	}

	fn host_msg(&mut self, m: Msg, from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		let Role::Host { peers, roster_sent, world, gone, level, owners, next_id, live, recent, story, vars, strangers, keeps, .. } = &mut self.role else { return };
		let sock = &self.sock;
		let known = peers.iter().position(|p| p.addr == from);
		match (m, known) {
			(Msg::Hello { .. }, Some(i)) => {
				// Our Welcome was lost: say it again.
				peers[i].heard = now;
				send_msg(sock, &Msg::Welcome { id: peers[i].id }, from);
			}
			(Msg::Hello { name, tick_hz }, None) => {
				if tick_hz != self.tick_hz {
					send_msg(sock, &Msg::Reject(RejectReason::Region), from);
					return;
				}
				let Some(id) = (1..MAX_PLAYERS as PlayerId).find(|id| peers.iter().all(|p| p.id != *id)) else {
					send_msg(sock, &Msg::Reject(RejectReason::Full), from);
					return;
				};
				let mut link = Link::default();
				// A late arrival is told everything that is already gone.
				world.iter().for_each(|k| link.push(Reliable::Removed(*k)));
				level.iter().for_each(|l| link.push(Reliable::Travel(*l)));
				owners.iter().for_each(|(key, owner)| link.push(Reliable::Owner { key: *key, owner: *owner }));
				live.iter().for_each(|s| link.push(*s));
				story.iter().for_each(|p| link.push(Reliable::Story(*p)));
				vars.values().for_each(|v| link.push(*v));
				// Stored copies, but not of a player who is playing now (the host, one heard from lately): his name may be a newcomer's
				// too. A game that stopped is silent, so a player back in gets his own.
				let playing = |n: &Name| same_player(n.as_str(), &self.name) || peers.iter().any(|p| same_player(&p.name, n.as_str()) && now.duration_since(p.heard) < KEEP_SILENT);
				keeps.iter().filter(|(n, _)| !playing(n)).for_each(|(name, keep)| link.push(Reliable::Keep { name: *name, keep: *keep, live: false }));
				peers.push(Peer { id, name: name.clone(), addr: from, heard: now, link });
				*roster_sent = None;
				send_msg(sock, &Msg::Welcome { id }, from);
				out.push(Event::Joined { id, name });
			}
			(Msg::State { hero, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let b = proto::encode(&Msg::State { id, hero });
				for p in peers.iter().filter(|p| p.id != id) {
					let _ = sock.send_to(&b, p.addr);
				}
				out.push(Event::State { id, hero });
			}
			(Msg::Ping, Some(i)) => peers[i].heard = now,
			(Msg::Reliable { seq, body }, Some(i)) => {
				peers[i].heard = now;
				let (got, ack) = peers[i].link.receive(seq, body);
				send_msg(sock, &Msg::Ack { seq: ack }, from);
				let id = peers[i].id;
				match got {
					Some(Reliable::Removed(key)) => {
						if accept(key, Some(id), peers, world, gone) {
							forget(key, live, owners);
							out.push(Event::Removed(key));
						}
					}
					Some(b @ Reliable::Back(key)) => {
						if gone.remove(&key) {
							world.retain(|k| *k != key);
						}
						peers.iter_mut().filter(|q| q.id != id).for_each(|q| q.link.push(b));
						out.push(Event::Back(key));
					}
					Some(s @ Reliable::Scene { level, id: scene }) => {
						peers.iter_mut().filter(|q| q.id != id).for_each(|q| q.link.push(s));
						out.push(Event::Scene { level, id: scene });
					}
					Some(v @ Reliable::Vars { key, state, len, words }) => {
						vars.insert(key, v);
						peers.iter_mut().filter(|q| q.id != id).for_each(|q| q.link.push(v));
						out.push(Event::Vars { key, state, words: words[..len as usize].to_vec() });
					}
					Some(Reliable::Story(p)) => {
						if !story.contains(&p) {
							story.push(p);
							peers.iter_mut().filter(|q| q.id != id).for_each(|q| q.link.push(Reliable::Story(p)));
							out.push(Event::Story(p));
						}
					}
					// The sender's own things are the newest of him; another player's, which a client back in remembers, are kept only
					// when this host has none of that player (it started again since).
					Some(Reliable::Keep { name, keep, live }) => {
						let live = live && same_player(name.as_str(), &peers[i].name);
						if store_keep(keeps, name, keep, live) {
							peers.iter_mut().filter(|q| q.id != id).for_each(|q| q.link.push(Reliable::Keep { name, keep, live }));
							out.push(Event::Keep { name, keep, live });
						}
					}
					Some(Reliable::Touch { key, damage, mask }) => {
						let t = Reliable::Touch { key, damage, mask };
						peers.iter_mut().filter(|p| p.id != id).for_each(|p| p.link.push(t));
						out.push(Event::Touch { key, damage, mask });
					}
					Some(Reliable::Hit { key, damage, mask }) => {
						let hit = Reliable::Hit { key, damage, mask };
						peers.iter_mut().filter(|p| p.id != id).for_each(|p| p.link.push(hit));
						out.push(Event::Hit { key, damage, mask });
					}
					Some(Reliable::Spawn { local, level, class, pos, lasting, solo }) => match decide(id, level, class, pos, lasting, solo, now, next_id, recent) {
						Ok(new) => {
							let s = Reliable::Spawned { id: new, owner: id, local, level, class };
							live.push(s);
							peers.iter_mut().for_each(|p| p.link.push(s));
							out.push(Event::Spawned { id: new, owner: id, local, level, class });
						}
						Err(of) => peers[i].link.push(Reliable::Duplicate { local, of }),
					},
					// The first player in a level the host isn't in gives out its enemies.
					Some(Reliable::Owner { key, owner }) => {
						owns(peers, owners, live, recent, key, owner, Some(id));
						out.push(Event::Owner { key, owner });
					}
					// A client that travelled by itself: the host's game follows it (`Session::went`).
					Some(Reliable::Travel(level)) => out.push(Event::Went { id, level }),
					// Only the host says what an object's id is.
					Some(Reliable::Spawned { .. } | Reliable::Duplicate { .. }) | None => {}
				}
			}
			(Msg::Enemies { level, tick, poses, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Enemies { id, level, tick, poses: poses.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Enemies { id, level, tick, poses });
			}
			(Msg::Machines { level, tick, poses, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Machines { id, level, tick, poses: poses.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Machines { id, level, tick, poses });
			}
			(Msg::Arcs { level, tick, arcs, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Arcs { id, level, tick, arcs: arcs.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Arcs { id, level, tick, arcs });
			}
			(Msg::Particles { level, particles, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Particles { id, level, particles: particles.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Particles { id, level, particles });
			}
			(Msg::Draws { level, tick, draws, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Draws { id, level, tick, draws: draws.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Draws { id, level, tick, draws });
			}
			(Msg::Quads { level, tick, part, parts, rope, quads, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Quads { id, level, tick, part, parts, rope, quads: quads.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Quads { id, level, tick, part, parts, rope, quads });
			}
			(Msg::Fx { level, events, .. }, Some(i)) => {
				peers[i].heard = now;
				let id = peers[i].id;
				let m = Msg::Fx { id, level, events: events.clone() };
				peers.iter().filter(|p| p.id != id).for_each(|p| send_msg(sock, &m, p.addr));
				out.push(Event::Fx { id, level, events });
			}
			(Msg::Ack { seq }, Some(i)) => {
				peers[i].heard = now;
				peers[i].link.ack(seq);
			}
			(Msg::Bye, Some(i)) => {
				let p = peers.remove(i);
				*roster_sent = None;
				out.push(Event::Left { id: p.id });
			}
			// A stranger still playing (a client this host dropped as silent, or from an earlier session on this port): told it is not
			// in, so it joins again at once instead of waiting out its own timeout. Bye is smaller than anything it answers.
			(Msg::State { .. } | Msg::Ping | Msg::Reliable { .. } | Msg::Enemies { .. } | Msg::Machines { .. } | Msg::Fx { .. } | Msg::Arcs { .. } | Msg::Particles { .. } | Msg::Draws { .. } | Msg::Quads { .. } | Msg::Ack { .. }, None) => {
				send_msg(sock, &Msg::Bye, from);
				self.stats.from_strangers += 1;
				if !strangers.contains(&from) {
					if strangers.len() >= MAX_STRANGERS {
						strangers.clear();
					}
					strangers.push(from);
					out.push(Event::Stranger(from));
				}
			}
			// Anything else is ignored.
			_ => {}
		}
	}

	fn client_msg(&mut self, msg: Result<Msg, DecodeError>, from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		let Role::Client { link, host, me, heard, done, .. } = &mut self.role else { return };
		if from != *host || *done {
			return;
		}
		*heard = now;
		let m = match msg {
			Ok(m) => m,
			Err(DecodeError::Version(_)) => Msg::Reject(RejectReason::Version),
			Err(_) => return,
		};
		match m {
			Msg::Welcome { id } if me.is_none() => {
				*me = Some(id);
				out.push(Event::Connected { id });
			}
			Msg::Reject(r) if me.is_none() => {
				*done = true;
				out.push(Event::Rejected(r));
			}
			Msg::Roster(list) => {
				let me = *me;
				for (id, name) in list.iter().filter(|(id, _)| Some(*id) != me && self.roster.iter().all(|(i, _)| i != id)) {
					out.push(Event::Joined { id: *id, name: name.clone() });
				}
				for (id, _) in self.roster.iter().filter(|(id, _)| Some(*id) != me && list.iter().all(|(i, _)| i != id)) {
					out.push(Event::Left { id: *id });
				}
				self.roster = list;
			}
			Msg::State { id, hero } if me.is_some_and(|m| m != id) => out.push(Event::State { id, hero }),
			Msg::Reliable { seq, body } if me.is_some() => {
				let (got, ack) = link.receive(seq, body);
				send_msg(&self.sock, &Msg::Ack { seq: ack }, *host);
				match got {
					Some(Reliable::Removed(key)) => out.push(Event::Removed(key)),
					Some(Reliable::Back(key)) => out.push(Event::Back(key)),
					Some(Reliable::Travel(level)) => out.push(Event::Travel(level)),
					Some(Reliable::Owner { key, owner }) => out.push(Event::Owner { key, owner }),
					Some(Reliable::Hit { key, damage, mask }) => out.push(Event::Hit { key, damage, mask }),
					Some(Reliable::Touch { key, damage, mask }) => out.push(Event::Touch { key, damage, mask }),
					Some(Reliable::Story(p)) => out.push(Event::Story(p)),
					Some(Reliable::Vars { key, state, len, words }) => out.push(Event::Vars { key, state, words: words[..len as usize].to_vec() }),
					Some(Reliable::Scene { level, id }) => out.push(Event::Scene { level, id }),
					Some(Reliable::Keep { name, keep, live }) => out.push(Event::Keep { name, keep, live }),
					Some(Reliable::Spawned { id, owner, local, level, class }) => out.push(Event::Spawned { id, owner, local, level, class }),
					Some(Reliable::Duplicate { local, of }) => out.push(Event::Duplicate { local, of }),
					Some(Reliable::Spawn { .. }) | None => {}
				}
			}
			Msg::Ack { seq } => link.ack(seq),
			Msg::Enemies { id, level, tick, poses } if me.is_some_and(|m| m != id) => out.push(Event::Enemies { id, level, tick, poses }),
			Msg::Fx { id, level, events } if me.is_some_and(|m| m != id) => out.push(Event::Fx { id, level, events }),
			Msg::Machines { id, level, tick, poses } if me.is_some_and(|m| m != id) => out.push(Event::Machines { id, level, tick, poses }),
			Msg::Arcs { id, level, tick, arcs } if me.is_some_and(|m| m != id) => out.push(Event::Arcs { id, level, tick, arcs }),
			Msg::Particles { id, level, particles } if me.is_some_and(|m| m != id) => out.push(Event::Particles { id, level, particles }),
			Msg::Draws { id, level, tick, draws } if me.is_some_and(|m| m != id) => out.push(Event::Draws { id, level, tick, draws }),
			Msg::Quads { id, level, tick, part, parts, rope, quads } if me.is_some_and(|m| m != id) => out.push(Event::Quads { id, level, tick, part, parts, rope, quads }),
			Msg::Bye => {
				*done = true;
				out.push(Event::Lost);
			}
			_ => {}
		}
	}

	fn on_time(&mut self, now: Instant, out: &mut Vec<Event>) {
		match &mut self.role {
			Role::Host { peers, roster_sent, .. } => {
				for p in peers.iter_mut() {
					for m in p.link.due(now) {
						send_msg(&self.sock, &m, p.addr);
					}
				}
				let before = peers.len();
				peers.retain(|p| {
					let alive = now.saturating_duration_since(p.heard) < TIMEOUT;
					if !alive {
						out.push(Event::Left { id: p.id });
					}
					alive
				});
				if peers.len() != before {
					*roster_sent = None;
				}
				if roster_sent.is_none_or(|t| now.saturating_duration_since(t) >= ROSTER_EVERY) {
					*roster_sent = Some(now);
					self.roster.truncate(1);
					self.roster.extend(peers.iter().map(|p| (p.id, p.name.clone())));
					let b = proto::encode(&Msg::Roster(self.roster.clone()));
					for p in peers.iter() {
						let _ = self.sock.send_to(&b, p.addr);
					}
				}
			}
			Role::Client { link, host, me, hello_sent, heard, done, sent, state_out } => {
				if *done {
					return;
				}
				if me.is_some() {
					for m in link.due(now) {
						send_msg(&self.sock, &m, *host);
					}
				}
				if std::mem::take(state_out) {
					*sent = Some(now);
				}
				if me.is_some() && sent.is_none_or(|t| now.saturating_duration_since(t) >= PING_EVERY) {
					*sent = Some(now);
					let _ = self.sock.send_to(&proto::encode(&Msg::Ping), *host);
				}
				// The wait for the host starts with the first Hello, not at `join`: the game may poll its first time much later.
				if hello_sent.is_none() {
					*heard = now;
				}
				let limit = if me.is_some() { TIMEOUT } else { CONNECT_TIMEOUT };
				if now.saturating_duration_since(*heard) >= limit {
					// Silent host, or none answering at all.
					*done = true;
					out.push(Event::Lost);
					return;
				}
				if me.is_none() && hello_sent.is_none_or(|t| now.saturating_duration_since(t) >= HELLO_EVERY) {
					*hello_sent = Some(now);
					let b = proto::encode(&Msg::Hello { name: self.name.clone(), tick_hz: self.tick_hz });
					let _ = self.sock.send_to(&b, *host);
				}
			}
		}
	}
}

/// Host: `owner`'s game runs this from now on. Everyone but `from` (who said so) is told, and whoever joins later.
fn owns(peers: &mut [Peer], owners: &mut HashMap<WorldKey, PlayerId>, live: &mut [Reliable], recent: &mut [Report], key: WorldKey, owner: PlayerId, from: Option<PlayerId>) {
	owners.insert(key, owner);
	for s in live.iter_mut() {
		if let Reliable::Spawned { id, owner: o, .. } = s {
			if *id == key.slot {
				*o = owner;
			}
		}
	}
	// The game it left can come back to the level: what it makes then is a copy.
	recent.iter_mut().filter(|r| r.id == key.slot).for_each(|r| r.from = owner);
	peers.iter_mut().filter(|p| Some(p.id) != from).for_each(|p| p.link.push(Reliable::Owner { key, owner }));
}

/// The host's answer to a spawn report: a new id, or the id of the object it is a copy of, the same class another game just reported
/// near the same place. A lasting object is a copy whenever a game gets there, so its report stays until the host leaves the level.
/// A game is never a copy of itself, and each report has one copy per game: a burst stays a burst. A solo object is new, and nothing
/// is ever a copy of it.
fn decide(from: PlayerId, level: u8, class: u16, pos: [f32; 3], lasting: bool, solo: bool, now: Instant, next_id: &mut u16, recent: &mut Vec<Report>) -> Result<u16, u16> {
	// The host's ids stay below the shots' (`SHOT_ID`).
	if solo {
		let id = SPAWNED_ID | (*next_id & (crate::proto::SHOT_ID - SPAWNED_ID - 1));
		*next_id = next_id.wrapping_add(1);
		return Ok(id);
	}
	let new = |r: &Report| now.saturating_duration_since(r.at) < SAME_SPAWN_SINCE;
	recent.retain(|r| r.lasting || new(r));
	let d2 = |p: [f32; 3]| (0..3).map(|i| (p[i] - pos[i]) * (p[i] - pos[i])).sum::<f32>();
	let me = 1u32 << (from as u32 & 31);
	let same = |r: &&mut Report| r.from != from && r.copies & me == 0 && r.level == level && r.class == class && d2(r.pos) < SAME_SPAWN_NEAR * SAME_SPAWN_NEAR && (r.lasting && lasting || new(r));
	if let Some(r) = recent.iter_mut().find(same) {
		r.copies |= me;
		return Err(r.id);
	}
	let id = SPAWNED_ID | (*next_id & (crate::proto::SHOT_ID - SPAWNED_ID - 1));
	*next_id = next_id.wrapping_add(1);
	if recent.len() < MAX_WORLD {
		recent.push(Report { id, from, level, class, pos, at: now, lasting, copies: 0 });
	}
	Ok(id)
}

/// A spawned object is gone: nobody joining later needs to hear of it.
fn forget(key: WorldKey, live: &mut Vec<Reliable>, owners: &mut HashMap<WorldKey, PlayerId>) {
	live.retain(|s| !matches!(s, Reliable::Spawned { id, .. } if *id == key.slot));
	owners.remove(&key);
}

/// The host decides: the first report of an object stands, and everyone but the reporter is told. False for an object already
/// gone (or a list that is full). A spawned object's id is not used again, and whoever joins later has no use for it.
fn accept(key: WorldKey, from: Option<PlayerId>, peers: &mut [Peer], world: &mut Vec<WorldKey>, gone: &mut HashSet<WorldKey>) -> bool {
	if key.slot < SPAWNED_ID {
		if world.len() >= MAX_WORLD || !gone.insert(key) {
			return false;
		}
		world.push(key);
	}
	for p in peers.iter_mut().filter(|p| Some(p.id) != from) {
		p.link.push(Reliable::Removed(key));
	}
	true
}

/// Unreliable by design: a full send buffer drops the packet as the network would.
fn send_msg(sock: &UdpSocket, m: &Msg, to: SocketAddr) {
	let _ = sock.send_to(&proto::encode(m), to);
}

impl Drop for Session {
	fn drop(&mut self) {
		self.close();
	}
}

/// A player heard from this lately is playing: his stored things are not given to a newcomer of the same name.
const KEEP_SILENT: Duration = Duration::from_secs(2);

/// Two names are the same player whatever their case and the spaces around them.
pub fn same_player(a: &str, b: &str) -> bool {
	a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Keeps `name`'s things: always when `newest` (the player's own game sent them), else only if there are none of that player yet.
/// Whether they were kept.
fn store_keep(keeps: &mut Vec<(Name, Keep)>, name: Name, keep: Keep, newest: bool) -> bool {
	match keeps.iter_mut().find(|(n, _)| same_player(n.as_str(), name.as_str())) {
		Some(k) if newest => {
			*k = (name, keep);
			true
		}
		Some(_) => false,
		None => {
			keeps.push((name, keep));
			true
		}
	}
}

#[cfg(test)]
mod tests;
