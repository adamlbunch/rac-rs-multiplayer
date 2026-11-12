//! Lightning arcs of the enemies another game runs: Veldin's robots (class 1440; the same code on Oltanis and Quartu). A robot's
//! own update claims one of the level's three arc slots and sets its origin matrix and end point every tick (Veldin, USA disc:
//! `arc_claim` 0x2e1f28, `arc_set` 0x2e2190); a placed helper (class 1471) turns each live slot into strands, two lights and glints,
//! and kills a slot whose owner did not set it since its last tick. A robot that another game runs has its update skipped here
//! (mp/enemies.rs), so its arc never showed in the other games (Raj saw no electricity on Veldin's first enemies, v0.4.1.4).
//!
//! The game that runs a robot sends each live slot it holds with the poses (`Msg::Arcs`): the robot's id, the claim's two words,
//! the origin matrix and the end point, both relative to the robot. Every other game claims a slot for its copy and sets it each
//! field between two fields, with the writes `arc_claim` and `arc_set` make (they call nothing but a copy); the helper then draws
//! it as its own. When the arc stops coming, the slot is not set again and the helper kills it, its lights freed.

use super::{rd, MASK};
use rac_mp::ArcPose;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Arcs sent to the others and set here for their enemies, for the run log.
pub(super) static SENT: AtomicU64 = AtomicU64::new(0);
pub(super) static SET: AtomicU64 = AtomicU64::new(0);

/// The level's arc slots.
const SLOTS: u32 = 3;
/// An arc not heard again for this many fields is no longer set here (the helper then kills its slot).
pub(super) const STALE: u64 = 8;

/// The arc tables of the level loaded now: the slot words from `state` (state, phases, lights, claim words, UV, owner, 0x10 apart),
/// the origin matrices (0x40 a slot) and the end points (0x10 a slot).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tables {
	state: u32,
	matrices: u32,
	ends: u32,
}

/// The tables, read from the level's arc draw callback once a level (None: this level has no arcs).
static TABLES: Mutex<Option<(u32, Option<Tables>)>> = Mutex::new(None);

fn word(ram: &[u8], a: u32) -> u32 {
	rd(ram, a)
}

fn put(ram: &mut [u8], a: u32, v: u32) {
	let i = (a & MASK) as usize;
	if let Some(b) = ram.get_mut(i..i + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

fn tables(ram: &[u8], level: i8) -> Option<Tables> {
	if level < 0 {
		return None;
	}
	let level = level as u32;
	let f = crate::retail::queue_b::arc_fn(level)?;
	let mut g = TABLES.lock().unwrap_or_else(|e| e.into_inner());
	// The code has to be in RAM, at its first words (queue_b.rs `arc_live`), before its tables are read; read again on another level.
	if let Some((l, t)) = *g {
		if l == level && t.is_some() {
			return t;
		}
	}
	if word(ram, f) != 0x27bd_ff40 || word(ram, f + 4) != 0x3c05_0005 {
		return None;
	}
	let t = crate::retail::queue_b::arc_tables_in(|a| word(ram, a), f).map(|t| Tables { state: t.state, matrices: t.poly - 0x120, ends: t.poly - 0x60 });
	if t.is_some() {
		eprintln!("mp: level {level}'s arc tables at {:#x?}", t);
	}
	*g = Some((level, t));
	t
}

/// The arcs held by what this game runs (`ours`: moby address -> its id), relative to each, for the others.
pub(super) fn read(ram: &[u8], level: i8, ours: &BTreeMap<u32, u16>) -> Vec<ArcPose> {
	let Some(t) = tables(ram, level) else { return Vec::new() };
	let mut out = Vec::new();
	for i in 0..SLOTS {
		let owner = word(ram, t.state + 0x90 + 4 * i) & MASK;
		let state = word(ram, t.state + 4 * i);
		let Some(&slot) = ours.get(&owner).filter(|_| owner != 0 && (2..=4).contains(&state)) else { continue };
		let f = |a: u32| f32::from_bits(word(ram, a));
		let at = [f(owner + 0x10), f(owner + 0x14), f(owner + 0x18)];
		let m = t.matrices + 0x40 * i;
		let mut rows = [0.0f32; 16];
		for (k, r) in rows.iter_mut().enumerate() {
			*r = f(m + 4 * k as u32);
		}
		for k in 0..3 {
			rows[12 + k] -= at[k];
		}
		let e = t.ends + 0x10 * i;
		let end = [f(e) - at[0], f(e + 4) - at[1], f(e + 8) - at[2], f(e + 12)];
		if rows.iter().chain(&end).all(|v| v.is_finite()) {
			SENT.fetch_add(1, Ordering::Relaxed);
			out.push(ArcPose { slot, p1: word(ram, t.state + 0x60 + 4 * i), p2: word(ram, t.state + 0x70 + 4 * i), rows, end });
		}
	}
	out
}

/// Sets the arc of the copy at `moby`, placed at `at`, for this field: its slot claimed first if it has none (`arc_claim`'s writes),
/// then the state moved on and the matrix and end point written (`arc_set`'s). Nothing when every slot is taken.
pub(super) fn set(ram: &mut [u8], level: i8, moby: u32, at: [f32; 3], a: &ArcPose) {
	let Some(t) = tables(ram, level) else { return };
	let moby = moby & MASK;
	let owner = |ram: &[u8], i: u32| word(ram, t.state + 0x90 + 4 * i) & MASK;
	let i = match (0..SLOTS).find(|&i| owner(ram, i) == moby) {
		Some(i) => i,
		None => {
			let Some(i) = (0..SLOTS).find(|&i| owner(ram, i) == 0) else { return };
			let s = t.state + 4 * i;
			put(ram, s + 0x90, moby);
			put(ram, s + 0x50, u32::MAX);
			put(ram, s + 0x60, a.p1);
			put(ram, s + 0x70, a.p2);
			put(ram, s, 1);
			put(ram, s + 0x10, 0);
			put(ram, s + 0x20, 0);
			put(ram, s + 0x30, 0);
			put(ram, s + 0x40, u32::MAX);
			put(ram, s + 0x80, 0);
			i
		}
	};
	let s = t.state + 4 * i;
	match word(ram, s) {
		1 => put(ram, s, 2),
		4 => put(ram, s, 3),
		_ => {}
	}
	let m = t.matrices + 0x40 * i;
	for (k, v) in a.rows.iter().enumerate() {
		let v = if (12..15).contains(&k) { v + at[k - 12] } else { *v };
		put(ram, m + 4 * k as u32, v.to_bits());
	}
	SET.fetch_add(1, Ordering::Relaxed);
	let e = t.ends + 0x10 * i;
	for k in 0..4u32 {
		let v = if k < 3 { a.end[k as usize] + at[k as usize] } else { a.end[3] };
		put(ram, e + 4 * k, v.to_bits());
	}
}

/// A new level: its tables are read again.
pub(super) fn forget() {
	*TABLES.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
