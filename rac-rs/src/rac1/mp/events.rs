//! What the session reported since the last field, taken in (`tick`): players coming and going, the host lost or found again,
//! and what the others' games stream and say.

use super::*;

/// One thing the session reported, taken in.
pub(super) fn on_event(c: &mut Cpu, mp: &mut Mp, me: &HeroState, ev: Event) {
	match ev {
		Event::State { id, hero } => mp.tracks.entry(id).or_default().push(hero),
		Event::Joined { id, name } => {
			eprintln!("mp: {name:?} joined as player {id}");
			// The host a client joined is named by the title's line (`title_hint`); everyone else is announced.
			if id != 0 || mp.my_id == Some(0) {
				notice::say(format!("{name} joined"), 6);
			}
		}
		Event::Left { id } => {
			mp.tracks.remove(&id);
			eprintln!("mp: player {id} left");
			notice::say(format!("{} left", notice::name_of(id)), 6);
		}
		Event::Connected { id } => connected(mp, id),
		Event::Rejected(why) => rejected(why),
		Event::Stranger(_) => {}
		Event::Lost => lost(c, mp),
		Event::Removed(key) => {
			mp.removed.insert(key);
		}
		Event::Back(key) => {
			mp.removed.remove(&key);
		}
		Event::Travel(level) => {
			mp.host_level = Some(level);
			if follows() {
				mp.follow = Some(level);
				mp.next_follow = 0;
			}
		}
		// A team travels together: a client took a ship by itself, and the host's game goes after it. The others then follow the host.
		Event::Went { id, level } if follows() && mp.my_id == Some(0) => {
			eprintln!("mp: player {id} went to level {level}: the team goes too");
			mp.follow = Some(level);
			mp.next_follow = 0;
		}
		Event::Went { .. } => {}
		Event::Owner { key, owner } => {
			mp.owners.insert(key, owner);
			mp.poses.remove(&key.slot);
			if key.slot >= SPAWNED_ID {
				spawned::new_owner(mp, key, owner);
			}
		}
		Event::Hit { key, damage, mask } => mp.hits_in.push((key, damage, mask)),
		Event::Enemies { id, level, tick, poses } if level as i8 == me.level => poses_heard(mp, me, id, level, tick, poses),
		Event::Enemies { .. } => {}
		Event::Spawned { id, owner, local, level, class } => spawned::answered(mp, id, owner, local, level, class),
		Event::Duplicate { local, of } => spawned::copy_of(mp, local, of),
		Event::Fx { id, level, events } if level as i8 == me.level => mp.fx_in.extend(events.into_iter().map(|e| (mp.fields + FX_DELAY, id, e))),
		Event::Fx { .. } => {}
		Event::Touch { key, damage, mask } => mp.touches_in.push((key, damage, mask)),
		Event::Machines { id, level, tick, poses } if level as i8 == me.level => machines::heard(mp, id, tick, poses),
		Event::Machines { .. } => {}
		Event::Vars { key, state, words } => machines::vars_heard(mp, key, state, words),
		Event::Scene { level, id } => scenes::heard(mp, level, id),
		Event::Story(p) => story_heard(mp, p),
		Event::Keep { name, keep, live } => {
			let me = mp.my_name.clone();
			keep::heard(mp, name, keep, live, &me);
		}
		Event::Arcs { id, level, arcs, .. } if level as i8 == me.level => arcs_heard(mp, id, level, arcs),
		Event::Arcs { .. } => {}
		Event::Particles { level, particles, .. } if level as i8 == me.level => {
			mp.particles_in.extend(particles);
			let over = mp.particles_in.len().saturating_sub(PARTICLES_KEEP);
			mp.particles_in.drain(..over);
		}
		Event::Draws { id, level, draws, .. } if level as i8 == me.level => draws_heard(mp, id, level, draws),
		Event::Quads { id, level, tick, part, rope, quads, .. } if level as i8 == me.level => quads_heard(mp, id, tick, part, rope, quads),
		Event::Particles { .. } | Event::Draws { .. } | Event::Quads { .. } => {}
	}
}

/// In with the host, as player `id`: at first, or back after losing it.
fn connected(mp: &mut Mp, id: PlayerId) {
	eprintln!("mp: in, as player {id}");
	notice::status(None, false);
	notice::say(if mp.my_id.is_some() { "Back in with the host." } else { "Connected." }, 6);
	mp.my_id = Some(id);
	// Back after losing the host, perhaps a new one: it hears again what this game knows is gone. Ids of spawned objects were the old one's.
	mp.removed.retain(|k| k.slot < SPAWNED_ID);
	wire().gone.extend(mp.removed.iter().copied());
	mp.live.clear();
	mp.rejoined = true;
	// The host may be a new one: it hears every player's things this game remembers, and the team's progress (it keeps
	// what it has none of).
	wire().remembered = keep::remembered(mp, &mp.my_name.clone());
	if id != 0 {
		wire().story.extend(mp.story_seen.iter().copied());
	}
	arrive::connected(mp);
}

/// The host refused us: the title says why.
fn rejected(why: rac_mp::RejectReason) {
	eprintln!("mp: the host refused us: {why:?}");
	notice::status(Some(match why {
		rac_mp::RejectReason::Version => "Could not join: your friend's game is another version. Update both games to the same release.",
		rac_mp::RejectReason::Region => "Could not join: your friend's disc is from the other region (a PAL and a USA game can't play together).",
		rac_mp::RejectReason::Full => "Could not join: that game already has 4 players.",
	}.into()), true);
}

/// The host is gone: nobody is shown or runs anything for us until a host takes us back.
fn lost(c: &mut Cpu, mp: &mut Mp) {
	mp.tracks.clear();
	mp.owners.clear();
	mp.poses.clear();
	mp.orphaned = true;
	machines::stop(c, mp);
	if mp.my_id.is_some() {
		eprintln!("mp: the host is gone, trying to get back in");
		notice::status(Some("Lost the host. Trying to get back in ...".into()), true);
	}
}

/// The enemies another game runs in this level: their poses, for the mirror.
fn poses_heard(mp: &mut Mp, me: &HeroState, id: PlayerId, level: u8, tick: u32, poses: Vec<EnemyPose>) {
	for p in poses {
		let key = WorldKey { level, slot: p.slot, class: p.class };
		if !mp.removed.contains(&key) && owner_of(mp, &key) == id {
			let t = mp.poses.entry(p.slot).or_default();
			t.track.push(HeroState { tick, level: me.level, pos: p.pos, rot: p.rot, anim: p.anim, ..Default::default() });
			(t.class, t.health, t.scale, t.heard, t.alpha, t.ovr) = (p.class, p.health, p.scale, mp.fields, p.alpha, p.ovr);
		}
	}
}

/// Another game's progress: to give this game, and to give a host we get back in with.
fn story_heard(mp: &mut Mp, p: Progress) {
	if !mp.story_in.contains(&p) {
		mp.story_in.push(p);
	}
	if !mp.story_seen.contains(&p) {
		mp.story_seen.push(p);
	}
}

/// The arcs of the enemies another game runs in this level.
fn arcs_heard(mp: &mut Mp, id: PlayerId, level: u8, arcs: Vec<ArcPose>) {
	for a in arcs {
		if owner_of(mp, &WorldKey { level, slot: a.slot, class: mp.poses.get(&a.slot).map_or(0, |t| t.class) }) == id || a.slot >= SPAWNED_ID {
			mp.arcs_in.insert(a.slot, (a, mp.fields));
		}
	}
}

/// What an enemy another game runs in this level drew.
fn draws_heard(mp: &mut Mp, id: PlayerId, level: u8, draws: Draws) {
	let class = mp.poses.get(&draws.slot).map_or(0, |t| t.class);
	if draws.slot >= SPAWNED_ID || owner_of(mp, &WorldKey { level, slot: draws.slot, class }) == id {
		let now = mp.fields;
		mp.draws.got(draws, now);
	}
}

/// Part `part` of another player's weapon beams of his field `tick` (the rope comes with the first part).
fn quads_heard(mp: &mut Mp, id: PlayerId, tick: u32, part: u8, rope: Option<RopePose>, quads: Vec<FxQuad>) {
	let now = mp.fields;
	let e = mp.quads_in.entry(id).or_insert((now, tick, None, Vec::new()));
	if e.1 != tick {
		*e = (now, tick, None, Vec::new());
	}
	e.0 = now;
	if part == 0 {
		e.2 = rope;
	}
	e.3.extend(quads);
}
