//! The wire format, version 1. Every datagram is `MAGIC`, `VERSION` (u16), a tag byte and the message. Little-endian,
//! fixed-size fields; a string is a length byte and UTF-8. `decode` checks every length and value, refuses NaN and infinite
//! floats and trailing bytes, and never panics: the bytes come from other people's PCs.

pub const MAGIC: [u8; 4] = *b"RCMP";
/// 2: the hero carries his rotation, animation clock and light (puppets). 3: and the parts on him (`Part`). 4: reliable messages. 5: the host's travel. 6: synced enemies. 7: spawned objects. 8: enemy sounds and effects.
pub const VERSION: u16 = 10;
/// The largest datagram sent or accepted: below the usual internet MTU, so nothing is fragmented.
pub const MAX_PACKET: usize = 1200;
/// The longest player name, in bytes.
pub const MAX_NAME: usize = 24;
/// Players in one session, the host included.
pub const MAX_PLAYERS: usize = 4;
/// Parts on one hero: the two back objects (Clank, a pack) and the held item.
pub const MAX_PARTS: usize = 3;
/// Enemy poses in one datagram: 53 bytes each, under `MAX_PACKET` with the header.
pub const MAX_POSES: usize = 22;

/// Sound and effect events in one datagram (`Msg::Fx`): 99 bytes at most each.
pub const MAX_FX: usize = 10;
/// The `slot` of an effect made for nothing this game can name.
pub const NO_SLOT: u16 = 0xffff;

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

/// The ids the host gives spawned objects: the `slot` of their `WorldKey` and `EnemyPose`, above any placed slot.
pub const SPAWNED_ID: u16 = 0x8000;

/// One enemy as the game that runs it left it this tick (`Msg::Enemies`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnemyPose {
	pub slot: u16,
	pub class: u16,
	pub pos: [f32; 3],
	pub rot: [f32; 3],
	pub anim: AnimClock,
	pub health: f32,
	pub scale: f32,
	/// The object's state byte (+0x20).
	pub state: u8,
}

/// What must arrive (`reliable.rs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reliable {
	/// An object is gone for good: killed, broken or picked up. Client to host: reported; host to clients: decided.
	Removed(WorldKey),
	/// Either way, through the host: an object reported gone was brought back by a player's own game (an enemy the level sends
	/// again). It is no longer gone: nobody finishes it, and its next end is reported anew.
	Back(WorldKey),
	/// Host to clients: the host is in this level now, follow.
	Travel(u8),
	/// Host to clients: this player's game runs this enemy from now on.
	Owner { key: WorldKey, owner: PlayerId },
	/// A player hit an enemy another game runs: for that game, through the host.
	Hit { key: WorldKey, damage: f32, mask: u32 },
	/// Client to host: our game spawned this object, known here as `local`. `lasting`: a copy of it counts whenever it shows up (an
	/// enemy, not a shot). `solo`: only this game makes it (a player's own shot), so it is nobody's copy.
	Spawn { local: u16, level: u8, class: u16, pos: [f32; 3], lasting: bool, solo: bool },
	/// Host to clients: the object has the id `id` and `owner`'s game runs it (`local` is the owner's name for it).
	Spawned { id: u16, owner: PlayerId, local: u16, level: u8, class: u16 },
	/// Host to the reporter: another game's object `of` is already this one. Yours stands down.
	Duplicate { local: u16, of: u16 },
	/// Either way, through the host: a player's blow set off this placed object (a switch, a lever). Every other game deals it too.
	Touch { key: WorldKey, damage: f32, mask: u32 },
	/// Either way, through the host: the team got further. Host to a late arrival: everything so far.
	Story(Progress),
	/// Either way, through the host: a placed object's variables as the game that moved it has them, its state byte and the first
	/// `len` words of its pvars (a crank's turn, a platform's height). Host to a late arrival: the newest of each object.
	Vars { key: WorldKey, state: u8, len: u8, words: [u32; MAX_VARS] },
	/// Either way, through the host: a player's game started cutscene `id` of `level` (the level's own scene number). Every other
	/// game in that level plays it too.
	Scene { level: u8, id: i32 },
}

/// The pvar words of an object that travel at most.
pub const MAX_VARS: usize = 32;

/// What a player's game got that the whole team keeps, as a game made for two would.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Progress {
	/// The game gave the player this item (a gadget, a weapon found, not bought).
	Item(u8),
	/// The game unlocked this planet's coordinates.
	Planet(u8),
	/// The player collected a gold bolt: the level's `n`th, as `level * 4 + n`.
	Gold(u8),
	/// A mission byte went up (0xff: the mission is complete): the level's `slot`th, as `level * 16 + slot`.
	Mission { index: u16, value: u8 },
}

/// Mission bytes the game keeps: 16 a level, 20 levels.
pub const MISSION_BYTES: u16 = 0x140;

/// Gold bolts the game keeps: 4 a level.
pub const GOLD_BOLTS: u8 = 80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
	/// No free player slot.
	Full,
	/// The other side speaks another protocol version.
	Version,
	/// The other side runs the other disc region: PAL ticks 50 times a second, USA 60.
	Region,
}

/// A sound or an effect a player's game made for something it runs, made again by the others (`Msg::Fx`). `slot` is a placed slot, a
/// spawned object's id or `NO_SLOT`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FxEvent {
	/// The game's `moby_cue(cue, arg, moby)`: sound cue `cue` of the enemy at `slot`.
	Cue { slot: u16, cue: i32, arg: u32 },
	/// The game's explosion at `pos`, made for the object at `slot`: its other arguments as they were, the integer ones (a1, a3, t0..t3),
	/// the float ones (f12..f19, as bits) and the stack words after them.
	/// `owner` is the class of the object it was made for, `NO_SLOT` when it was made for none.
	Explode { slot: u16, owner: u16, pos: [f32; 4], ints: [u32; 6], floats: [u32; 8], stack: [u32; 6] },
	/// A sound cue of the player himself: his hero, or what he carries, of class `class`.
	PlayerCue { class: u16, cue: i32, arg: u32 },
	/// The game's spark (`spark_spawn`): from `pos`, at `vel`, with its other arguments as they were.
	Spark { pos: [f32; 4], vel: [f32; 4], a2: u32, a3: u64, t0: u8 },
	/// The game's glow flare (`glow_flare_spawn`), for the object at `slot`: its size, position, base, life and colour.
	Glow { slot: u16, owner: u16, size: f32, pos: [f32; 4], base: [f32; 4], life: u32, rgba: [u32; 4] },
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
	/// The enemies a player's game runs, at its tick `tick`. Client to host: `id` is ignored. Host to clients: whose they are.
	Enemies { id: PlayerId, level: u8, tick: u32, poses: Vec<EnemyPose> },
	/// Sounds and effects a player's game made for what it runs. Client to host: `id` is ignored. Host to clients: whose they are.
	Fx { id: PlayerId, level: u8, events: Vec<FxEvent> },
	/// The level's machinery (doors, lifts, platforms, traffic) a player's game has moving, as poses by placed slot. Client to host:
	/// `id` is ignored. Host to clients: whose they are.
	Machines { id: PlayerId, level: u8, tick: u32, poses: Vec<EnemyPose> },
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
const TAG_ENEMIES: u8 = 10;
const TAG_FX: u8 = 11;
const TAG_MACHINES: u8 = 12;
/// Reliable bodies.
const BODY_REMOVED: u8 = 1;
const BODY_TRAVEL: u8 = 2;
const BODY_OWNER: u8 = 3;
const BODY_HIT: u8 = 4;
const BODY_SPAWN: u8 = 5;
const BODY_SPAWNED: u8 = 6;
const BODY_DUPLICATE: u8 = 7;
const BODY_TOUCH: u8 = 8;
const BODY_STORY: u8 = 9;
const BODY_VARS: u8 = 10;
const BODY_SCENE: u8 = 11;
const BODY_BACK: u8 = 12;

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
		Msg::Reliable { seq, body } => {
			w.push(TAG_RELIABLE);
			w.extend_from_slice(&seq.to_le_bytes());
			match body {
				Reliable::Removed(k) => {
					w.push(BODY_REMOVED);
					put_key(&mut w, k);
				}
				Reliable::Back(k) => {
					w.push(BODY_BACK);
					put_key(&mut w, k);
				}
				Reliable::Travel(level) => w.extend_from_slice(&[BODY_TRAVEL, *level]),
				Reliable::Owner { key, owner } => {
					w.push(BODY_OWNER);
					put_key(&mut w, key);
					w.push(*owner);
				}
				Reliable::Touch { key, damage, mask } => {
					w.push(BODY_TOUCH);
					put_key(&mut w, key);
					w.extend_from_slice(&damage.to_le_bytes());
					w.extend_from_slice(&mask.to_le_bytes());
				}
				Reliable::Hit { key, damage, mask } => {
					w.push(BODY_HIT);
					put_key(&mut w, key);
					w.extend_from_slice(&damage.to_le_bytes());
					w.extend_from_slice(&mask.to_le_bytes());
				}
				Reliable::Spawn { local, level, class, pos, lasting, solo } => {
					w.push(BODY_SPAWN);
					w.extend_from_slice(&local.to_le_bytes());
					w.push(*level);
					w.extend_from_slice(&class.to_le_bytes());
					pos.iter().for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
					w.push(*lasting as u8 | (*solo as u8) << 1);
				}
				Reliable::Spawned { id, owner, local, level, class } => {
					w.push(BODY_SPAWNED);
					w.extend_from_slice(&id.to_le_bytes());
					w.push(*owner);
					w.extend_from_slice(&local.to_le_bytes());
					w.push(*level);
					w.extend_from_slice(&class.to_le_bytes());
				}
				Reliable::Duplicate { local, of } => {
					w.push(BODY_DUPLICATE);
					w.extend_from_slice(&local.to_le_bytes());
					w.extend_from_slice(&of.to_le_bytes());
				}
				Reliable::Story(p) => match p {
					Progress::Item(i) => w.extend_from_slice(&[BODY_STORY, 0, *i]),
					Progress::Planet(l) => w.extend_from_slice(&[BODY_STORY, 1, *l]),
					Progress::Gold(g) => w.extend_from_slice(&[BODY_STORY, 2, *g]),
					Progress::Mission { index, value } => {
						w.extend_from_slice(&[BODY_STORY, 3]);
						w.extend_from_slice(&index.to_le_bytes());
						w.push(*value);
					}
				},
				Reliable::Scene { level, id } => {
					w.extend_from_slice(&[BODY_SCENE, *level]);
					w.extend_from_slice(&id.to_le_bytes());
				}
				Reliable::Vars { key, state, len, words } => {
					w.push(BODY_VARS);
					put_key(&mut w, key);
					let n = (*len as usize).min(MAX_VARS);
					w.extend_from_slice(&[*state, n as u8]);
					words[..n].iter().for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
				}
			}
		}
		Msg::Ack { seq } => {
			w.push(TAG_ACK);
			w.extend_from_slice(&seq.to_le_bytes());
		}
		Msg::Enemies { id, level, tick, poses } | Msg::Machines { id, level, tick, poses } => {
			let poses = &poses[..poses.len().min(MAX_POSES)];
			let tag = if matches!(m, Msg::Machines { .. }) { TAG_MACHINES } else { TAG_ENEMIES };
			w.extend_from_slice(&[tag, *id, *level]);
			w.extend_from_slice(&tick.to_le_bytes());
			w.push(poses.len() as u8);
			for p in poses {
				w.extend_from_slice(&p.slot.to_le_bytes());
				w.extend_from_slice(&p.class.to_le_bytes());
				for v in p.pos.iter().chain(&p.rot) {
					w.extend_from_slice(&v.to_le_bytes());
				}
				put_anim(&mut w, &p.anim);
				w.extend_from_slice(&p.health.to_le_bytes());
				w.extend_from_slice(&p.scale.to_le_bytes());
				w.push(p.state);
			}
		}
		Msg::Fx { id, level, events } => {
			let events = &events[..events.len().min(MAX_FX)];
			w.extend_from_slice(&[TAG_FX, *id, *level, events.len() as u8]);
			for e in events {
				match e {
					FxEvent::Cue { slot, cue, arg } => {
						w.push(0);
						w.extend_from_slice(&slot.to_le_bytes());
						w.extend_from_slice(&cue.to_le_bytes());
						w.extend_from_slice(&arg.to_le_bytes());
					}
					FxEvent::Spark { pos, vel, a2, a3, t0 } => {
						w.push(3);
						pos.iter().chain(vel).for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
						w.extend_from_slice(&a2.to_le_bytes());
						w.extend_from_slice(&a3.to_le_bytes());
						w.push(*t0);
					}
					FxEvent::Glow { slot, owner, size, pos, base, life, rgba } => {
						w.push(4);
						w.extend_from_slice(&slot.to_le_bytes());
						w.extend_from_slice(&owner.to_le_bytes());
						w.extend_from_slice(&size.to_le_bytes());
						pos.iter().chain(base).for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
						w.extend_from_slice(&life.to_le_bytes());
						rgba.iter().for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
					}
					FxEvent::PlayerCue { class, cue, arg } => {
						w.push(2);
						w.extend_from_slice(&class.to_le_bytes());
						w.extend_from_slice(&cue.to_le_bytes());
						w.extend_from_slice(&arg.to_le_bytes());
					}
					FxEvent::Explode { slot, owner, pos, ints, floats, stack } => {
						w.push(1);
						w.extend_from_slice(&slot.to_le_bytes());
						w.extend_from_slice(&owner.to_le_bytes());
						pos.iter().for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
						ints.iter().chain(floats).chain(stack).for_each(|v| w.extend_from_slice(&v.to_le_bytes()));
					}
				}
			}
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
	let tag = r.u8()?;
	Some(match tag {
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
				BODY_REMOVED => Reliable::Removed(r.key()?),
				BODY_BACK => Reliable::Back(r.key()?),
				BODY_TRAVEL => Reliable::Travel(r.u8().filter(|&l| l <= 18)?),
				BODY_OWNER => Reliable::Owner { key: r.key()?, owner: r.id()? },
				BODY_HIT => Reliable::Hit { key: r.key()?, damage: r.f32()?, mask: r.u32()? },
				BODY_TOUCH => Reliable::Touch { key: r.key()?, damage: r.f32()?, mask: r.u32()? },
				BODY_SPAWN => {
					let (local, level, class, pos) = (r.u16()?, r.u8().filter(|&l| l <= 18)?, r.u16()?, [r.f32()?, r.f32()?, r.f32()?]);
					let flags = r.u8().filter(|&f| f < 4)?;
					Reliable::Spawn { local, level, class, pos, lasting: flags & 1 != 0, solo: flags & 2 != 0 }
				}
				BODY_SPAWNED => Reliable::Spawned { id: r.u16()?, owner: r.id()?, local: r.u16()?, level: r.u8().filter(|&l| l <= 18)?, class: r.u16()? },
				BODY_DUPLICATE => Reliable::Duplicate { local: r.u16()?, of: r.u16()? },
				BODY_STORY => Reliable::Story(match r.u8()? {
					0 => Progress::Item(r.u8()?),
					1 => Progress::Planet(r.u8().filter(|&l| l <= 18)?),
					2 => Progress::Gold(r.u8().filter(|&g| g < GOLD_BOLTS)?),
					3 => Progress::Mission { index: r.u16().filter(|&i| i < MISSION_BYTES)?, value: r.u8()? },
					_ => return None,
				}),
				BODY_SCENE => Reliable::Scene { level: r.u8().filter(|&l| l <= 18)?, id: r.u32()? as i32 },
				BODY_VARS => {
					let (key, state, len) = (r.key()?, r.u8()?, r.u8()?);
					if len as usize > MAX_VARS {
						return None;
					}
					let mut words = [0u32; MAX_VARS];
					for v in words.iter_mut().take(len as usize) {
						*v = r.u32()?;
					}
					Reliable::Vars { key, state, len, words }
				}
				_ => return None,
			};
			Msg::Reliable { seq, body }
		}
		TAG_ACK => Msg::Ack { seq: r.u32()? },
		TAG_ENEMIES | TAG_MACHINES => {
			let (id, level, tick) = (r.id()?, r.u8().filter(|&l| l <= 18)?, r.u32()?);
			let n = r.u8()? as usize;
			if n > MAX_POSES {
				return None;
			}
			let mut poses = Vec::with_capacity(n);
			for _ in 0..n {
				let (slot, class) = (r.u16()?, r.u16()?);
				let pos = [r.f32()?, r.f32()?, r.f32()?];
				let rot = [r.f32()?, r.f32()?, r.f32()?];
				poses.push(EnemyPose { slot, class, pos, rot, anim: r.anim()?, health: r.f32()?, scale: r.f32()?, state: r.u8()? });
			}
			if tag == TAG_MACHINES {
				Msg::Machines { id, level, tick, poses }
			} else {
				Msg::Enemies { id, level, tick, poses }
			}
		}
		TAG_FX => {
			let (id, level) = (r.id()?, r.u8().filter(|&l| l <= 18)?);
			let n = r.u8()? as usize;
			if n > MAX_FX {
				return None;
			}
			let mut events = Vec::with_capacity(n);
			for _ in 0..n {
				events.push(match r.u8()? {
					0 => FxEvent::Cue { slot: r.u16()?, cue: r.u32()? as i32, arg: r.u32()? },
					2 => FxEvent::PlayerCue { class: r.u16()?, cue: r.u32()? as i32, arg: r.u32()? },
					3 => {
						let pos = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
						let vel = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
						let a2 = r.u32()?;
						let a3 = r.u32()? as u64 | (r.u32()? as u64) << 32;
						FxEvent::Spark { pos, vel, a2, a3, t0: r.u8()? }
					}
					4 => {
						let (slot, owner, size) = (r.u16()?, r.u16()?, r.f32()?);
						let pos = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
						let base = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
						FxEvent::Glow { slot, owner, size, pos, base, life: r.u32()?, rgba: [r.u32()?, r.u32()?, r.u32()?, r.u32()?] }
					}
					1 => {
						let (slot, owner) = (r.u16()?, r.u16()?);
						let pos = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
						let mut words = [0u32; 20];
						for v in words.iter_mut() {
							*v = r.u32()?;
						}
						let (ints, rest) = words.split_at(6);
						let (floats, stack) = rest.split_at(8);
						FxEvent::Explode { slot, owner, pos, ints: ints.try_into().ok()?, floats: floats.try_into().ok()?, stack: stack.try_into().ok()? }
					}
					_ => return None,
				});
			}
			Msg::Fx { id, level, events }
		}
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

fn put_key(w: &mut Vec<u8>, k: &WorldKey) {
	w.push(k.level);
	w.extend_from_slice(&k.slot.to_le_bytes());
	w.extend_from_slice(&k.class.to_le_bytes());
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

	fn key(&mut self) -> Option<WorldKey> {
		let level = self.u8().filter(|&l| l <= 18)?;
		Some(WorldKey { level, slot: self.u16()?, class: self.u16()? })
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
			Msg::Reliable { seq: 81, body: Reliable::Spawn { local: 3, level: 5, class: 872, pos: [1.0, 2.0, 3.0], lasting: true, solo: true } },
			Msg::Reliable { seq: 82, body: Reliable::Spawned { id: SPAWNED_ID + 1, owner: 2, local: 3, level: 5, class: 872 } },
			Msg::Reliable { seq: 83, body: Reliable::Duplicate { local: 3, of: SPAWNED_ID + 1 } },
			Msg::Reliable { seq: 84, body: Reliable::Touch { key: WorldKey { level: 7, slot: 412, class: 1201 }, damage: 1.0, mask: 0x1_0000 } },
			Msg::Reliable { seq: 85, body: Reliable::Story(Progress::Item(26)) },
			Msg::Reliable { seq: 86, body: Reliable::Story(Progress::Planet(18)) },
			Msg::Reliable { seq: 87, body: Reliable::Story(Progress::Gold(79)) },
			Msg::Reliable { seq: 90, body: Reliable::Story(Progress::Mission { index: MISSION_BYTES - 1, value: 0xff }) },
			Msg::Reliable { seq: 91, body: Reliable::Scene { level: 18, id: 0x4001 } },
			Msg::Reliable { seq: 92, body: Reliable::Back(WorldKey { level: 1, slot: 0x1a5b, class: 660 }) },
			Msg::Reliable { seq: 88, body: Reliable::Vars { key: WorldKey { level: 1, slot: 297, class: 280 }, state: 3, len: MAX_VARS as u8, words: [0x3e85_d7a8; MAX_VARS] } },
			Msg::Reliable { seq: 89, body: Reliable::Vars { key: WorldKey { level: 1, slot: 297, class: 280 }, state: 1, len: 0, words: [0; MAX_VARS] } },
			Msg::Reliable { seq: 79, body: Reliable::Owner { key: WorldKey { level: 2, slot: 143, class: 749 }, owner: 1 } },
			Msg::Reliable { seq: 80, body: Reliable::Hit { key: WorldKey { level: 2, slot: 143, class: 749 }, damage: 2.5, mask: 0x20 } },
			Msg::Enemies { id: 1, level: 3, tick: 99, poses: vec![EnemyPose { slot: 143, class: 749, pos: [1.0, 2.0, 3.0], rot: [0.0, 0.0, 1.5], anim: hero().anim, health: 4.0, scale: 0.25, state: 2 }; MAX_POSES] },
			Msg::Ack { seq: 77 },
			Msg::Machines { id: 2, level: 3, tick: 7, poses: vec![EnemyPose { slot: 855, class: 816, pos: [1.0, 2.0, 3.0], rot: [0.0, 0.5, 1.5], anim: hero().anim, health: 0.0, scale: 0.5, state: 5 }; MAX_POSES] },
			Msg::Fx { id: 2, level: 3, events: (0..MAX_FX as u16).map(|k| if k == 2 { FxEvent::PlayerCue { class: 71, cue: 4, arg: 0 } } else if k == 4 { FxEvent::Spark { pos: [1.0; 4], vel: [0.5; 4], a2: 3, a3: u64::MAX - 1, t0: 9 } } else if k == 6 { FxEvent::Glow { slot: 12, owner: 900, size: 3.5, pos: [1.0; 4], base: [2.0; 4], life: 20, rgba: [0x7f, 0x3f, 0, 0x30] } } else if k % 2 == 0 { FxEvent::Cue { slot: k, cue: -3, arg: 7 } } else { FxEvent::Explode { slot: NO_SLOT, owner: if k == 1 { NO_SLOT } else { 121 }, pos: [1.0, 2.0, 3.0, 0.5], ints: [k as u32; 6], floats: [0x3f80_0000; 8], stack: [u32::MAX; 6] } }).collect() },
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
		b[4..6].copy_from_slice(&(VERSION + 1).to_le_bytes());
		assert_eq!(decode(&b), Err(DecodeError::Version(VERSION + 1)));
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
