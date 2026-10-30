//! The wire format, version 1. Every datagram is `MAGIC`, `VERSION` (u16), a tag byte and the message. Little-endian,
//! fixed-size fields; a string is a length byte and UTF-8. `decode` checks every length and value, refuses NaN and infinite
//! floats and trailing bytes, and never panics: the bytes come from other people's PCs.

pub const MAGIC: [u8; 4] = *b"RCMP";
/// 2: the hero carries his rotation, animation clock and light (puppets). 3: and the parts on him (`Part`). 4: reliable messages. 5: the host's travel.
pub const VERSION: u16 = 5;
/// The largest datagram sent or accepted: below the usual internet MTU, so nothing is fragmented.
pub const MAX_PACKET: usize = 1200;
/// The longest player name, in bytes.
pub const MAX_NAME: usize = 24;
/// Players in one session, the host included.
pub const MAX_PLAYERS: usize = 4;
/// Parts on one hero: the two back objects (Clank, a pack) and the held item.
pub const MAX_PARTS: usize = 3;

/// 0 is the host, 1..MAX_PLAYERS the others.
pub type PlayerId = u8;

/// One player's hero at one game tick: what the others need to show him.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeroState {
	/// The sender's game tick (fields since multiplayer started there).
	pub tick: u32,
	/// The level (0..=18), or -1 with no level running (title screen, loading).
	pub level: i8,
	/// The game mode word: 0 gameplay, 1 movie, 2 cutscene, 3 pause, 4 dialog, 5 vendor, 6 ship, 7 sprite screen.
	pub mode: u8,
	/// World position in metres (his moby's +0x10).
	pub pos: [f32; 3],
	/// Rotation in radians: x and y tilt, z the way he faces (his moby's +0x40).
	pub rot: [f32; 3],
	/// His animation clock (his moby's +0x50).
	pub anim: AnimClock,
	/// The hero's state word (0 standing, 2 running, 7 jumping, ...).
	pub state: i32,
	/// Nanotech orbs; 0 is dead.
	pub health: i8,
	/// The item in his first slot (8 wrench, 10 Bomb Glove, ...).
	pub item: i32,
	/// His ambient colour and light word (his moby's +0x3c, +0x38): he is lit as where he stands.
	pub rgb: [u8; 3],
	pub light: u32,
	/// What he carries; a slot with class 0 is empty, and the used slots come first.
	pub parts: [Part; MAX_PARTS],
}

/// One moby the game keeps on a hero's joints (Clank on his back, the wrench in his hand), as his game placed it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Part {
	/// The moby class; 0 is an empty slot (class 0 is the hero himself).
	pub class: u16,
	/// Where it sits from the hero, in world axes: smoothing moves it with him.
	pub offset: [f32; 3],
	pub rot: [f32; 3],
	pub scale: f32,
	pub anim: AnimClock,
}

/// A moby's animation clock (+0x50..+0x5f): the frame shown and the next one, the sequence playing and the one asked for, the
/// progress between the two frames, its multiplier and rate. The renderer poses the model from it as it does every moby.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimClock {
	pub cur: u8,
	pub next: u8,
	pub old_seq: u8,
	pub new_seq: u8,
	pub p: f32,
	pub m: f32,
	pub r: f32,
}

/// One object the level placed, the same in every game on one disc: its level, its slot in the moby table (placed objects keep
/// theirs) and its class, which guards against a slot that holds something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldKey {
	pub level: u8,
	pub slot: u16,
	pub class: u16,
}

/// What must arrive (`reliable.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reliable {
	/// An object is gone for good: killed, broken or picked up. Client to host: reported; host to clients: decided.
	Removed(WorldKey),
	/// Host to clients: the host is in this level now, follow.
	Travel(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
	/// No free player slot.
	Full,
	/// The other side speaks another protocol version.
	Version,
	/// The other side runs the other disc region: PAL ticks 50 times a second, USA 60.
	Region,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
	/// Client to host, sent again until answered.
	Hello { name: String, tick_hz: u8 },
	/// Host to client: you are in, as player `id`.
	Welcome { id: PlayerId },
	/// Host to client: you are not.
	Reject(RejectReason),
	/// Host to all, on every change and every second: who is playing.
	Roster(Vec<(PlayerId, String)>),
	/// A hero state. Client to host: `id` is ignored (the host knows who sent it). Host to clients: whose it is.
	State { id: PlayerId, hero: HeroState },
	/// Either way: leaving.
	Bye,
	/// Client to host: still here (sent when no state went out for a while, e.g. while the game loads a level).
	Ping,
	/// Either way: number `seq` of the stream of `reliable.rs`.
	Reliable { seq: u32, body: Reliable },
	/// Either way: everything up to `seq` arrived.
	Ack { seq: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
	/// Not ours: no magic.
	Magic,
	/// Ours, but another protocol version.
	Version(u16),
	/// Ours and this version, but broken.
	Malformed,
}

const TAG_HELLO: u8 = 1;
const TAG_WELCOME: u8 = 2;
const TAG_REJECT: u8 = 3;
const TAG_ROSTER: u8 = 4;
const TAG_STATE: u8 = 5;
const TAG_BYE: u8 = 6;
const TAG_PING: u8 = 7;
const TAG_RELIABLE: u8 = 8;
const TAG_ACK: u8 = 9;
/// Reliable bodies.
const BODY_REMOVED: u8 = 1;
const BODY_TRAVEL: u8 = 2;

pub fn encode(m: &Msg) -> Vec<u8> {
	let mut w = Vec::with_capacity(64);
	w.extend_from_slice(&MAGIC);
	w.extend_from_slice(&VERSION.to_le_bytes());
	match m {
		Msg::Hello { name, tick_hz } => {
			w.push(TAG_HELLO);
			put_str(&mut w, name);
			w.push(*tick_hz);
		}
		Msg::Welcome { id } => w.extend_from_slice(&[TAG_WELCOME, *id]),
		Msg::Reject(r) => w.extend_from_slice(&[TAG_REJECT, reason_byte(*r)]),
		Msg::Roster(list) => {
			let list = &list[..list.len().min(MAX_PLAYERS)];
			w.extend_from_slice(&[TAG_ROSTER, list.len() as u8]);
			for (id, name) in list {
				w.push(*id);
				put_str(&mut w, name);
			}
		}
		Msg::State { id, hero } => {
			w.extend_from_slice(&[TAG_STATE, *id]);
			put_hero(&mut w, hero);
		}
		Msg::Bye => w.push(TAG_BYE),
		Msg::Ping => w.push(TAG_PING),
		Msg::Reliable { seq, body: Reliable::Removed(k) } => {
			w.push(TAG_RELIABLE);
			w.extend_from_slice(&seq.to_le_bytes());
			w.extend_from_slice(&[BODY_REMOVED, k.level]);
			w.extend_from_slice(&k.slot.to_le_bytes());
			w.extend_from_slice(&k.class.to_le_bytes());
		}
		Msg::Reliable { seq, body: Reliable::Travel(level) } => {
			w.push(TAG_RELIABLE);
			w.extend_from_slice(&seq.to_le_bytes());
			w.extend_from_slice(&[BODY_TRAVEL, *level]);
		}
		Msg::Ack { seq } => {
			w.push(TAG_ACK);
			w.extend_from_slice(&seq.to_le_bytes());
		}
	}
	w
}

pub fn decode(b: &[u8]) -> Result<Msg, DecodeError> {
	if b.get(..4) != Some(&MAGIC[..]) {
		return Err(DecodeError::Magic);
	}
	let mut r = Reader { b, i: 4 };
	let v = r.u16().ok_or(DecodeError::Malformed)?;
	if v != VERSION {
		return Err(DecodeError::Version(v));
	}
	if b.len() > MAX_PACKET {
		return Err(DecodeError::Malformed);
	}
	let m = body(&mut r).ok_or(DecodeError::Malformed)?;
	if r.i != b.len() {
		return Err(DecodeError::Malformed);
	}
	Ok(m)
}

fn body(r: &mut Reader) -> Option<Msg> {
	Some(match r.u8()? {
		TAG_HELLO => Msg::Hello { name: r.name()?, tick_hz: r.u8()? },
		TAG_WELCOME => Msg::Welcome { id: r.id()? },
		TAG_REJECT => Msg::Reject(match r.u8()? {
			1 => RejectReason::Full,
			2 => RejectReason::Version,
			3 => RejectReason::Region,
			_ => return None,
		}),
		TAG_ROSTER => {
			let n = r.u8()? as usize;
			if n > MAX_PLAYERS {
				return None;
			}
			let mut list = Vec::with_capacity(n);
			for _ in 0..n {
				list.push((r.id()?, r.name()?));
			}
			Msg::Roster(list)
		}
		TAG_STATE => Msg::State { id: r.id()?, hero: r.hero()? },
		TAG_BYE => Msg::Bye,
		TAG_PING => Msg::Ping,
		TAG_RELIABLE => {
			let seq = r.u32()?;
			let body = match r.u8()? {
				BODY_REMOVED => {
					let level = r.u8().filter(|&l| l <= 18)?;
					Reliable::Removed(WorldKey { level, slot: r.u16()?, class: r.u16()? })
				}
				BODY_TRAVEL => Reliable::Travel(r.u8().filter(|&l| l <= 18)?),
				_ => return None,
			};
			Msg::Reliable { seq, body }
		}
		TAG_ACK => Msg::Ack { seq: r.u32()? },
		_ => return None,
	})
}

fn reason_byte(r: RejectReason) -> u8 {
	match r {
		RejectReason::Full => 1,
		RejectReason::Version => 2,
		RejectReason::Region => 3,
	}
}

/// A name cut to `MAX_NAME` bytes at a character boundary, without control characters (it is drawn in the game later).
pub fn clean_name(name: &str) -> String {
	let mut out = String::new();
	for ch in name.chars().filter(|c| !c.is_control()) {
		if out.len() + ch.len_utf8() > MAX_NAME {
			break;
		}
		out.push(ch);
	}
	out
}

fn put_str(w: &mut Vec<u8>, s: &str) {
	let s = clean_name(s);
	w.push(s.len() as u8);
	w.extend_from_slice(s.as_bytes());
}

fn put_hero(w: &mut Vec<u8>, h: &HeroState) {
	w.extend_from_slice(&h.tick.to_le_bytes());
	w.push(h.level as u8);
	w.push(h.mode);
	for v in h.pos.iter().chain(&h.rot) {
		w.extend_from_slice(&v.to_le_bytes());
	}
	put_anim(w, &h.anim);
	w.extend_from_slice(&h.state.to_le_bytes());
	w.push(h.health as u8);
	w.extend_from_slice(&h.item.to_le_bytes());
	w.extend_from_slice(&h.rgb);
	w.extend_from_slice(&h.light.to_le_bytes());
	let parts = h.parts.iter().take_while(|p| p.class != 0);
	w.push(parts.clone().count() as u8);
	for p in parts {
		w.extend_from_slice(&p.class.to_le_bytes());
		for v in p.offset.iter().chain(&p.rot).chain(std::iter::once(&p.scale)) {
			w.extend_from_slice(&v.to_le_bytes());
		}
		put_anim(w, &p.anim);
	}
}

fn put_anim(w: &mut Vec<u8>, a: &AnimClock) {
	w.extend_from_slice(&[a.cur, a.next, a.old_seq, a.new_seq]);
	for v in [a.p, a.m, a.r] {
		w.extend_from_slice(&v.to_le_bytes());
	}
}

struct Reader<'a> {
	b: &'a [u8],
	i: usize,
}

impl Reader<'_> {
	fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
		let s = self.b.get(self.i..self.i.checked_add(N)?)?;
		self.i += N;
		s.try_into().ok()
	}

	fn u8(&mut self) -> Option<u8> {
		Some(self.take::<1>()?[0])
	}

	fn u16(&mut self) -> Option<u16> {
		Some(u16::from_le_bytes(self.take()?))
	}

	fn u32(&mut self) -> Option<u32> {
		Some(u32::from_le_bytes(self.take()?))
	}

	fn i32(&mut self) -> Option<i32> {
		Some(i32::from_le_bytes(self.take()?))
	}

	fn f32(&mut self) -> Option<f32> {
		Some(f32::from_le_bytes(self.take()?)).filter(|v| v.is_finite())
	}

	fn id(&mut self) -> Option<PlayerId> {
		self.u8().filter(|&id| (id as usize) < MAX_PLAYERS)
	}

	fn name(&mut self) -> Option<String> {
		let n = self.u8()? as usize;
		if n > MAX_NAME {
			return None;
		}
		let bytes = self.b.get(self.i..self.i + n)?;
		self.i += n;
		let s = std::str::from_utf8(bytes).ok()?;
		(!s.chars().any(char::is_control)).then(|| s.to_owned())
	}

	fn hero(&mut self) -> Option<HeroState> {
		let tick = self.u32()?;
		let level = self.u8()? as i8;
		if !(-1..=18).contains(&level) {
			return None;
		}
		let mode = self.u8()?;
		let pos = [self.f32()?, self.f32()?, self.f32()?];
		let rot = [self.f32()?, self.f32()?, self.f32()?];
		let anim = self.anim()?;
		let state = self.i32()?;
		let health = self.u8()? as i8;
		let item = self.i32()?;
		let rgb = self.take::<3>()?;
		let light = self.u32()?;
		let n = self.u8()? as usize;
		if n > MAX_PARTS {
			return None;
		}
		let mut parts = [Part::default(); MAX_PARTS];
		for p in &mut parts[..n] {
			*p = self.part()?;
		}
		Some(HeroState { tick, level, mode, pos, rot, anim, state, health, item, rgb, light, parts })
	}

	fn anim(&mut self) -> Option<AnimClock> {
		let [cur, next, old_seq, new_seq] = self.take::<4>()?;
		Some(AnimClock { cur, next, old_seq, new_seq, p: self.f32()?, m: self.f32()?, r: self.f32()? })
	}

	fn part(&mut self) -> Option<Part> {
		let class = self.u16().filter(|&c| c != 0)?;
		let offset = [self.f32()?, self.f32()?, self.f32()?];
		let rot = [self.f32()?, self.f32()?, self.f32()?];
		Some(Part { class, offset, rot, scale: self.f32()?, anim: self.anim()? })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn hero() -> HeroState {
		let anim = AnimClock { cur: 3, next: 4, old_seq: 12, new_seq: 12, p: 0.25, m: 1.0, r: 0.5 };
		let mut parts = [Part::default(); MAX_PARTS];
		parts[0] = Part { class: 601, offset: [0.0, -0.2, 0.9], rot: [0.0, 0.0, 1.5], scale: 1.0, anim };
		parts[1] = Part { class: 71, offset: [0.3, 0.1, 0.6], rot: [0.1, 0.2, 0.3], scale: 0.9, anim };
		HeroState { tick: 1234, level: 3, mode: 0, pos: [10.5, -2.25, 48.0], rot: [0.0, 0.1, -1.5], anim, state: 7, health: 4, item: 10, rgb: [0x40, 0x50, 0x60], light: 0x1234, parts }
	}

	fn all() -> Vec<Msg> {
		vec![
			Msg::Hello { name: "Ratchet".into(), tick_hz: 60 },
			Msg::Welcome { id: 2 },
			Msg::Reject(RejectReason::Full),
			Msg::Reject(RejectReason::Version),
			Msg::Reject(RejectReason::Region),
			Msg::Roster(vec![(0, "host".into()), (1, "Clank".into()), (3, "Qwark ünïcode".into())]),
			Msg::State { id: 1, hero: hero() },
			Msg::Bye,
			Msg::Ping,
			Msg::Reliable { seq: 77, body: Reliable::Removed(WorldKey { level: 18, slot: 300, class: 1134 }) },
			Msg::Reliable { seq: 78, body: Reliable::Travel(3) },
			Msg::Ack { seq: 77 },
		]
	}

	#[test]
	fn every_message_survives_the_round_trip() {
		for m in all() {
			let b = encode(&m);
			assert!(b.len() <= MAX_PACKET);
			assert_eq!(decode(&b), Ok(m));
		}
	}

	#[test]
	fn every_cut_short_message_is_refused() {
		for m in all() {
			let b = encode(&m);
			for n in 0..b.len() {
				assert!(decode(&b[..n]).is_err(), "{m:?} cut to {n} bytes");
			}
		}
	}

	#[test]
	fn trailing_bytes_are_refused() {
		let mut b = encode(&Msg::Bye);
		b.push(0);
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
	}

	#[test]
	fn foreign_and_other_version_packets_are_told_apart() {
		assert_eq!(decode(b"GET / HTTP/1.1"), Err(DecodeError::Magic));
		let mut b = encode(&Msg::Bye);
		b[4..6].copy_from_slice(&7u16.to_le_bytes());
		assert_eq!(decode(&b), Err(DecodeError::Version(7)));
	}

	#[test]
	fn bad_values_are_refused() {
		let base = encode(&Msg::State { id: 1, hero: hero() });
		// Header (6), tag, id, tick (4): the level byte is at 12, the first float at 14.
		let mut b = base.clone();
		b[12] = 19;
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
		let mut b = base.clone();
		b[14..18].copy_from_slice(&f32::NAN.to_le_bytes());
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
		let mut b = base;
		b[7] = MAX_PLAYERS as u8;
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
	}

	#[test]
	fn a_removal_of_no_level_is_refused() {
		let mut b = encode(&Msg::Reliable { seq: 1, body: Reliable::Removed(WorldKey { level: 1, slot: 2, class: 3 }) });
		// Header (6), tag, seq (4), body tag: the level byte is at 12.
		b[12] = 19;
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
		let mut b = encode(&Msg::Reliable { seq: 1, body: Reliable::Travel(3) });
		b[12] = 19;
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
	}

	#[test]
	fn bad_parts_are_refused() {
		let mut one = hero();
		one.parts[1] = Part::default();
		let base = encode(&Msg::State { id: 1, hero: one });
		// The part count sits right before the one part (2 + 28 + 16 bytes).
		let count = base.len() - 46 - 1;
		assert_eq!(base[count], 1);
		let mut b = base.clone();
		b[count] = MAX_PARTS as u8 + 1;
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
		let mut b = base;
		b[count + 1..count + 3].copy_from_slice(&0u16.to_le_bytes());
		assert_eq!(decode(&b), Err(DecodeError::Malformed));
	}

	#[test]
	fn names_are_cleaned_and_checked() {
		assert_eq!(clean_name("a\nb\u{7}c"), "abc");
		assert_eq!(clean_name(&"é".repeat(20)).len(), 24);
		let b = encode(&Msg::Hello { name: "x".repeat(100), tick_hz: 50 });
		assert_eq!(decode(&b), Ok(Msg::Hello { name: "x".repeat(MAX_NAME), tick_hz: 50 }));
		let mut raw = encode(&Msg::Hello { name: "ab".into(), tick_hz: 50 });
		raw[8] = b'\n';
		assert_eq!(decode(&raw), Err(DecodeError::Malformed));
	}

	#[test]
	fn random_bytes_never_panic() {
		let mut seed = 0x2545_f491_4f6c_dd1du64;
		let mut next = || {
			seed ^= seed << 13;
			seed ^= seed >> 7;
			seed ^= seed << 17;
			seed
		};
		let valid = encode(&Msg::State { id: 1, hero: hero() });
		for _ in 0..20_000 {
			let n = (next() % 64) as usize;
			let mut b: Vec<u8> = if next() % 2 == 0 { valid.clone() } else { MAGIC.iter().copied().chain(VERSION.to_le_bytes()).collect() };
			b.truncate(6 + n.min(b.len().saturating_sub(6)));
			for _ in 0..n {
				let at = (next() as usize) % (b.len() + 1);
				b.insert(at, next() as u8);
			}
			let _ = decode(&b);
		}
	}
}
