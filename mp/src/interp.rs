//! Smooth remote heroes. States arrive a few times a second late, out of order or not at all; a `Track` keeps the recent ones
//! and plays them back `delay` ticks behind the newest, blending between the two around its playback clock. The clock runs at
//! one tick a local tick and is nudged towards (newest - delay), so it follows the sender's pace without jumping.
//!
//! Only `+ - * /` and comparisons: the same bits on every OS.

use crate::proto::HeroState;
use std::collections::VecDeque;

const KEEP: usize = 64;
/// A clock this far (ticks) from where it should be jumps there instead of drifting.
const SNAP: f64 = 30.0;
/// How much of the clock's error is taken back each tick.
const NUDGE: f64 = 0.05;
const PI: f32 = std::f32::consts::PI;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Default)]
pub struct Track {
	states: VecDeque<HeroState>,
	clock: Option<f64>,
}

impl Track {
	/// A state that arrived. An older or repeated tick than the newest held is dropped.
	pub fn push(&mut self, s: HeroState) {
		if self.states.back().is_some_and(|b| s.tick <= b.tick) {
			return;
		}
		self.states.push_back(s);
		if self.states.len() > KEEP {
			self.states.pop_front();
		}
	}

	/// The newest state that arrived.
	pub fn newest(&self) -> Option<&HeroState> {
		self.states.back()
	}

	/// Once per local tick: the hero to show now, `delay` ticks behind the newest state.
	pub fn sample(&mut self, delay: f64) -> Option<HeroState> {
		let newest = self.states.back()?.tick as f64;
		let target = newest - delay;
		let clock = match self.clock {
			Some(c) if (target - c).abs() <= SNAP => c + 1.0 + (target - c) * NUDGE,
			_ => target,
		};
		// Never past the newest state: with nothing newer the hero stands where he was last seen.
		let clock = clock.min(newest);
		self.clock = Some(clock);
		while self.states.len() > 2 && (self.states[1].tick as f64) <= clock {
			self.states.pop_front();
		}
		let a = self.states[0];
		let Some(&b) = self.states.get(1) else { return Some(a) };
		if clock <= a.tick as f64 {
			return Some(a);
		}
		let t = ((clock - a.tick as f64) / (b.tick - a.tick) as f64).min(1.0) as f32;
		Some(blend(&a, &b, t))
	}
}

/// `a` to `b` at `t` (0..=1). Across a level change there is nothing to blend: the newer one wins.
pub fn blend(a: &HeroState, b: &HeroState, t: f32) -> HeroState {
	if a.level != b.level {
		return *b;
	}
	let lerp = |x: f32, y: f32| x + (y - x) * t;
	let near = if t < 0.5 { a } else { b };
	HeroState {
		tick: near.tick,
		pos: [lerp(a.pos[0], b.pos[0]), lerp(a.pos[1], b.pos[1]), lerp(a.pos[2], b.pos[2])],
		facing: wrap(a.facing + wrap(b.facing - a.facing) * t),
		..*near
	}
}

/// An angle into -PI..=PI.
fn wrap(mut a: f32) -> f32 {
	// Finite input only (decode refuses the rest); the loops end within a few turns for any game angle.
	while a > PI {
		a -= TAU;
	}
	while a < -PI {
		a += TAU;
	}
	a
}

#[cfg(test)]
mod tests {
	use super::*;

	fn at(tick: u32, x: f32, facing: f32) -> HeroState {
		HeroState { tick, level: 1, pos: [x, 0.0, 0.0], facing, ..Default::default() }
	}

	#[test]
	fn blends_halfway() {
		let h = blend(&at(10, 0.0, 0.0), &at(12, 4.0, 1.0), 0.5);
		assert_eq!(h.pos[0], 2.0);
		assert_eq!(h.facing, 0.5);
	}

	#[test]
	fn turns_the_short_way_round() {
		let h = blend(&at(0, 0.0, 3.0), &at(1, 0.0, -3.0), 0.5);
		assert!(h.facing.abs() > 3.1, "{}", h.facing);
	}

	#[test]
	fn does_not_blend_across_levels() {
		let mut b = at(2, 100.0, 0.0);
		b.level = 2;
		assert_eq!(blend(&at(0, 0.0, 0.0), &b, 0.25), b);
	}

	#[test]
	fn plays_back_behind_the_newest_and_holds_at_the_end() {
		let mut t = Track::default();
		for tick in (0..=20).step_by(2) {
			t.push(at(tick, tick as f32, 0.0));
		}
		// First sample snaps to newest - delay.
		assert_eq!(t.sample(6.0).unwrap().pos[0], 14.0);
		// Then one tick a tick, between the states that arrived every second tick.
		assert_eq!(t.sample(6.0).unwrap().pos[0], 15.0);
		for _ in 0..20 {
			t.sample(6.0);
		}
		// Nothing newer came: he stands at the last state.
		assert_eq!(t.sample(6.0).unwrap().pos[0], 20.0);
	}

	#[test]
	fn late_and_repeated_states_are_dropped() {
		let mut t = Track::default();
		t.push(at(10, 1.0, 0.0));
		t.push(at(8, 9.0, 0.0));
		t.push(at(10, 9.0, 0.0));
		assert_eq!(t.newest().unwrap().pos[0], 1.0);
	}

	#[test]
	fn a_far_off_clock_jumps() {
		let mut t = Track::default();
		t.push(at(0, 0.0, 0.0));
		t.sample(0.0);
		t.push(at(1000, 50.0, 0.0));
		assert_eq!(t.sample(0.0).unwrap().pos[0], 50.0);
	}
}
