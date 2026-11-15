//! The level's machinery: the placed objects that move and are neither enemies nor crates (those are shared their own way) — doors,
//! lifts, platforms, gates, traffic. Nobody is given them: each game runs its own, and a door opens for whoever comes to it. What the
//! others see is the game of the player who set it going: while one moves there, the game of the player nearest to it, or of a player
//! at it (`OPERATE`), streams where it has it (pose and animation), and every other game shows it so, its update skipped: a door
//! another player opened stands open, a lift another player rides goes up with him, a switch another player threw shows thrown. That
//! holds for a game whose own hero is at it too, as in a game made for two, unless his own game set it going as well: a partner who
//! steps onto a lift the other player started rides it with him, carried by the game as on a lift it runs (a shown object keeps its
//! collision: `stub`). Seen on Aridia's elevator (class 651): the joiner, dropped onto the host's moving elevator, rode it 9 m down
//! and 23 m up standing on it. Two players who step on together each set their own going, and ride side by side. When the stream
//! stops the object stays as it was last shown. When it is at rest there and a game's own hero is near, that game runs the object
//! itself again, from where and in what state (+0x20) it came to rest there, with its variables: the door stays open, the switch
//! thrown, the lift at the floor it went to, with whoever stands on it.
//!
//! An object's progress is in its variables (pvars) too: how far a bolt crank was turned (a float at +0x18 of Novalis' class 280),
//! how high a platform is, where a lift is going (Aridia's elevator: word 42). The game that moves an object sends them a little after
//! it comes to rest (`VARS_AFTER`), and another game that shows that object as the first has it writes them in before it runs it again,
//! waiting for them (`VARS_WAIT`): pose, state and variables then all are one game's object at rest, and a crank another player turned
//! is turned here. Variables from an earlier rest are dropped while the object moves there: a lift slowing into its top floor read as
//! still for a moment, took the bottom floor's and went back down. Nowhere else: in an object this game runs itself, another game's
//! variables broke it (one of Kerwan's trains, far from this game's hero and not moved here yet, took another game's route and stopped
//! the game). What moves on its own is left out: what kept moving for `AMBIENT` fields is not streamed, a class one of whose objects
//! started again by itself after coming to rest, with no player near (`SELF_MOVING_AFTER`, `SET_GOING`), sends no variables, and no
//! object is run again from another game's state unless its stream ended at rest. A word that looks like a pointer, here or there, is
//! left as it is: what it points to is each game's. An object's pvars lie one after another, so its words go up to the next placed
//! object's pvars, `MAX_VARS` at most.
//!
//! Only memory is written: the pose, the state byte, the pvar words, and the update pointer (+0x74), which the game's moby loop calls
//! each field. A shown object's points at an update function of the level that does nothing (its first instruction returns: `stub`),
//! so its own does not run but the loop's work around it does: with the pointer clear the loop skips the object whole, and it has no
//! collision then (a hero on a lift shown so fell through it). With no such function in the level it is cleared. A state of 0x80 and
//! up is a deleted object's, and never written.

use super::ids::Ids;
use super::{dist2, enemies, rd, rd8, rd_f, read_pose, Mp, ENEMY, CRATE, DELAY, MASK, PART_CELLS, SEND_EVERY, STREAM_RANGE};
use crate::retail::table as table_of;
use ee_oracle::cpu::Cpu;
use rac_mp::{EnemyPose, HeroState, PlayerId, Track, WorldKey, MAX_VARS};
use std::collections::{BTreeMap, BTreeSet};

/// Class types that are no machinery: the hero, shots, the ambient flyers, bolts (enemies and crates are `ENEMY`, `CRATE`). Type 13
/// is a flyer's, and also that of some machines a hero rides (Aridia's elevators, class 651): one with the carry bit (`CARRY`) of its
/// flags (+0x34), which the game sets on what moves a hero standing on it, is machinery.
const HERO: i16 = 1;
const SHOT: i16 = 6;
const FLYER: i16 = 13;
const BOLT: i16 = 19;
const CARRY: u32 = 0x20;
/// An object that moved within this many fields is moving.
const MOVING_FOR: u64 = 90;
/// A move smaller than this (metres, radians) is none.
const STILL: f32 = 0.005;
/// A game whose hero is within this of an object runs it itself (metres), and so does one whose hero is not this much further
/// from it than the hero of the game streaming it (two games streaming the same object never show each other's).
const NEAR: f32 = 5.0;
const NEARER_BY: f32 = 2.0;
/// What has kept moving for longer than this (fields) moves on its own (traffic, critters): it is each game's own, not streamed. What
/// such an update keeps (a path, a route) is its game's alone, and another car's in it stopped the game: an object's variables travel
/// only when it comes to rest, and the state it was shown in is written only when the stream of it ended at rest. Poses at rest in
/// a row, received, for that.
const AMBIENT: u64 = 600;
const REST_POSES: u32 = 8;
/// A class one of whose objects started moving again by itself this many times after coming to rest moves on its own (traffic that
/// stops at stations, a platform on a cycle): its variables never travel, and its objects are never woken in another game's state. A
/// start with a player within `SET_GOING` of it is his doing (a lift he stepped on), not counted.
const SELF_MOVING_AFTER: u32 = 2;
const SET_GOING: f32 = 8.0;
/// A player within this of a machine (metres) works it: his game streams it while it moves, and the others show it so even with their
/// own hero at it.
const OPERATE: f32 = 6.0;
/// A game sends an object's variables this many fields after it came to rest (vars are sent again whenever they change), and another
/// game that shows it waits this long after its rest for them before it runs the object itself.
const VARS_AFTER: u64 = 24;
const VARS_WAIT: u64 = 90;
/// At most this many poses a send, the nearest to the others first.
const MAX_SENT: usize = 48;
/// A sender not heard from for this many fields gives way to another.
const QUIET: u64 = 30;
/// The table holds no more placed objects than this (as `world_tick` checks).
const SLOTS: u32 = 4096;

/// What the machinery of the level now looks like to this game.
#[derive(Default)]
pub(super) struct Machines {
	level: i8,
	/// Each placed object's last pose here (position, rotation, animation sequence and state) and the field it last moved.
	seen: BTreeMap<u16, ([f32; 6], [u8; 2], u64)>,
	/// Poses other games stream, by slot: whose, the class, the track and the field the last came.
	theirs: BTreeMap<u16, Theirs>,
	/// The objects shown as another game has them, by slot: their update pointer and own pose (`Shown`).
	mirrored: BTreeMap<u16, Shown>,
	/// The state each object shown here was last shown in.
	last: BTreeMap<u16, u8>,
	/// Variables other games sent (state byte, pvar words), by object, in any level, until they are written here.
	vars: BTreeMap<WorldKey, (u8, Vec<u32>)>,
	/// The variables this game last sent of each object it moves, and when.
	sent: BTreeMap<u16, (u64, u8, Vec<u32>)>,
	/// When each object last started to move after standing still, and how often it started again after coming to rest.
	since: BTreeMap<u16, u64>,
	cycles: BTreeMap<u16, u32>,
	/// The classes that move on their own here (`SELF_MOVING_AFTER`).
	self_moving: BTreeSet<u16>,
	/// An update function of this level that does nothing, for what is shown here (0: none found yet), and whether it was looked for.
	stub: u32,
	looked: bool,
}

/// The first instruction of an update function that does nothing: it returns.
const JR_RA: u32 = 0x03E0_0008;

/// An update pointer of a placed object in the table whose function returns at once (0: none).
fn find_stub(ram: &[u8], table: u32, slots: u32) -> u32 {
	(1..slots)
		.map(|s| table + s * 0x100)
		.filter(|&a| rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80)
		.map(|a| rd(ram, a + 0x74))
		.find(|&u| u & MASK != 0 && rd(ram, u & MASK) == JR_RA)
		.unwrap_or(0)
}

/// An object shown as another game has it: its update pointer, and its own pose when it was taken (`POSE`).
struct Shown {
	update: u32,
	own: [u32; POSE.len()],
}

/// The words of a moby that make its pose: position, scale, rotation, animation clock, and the rows of its matrix.
const POSE: [u32; 24] = [0x10, 0x14, 0x18, 0x2c, 0x40, 0x44, 0x48, 0x50, 0x54, 0x58, 0x5c, 0xc0, 0xc4, 0xc8, 0xcc, 0xd0, 0xd4, 0xd8, 0xdc, 0xe0, 0xe4, 0xe8, 0xec, 0x4c];

struct Theirs {
	from: PlayerId,
	class: u16,
	track: Track,
	heard: u64,
	/// Poses in a row that did not move, and the field they came to `REST_POSES`.
	still: u32,
	rest_at: u64,
}

/// Poses another game streams.
pub(super) fn heard(mp: &mut Mp, from: PlayerId, tick: u32, poses: Vec<EnemyPose>) {
	let m = &mut mp.machines;
	for p in poses {
		let t = m.theirs.entry(p.slot).or_insert_with(|| Theirs { from, class: p.class, track: Track::default(), heard: mp.fields, still: 0, rest_at: 0 });
		if t.from != from {
			if mp.fields.saturating_sub(t.heard) < QUIET {
				continue;
			}
			*t = Theirs { from, class: p.class, track: Track::default(), heard: mp.fields, still: 0, rest_at: 0 };
		}
		let same = t.track.newest().is_some_and(|h| h.pos.iter().chain(&h.rot).zip(p.pos.iter().chain(&p.rot)).all(|(a, b)| (a - b).abs() <= STILL));
		// A stream heard afresh is at rest until a pose moves: after a machine ran here again, the next pose of the same one at rest
		// there read as moving, and it was shown again.
		t.still = if t.track.newest().is_none() { REST_POSES } else if same { t.still + 1 } else { 0 };
		if t.still == REST_POSES {
			t.rest_at = mp.fields;
		}
		t.track.push(HeroState { tick, level: mp.level, pos: p.pos, rot: p.rot, anim: p.anim, state: p.state as i32, ..Default::default() });
		(t.class, t.heard) = (p.class, mp.fields);
	}
}

/// Another game's variables of an object: kept until they can be written here.
pub(super) fn vars_heard(mp: &mut Mp, key: WorldKey, state: u8, words: Vec<u32>) {
	mp.machines.vars.insert(key, (state, words));
}

/// Where the pvars of the object at `a` are and how many words of them travel: up to the next placed object's pvars.
fn pvar_span(ram: &[u8], table: u32, slots: u32, a: u32) -> Option<(u32, usize)> {
	let pv = rd(ram, a + 0x78) & MASK;
	if pv == 0 {
		return None;
	}
	let next = (1..slots).map(|s| rd(ram, table + s * 0x100 + 0x78) & MASK).filter(|&p| p > pv).min()?;
	Some((pv, (((next - pv) / 4) as usize).min(MAX_VARS)))
}

/// A word that reads as an address in main RAM (cached, uncached or accelerated).
fn pointer_like(v: u32) -> bool {
	matches!(v >> 28, 0 | 2 | 3) && (0x10_0000..0x200_0000).contains(&(v & 0x0fff_ffff))
}

/// Writes another game's variables into the object at `a`: its pvar words but those that look like pointers, and its state.
fn put_vars(ram: &mut [u8], table: u32, slots: u32, a: u32, state: u8, words: &[u32]) {
	if let Some((pv, n)) = pvar_span(ram, table, slots, a) {
		for (k, &w) in words.iter().take(n).enumerate() {
			let at = ((pv + k as u32 * 4) & MASK) as usize;
			let here = u32::from_le_bytes([ram[at], ram[at + 1], ram[at + 2], ram[at + 3]]);
			if !pointer_like(here) && !pointer_like(w) {
				ram[at..at + 4].copy_from_slice(&w.to_le_bytes());
			}
		}
	}
	if state < 0x80 && rd8(ram, a + 0x20) < 0x80 {
		ram[((a + 0x20) & MASK) as usize] = state;
	}
}

fn alive(ram: &[u8], a: u32, class: u16) -> bool {
	rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 && (rd(ram, a + 0xa4) >> 16) as u16 == class
}

/// The object at `a` stops running here and shows another game's: its update the level's that does nothing (`stub`, else none), its
/// own pose kept aside.
fn show(ram: &mut [u8], a: u32, stub: u32) -> Shown {
	let own = POSE.map(|o| rd(ram, a + o));
	let update = enemies::no_update(ram, a);
	enemies::set_update(ram, a, stub);
	Shown { update, own }
}

/// The object at `a` runs here again, with its update back: from the other game's object at rest it was last shown as (`state`, its
/// variables already written), or else from its own pose when it was taken, as if it had only paused. Its own update, with another
/// game's place and its own route, broke Kerwan's traffic.
fn wake(ram: &mut [u8], a: u32, shown: Shown, state: Option<u8>, stub: u32) {
	let now = rd(ram, a + 0x74);
	if now != 0 && now != stub {
		return;
	}
	match state {
		Some(s) => {
			if s < 0x80 && rd8(ram, a + 0x20) < 0x80 {
				ram[((a + 0x20) & MASK) as usize] = s;
			}
		}
		None => {
			for (o, w) in POSE.iter().zip(shown.own) {
				let at = ((a + o) & MASK) as usize;
				ram[at..at + 4].copy_from_slice(&w.to_le_bytes());
			}
		}
	}
	enemies::set_update(ram, a, shown.update);
}

/// Runs the game's own update of every object shown here again (`keep`: the table is still the one they are in).
fn release_all(c: &mut Cpu, m: &mut Machines, ids: &Ids, keep: bool) {
	for (slot, shown) in std::mem::take(&mut m.mirrored) {
		let Some(a) = ids.addr(slot) else { continue };
		if keep && rd(&c.mem.ram, a + 0x24) != 0 {
			let state = m.last.remove(&slot);
			wake(&mut c.mem.ram, a, shown, state, m.stub);
			m.seen.remove(&slot);
		}
	}
}

/// One field: what moves here and is ours to stream, and what the others' streams show here.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let fields = mp.fields;
	let m = &mut mp.machines;
	if me.level != m.level {
		// Another level (or none): the table was cleared with what was in it.
		*m = Machines { level: me.level, vars: std::mem::take(&mut m.vars), self_moving: std::mem::take(&mut m.self_moving), ..Default::default() };
		return;
	}
	if mp.my_id.is_none() || me.level < 0 || mp.table == 0 {
		return;
	}
	let table = mp.table;
	if me.mode != 0 || mp.held {
		// A scene, a menu or a respawn on its way or just done: the game moves its objects its own way.
		release_all(c, m, &mp.ids, true);
		m.theirs.clear();
		return;
	}
	let ram = &c.mem.ram[..];
	let (cell, _) = table_of(me.level as u32);
	let end = rd(ram, cell + 4) & MASK;
	let slots = end.saturating_sub(table) / 0x100;
	if slots > SLOTS {
		return;
	}
	if !m.looked {
		m.looked = true;
		m.stub = find_stub(ram, table, slots);
		eprintln!("mp: what is shown here runs {}", if m.stub != 0 { format!("the update at {:#x}, which does nothing", m.stub) } else { "no update (none that does nothing found)".into() });
	}
	let others: Vec<[f32; 3]> = mp.shown.iter().filter(|(_, h)| h.level == me.level && h.mode == 0).map(|(_, h)| h.pos).collect();
	let player: Vec<u32> = std::iter::once(enemies::hero_moby(ram)).chain(PART_CELLS.iter().map(|&c| rd(ram, c) & MASK)).collect();
	let nearest_is_me = |p: [f32; 3]| others.iter().all(|o| dist2(p, me.pos) <= dist2(p, *o));
	// What moves here: every placed object but the hero's, enemies, crates and the shared ones, and those shown as another game has them.
	let mut ours = Vec::new();
	let mut moving = Vec::new();
	for s in 1..slots {
		let a = table + s * 0x100;
		let Some(slot) = mp.ids.id(a) else { continue };
		let h = rd(ram, a + 0x24) & MASK;
		if h == 0 || rd8(ram, a + 0x20) >= 0x80 || m.mirrored.contains_key(&slot) || mp.watched.contains_key(&slot) || player.contains(&a) {
			continue;
		}
		let kind = (rd(ram, h + 0x44) >> 16) as i16;
		let flags = rd(ram, a + 0x34);
		if matches!(kind, HERO | ENEMY | SHOT | CRATE | BOLT) || (kind == FLYER && flags & CARRY == 0) || rd(ram, a + 0x74) == 0 || flags & 1 != 0 {
			continue;
		}
		let now = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18), rd_f(ram, a + 0x40), rd_f(ram, a + 0x44), rd_f(ram, a + 0x48)];
		let seq = [rd8(ram, a + 0x53), rd8(ram, a + 0x20)];
		let moved = match m.seen.get(&slot) {
			Some((was, s, at)) => {
				if *s != seq || was.iter().zip(&now).any(|(x, y)| (x - y).abs() > STILL) {
					fields
				} else {
					*at
				}
			}
			// First seen: not moving yet.
			None => 0,
		};
		let class = (rd(ram, a + 0xa4) >> 16) as u16;
		if moved == fields && m.seen.get(&slot).is_none_or(|(_, _, at)| *at == 0 || fields - at >= MOVING_FOR) {
			m.since.insert(slot, fields);
			let p = [now[0], now[1], now[2]];
			let by_a_player = std::iter::once(me.pos).chain(others.iter().copied()).any(|o| dist2(p, o) < SET_GOING * SET_GOING);
			if m.seen.get(&slot).is_some_and(|(_, _, at)| *at != 0) && !by_a_player {
				let n = m.cycles.entry(slot).or_default();
				*n += 1;
				if *n >= SELF_MOVING_AFTER {
					m.self_moving.insert(class);
				}
			}
		}
		m.seen.insert(slot, (now, seq, moved));
		let pos = [now[0], now[1], now[2]];
		let ambient = fields - m.since.get(&slot).copied().unwrap_or(moved) > AMBIENT;
		let at_it = dist2(pos, me.pos) < OPERATE * OPERATE;
		if moved != 0 && fields - moved < MOVING_FOR && !ambient && (nearest_is_me(pos) || at_it) && others.iter().any(|o| dist2(pos, *o) < STREAM_RANGE * STREAM_RANGE) {
			ours.push((others.iter().map(|o| dist2(pos, *o)).fold(f32::MAX, f32::min), read_pose(ram, a, slot, (rd(ram, a + 0xa4) >> 16) as u16)));
			moving.push((slot, a, moved, m.since.get(&slot).copied().unwrap_or(moved)));
			if fields % 30 == 0 && super::testaid::watched(class) {
				eprintln!("mp test: f{fields} id {slot:#x} streamed from here (at it {at_it})");
			}
		}
	}
	// Their variables, when due and changed: a little after it came to rest.
	let level = me.level as u8;
	for &(slot, a, moved, _) in &moving {
		if fields - moved < VARS_AFTER || m.self_moving.contains(&((rd(ram, a + 0xa4) >> 16) as u16)) {
			continue;
		}
		let Some((pv, n)) = pvar_span(ram, table, slots, a) else { continue };
		let words: Vec<u32> = (0..n as u32).map(|k| rd(ram, pv + k * 4)).collect();
		let state = rd8(ram, a + 0x20);
		if m.sent.get(&slot).is_none_or(|l| l.1 != state || l.2 != words) {
			super::wire().vars.push((WorldKey { level, slot, class: (rd(ram, a + 0xa4) >> 16) as u16 }, state, words.clone()));
			if super::testaid::watched((rd(ram, a + 0xa4) >> 16) as u16) {
				eprintln!("mp test: f{fields} id {slot:#x} variables sent (state {state:#x}, {} words)", words.len());
			}
			m.sent.insert(slot, (fields, state, words));
		}
	}
	if fields % SEND_EVERY == 0 && !ours.is_empty() {
		ours.sort_by(|a, b| a.0.total_cmp(&b.0));
		ours.truncate(MAX_SENT);
		super::wire().machines = Some((me.level as u8, fields as u32, ours.into_iter().map(|(_, p)| p).collect()));
	}
	// The others' streams: shown here while this hero is not near, and that player's is nearer.
	let senders: Vec<(PlayerId, [f32; 3])> = mp.shown.iter().filter(|(_, h)| h.level == me.level && h.mode == 0).map(|(id, h)| (*id, h.pos)).collect();
	let ram = &mut c.mem.ram[..];
	// The others' variables: kept only for what this game shows as they have it, to run it again from there (pose, state and
	// variables all of one game's object at rest). Written into an object this game runs itself, they broke it: one of Kerwan's trains
	// far from this game's hero, not moved here yet, took another game's route and stopped the game.
	m.vars.retain(|k, _| k.level == level && m.mirrored.contains_key(&k.slot));
	let mut gone = Vec::new();
	for (slot, t) in m.theirs.iter_mut() {
		let Some(a) = mp.ids.addr(*slot) else {
			gone.push(*slot);
			continue;
		};
		if !alive(ram, a, t.class) {
			gone.push(*slot);
			continue;
		}
		let here = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)];
		let there = t.track.newest().map_or(here, |h| h.pos);
		let mine = dist2(here, me.pos).min(dist2(there, me.pos)).sqrt();
		let theirs = senders.iter().find(|s| s.0 == t.from).map_or(f32::MAX, |s| dist2(there, s.1).sqrt());
		let key = WorldKey { level, slot: *slot, class: t.class };
		// Moving there: a fresh stream that has not come to rest. Variables from an earlier rest are old then (where a lift is going):
		// only those sent after it stopped again count (a lift slowing into the top took the bottom's and went back down).
		let moving_there = fields.saturating_sub(t.heard) <= QUIET && t.still < REST_POSES;
		if moving_there {
			m.vars.remove(&key);
		}
		// At rest here: seen, and not moved for a while (a game that set it going itself keeps its own).
		let at_rest_here = m.seen.get(slot).is_some_and(|v| v.2 == 0 || fields - v.2 >= MOVING_FOR);
		// A partner works it while this hero is at it too: shown as his game has it while it moves (the co-op rule above).
		let worked = moving_there && at_rest_here && theirs < OPERATE && !m.self_moving.contains(&t.class);
		if mine < NEAR || mine < theirs + NEARER_BY {
			if m.mirrored.contains_key(slot) && moving_there {
				// Shown and still moving there: it stays so until it comes to rest, a hero on it carried with it.
			} else if m.mirrored.contains_key(slot) {
				// At rest there: run here from where and in what state it came to rest there, with its variables, once they came (they
				// are sent a little after it stopped, so they also say it really did: a lift slowing at its floor reads as still for a
				// moment) or the wait for them is over. A stream that stopped while it moved leaves it as it was before it was shown.
				let at_rest = t.still >= REST_POSES && !m.self_moving.contains(&t.class);
				if !at_rest || m.vars.contains_key(&key) || fields.saturating_sub(t.rest_at) >= VARS_WAIT {
					if let Some(shown) = m.mirrored.remove(slot) {
						m.last.remove(slot);
						let end = t.track.newest().filter(|_| at_rest);
						if let Some(h) = end {
							enemies::place(ram, a, &enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: 0.0, scale: rd_f(ram, a + 0x2c), look: None });
						}
						let snapshot = end.map(|h| h.state as u8);
						let vars = m.vars.remove(&key).filter(|_| snapshot.is_some());
						if super::testaid::watched(t.class) {
							eprintln!("mp test: f{fields} id {slot:#x} runs here again, state {snapshot:?}, variables {}", vars.is_some());
						}
						if let Some((state, words)) = vars {
							put_vars(ram, table, slots, a, state, &words);
						}
						wake(ram, a, shown, snapshot, m.stub);
						m.seen.remove(slot);
					}
					gone.push(*slot);
					continue;
				}
				// Waiting for its variables: still shown as it is there.
			} else if !worked {
				if super::testaid::watched(t.class) {
					eprintln!("mp test: f{fields} id {slot:#x} player {}'s stream not shown: mine {mine:.1} m, theirs {theirs:.1} m, moving there {moving_there}, at rest here {at_rest_here}", t.from);
				}
				gone.push(*slot);
				continue;
			}
		}
		if fields.saturating_sub(t.heard) > QUIET && t.still < REST_POSES {
			// The stream stopped while it moved (it moves on its own, or went out of reach): it runs here again, as it is.
			if super::testaid::watched(t.class) && m.mirrored.contains_key(slot) {
				eprintln!("mp test: f{fields} id {slot:#x} runs here again, its stream stopped while it moved");
			}
			if let Some(shown) = m.mirrored.remove(slot) {
				m.last.remove(slot);
				wake(ram, a, shown, None, m.stub);
				m.seen.remove(slot);
			}
			gone.push(*slot);
			continue;
		}
		if !m.mirrored.contains_key(slot) {
			// Shown from now (only while a stream comes: one that stopped before this game took it shows nothing), and only what has
			// not moved here by itself: what moves on its own here (traffic, critters, what this player used) stays this game's. Paused
			// while the rest of its kind went on, one of Kerwan's cars broke their update.
			if !worked && (fields.saturating_sub(t.heard) > QUIET || m.seen.get(slot).is_none_or(|v| v.2 != 0) || m.self_moving.contains(&t.class)) {
				gone.push(*slot);
				continue;
			}
			if super::testaid::watched(t.class) {
				eprintln!("mp test: f{fields} id {slot:#x} shown as player {}'s (mine {mine:.1} m, theirs {theirs:.1} m, worked {worked})", t.from);
			}
			m.mirrored.insert(*slot, show(ram, a, m.stub));
		}
		if let Some(h) = t.track.sample(DELAY) {
			if t.still >= REST_POSES && !m.self_moving.contains(&t.class) {
				m.last.insert(*slot, h.state as u8);
			} else {
				m.last.remove(slot);
			}
			enemies::place(ram, a, &enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: 0.0, scale: rd_f(ram, a + 0x2c), look: None });
		}
	}
	for slot in gone {
		m.theirs.remove(&slot);
	}
	// Shown and no stream left: held as last shown until this hero comes near.
	let held: Vec<u16> = m.mirrored.keys().filter(|s| !m.theirs.contains_key(s)).copied().collect();
	for slot in held {
		let Some(a) = mp.ids.addr(slot) else {
			m.mirrored.remove(&slot);
			continue;
		};
		let here = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)];
		if rd(ram, a + 0x24) == 0 || rd8(ram, a + 0x20) >= 0x80 {
			m.mirrored.remove(&slot);
			m.last.remove(&slot);
		} else if dist2(here, me.pos) < NEAR * NEAR {
			let Some(shown) = m.mirrored.remove(&slot) else { continue };
			let class = (rd(ram, a + 0xa4) >> 16) as u16;
			let snapshot = m.last.remove(&slot);
			if let Some((state, words)) = m.vars.remove(&WorldKey { level, slot, class }).filter(|_| snapshot.is_some()) {
				put_vars(ram, table, slots, a, state, &words);
			}
			wake(ram, a, shown, snapshot, m.stub);
			m.seen.remove(&slot);
		}
	}
}

/// How many objects are shown here as another game has them, for the run log.
pub(super) fn shown(mp: &Mp) -> usize {
	mp.machines.mirrored.len()
}

/// Leaving the session or the level: every object runs here again.
pub(super) fn stop(c: &mut Cpu, mp: &mut Mp) {
	let keep = mp.table != 0;
	release_all(c, &mut mp.machines, &mp.ids, keep);
	mp.machines.theirs.clear();
}
