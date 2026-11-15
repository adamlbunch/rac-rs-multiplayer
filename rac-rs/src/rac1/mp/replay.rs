//! Other games' sounds and effects made here (`fx_tick`), and the hidden mobys (voices) the other players' own sounds come from.

use super::*;

/// Other games' sounds and effects that are due, made on this game's copies, in play.
pub(super) fn fx_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let now = mp.fields;
	// Cues all; explosions a few a field, the rest the next.
	let mut blasts = 0;
	let mut due = Vec::new();
	mp.fx_in.retain(|(f, id, e)| {
		let blast = matches!(e, FxEvent::Explode { .. } | FxEvent::Spark { .. } | FxEvent::Glow { .. } | FxEvent::Burst { .. } | FxEvent::Flash { .. });
		if *f > now || (blast && blasts == BLOWS_PER_FIELD) {
			return true;
		}
		blasts += blast as usize;
		due.push((*id, *e));
		false
	});
	if dying(me) {
		for ((id, class), m) in std::mem::take(&mut mp.voices) {
			if enemies::still_ours(&c.mem.ram, m, class) {
				note(format!("voice of player {id} class {class} at {m:#x} removed: the hero dies"));
				let _ = crate::fork_pack::delete_moby(c, m);
			}
		}
	}
	if mp.held {
		return;
	}
	voices_tick(c, mp, me);
	if due.is_empty() || me.mode != 0 || mp.table == 0 {
		return;
	}
	let mut made = 0;
	for (id, e) in due {
		// This game's copy of what a slot or id names, alive.
		let copy = |mp: &Mp, ram: &[u8], slot: u16| {
			match slot {
				NO_SLOT => None,
				s if s >= SPAWNED_ID => spawned::local_of(mp, s),
				s => mp.ids.addr(s),
			}
			.filter(|&a| rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80)
		};
		let voice = match e {
			FxEvent::PlayerCue { class, .. } => voice_of(c, mp, id, class, &mut made),
			// Made for something this game has no copy of: a hidden one of its class stands in.
			FxEvent::Explode { slot, owner, .. } | FxEvent::Glow { slot, owner, .. } | FxEvent::Burst { slot, owner, .. } if owner != NO_SLOT && copy(mp, &c.mem.ram, slot).is_none() => voice_of(c, mp, id, owner, &mut made),
			_ => None,
		};
		let ram = &c.mem.ram[..];
		let local = |slot: u16| copy(mp, ram, slot);
		let raw = match e {
			FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Glow { slot, .. } | FxEvent::Burst { slot, .. } if slot != NO_SLOT && slot < SPAWNED_ID => mp.ids.addr(slot),
			_ => None,
		};
		if raw.is_some_and(|a| mp.dying.contains_key(&a)) {
			continue;
		}
		let named: BTreeMap<u16, u32> = match e {
			FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Glow { slot, .. } | FxEvent::Burst { slot, .. } => local(slot).map(|a| (slot, a)).into_iter().collect(),
			FxEvent::PlayerCue { .. } | FxEvent::Spark { .. } | FxEvent::Flash { .. } => BTreeMap::new(),
		};
		note(format!("player {id}'s {e:?} replayed (voice {voice:x?}, named {named:x?})"));
		if !fx::play(c, &e, |s| named.get(&s).copied(), voice) {
			mp.fx_in.push((now + 1, id, e));
		}
	}
}

/// The hidden moby player `id`'s sounds of class `class` come from here, made the first time (two a field at most).
fn voice_of(c: &mut Cpu, mp: &mut Mp, id: PlayerId, class: u16, made: &mut usize) -> Option<u32> {
	// Checked at each use: the call before this one can have taken it away (an explosion clears out older effects of its class).
	if let Some(&v) = mp.voices.get(&(id, class)) {
		if enemies::still_ours(&c.mem.ram, v, class) {
			return Some(v);
		}
		mp.voices.remove(&(id, class));
	}
	if *made == 2 || mp.voices.keys().filter(|(p, _)| *p == id).count() >= VOICES_EACH {
		return None;
	}
	*made += 1;
	match crate::fork_pack::create_moby(c, class as u32) {
		Ok(m) if m != 0 => {
			let m = m & MASK;
			enemies::no_update(&mut c.mem.ram, m);
			enemies::hide(&mut c.mem.ram, m);
			eprintln!("mp: player {id}'s class {class} sounds come from a voice here");
			note(format!("voice of player {id} class {class} made at {m:#x}"));
			mp.voices.insert((id, class), m);
			Some(m)
		}
		_ => None,
	}
}

/// The voices stay at their players' puppets, and go with them.
fn voices_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let at: BTreeMap<PlayerId, [f32; 3]> = mp.shown.iter().filter(|(_, h)| h.level == me.level).map(|(id, h)| (*id, h.pos)).collect();
	let gone: Vec<(PlayerId, u16)> = mp.voices.keys().copied().filter(|(id, _)| !at.contains_key(id)).collect();
	// A voice the game took away (a respawn) is made again when it is next needed.
	mp.voices.retain(|&(_, class), m| enemies::still_ours(&c.mem.ram, *m, class));
	for k in gone {
		if let Some(m) = mp.voices.remove(&k) {
			note(format!("voice of player {} class {} at {m:#x} removed: its player went", k.0, k.1));
			if let Err(e) = crate::fork_pack::delete_moby(c, m) {
				eprintln!("mp: could not remove a voice: {e}");
			}
		}
	}
	for (&(id, _), &m) in &mp.voices {
		let p = at[&id];
		for k in 0..3u32 {
			c.mem.wr32(m + 0x10 + k * 4, (p[k as usize] + if k == 2 { VOICE_LIFT } else { 0.0 }).to_bits());
		}
	}
}
