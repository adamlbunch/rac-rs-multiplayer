//! The level's machinery: the placed objects that move and are neither enemies nor crates (those are shared their own way) — doors,
//! lifts, platforms, gates, traffic. Nobody is given them: each game runs its own, and a door opens for whoever comes to it. What the
//! others see is the game of the player nearest to it: while one moves there, that game streams where it has it (pose and animation),
//! and every other game whose own hero is not near it shows it so, its update skipped: a door another player opened stands open, a
//! lift another player rides goes up with him, a switch another player threw shows thrown. When the stream stops the object stays as
//! it was last shown. When a game's own hero comes near, that game runs the object itself again, from the state byte (+0x20) it was
//! last shown in, with its update back as it was: the door stays open, the switch thrown.
//!
//! Only memory is written: the pose, the state byte, and the update pointer (+0x74), which the game's moby loop calls each field and
//! skips while it is clear (as for the stand-ins, mp/spawned.rs). A state of 0x80 and up is a deleted object's, and never written.

use super::{dist2, dying, enemies, rd, rd8, rd_f, read_pose, Mp, ENEMY, CRATE, DELAY, MASK, PART_CELLS, SEND_EVERY, STREAM_RANGE};
use crate::retail::table as table_of;
use ee_oracle::cpu::Cpu;
use rac_mp::{EnemyPose, HeroState, PlayerId, Track};
use std::collections::BTreeMap;

/// Class types that are no machinery: the hero, shots, the ambient flyers, bolts (enemies and crates are `ENEMY`, `CRATE`).
const HERO: i16 = 1;
const SHOT: i16 = 6;
const FLYER: i16 = 13;
const BOLT: i16 = 19;
/// An object that moved within this many fields is moving.
const MOVING_FOR: u64 = 90;
/// A move smaller than this (metres, radians) is none.
const STILL: f32 = 0.005;
/// A game whose hero is within this of an object runs it itself (metres), and so does one whose hero is not this much further
/// from it than the hero of the game streaming it (two games streaming the same object never show each other's).
const NEAR: f32 = 10.0;
const NEARER_BY: f32 = 2.0;
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
	/// The objects shown as another game has them, by slot: their update pointer.
	mirrored: BTreeMap<u16, u32>,
	/// The state each object shown here was last shown in.
	last: BTreeMap<u16, u8>,
}

struct Theirs {
	from: PlayerId,
	class: u16,
	track: Track,
	heard: u64,
}

/// Poses another game streams.
pub(super) fn heard(mp: &mut Mp, from: PlayerId, tick: u32, poses: Vec<EnemyPose>) {
	let m = &mut mp.machines;
	for p in poses {
		let t = m.theirs.entry(p.slot).or_insert_with(|| Theirs { from, class: p.class, track: Track::default(), heard: mp.fields });
		if t.from != from {
			if mp.fields.saturating_sub(t.heard) < QUIET {
				continue;
			}
			*t = Theirs { from, class: p.class, track: Track::default(), heard: mp.fields };
		}
		t.track.push(HeroState { tick, level: mp.level, pos: p.pos, rot: p.rot, anim: p.anim, state: p.state as i32, ..Default::default() });
		(t.class, t.heard) = (p.class, mp.fields);
	}
}

fn alive(ram: &[u8], a: u32, class: u16) -> bool {
	rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 && (rd(ram, a + 0xa4) >> 16) as u16 == class
}

/// The object at `a` runs here again: from the state it was last shown in (`state`, if any), with its update back.
fn wake(ram: &mut [u8], a: u32, update: u32, state: Option<u8>) {
	if rd(ram, a + 0x74) != 0 {
		return;
	}
	if let Some(s) = state.filter(|&s| s < 0x80 && rd8(ram, a + 0x20) < 0x80) {
		ram[((a + 0x20) & MASK) as usize] = s;
	}
	enemies::set_update(ram, a, update);
}

/// Runs the game's own update of every object shown here again (`keep`: the table is still the one they are in).
fn release_all(c: &mut Cpu, m: &mut Machines, table: u32, keep: bool) {
	for (slot, update) in std::mem::take(&mut m.mirrored) {
		let a = table + slot as u32 * 0x100;
		if keep && rd(&c.mem.ram, a + 0x24) != 0 {
			wake(&mut c.mem.ram, a, update, m.last.remove(&slot));
		}
	}
}

/// One field: what moves here and is ours to stream, and what the others' streams show here.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let fields = mp.fields;
	let m = &mut mp.machines;
	if me.level != m.level {
		// Another level (or none): the table was cleared with what was in it.
		*m = Machines { level: me.level, ..Default::default() };
		return;
	}
	if mp.my_id.is_none() || me.level < 0 || mp.table == 0 {
		return;
	}
	let table = mp.table;
	if me.mode != 0 || dying(me) {
		// A scene, a menu or a respawn on its way: the game moves its objects its own way.
		release_all(c, m, table, true);
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
	let others: Vec<[f32; 3]> = mp.shown.iter().filter(|(_, h)| h.level == me.level && h.mode == 0).map(|(_, h)| h.pos).collect();
	let player: Vec<u32> = std::iter::once(enemies::hero_moby(ram)).chain(PART_CELLS.iter().map(|&c| rd(ram, c) & MASK)).collect();
	let nearest_is_me = |p: [f32; 3]| others.iter().all(|o| dist2(p, me.pos) <= dist2(p, *o));
	// What moves here: every placed object but the hero's, enemies, crates and the shared ones, and those shown as another game has them.
	let mut ours = Vec::new();
	for slot in 1..slots as u16 {
		let a = table + slot as u32 * 0x100;
		let h = rd(ram, a + 0x24) & MASK;
		if h == 0 || rd8(ram, a + 0x20) >= 0x80 || m.mirrored.contains_key(&slot) || mp.watched.contains_key(&slot) || player.contains(&a) {
			continue;
		}
		let kind = (rd(ram, h + 0x44) >> 16) as i16;
		if matches!(kind, HERO | ENEMY | SHOT | CRATE | FLYER | BOLT) || rd(ram, a + 0x74) == 0 || rd(ram, a + 0x34) & 1 != 0 {
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
		m.seen.insert(slot, (now, seq, moved));
		let pos = [now[0], now[1], now[2]];
		if moved != 0 && fields - moved < MOVING_FOR && nearest_is_me(pos) && others.iter().any(|o| dist2(pos, *o) < STREAM_RANGE * STREAM_RANGE) {
			ours.push((others.iter().map(|o| dist2(pos, *o)).fold(f32::MAX, f32::min), read_pose(ram, a, slot, (rd(ram, a + 0xa4) >> 16) as u16)));
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
	let mut gone = Vec::new();
	for (slot, t) in m.theirs.iter_mut() {
		let a = table + *slot as u32 * 0x100;
		if !alive(ram, a, t.class) {
			gone.push(*slot);
			continue;
		}
		let here = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)];
		let there = t.track.newest().map_or(here, |h| h.pos);
		let mine = dist2(here, me.pos).min(dist2(there, me.pos)).sqrt();
		let theirs = senders.iter().find(|s| s.0 == t.from).map_or(f32::MAX, |s| dist2(there, s.1).sqrt());
		if mine < NEAR || mine < theirs + NEARER_BY {
			if let Some(update) = m.mirrored.remove(slot) {
				wake(ram, a, update, m.last.remove(slot));
			}
			gone.push(*slot);
			continue;
		}
		if !m.mirrored.contains_key(slot) {
			// Shown from now (only while a stream comes: one that stopped before this game took it shows nothing).
			if fields.saturating_sub(t.heard) > QUIET {
				gone.push(*slot);
				continue;
			}
			m.mirrored.insert(*slot, enemies::no_update(ram, a));
		}
		if let Some(h) = t.track.sample(DELAY) {
			m.last.insert(*slot, h.state as u8);
			enemies::place(ram, a, &enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: 0.0, scale: rd_f(ram, a + 0x2c) });
		}
	}
	for slot in gone {
		m.theirs.remove(&slot);
	}
	// Shown and no stream left: held as last shown until this hero comes near.
	let held: Vec<u16> = m.mirrored.keys().filter(|s| !m.theirs.contains_key(s)).copied().collect();
	for slot in held {
		let a = table + slot as u32 * 0x100;
		let here = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)];
		if rd(ram, a + 0x24) == 0 || rd8(ram, a + 0x20) >= 0x80 {
			m.mirrored.remove(&slot);
			m.last.remove(&slot);
		} else if dist2(here, me.pos) < NEAR * NEAR {
			let update = m.mirrored.remove(&slot).unwrap_or(0);
			wake(ram, a, update, m.last.remove(&slot));
		}
	}
}

/// How many objects are shown here as another game has them, for the run log.
pub(super) fn shown(mp: &Mp) -> usize {
	mp.machines.mirrored.len()
}

/// Leaving the session or the level: every object runs here again.
pub(super) fn stop(c: &mut Cpu, mp: &mut Mp) {
	let table = mp.table;
	release_all(c, &mut mp.machines, table, table != 0);
	mp.machines.theirs.clear();
}
