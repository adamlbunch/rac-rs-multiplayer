//! A player who comes into the host's level is put beside the host: Raj, back in after his game stopped, loaded his save and stood
//! at Veldin's start, far from his partner (v0.4.1.4). A client that is newly in a level (a trip, a load) or newly in the session
//! looks for the host in its level for `LOOK` fields of play; once the host and our hero both stand on the ground (no height change
//! for `STEADY` fields), and they are not near already, our hero is put on the host's spot, a little above it, and lands there. Only
//! a client moves, to the host: two games that each went to the other would swap places. Not in a scene, a menu or a respawn.

use super::{dist2, notice, rd, HeroState, Mp, DEATH_STATES, HERO, HERO_MOBY, MASK};
use ee_oracle::cpu::Cpu;

/// Fields of play in which a client looks for the host after it came.
const LOOK: u64 = 900;
/// Fields both heroes keep their height before ours is moved.
const STEADY: u32 = 20;
/// Already this near the host (metres): nothing moves.
const NEAR: f32 = 12.0;
/// Our hero is put this far above the host's feet, and falls onto the ground there.
const LIFT: f32 = 0.4;
/// A height change smaller than this in a field (metres) is standing on the ground.
const STILL: f32 = 0.02;

#[derive(Default)]
pub(super) struct Arrive {
	/// The level we last came to, and whether a look is due at the next field of play there.
	level: i8,
	due: bool,
	/// The field the look ends (0: not looking).
	until: u64,
	steady: u32,
	/// Our height and the host's at the last field.
	last: Option<(f32, f32)>,
}

/// Back in the session (joined, or rejoined): a look is due.
pub(super) fn connected(mp: &mut Mp) {
	mp.arrive.due = true;
}

/// One field, after the world's (the table read).
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if me.level != mp.arrive.level {
		mp.arrive.level = me.level;
		mp.arrive.due = me.level >= 0;
		mp.arrive.until = 0;
	}
	if mp.my_id.is_none_or(|id| id == 0) || me.level < 0 || me.mode != 0 || mp.held || mp.table == 0 {
		return;
	}
	if std::mem::take(&mut mp.arrive.due) {
		mp.arrive.until = mp.fields + LOOK;
		mp.arrive.steady = 0;
		mp.arrive.last = None;
	}
	if mp.fields >= mp.arrive.until {
		return;
	}
	let Some(host) = mp.shown.iter().find(|(id, h)| *id == 0 && h.level == me.level && h.mode == 0 && h.health > 0 && !DEATH_STATES.contains(&h.state)).map(|(_, h)| *h) else {
		mp.arrive.steady = 0;
		mp.arrive.last = None;
		return;
	};
	if dist2(me.pos, host.pos) < NEAR * NEAR {
		mp.arrive.until = 0;
		return;
	}
	let now = (me.pos[2], host.pos[2]);
	mp.arrive.steady = match mp.arrive.last {
		Some((a, b)) if (now.0 - a).abs() < STILL && (now.1 - b).abs() < STILL => mp.arrive.steady + 1,
		_ => 0,
	};
	mp.arrive.last = Some(now);
	if mp.arrive.steady < STEADY {
		return;
	}
	let p = [host.pos[0], host.pos[1], host.pos[2] + LIFT];
	let hero = rd(&c.mem.ram, HERO_MOBY) & MASK;
	for k in 0..3u32 {
		c.mem.wr32(HERO + 0x80 + k * 4, p[k as usize].to_bits());
		if hero != 0 {
			c.mem.wr32(hero + 0x10 + k * 4, p[k as usize].to_bits());
		}
	}
	mp.arrive.until = 0;
	eprintln!("mp: put beside the host at ({:.1}, {:.1}, {:.1}), from ({:.1}, {:.1}, {:.1})", p[0], p[1], p[2], me.pos[0], me.pos[1], me.pos[2]);
	super::note(format!("put beside the host at {p:.1?}"));
	notice::say(format!("You're with {} now.", notice::name_of(0)), 5);
}
