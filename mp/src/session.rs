//! A listen server over one non-blocking UDP socket. The host is player 0 and keeps the list of who is in; a client sends
//! `Hello` until it is welcomed, then its hero state. The host relays each state to the others and sends the roster on every
//! change and every second (states and rosters are unreliable: a newer one replaces a lost one). A player silent for
//! `TIMEOUT` is gone.
//!
//! `poll` takes the time as an argument, so the timeouts can be tested without waiting.

use crate::proto::{self, DecodeError, HeroState, MAX_PACKET, MAX_PLAYERS, Msg, PlayerId, RejectReason};
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(5);
const HELLO_EVERY: Duration = Duration::from_millis(500);
const ROSTER_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
	/// Client: the host let us in as `id`.
	Connected { id: PlayerId },
	/// Client: the host refused us. The session is over.
	Rejected(RejectReason),
	/// Client: the host left, went silent or never answered. The session is over.
	Lost,
	/// Another player came in.
	Joined { id: PlayerId, name: String },
	/// Another player left or went silent.
	Left { id: PlayerId },
	/// Another player's hero.
	State { id: PlayerId, hero: HeroState },
}

struct Peer {
	id: PlayerId,
	name: String,
	addr: SocketAddr,
	heard: Instant,
}

enum Role {
	Host { peers: Vec<Peer>, roster_sent: Option<Instant> },
	Client { host: SocketAddr, me: Option<PlayerId>, hello_sent: Option<Instant>, heard: Instant, done: bool },
}

pub struct Session {
	sock: UdpSocket,
	role: Role,
	name: String,
	tick_hz: u8,
	/// Everyone in, us included, as the host last said (the host's own list on the host).
	roster: Vec<(PlayerId, String)>,
}

impl Session {
	/// Host on `bind` (for example 0.0.0.0:2410). `tick_hz` is the disc's tick rate: players on the other region are refused.
	pub fn host(bind: SocketAddr, name: &str, tick_hz: u8) -> std::io::Result<Session> {
		let name = proto::clean_name(name);
		let role = Role::Host { peers: Vec::new(), roster_sent: None };
		Session::open(bind, role, name.clone(), tick_hz, vec![(0, name)])
	}

	/// Join the host at `host`.
	pub fn join(host: SocketAddr, name: &str, tick_hz: u8, now: Instant) -> std::io::Result<Session> {
		let any: SocketAddr = if host.is_ipv4() { ([0, 0, 0, 0], 0).into() } else { (std::net::Ipv6Addr::UNSPECIFIED, 0).into() };
		let role = Role::Client { host, me: None, hello_sent: None, heard: now, done: false };
		Session::open(any, role, proto::clean_name(name), tick_hz, Vec::new())
	}

	fn open(bind: SocketAddr, role: Role, name: String, tick_hz: u8, roster: Vec<(PlayerId, String)>) -> std::io::Result<Session> {
		let sock = UdpSocket::bind(bind)?;
		sock.set_nonblocking(true)?;
		Ok(Session { sock, role, name, tick_hz, roster })
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
	pub fn send_state(&self, hero: &HeroState) {
		match &self.role {
			Role::Host { peers, .. } => {
				let b = proto::encode(&Msg::State { id: 0, hero: *hero });
				for p in peers {
					self.send_raw(&b, p.addr);
				}
			}
			Role::Client { host, me: Some(id), done: false, .. } => self.send(&Msg::State { id: *id, hero: *hero }, *host),
			Role::Client { .. } => {}
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
		out
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

	fn send_raw(&self, b: &[u8], to: SocketAddr) {
		// Unreliable by design; a full send buffer drops the packet like the network would.
		let _ = self.sock.send_to(b, to);
	}

	fn on_packet(&mut self, b: &[u8], from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		let msg = proto::decode(b);
		if self.is_host() {
			match msg {
				Ok(m) => self.host_msg(m, from, now, out),
				Err(DecodeError::Version(_)) => self.send(&Msg::Reject(RejectReason::Version), from),
				Err(_) => {}
			}
		} else {
			self.client_msg(msg, from, now, out);
		}
	}

	fn host_msg(&mut self, m: Msg, from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		let Role::Host { peers, roster_sent } = &mut self.role else { return };
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
				peers.push(Peer { id, name: name.clone(), addr: from, heard: now });
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
			(Msg::Bye, Some(i)) => {
				let p = peers.remove(i);
				*roster_sent = None;
				out.push(Event::Left { id: p.id });
			}
			// Anything else, or anything from a stranger that is not a Hello, is ignored.
			_ => {}
		}
	}

	fn client_msg(&mut self, msg: Result<Msg, DecodeError>, from: SocketAddr, now: Instant, out: &mut Vec<Event>) {
		let Role::Client { host, me, heard, done, .. } = &mut self.role else { return };
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
			Msg::Bye => {
				*done = true;
				out.push(Event::Lost);
			}
			_ => {}
		}
	}

	fn on_time(&mut self, now: Instant, out: &mut Vec<Event>) {
		match &mut self.role {
			Role::Host { peers, roster_sent } => {
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
			Role::Client { host, me, hello_sent, heard, done } => {
				if *done {
					return;
				}
				if now.saturating_duration_since(*heard) >= TIMEOUT {
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

fn send_msg(sock: &UdpSocket, m: &Msg, to: SocketAddr) {
	let _ = sock.send_to(&proto::encode(m), to);
}

impl Drop for Session {
	fn drop(&mut self) {
		self.close();
	}
}

#[cfg(test)]
mod tests;
