//! One field of synced enemies (`enemy_tick`; the hands are in mp/enemies.rs): the hits that came and went, who runs what (host),
//! the stream of what this game runs, and the mirror of what other games run.

use super::*;

/// One field of synced enemies (mp/enemies.rs): the hits that came and went, who runs what (host), the stream of what this game runs,
/// and the mirror of what other games run.
pub(super) fn enemy_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let (Some(my), true) = (mp.my_id, me.level >= 0 && mp.table != 0) else {
		enemies::set_driven(BTreeMap::new());
		return;
	};
	let level = me.level as u8;
	hits_in(c, mp, my, level);
	hits_out(c, mp, level);
	// The other players here who can run an enemy: in play and alive.
	let others: Vec<(PlayerId, [f32; 3])> = mp.shown.iter().filter(|(_, h)| h.level == me.level && h.mode == 0 && h.health > 0).map(|(id, h)| (*id, h.pos)).collect();
	let foes: Vec<(u16, u16)> = mp.watched.iter().filter(|(_, v)| v.1 == ENEMY).map(|(s, v)| (*s, v.0)).collect();
	give_out(c, mp, me, my, &others, &foes);
	let ours = scene(c, mp, me, my, &others, &foes);
	touches_out(c, mp, level);
	touches_in(c, mp, level);
	fx_out(c, level, &others);
	if mp.fields % SEND_EVERY == 0 && !others.is_empty() {
		stream(c, mp, me, my, &others, &foes, &ours);
	}
	looks_out(c, mp, me, &others, &ours);
	// The local hero dies or respawns: every enemy runs here, its own way, through the respawn's reset.
	if mp.held {
		enemies::set_driven(BTreeMap::new());
		mp.draws.show(c, mp.fields, &BTreeMap::new());
		return;
	}
	let shown_at = mirror(c, mp, my, level);
	show_theirs(c, mp, me, shown_at);
}

/// Other games' hits on what this game runs, a few a field.
fn hits_in(c: &mut Cpu, mp: &mut Mp, my: PlayerId, level: u8) {
	// Not while the local hero dies or respawns: the blow is dealt by him (his moby the attacker), and the respawn resets the level.
	let hits: Vec<_> = if mp.held { Vec::new() } else { mp.hits_in.drain(..mp.hits_in.len().min(BLOWS_PER_FIELD)).collect() };
	for (key, damage, mask) in hits {
		let moby = if key.slot >= SPAWNED_ID {
			mp.spawned.iter().find(|(_, s)| s.id == Some(key.slot)).map(|(a, _)| *a)
		} else {
			mp.ids.addr(key.slot).filter(|_| owner_of(mp, &key) == my)
		};
		if let Some(moby) = moby.filter(|_| key.level == level) {
			note(format!("another player's hit on id {:#x} class {} at {moby:#x}", key.slot, key.class));
			let hero = enemies::hero_moby(&c.mem.ram);
			if let Err(e) = enemies::post_hit(c, moby, hero, damage, mask) {
				eprintln!("mp: could not post a hit on slot {}: {e}", key.slot);
			}
		}
	}
}

/// This game's player's hits on what other games run, to the game that runs it.
fn hits_out(c: &mut Cpu, mp: &Mp, level: u8) {
	for h in enemies::take_hits() {
		let stand_in = mp.stand_ins.iter().find(|(_, s)| s.moby == h.moby).map(|(id, s)| (*id, s.class));
		let Some((slot, class)) = stand_in.or_else(|| Some((mp.ids.id(h.moby)?, (rd(&c.mem.ram, h.moby + 0xa4) >> 16) as u16))) else { continue };
		// A mod that scores blows (Arsenal Mastery) scores this one here, for this player; what it came to goes to the enemy's game.
		let damage = enemies::resolve_here(c, &h).unwrap_or(h.damage);
		if damage > 0.0 {
			wire().hits.push((WorldKey { level, slot, class }, damage, h.mask));
		}
	}
}

/// Who runs which enemy, every `OWN_EVERY` fields: one goes to the player much nearer to it than the one who runs it.
fn give_out(c: &Cpu, mp: &mut Mp, me: &HeroState, my: PlayerId, others: &[(PlayerId, [f32; 3])], foes: &[(u16, u16)]) {
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	// Who gives out the enemies here: the host, or in a level the host isn't in, the first player in it.
	let leader = std::iter::once(my).chain(mp.shown.iter().filter(|(_, h)| h.level == me.level).map(|(id, _)| *id)).min().unwrap_or(my);
	if leader == my && mp.fields % OWN_EVERY == 0 {
		let mut players = others.to_vec();
		if me.mode == 0 && me.health > 0 {
			players.push((my, me.pos));
		}
		for &(slot, class) in foes {
			let key = WorldKey { level, slot, class };
			let Some(a) = mp.ids.addr(slot) else { continue };
			let (cur, at) = (owner_of(mp, &key), read_pose(ram, a, slot, class).pos);
			let d = |p: &(PlayerId, [f32; 3])| dist2(at, p.1).sqrt();
			let now = players.iter().find(|p| p.0 == cur).map_or(f32::MAX, d);
			let best = players.iter().min_by(|a, b| d(a).total_cmp(&d(b)));
			if let Some(b) = best.filter(|b| b.0 != cur && d(b) + OWN_MARGIN < now) {
				mp.owners.insert(key, b.0);
				mp.poses.remove(&slot);
				wire().assign.push((key, b.0));
			}
		}
	}
}

/// What this game runs that others mirror (by moby, its id), told to the hands for sounds and blows with whether anyone else is here.
fn scene(c: &Cpu, mp: &Mp, me: &HeroState, my: PlayerId, others: &[(PlayerId, [f32; 3])], foes: &[(u16, u16)]) -> BTreeMap<u32, u16> {
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let ours: BTreeMap<u32, u16> = foes.iter().filter(|&&(slot, class)| owner_of(mp, &WorldKey { level, slot, class }) == my).filter_map(|&(slot, _)| Some((mp.ids.addr(slot)?, slot))).chain(mp.spawned.iter().filter_map(|(a, s)| Some((*a, s.id?)))).collect();
	let player: Vec<u32> = std::iter::once(enemies::hero_moby(ram)).chain(PART_CELLS.iter().map(|&c| rd(ram, c) & MASK)).filter(|&a| a != 0).collect();
	fx::set_scene(ours.clone(), player, !others.is_empty() && me.mode == 0, mp.ids.by_addr(), mp.fields);
	touch::set_scene(!others.is_empty() && me.mode == 0, mp.ids.by_addr(), mp.fields);
	ours
}

/// This game's player's blows on the level's placed objects, for the others too.
fn touches_out(c: &Cpu, mp: &mut Mp, level: u8) {
	let ram = &c.mem.ram[..];
	for (slot, class, damage, mask) in touch::take() {
		eprintln!("mp: a blow on slot {slot} class {class}, for the others too");
		wire().touches.push((WorldKey { level, slot, class }, damage, mask));
		// What a player's blow breaks (Novalis' and Kerwan's breakable props) is then gone for everyone, as a crate is, a player who
		// comes later or was too far for the blow to land included: watched from the blow on.
		if let Some(a) = mp.ids.addr(slot) {
			let kind = (rd(ram, (rd(ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
			mp.watched.entry(slot).or_insert((class, kind));
		}
	}
}

/// The other players' blows on the level's placed objects, dealt here.
fn touches_in(c: &mut Cpu, mp: &mut Mp, level: u8) {
	// Theirs here, a few a field, and those still waiting for their object.
	let hero = enemies::hero_moby(&c.mem.ram);
	let theirs: Vec<_> = if mp.held { Vec::new() } else {
		touch::keep_up(c, hero, BLOWS_PER_FIELD);
		mp.touches_in.drain(..mp.touches_in.len().min(BLOWS_PER_FIELD)).collect()
	};
	for (key, damage, mask) in theirs {
		let Some(a) = mp.ids.addr(key.slot) else { continue };
		let ram = &c.mem.ram[..];
		if key.level == level && rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 && (rd(ram, a + 0xa4) >> 16) as u16 == key.class {
			match touch::take_blow(c, a, hero, damage, mask) {
				Ok(()) => eprintln!("mp: another player's blow on slot {} class {}", key.slot, key.class),
				Err(e) => eprintln!("mp: could not deal another player's blow: {e}"),
			}
		}
	}
}

/// The sounds and effects of what this game runs, made near another player, to the others.
fn fx_out(c: &Cpu, level: u8, others: &[(PlayerId, [f32; 3])]) {
	let ram = &c.mem.ram[..];
	let near = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < STREAM_RANGE * STREAM_RANGE);
	let caught: Vec<FxEvent> = fx::take()
		.into_iter()
		.filter(|(moby, e)| match e {
			FxEvent::Explode { pos, .. } | FxEvent::Spark { pos, .. } | FxEvent::Glow { pos, .. } | FxEvent::Burst { pos, .. } | FxEvent::Flash { pos, .. } => near([pos[0], pos[1], pos[2]]),
			FxEvent::Cue { .. } | FxEvent::PlayerCue { .. } => near([rd_f(ram, moby + 0x10), rd_f(ram, moby + 0x14), rd_f(ram, moby + 0x18)]),
		})
		.map(|(_, e)| e)
		.collect();
	if !caught.is_empty() {
		fx::SENT.fetch_add(caught.len() as u64, Ordering::Relaxed);
		caught.iter().for_each(|e| {
			fx::SENT_KINDS[fx::kind(e)].fetch_add(1, Ordering::Relaxed);
		});
		wire().fx = Some((level, caught));
	}
}

/// The stream of what this game runs: the arcs its enemies keep up, and their poses near another player (a spawned enemy's also far
/// from them, now and then).
fn stream(c: &Cpu, mp: &Mp, me: &HeroState, my: PlayerId, others: &[(PlayerId, [f32; 3])], foes: &[(u16, u16)], ours: &BTreeMap<u32, u16>) {
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let held_arcs = arcs::read(ram, me.level, ours);
	if !held_arcs.is_empty() {
		wire().arcs = Some((level, mp.fields as u32, held_arcs));
	}
	let near = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < STREAM_RANGE * STREAM_RANGE);
	let placed = foes
		.iter()
		.filter(|&&(slot, class)| owner_of(mp, &WorldKey { level, slot, class }) == my)
		.filter_map(|&(slot, class)| Some(read_pose(ram, mp.ids.addr(slot)?, slot, class)));
	let far = mp.fields % FAR_EVERY == 0;
	let made = mp.spawned.iter().filter_map(|(a, s)| Some((read_pose(ram, *a, s.id?, s.class), s.kind))).filter(|(p, kind)| near(p.pos) || (far && *kind == ENEMY)).map(|(p, _)| p);
	let poses: Vec<EnemyPose> = placed.filter(|p| near(p.pos)).chain(made).collect();
	if !poses.is_empty() {
		wire().enemies = Some((level, mp.fields as u32, poses));
	}
}

/// The particles and draws of what this game runs that the others see.
fn looks_out(c: &mut Cpu, mp: &mut Mp, me: &HeroState, others: &[(PlayerId, [f32; 3])], ours: &BTreeMap<u32, u16>) {
	let level = me.level as u8;
	// Particles and draws of ours that others see (mp/particles.rs, mp/draws.rs).
	let seen = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < DRAW_RANGE * DRAW_RANGE);
	let in_play = me.mode == 0 && !mp.held && !others.is_empty();
	let ram = &c.mem.ram[..];
	let ps = if in_play { mp.particles.take(ram, me.level, mp.fields, ours, Some(enemies::hero_moby(ram)).filter(|&h| h != 0), seen, &mp.dying) } else { Vec::new() };
	if !ps.is_empty() {
		wire().particles = Some((level, ps));
	}
	// What they draw only for those a player is near enough to see it (DRAW_RANGE), the nearest first, DRAWS_AT_ONCE at most.
	let mut near_draws: Vec<(f32, u32, u16)> = if in_play {
		ours.iter().filter(|(m, _)| !mp.dying.contains_key(*m)).map(|(m, id)| {
			let at = [rd_f(ram, *m + 0x10), rd_f(ram, *m + 0x14), rd_f(ram, *m + 0x18)];
			(others.iter().map(|o| dist2(at, o.1)).fold(f32::MAX, f32::min), *m, *id)
		}).filter(|d| d.0 < DRAW_RANGE * DRAW_RANGE).collect()
	} else {
		Vec::new()
	};
	near_draws.sort_by(|a, b| a.0.total_cmp(&b.0));
	let watch: BTreeMap<u32, u16> = near_draws.iter().take(DRAWS_AT_ONCE).map(|d| (d.1, d.2)).collect();
	let table_end = rd(ram, table(me.level as u32).0 + 4) & MASK;
	let ds = mp.draws.take(c, me.level, mp.table, table_end, watch, mp.fields % SEND_EVERY == 0);
	if !ds.is_empty() {
		wire().draws = Some((level, mp.fields as u32, ds));
	}
}

/// The enemies other games run, shown here as they have them (the hands skip their updates): the copies shown, with their id and
/// where they are.
fn mirror(c: &mut Cpu, mp: &mut Mp, my: PlayerId, level: u8) -> Vec<(u32, u16, [f32; 3])> {
	let fields = mp.fields;
	let watched = &mp.watched;
	let stand_ins = &mp.stand_ins;
	let owners = &mp.owners;
	let dying = &mp.dying;
	let ids = &mp.ids;
	let mut driven = BTreeMap::new();
	// The copies shown from another game, with their id and where they are, for their arcs.
	let mut shown_at: Vec<(u32, u16, [f32; 3])> = Vec::new();
	for (slot, t) in mp.poses.iter_mut() {
		if let Some(s) = stand_ins.get(slot) {
			// A stand-in has no update to skip: it is placed from here, while it is still ours (making another can have taken it away).
			if let Some(h) = t.track.sample(DELAY).filter(|_| enemies::still_ours(&c.mem.ram, s.moby, s.class)) {
				let d = enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: t.health, scale: t.scale, look: Some((t.alpha, t.ovr)) };
				enemies::apply(&mut c.mem.ram, s.moby, &d);
				driven.insert(s.moby, d);
				shown_at.push((s.moby, *slot, h.pos));
			}
			continue;
		}
		let Some(&(class, _)) = watched.get(slot) else { continue };
		let Some(a) = ids.addr(*slot) else { continue };
		if dying.contains_key(&a) {
			continue;
		}
		let owner = owners.get(&WorldKey { level, slot: *slot, class }).copied().unwrap_or(0);
		if owner == my || fields.saturating_sub(t.heard) > POSE_STALE {
			continue;
		}
		if let Some(h) = t.track.sample(DELAY) {
			driven.insert(a, enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: t.health, scale: t.scale, look: Some((t.alpha, t.ovr)) });
			shown_at.push((a, *slot, h.pos));
		}
	}
	enemies::set_driven(driven);
	shown_at
}

/// What the copies shown from other games draw, the particles their enemies made near them, and their arcs.
fn show_theirs(c: &mut Cpu, mp: &mut Mp, me: &HeroState, shown_at: Vec<(u32, u16, [f32; 3])>) {
	let fields = mp.fields;
	// What they draw, and the particles they made near them.
	let shown: BTreeMap<u16, u32> = shown_at.iter().map(|&(m, s, _)| (s, m)).collect();
	mp.draws.show(c, fields, &shown);
	let ps = std::mem::take(&mut mp.particles_in);
	if !ps.is_empty() && me.mode == 0 {
		let ram = &c.mem.ram[..];
		let local = |id: u16| shown.get(&id).copied().filter(|&a| rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80);
		let local: BTreeMap<u16, u32> = ps.iter().filter_map(|p| Some((p.slot, local(p.slot)?))).collect();
		mp.particles.put(&mut c.mem.ram, me.level, fields, &ps, |id| local.get(&id).copied());
	}
	// Their arcs, set for this field (mp/arcs.rs): one no longer heard is left to the level's helper, which ends it.
	mp.arcs_in.retain(|_, (_, t)| fields.saturating_sub(*t) <= arcs::STALE);
	for (moby, slot, at) in shown_at {
		if let Some((a, _)) = mp.arcs_in.get(&slot) {
			arcs::set(&mut c.mem.ram, me.level, moby, at, a);
		}
	}
}
