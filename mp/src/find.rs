//! Finding hosts: the setup window (setup.ps1, "Play together") asks every PC it can reach whether a game is hosting there, and a
//! host answers with its player name, its level and how many play. This sits outside the versioned game protocol (`proto`), so a
//! window of any version reads a host of any version:
//!
//! - the query (`query`): `RCMQ`, format 1, then zeros up to `QUERY_LEN` bytes. It is longer than any answer, so a sender that
//!   forges its address to point the answer at someone else gets nothing bigger out of a host than it put in.
//! - the answer (`info`): `RCMI`, format 1, the host's protocol `VERSION` (u16), its tick rate (50 PAL, 60 USA), the players in,
//!   the most there can be, its level (-1: none yet, the title screen) and its player name (a length byte and UTF-8).
//!
//! A host from before this (0.4.1.3 and older) ignores the query. It is found with `VERSION_PROBE`, a game datagram of protocol
//! version 0: every host answers a datagram of another version with `Reject(Version)`, its own version in the header
//! (`version_answer`).

use crate::proto::{self, MAGIC, MAX_NAME, MAX_PACKET};

pub const QUERY_MAGIC: [u8; 4] = *b"RCMQ";
pub const INFO_MAGIC: [u8; 4] = *b"RCMI";
const FORMAT: u8 = 1;
/// The size of a query; a longer one (a later format) is answered too.
pub const QUERY_LEN: usize = 48;
/// The protocol version of `VERSION_PROBE`: no game ever spoke it, so a host does not count the probe as a broken datagram.
pub const PROBE_VERSION: u16 = 0;
/// The game datagram every host answers with `Reject(Version)`: the magic, version 0 and no message.
pub const VERSION_PROBE: [u8; 7] = [MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], 0, 0, 0];

/// A host's answer to the query.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
	/// Its game protocol (`proto::VERSION`): players join only a host of their own.
	pub version: u16,
	/// Its disc's tick rate: 50 PAL, 60 USA (players join only a host of their own region).
	pub tick_hz: u8,
	/// Players in, the host included, and the most there can be.
	pub players: u8,
	pub max: u8,
	/// The level the host is in (0..=18), or -1 before its first (the title screen).
	pub level: i8,
	/// The host's player name.
	pub name: String,
}

pub fn query() -> Vec<u8> {
	let mut q = vec![0u8; QUERY_LEN];
	q[..4].copy_from_slice(&QUERY_MAGIC);
	q[4] = FORMAT;
	q
}

/// Whether `b` is a query (of this format or a later one).
pub fn is_query(b: &[u8]) -> bool {
	(QUERY_LEN..=MAX_PACKET).contains(&b.len()) && b[..4] == QUERY_MAGIC && b[4] >= FORMAT
}

pub fn info(i: &Info) -> Vec<u8> {
	let name = proto::clean_name(&i.name);
	let mut w = Vec::with_capacity(12 + name.len());
	w.extend_from_slice(&INFO_MAGIC);
	w.push(FORMAT);
	w.extend_from_slice(&i.version.to_le_bytes());
	w.extend_from_slice(&[i.tick_hz, i.players, i.max, i.level as u8, name.len() as u8]);
	w.extend_from_slice(name.as_bytes());
	w
}

/// An answer, checked field by field (it comes from someone else's PC); None for anything else. A later format may add fields
/// after the name.
pub fn read_info(b: &[u8]) -> Option<Info> {
	if b.get(..4)? != INFO_MAGIC || *b.get(4)? < FORMAT {
		return None;
	}
	let h = b.get(5..12)?;
	let n = h[6] as usize;
	if n > MAX_NAME || (b[4] == FORMAT && b.len() != 12 + n) {
		return None;
	}
	let name = std::str::from_utf8(b.get(12..12 + n)?).ok()?;
	let level = h[5] as i8;
	if name.chars().any(char::is_control) || !(-1..=18).contains(&level) {
		return None;
	}
	Some(Info { version: u16::from_le_bytes([h[0], h[1]]), tick_hz: h[2], players: h[3], max: h[4], level, name: name.to_owned() })
}

/// The protocol version of a host that answered `VERSION_PROBE` (its `Reject(Version)`), None for anything else.
pub fn version_answer(b: &[u8]) -> Option<u16> {
	(b.len() == 8 && b[..4] == MAGIC && b[6] == proto::TAG_REJECT && b[7] == proto::REJECT_VERSION).then(|| u16::from_le_bytes([b[4], b[5]]))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::proto::{Msg, RejectReason, VERSION, encode};

	#[test]
	fn info_goes_and_comes_back() {
		let i = Info { version: VERSION, tick_hz: 60, players: 2, max: 4, level: 3, name: "Raj".into() };
		assert_eq!(read_info(&info(&i)), Some(i.clone()));
		let title = Info { level: -1, name: String::new(), ..i };
		assert_eq!(read_info(&info(&title)), Some(title));
	}

	#[test]
	fn an_answer_is_never_bigger_than_the_query() {
		let i = Info { version: u16::MAX, tick_hz: 60, players: 4, max: 4, level: 18, name: "é".repeat(40) };
		let b = info(&i);
		assert!(b.len() < QUERY_LEN, "{}", b.len());
		assert_eq!(read_info(&b).unwrap().name.len(), MAX_NAME);
	}

	#[test]
	fn broken_answers_are_refused() {
		let good = info(&Info { version: VERSION, tick_hz: 50, players: 1, max: 4, level: -1, name: "Adam".into() });
		for cut in 0..good.len() {
			assert_eq!(read_info(&good[..cut]), None, "cut at {cut}");
		}
		let mut long = good.clone();
		long.push(0);
		assert_eq!(read_info(&long), None);
		let mut ctl = good.clone();
		ctl[12] = b'\n';
		assert_eq!(read_info(&ctl), None);
		let mut lvl = good.clone();
		lvl[10] = 19;
		assert_eq!(read_info(&lvl), None);
		let mut magic = good.clone();
		magic[3] = b'Q';
		assert_eq!(read_info(&magic), None);
		// a later format may say more after the name
		let mut later = good;
		later[4] = 2;
		later.extend_from_slice(&[1, 2, 3]);
		assert_eq!(read_info(&later).map(|i| i.name), Some("Adam".into()));
	}

	#[test]
	fn queries_are_told_apart() {
		assert!(is_query(&query()));
		assert!(!is_query(&query()[..QUERY_LEN - 1]));
		assert!(!is_query(&VERSION_PROBE));
		assert!(!is_query(&encode(&Msg::Ping)));
		let mut later = query();
		later[4] = 2;
		later.extend_from_slice(&[0; 8]);
		assert!(is_query(&later));
	}

	#[test]
	fn the_probe_is_another_version_and_its_answer_names_the_host_version() {
		assert_eq!(proto::decode(&VERSION_PROBE), Err(proto::DecodeError::Version(PROBE_VERSION)));
		assert_eq!(version_answer(&encode(&Msg::Reject(RejectReason::Version))), Some(VERSION));
		assert_eq!(version_answer(&encode(&Msg::Reject(RejectReason::Full))), None);
		assert_eq!(version_answer(&encode(&Msg::Ping)), None);
	}
}
