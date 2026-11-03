//! Spawned objects: enemies and enemy shots made during play. They have no slot in common across games, so each game reports what it
//! makes (`tick`) and the host gives it an id, or says it is a copy of another game's, which stands down: hidden, out of reach and not
//! updated. The game that runs an object streams it with the placed enemies, and the others keep a stand-in, made by the game's own
//! `create_moby`, that never updates. A shot's stand-in that reaches the local hero hits him as a shot would.
//!
//! The class types say less than they seem to: a cutscene's Qwark, a lit pillar and a racer's bike are all "enemies" here, and every
//! game has its own, and needs it. So only what appears during play is shared, with an update of its own, and nothing the level
//! started with (a respawn puts those back, so an enemy in such a place is one of them). In all 19 levels that came to shots: the game places its enemies.
//!
//! An object belongs to one game at a time, and changes hands when that game is gone, out of the session or out of the level
//! (`hand_over`, on the host). The new owner wakes its copy where the object last was. With no copy it lets its stand-in's update run:
//! the class starts the object as new, where it stands, with the health it had. An object left with nobody in its level is gone, and
//! with no host each game keeps running what it has, until the next host says otherwise.

use super::*;

/// The class type of an enemy's shot.
const SHOT: i16 = 6;
/// The players' own weapons' shots, which have those types too: bomb, mine, R.Y.N.O. rocket, Doom agent and capsule, Visibomb missile,
/// Devastator rocket, Blaster shot. They are shared, so a player sees his partner's, but only the game that fired one makes it (no copy),
/// and its stand-ins hurt nobody: what they hit, they hit in the game that fired them.
const PLAYER_SHOTS: [u16; 8] = [121, 74, 457, 186, 230, 172, 153, 305];
/// A shot's stand-in this near the middle of the local hero (metres) hits him. The middle is this far above his feet.
const SHOT_REACH: f32 = 1.0;
const HERO_MIDDLE: f32 = 0.6;
/// A shot that moved less than this in a field (metres) has landed.
const SHOT_STILL: f32 = 0.01;
/// The damage a shot's stand-in does: the game's hits on the hero take at most one orb.
const SHOT_DAMAGE: f32 = 1.0;
/// Stand-ins made in one field, at most.
const STANDINS_PER_FIELD: usize = 4;
/// The first thing an update function does when it does nothing.
const JR_RA: u32 = 0x03E0_0008;
/// Fields an object's game has to be away from its level before the object changes hands: a game can be between two states of itself.
const OWNER_GONE: u64 = 120;

/// A spawned object of this game.
pub(super) struct Spawned {
	local: u16,
	pub(super) class: u16,
	pub(super) kind: i16,
	/// The host's id for it, while this game runs it.
	pub(super) id: Option<u16>,
	/// The object of another game it is a copy of.
	of: Option<u16>,
	/// Standing down: the update and flags it goes back to.
	down: Option<(u32, u32)>,
	/// The health to put back after its first update.
	health: Option<f32>,
}

/// The stand-in of another game's spawned object.
pub(super) struct StandIn {
	pub(super) moby: u32,
	pub(super) class: u16,
	kind: i16,
	/// A player's shot: it hits no hero and changes no hands.
	harmless: bool,
	/// The update `create_moby` gave it, which is the class's start.
	update: u32,
	/// Where it was last field, and whether it hit: a shot hits once, in flight.
	was: [f32; 3],
	hit: bool,
}

/// A spawned object as the host follows it, and the fields its game has been away from its level.
pub(super) struct Live {
	level: u8,
	class: u16,
	owner: PlayerId,
	away: u64,
}

/// The host's answer to a report: the object has an id, and whoever the owner is runs it.
pub(super) fn answered(mp: &mut Mp, id: u16, owner: PlayerId, local: u16, level: u8, class: u16) {
	let key = WorldKey { level, slot: id, class };
	mp.owners.insert(key, owner);
	if mp.my_id == Some(0) {
		mp.live.insert(id, Live { level, class, owner, away: 0 });
	}
	if Some(owner) == mp.my_id {
		match mp.spawned.values_mut().find(|s| s.local == local) {
			Some(s) => s.id = Some(id),
			// Gone before the answer came.
			None => wire().gone.push(key),
		}
	}
}

/// The host's answer to a report: what we made is a copy of its object `of`.
pub(super) fn copy_of(mp: &mut Mp, local: u16, of: u16) {
	if let Some(s) = mp.spawned.values_mut().find(|s| s.local == local) {
		s.of = Some(of);
	}
}

/// This game's moby for another game's spawned object `id`: its stand-in, or a copy of its own.
pub(super) fn local_of(mp: &Mp, id: u16) -> Option<u32> {
	mp.stand_ins.get(&id).map(|s| s.moby).or_else(|| mp.spawned.iter().find(|(_, s)| s.of == Some(id)).map(|(a, _)| *a))
}

/// Another game runs this object from now on, or this one does.
pub(super) fn new_owner(mp: &mut Mp, key: WorldKey, owner: PlayerId) {
	if Some(owner) == mp.my_id {
		mp.takeovers.push(key);
	} else if let Some(s) = mp.spawned.values_mut().find(|s| s.id == Some(key.slot)) {
		// Not gone after all: ours stands down.
		s.of = s.id.take();
	}
}

/// Host: an object whose game has gone goes to the nearest player still in its level. With nobody there it is gone.
pub(super) fn hand_over(mp: &mut Mp, me: &HeroState) {
	if mp.my_id != Some(0) || mp.fields % OWN_EVERY != 0 {
		return;
	}
	let Mp { live, removed, poses, shown, owners, takeovers, .. } = mp;
	live.retain(|id, o| !removed.contains(&WorldKey { level: o.level, slot: *id, class: o.class }));
	let players: Vec<(PlayerId, i8, [f32; 3])> = std::iter::once((0, me.level, me.pos)).chain(shown.iter().map(|(id, h)| (*id, h.level, h.pos))).collect();
	let mut gone = Vec::new();
	for (id, o) in live.iter_mut() {
		let (level, key) = (o.level as i8, WorldKey { level: o.level, slot: *id, class: o.class });
		if players.iter().any(|p| p.0 == o.owner && p.1 == level) {
			o.away = 0;
			continue;
		}
		o.away += OWN_EVERY;
		if o.away < OWNER_GONE {
			continue;
		}
		let at = poses.get(id).and_then(|t| t.track.newest()).map(|h| h.pos);
		let d = |p: &&(PlayerId, i8, [f32; 3])| at.map_or(0.0, |a| dist2(a, p.2));
		match players.iter().filter(|p| p.1 == level).min_by(|a, b| d(a).total_cmp(&d(b))) {
			Some(&(to, ..)) => {
				eprintln!("mp: class {} (id {id:#x}) goes from player {} to player {to}", o.class, o.owner);
				(o.owner, o.away) = (to, 0);
				owners.insert(key, to);
				poses.remove(id);
				wire().assign.push((key, to));
				if to == 0 {
					takeovers.push(key);
				}
			}
			None => gone.push(key),
		}
	}
	for key in gone {
		live.remove(&key.slot);
		removed.insert(key);
		wire().gone.push(key);
	}
}

/// This game runs the object from now on: its copy wakes where the object last was, or its stand-in becomes it.
fn take_over(c: &mut Cpu, mp: &mut Mp, key: WorldKey) {
	let id = key.slot;
	let stand_in = mp.stand_ins.remove(&id);
	mp.poses.remove(&id);
	let pose = stand_in.as_ref().map(|s| read_pose(&c.mem.ram, s.moby, id, s.class));
	let ram = &mut c.mem.ram[..];
	if let Some((&a, s)) = mp.spawned.iter_mut().find(|(_, s)| s.of == Some(id)) {
		if let Some(p) = pose {
			enemies::apply(ram, a, &enemies::Driven { pos: p.pos, rot: p.rot, anim: p.anim, health: p.health, scale: p.scale });
		}
		if let Some((update, flags)) = s.down.take() {
			enemies::wake(ram, a, update, flags);
		}
		(s.id, s.of) = (Some(id), None);
		eprintln!("mp: class {} (id {id:#x}) is ours now: our copy wakes", s.class);
		if let Some(s) = stand_in {
			remove(c, s.moby);
		}
		return;
	}
	match (stand_in, pose) {
		(Some(s), Some(p)) if s.kind == ENEMY && !s.harmless => {
			enemies::set_update(ram, s.moby, s.update);
			eprintln!("mp: class {} (id {id:#x}) is ours now: our stand-in starts", s.class);
			let local = mp.next_local;
			mp.next_local = local.wrapping_add(1);
			mp.spawned.insert(s.moby, Spawned { local, class: s.class, kind: s.kind, id: Some(id), of: None, down: None, health: Some(p.health) });
		}
		// A shot can't be taken on: it has no flight here. Nor can what never reached this game.
		(s, _) => {
			if let Some(s) = s {
				remove(c, s.moby);
			}
			mp.removed.insert(key);
			wire().gone.push(key);
		}
	}
}

fn remove(c: &mut Cpu, moby: u32) {
	if let Err(e) = crate::omni::delete_moby(c, moby) {
		eprintln!("mp: could not remove a stand-in: {e}");
	}
}

/// One field of spawned objects: a walk of the spawned part of the moby table for what this game made, the report of what is new and
/// of what went, the stand-down of a copy, what changes hands, and the stand-ins of what other games made.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if mp.spawned_level != me.level {
		// Another level: the game cleared the table, so there is nothing to delete.
		mp.spawned.clear();
		mp.stand_ins.clear();
		mp.takeovers.clear();
		mp.start_places.clear();
		mp.removed.retain(|k| k.slot < SPAWNED_ID);
		mp.spawned_level = me.level;
		mp.opening = WORLD_SETTLE;
	}
	if me.level < 0 || mp.table == 0 || mp.my_id.is_none() {
		return;
	}
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let (cell, end_cell) = table(level as u32);
	let (from, end) = (rd(ram, cell + 4) & MASK, rd(ram, end_cell) & MASK);
	let slots = end.wrapping_sub(from) / 0x100;
	if end <= from || slots > WORLD_SLOTS {
		return;
	}
	// The level goes on starting through the flight in and until it has been played for a moment.
	let starting = mp.opening > 0;
	if me.mode == 0 {
		mp.opening = mp.opening.saturating_sub(1);
	}
	// A respawn clears the table too, and the game can take a stand-in away itself: one that went comes back with the next pose.
	mp.stand_ins.retain(|_, s| enemies::still_ours(ram, s.moby, s.class));
	let ours: BTreeSet<u32> = mp.stand_ins.values().map(|s| s.moby).collect();
	let mut alive = BTreeSet::new();
	for i in 0..slots {
		let a = from + i * 0x100;
		let header = rd(ram, a + 0x24) & MASK;
		if header == 0 || rd8(ram, a + 0x20) >= 0x80 || ours.contains(&a) {
			continue;
		}
		let (class, kind) = ((rd(ram, a + 0xa4) >> 16) as u16, (rd(ram, header + 0x44) >> 16) as i16);
		if kind != ENEMY && kind != SHOT && !PLAYER_SHOTS.contains(&class) {
			continue;
		}
		if !mp.spawned.get(&a).is_some_and(|s| s.class == class) {
			let pos = [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)];
			// Its maker has yet to place it.
			if pos == [0.0; 3] {
				continue;
			}
			let place = (class, pos.map(|v| (v * 10.0) as i32));
			if starting {
				mp.start_places.insert(place);
			}
			let local = mp.next_local;
			mp.next_local = local.wrapping_add(1);
			// Kept here all the same: what is not shared is only looked at once.
			mp.spawned.insert(a, Spawned { local, class, kind, id: None, of: None, down: None, health: None });
			let update = rd(ram, a + 0x74) & MASK;
			if me.mode == 0 && !starting && (kind != ENEMY || !mp.start_places.contains(&place)) && update != 0 && rd(ram, update) != JR_RA {
				let solo = PLAYER_SHOTS.contains(&class);
				wire().spawns.push((local, level, class, pos, kind == ENEMY && !solo, solo));
			}
		}
		alive.insert(a);
	}
	let removed = &mut mp.removed;
	mp.spawned.retain(|a, s| {
		let here = alive.contains(a);
		if let (false, Some(id)) = (here, s.id) {
			let key = WorldKey { level, slot: id, class: s.class };
			removed.insert(key);
			wire().gone.push(key);
		}
		here
	});
	let ram = &mut c.mem.ram[..];
	for (a, s) in mp.spawned.iter_mut() {
		if let Some(health) = s.health.take() {
			enemies::set_health(ram, *a, health);
		}
		if s.of.is_some() && s.down.is_none() {
			s.down = Some((enemies::no_update(ram, *a), enemies::hide(ram, *a)));
			eprintln!("mp: our class {} is a copy of another game's: it stands down", s.class);
		}
	}
	// The host went: every game keeps what it has.
	if std::mem::take(&mut mp.orphaned) {
		let held = mp.stand_ins.iter().map(|(id, s)| (*id, s.class)).chain(mp.spawned.values().filter_map(|s| Some((s.of?, s.class))));
		let keys: BTreeSet<WorldKey> = held.map(|(slot, class)| WorldKey { level, slot, class }).collect();
		mp.takeovers.extend(keys);
	}
	for key in std::mem::take(&mut mp.takeovers) {
		if key.level == level && !mp.removed.contains(&key) {
			take_over(c, mp, key);
		}
	}
	// Back in: the host hears again of what this game has.
	if std::mem::take(&mut mp.rejoined) {
		mp.spawned.clear();
	}
	// Stand-ins go with their object. A shot's also goes when its poses stop: an enemy's can only wait for its next owner.
	let fields = mp.fields;
	let stale = |id: &u16| mp.poses.get(id).is_none_or(|t| fields.saturating_sub(t.heard) > POSE_STALE);
	let over: Vec<u16> = mp.stand_ins.iter().filter(|(id, s)| mp.removed.contains(&WorldKey { level, slot: **id, class: s.class }) || ((s.kind != ENEMY || s.harmless) && stale(id))).map(|(id, _)| *id).collect();
	for id in over {
		if let Some(s) = mp.stand_ins.remove(&id) {
			mp.poses.remove(&id);
			remove(c, s.moby);
		}
	}
	// New ones: an id with poses and no stand-in. The poses say the class. None while the local hero dies: a respawn is coming.
	let new: Vec<(u16, u16)> = mp.poses.iter().filter(|(id, t)| **id >= SPAWNED_ID && !mp.stand_ins.contains_key(*id) && fields.saturating_sub(t.heard) <= POSE_STALE).map(|(id, t)| (*id, t.class)).take(if super::dying(me) { 0 } else { STANDINS_PER_FIELD }).collect();
	for (id, class) in new {
		match crate::omni::create_moby(c, class as u32) {
			Ok(moby) if moby != 0 => {
				let moby = moby & MASK;
				let update = enemies::no_update(&mut c.mem.ram, moby);
				enemies::show(&mut c.mem.ram, moby);
				let kind = (rd(&c.mem.ram, (rd(&c.mem.ram, moby + 0x24) & MASK) + 0x44) >> 16) as i16;
				eprintln!("mp: stand-in for class {class} (id {id:#x})");
				mp.stand_ins.insert(id, StandIn { moby, class, kind, harmless: PLAYER_SHOTS.contains(&class), update, was: [0.0; 3], hit: false });
			}
			Ok(_) => {}
			Err(e) => eprintln!("mp: could not make a stand-in of class {class}: {e}"),
		}
	}
	// A shot that reaches this game's hero, in play.
	let middle = [me.pos[0], me.pos[1], me.pos[2] + HERO_MIDDLE];
	let hero = enemies::hero_moby(&c.mem.ram);
	let mut struck = Vec::new();
	for s in mp.stand_ins.values_mut().filter(|s| s.kind == SHOT && !s.harmless && !s.hit && me.mode == 0) {
		let at = [rd_f(&c.mem.ram, s.moby + 0x10), rd_f(&c.mem.ram, s.moby + 0x14), rd_f(&c.mem.ram, s.moby + 0x18)];
		let flying = dist2(at, s.was) > SHOT_STILL * SHOT_STILL;
		s.was = at;
		if flying && dist2(at, middle) < SHOT_REACH * SHOT_REACH {
			s.hit = true;
			struck.push(s.moby);
		}
	}
	for shot in struck {
		if let Err(e) = enemies::post_hit(c, hero, shot, SHOT_DAMAGE, 1) {
			eprintln!("mp: could not post a shot on the hero: {e}");
		}
	}
}
