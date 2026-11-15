//! The shared world (`world_tick`): what the level placed that matters, watched in the moby table; what went from this game is
//! reported, and what the host says is gone is finished or removed here.

use super::*;

/// One field of the shared world, in the level the local hero is in: walks the placed slots of the moby table, reports what went
/// from this game (near the hero, in play, alive, not a whole level clearing) and removes what the host says is gone.
pub(super) fn world_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if me.level < 0 {
		mp.watched.clear();
		mp.watched_level = -1;
		mp.table = 0;
		return;
	}
	if mp.watched_level != me.level {
		new_level(mp, me.level);
	}
	// A death clears and refills the table through the respawn: what goes then is nobody's doing, and the table settles again after.
	mp.settle = if dying(me) { WORLD_SETTLE } else { mp.settle.saturating_sub(1) };
	let Some((gone, delete)) = walk(c, mp, me) else { return };
	report(mp, me, gone);
	// Not while the local hero dies or respawns: the respawn puts the level's objects back, and what comes back is finished after.
	if me.mode == 0 && !mp.held {
		end_theirs(c, mp, me, delete);
	}
}

/// Another level: what was watched, shown or heard in the last one is forgotten.
fn new_level(mp: &mut Mp, level: i8) {
	mp.watched.clear();
	mp.dying.clear();
	// The game cleared the table, voices too; the arc tables are the new level's.
	mp.voices.clear();
	mp.arcs_in.clear();
	arcs::forget();
	fx::forget();
	mp.particles.forget();
	mp.particles_in.clear();
	mp.draws.forget();
	touch::forget();
	mp.watched_level = level;
	mp.settle = WORLD_SETTLE;
	mp.gone_here.clear();
	mp.finishing.clear();
	mp.poses.clear();
}

/// What went from this game (key, class type, where), and what the gone list has that is alive here (moby, id), from a walk of the
/// placed slots of the moby table; None when the table does not read as one.
#[allow(clippy::type_complexity)]
fn walk(c: &Cpu, mp: &mut Mp, me: &HeroState) -> Option<(Vec<(WorldKey, i16, [f32; 3])>, Vec<(u32, u16)>)> {
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let (cell, _) = table(level as u32);
	// The table: the placed objects from `start`, the spawned ones from `spawned` (omni/mobys.rs reads the same cells).
	let (start, spawned) = (rd(ram, cell) & MASK, rd(ram, cell + 4) & MASK);
	let slots = spawned.wrapping_sub(start) / 0x100;
	if start == 0 || spawned <= start || (spawned - start) % 0x100 != 0 || slots > WORLD_SLOTS {
		mp.table = 0;
		return None;
	}
	mp.table = start;
	mp.ids.update(ram, start, slots);
	// What a rebuilt table (a respawn) no longer has went with it, not by a player's doing.
	mp.watched.retain(|s, _| mp.ids.addr(*s).is_some());
	let (mut gone, mut delete) = (Vec::new(), Vec::new());
	// Slot 0 is the hero.
	for slot in 1..slots {
		let a = start + slot * 0x100;
		let Some(s) = mp.ids.id(a) else { continue };
		let class = (rd(ram, a + 0xa4) >> 16) as u16;
		if rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 {
			let key = WorldKey { level, slot: s, class };
			// Not while the table settles (a respawn of the hero refills it): what comes back then is finished again, as before.
			if mp.settle == 0 && mp.removed.contains(&key) && mp.gone_here.remove(&s) {
				// Ended here, and alive again: the level sent it again (Novalis' class 660 and 459 come back elsewhere). It lives, in
				// every game, and its next end is reported anew.
				mp.removed.remove(&key);
				wire().back.push(key);
				eprintln!("mp: back here: level {level} id {s:#x} class {class}, sent again by the level");
			} else if mp.removed.contains(&key) {
				delete.push((a, s));
				continue;
			}
			if !mp.watched.contains_key(&s) {
				let kind = (rd(ram, (rd(ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
				if WORLD_TYPES.contains(&kind) || WORLD_CLASSES.contains(&class) {
					mp.watched.insert(s, (class, kind));
				}
			}
		} else if let Some((class, kind)) = mp.watched.remove(&s) {
			let (x, y, z) = (rd_f(ram, a + 0x10) - me.pos[0], rd_f(ram, a + 0x14) - me.pos[1], rd_f(ram, a + 0x18) - me.pos[2]);
			if x * x + y * y + z * z < WORLD_NEAR * WORLD_NEAR {
				gone.push((WorldKey { level, slot: s, class }, kind, [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)]));
			}
		}
	}
	Some((gone, delete))
}

/// What went from this game, reported: only in play, alive and settled.
fn report(mp: &mut Mp, me: &HeroState, gone: Vec<(WorldKey, i16, [f32; 3])>) {
	// In play, alive and settled: anything else that takes objects away (a scene, a death, a level starting) is not a player's doing.
	if mp.settle == 0 && me.mode == 0 && me.health > 0 && gone.len() <= WORLD_BURST {
		for (k, kind, at) in gone {
			if mp.removed.insert(k) {
				eprintln!("mp: gone here: level {} id {:#x} class {} type {kind} at {at:.1?}", k.level, k.slot, k.class);
				wire().gone.push(k);
				mp.gone_here.insert(k.slot);
			}
		}
	}
}

/// What the gone list has that is alive here (moby, id), ended: an enemy or a crate its own way (`finish`), anything else taken away.
fn end_theirs(c: &mut Cpu, mp: &mut Mp, me: &HeroState, delete: Vec<(u32, u16)>) {
	let level = me.level as u8;
	let hero = enemies::hero_moby(&c.mem.ram);
	let listed: BTreeSet<u32> = delete.iter().map(|d| d.0).collect();
	// What this game was finishing and is alive no more ended here: alive again later, the level sent it again.
	let alive: BTreeSet<u16> = delete.iter().map(|d| d.1).collect();
	let ended: Vec<u16> = mp.finishing.iter().filter(|s| !alive.contains(s)).copied().collect();
	for s in ended {
		mp.finishing.remove(&s);
		if mp.settle == 0 {
			mp.gone_here.insert(s);
		}
	}
	mp.dying.retain(|a, _| listed.contains(a));
	let mut blows = 0;
	for (a, s) in delete {
		mp.watched.remove(&s);
		// An enemy or a crate goes here its own way: its death or break, its sounds, what it drops and what counts on it.
		let kind = (rd(&c.mem.ram, (rd(&c.mem.ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
		let finishing = kind == ENEMY || kind == CRATE;
		if finishing {
			finish(c, mp, me, (a, s, kind), hero, &mut blows);
			continue;
		}
		mp.dying.remove(&a);
		note(format!("id {s:#x} at {a:#x} removed for another player"));
		match crate::fork_pack::delete_moby(c, a) {
			Ok(()) => {
				eprintln!("mp: removed for another player: level {level} id {s:#x}");
				if mp.settle == 0 {
					mp.gone_here.insert(s);
				}
			}
			Err(e) => eprintln!("mp: could not remove slot {s}: {e}"),
		}
	}
}

/// An enemy or a crate at `a` (id `s`, class type `kind`) another game finished, finished here by a blow from the local hero (`hero`),
/// near him and `BLOWS_PER_FIELD` a field (`blows`, this field's so far), woken first if it never ran here; far from him, left as it is.
fn finish(c: &mut Cpu, mp: &mut Mp, me: &HeroState, (a, s, kind): (u32, u16, i16), hero: u32, blows: &mut usize) {
	let level = me.level as u8;
	let at = [rd_f(&c.mem.ram, a + 0x10), rd_f(&c.mem.ram, a + 0x14), rd_f(&c.mem.ram, a + 0x18)];
	let near = dist2(at, me.pos) < FINISH_NEAR * FINISH_NEAR;
	// Still in its first state here: hit so, the game takes paths it never set up (Novalis' birdbots, class 459, then divided
	// by a path count of 0 and stopped). It is woken first, its own update run, and the blow lands once it is awake.
	let asleep = kind == ENEMY && rd8(&c.mem.ram, a + 0x20) == 0;
	match mp.dying.get(&a).copied() {
		None if near && *blows < BLOWS_PER_FIELD => {
			*blows += 1;
			mp.dying.insert(a, mp.fields);
			mp.poses.remove(&s);
			mp.fx_in.retain(|(_, _, e)| !matches!(e, FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Burst { slot, .. } if *slot == s));
			enemies::release(a);
			if asleep {
				eprintln!("mp: waking id {s:#x} here, to finish it for another player");
			} else {
				mp.finishing.insert(s);
				match enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK) {
					Ok(()) => eprintln!("mp: finishing for another player: level {level} id {s:#x} at {at:.1?}"),
					Err(e) => eprintln!("mp: could not finish id {s:#x}: {e}"),
				}
			}
		}
		// Woken: the blow now. Not woken in time: taken away.
		Some(t) if near && !mp.finishing.contains(&s) => {
			if !asleep {
				mp.finishing.insert(s);
				match enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK) {
					Ok(()) => eprintln!("mp: finishing for another player (woken here): level {level} id {s:#x} at {at:.1?}"),
					Err(e) => eprintln!("mp: could not finish id {s:#x}: {e}"),
				}
			} else if mp.fields - t >= WAKE_WITHIN {
				mp.dying.remove(&a);
				match crate::fork_pack::delete_moby(c, a) {
					Ok(()) => {
						eprintln!("mp: removed for another player (it did not wake here): level {level} id {s:#x}");
						mp.gone_here.insert(s);
					}
					Err(e) => eprintln!("mp: could not remove slot {s}: {e}"),
				}
			}
		}
		// Far from this game's hero: left as it is until he comes, not hit, hidden or taken away from there. What carries it (one
		// of Kerwan's trains its cargo crates) broke on each of those; near, the blow breaks it as a player's would.
		None if !near => {
			mp.poses.remove(&s);
			enemies::release(a);
		}
		// Near and still standing: the blow again, now and then, till it reads it.
		Some(t) if near && (mp.fields - t) % FINISH_WITHIN == 0 && mp.fields != t => {
			let _ = enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK);
		}
		_ => {}
	}
}
