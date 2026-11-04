//! Cutscenes, shared. A game made for two plays a story scene for both players: when a player's game starts one in play, the others
//! in that level start the same scene. Each level's code has its own "start scene N" (USA Veldin 0x298840): it keeps N at +0x30 of
//! the level's scene block and sets the game mode to 2. It is found in the loaded level by its code (`find`): the mode word written
//! with 2 right after `a0 = 100, a1 = 2`, back to the function's entry, and the scene block from the `lui`/`addiu` of `s0` before
//! N is kept. A game whose mode goes from play to 2 reads N and sends it; the others start the same scene in play, inside their own
//! frame: the function waits for frames as it plays, and called between two fields it stopped halfway. A hand at the hero's
//! `update_motion_input` (early in his update each tick) turns that one call into "start scene N", which returns to the hero's update
//! as the game's own scene starts do. A scene started for another game is not sent back, and one this game already played in
//! this visit to the level is not played again (two players arriving together each see the arrival scene once).

use super::{disc, rd, Mp, MODE_WORD};
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use std::sync::{Mutex, OnceLock};

const HOOK: &str = "rac1.player.update_motion_input";
static SPEC: OnceLock<&'static Reloc> = OnceLock::new();
/// The scene to start at the next hero update: the function's entry and the scene.
static PENDING: Mutex<Option<(u32, i32)>> = Mutex::new(None);

fn spec() -> &'static Reloc {
	SPEC.get().expect("scene hand without a spec")
}

/// At the hero's motion input: a scene asked for starts here, in the game's own frame, instead of this one call.
fn motion_hand(c: &mut Cpu) -> bool {
	let Some((entry, id)) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else { return false };
	c.s(4, id as i64 as u64);
	c.pc = entry;
	c.npc = entry.wrapping_add(4);
	c.mem.now = c.icount;
	true
}

/// Registers the hand. `Retail::load`, before the recompiled table is built, once.
pub fn install() {
	if let Some((_, r)) = crate::omni::hook_specs(|id| id == HOOK).into_iter().next() {
		let _ = SPEC.set(r);
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: HOOK, reloc: spec, est: ee_oracle::hs::EXACT, f: motion_hand })));
		eprintln!("mp: a hand for shared cutscenes");
	}
}

/// The level code lies in here (USA Veldin's reaches 0x2f0cc8; PAL hook entries lie up to 0x31xxxx).
const CODE: std::ops::Range<u32> = 0x1f_0000..0x38_0000;
/// How far back from the mode write the function's entry may be.
const ENTRY_WITHIN: u32 = 0x400;
/// After starting another game's scene, this game's own entering a scene is that one, not news (fields).
const OURS_FOR: u64 = 60;
/// A level whose code showed no such function is looked at again after this many fields.
const LOOK_AGAIN: u64 = 600;
/// The game mode in play, and in an in-engine cutscene.
const PLAY: u32 = 0;
const SCENE: u32 = 2;

#[derive(Default)]
pub(super) struct Scenes {
	level: i8,
	/// The function's entry and the scene block, once found in this level's code; when it was last looked for.
	found: Option<(u32, u32)>,
	looked: Option<u64>,
	mode: u32,
	/// Until when this game's entering a scene is one another game asked for.
	theirs_until: u64,
	/// Scenes other games started (level, scene), and those played here in this visit.
	queue: Vec<(u8, i32)>,
	played: std::collections::BTreeSet<i32>,
}

/// Another game started a scene.
pub(super) fn heard(mp: &mut Mp, level: u8, id: i32) {
	mp.scenes.queue.push((level, id));
}

fn word(ram: &[u8], a: u32) -> u32 {
	rd(ram, a)
}

/// The level's "start scene" and its scene block, from its code.
fn find(ram: &[u8], mode_word: u32) -> Option<(u32, u32)> {
	let sext = |x: u32| (x & 0xffff) as i16 as i32;
	let mut a = CODE.start;
	while a < CODE.end {
		// addiu v0, zero, 2; addiu a0, zero, 100; addiu a1, zero, 2
		if word(ram, a) == 0x2402_0002 && word(ram, a + 4) == 0x2404_0064 && word(ram, a + 8) == 0x2405_0002 {
			// lui at, hi; sw v0, lo(at) within a few words, to the mode word.
			let store = (a + 12..a + 36).step_by(4).find(|&p| {
				let (lui, sw) = (word(ram, p - 4), word(ram, p));
				lui >> 16 == 0x3c01 && sw >> 16 == 0xac22 && ((lui & 0xffff) << 16).wrapping_add(sext(sw) as u32) == mode_word
			});
			if let Some(store) = store {
				// The entry: addiu sp, sp, -n.
				let entry = (store.saturating_sub(ENTRY_WITHIN)..store).step_by(4).rev().find(|&p| word(ram, p) >> 16 == 0x27bd && word(ram, p) & 0x8000 != 0)?;
				// sw s2, 0x30(s0), with s0 from lui s0 / addiu s0, s0 before it.
				let keep = (entry..store).step_by(4).find(|&p| word(ram, p) == 0xae12_0030)?;
				let hi = (entry..keep).step_by(4).rev().map(|p| word(ram, p)).find(|w| w >> 16 == 0x3c10)?;
				let lo = (entry..keep).step_by(4).rev().map(|p| word(ram, p)).find(|w| w >> 16 == 0x2610)?;
				return Some((entry, ((hi & 0xffff) << 16).wrapping_add(sext(lo) as u32)));
			}
		}
		a += 4;
	}
	None
}

/// One field: a scene this game starts goes to the others; one they started is started here.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, level: i8, in_play: bool) {
	let fields = mp.fields;
	let s = &mut mp.scenes;
	if level != s.level {
		*s = Scenes { level, queue: std::mem::take(&mut s.queue), ..Default::default() };
	}
	let mode_word = disc::va(MODE_WORD);
	let mode = rd(&c.mem.ram, mode_word);
	if level < 0 {
		s.queue.clear();
		return;
	}
	if s.found.is_none() && mode == PLAY && s.looked.is_none_or(|t| fields - t >= LOOK_AGAIN) {
		s.looked = Some(fields);
		s.found = find(&c.mem.ram, mode_word);
		match s.found {
			Some((entry, block)) => eprintln!("mp: level {level} starts its scenes at {entry:#x} (scene block {block:#x})"),
			None => eprintln!("mp: level {level}: no scene start found in its code"),
		}
	}
	// Ours: play to scene, not one another game asked for.
	if s.mode == PLAY && mode == SCENE && fields > s.theirs_until {
		if let Some((_, block)) = s.found {
			let id = rd(&c.mem.ram, block + 0x30) as i32;
			s.played.insert(id);
			eprintln!("mp: a scene starts here (scene {id}), for the others too");
			super::wire().scenes.push((level as u8, id));
		}
	}
	s.mode = mode;
	let played = &s.played;
	s.queue.retain(|q| q.0 as i8 == level && !played.contains(&q.1));
	let Some(&(_, id)) = s.queue.first() else { return };
	let Some((entry, _)) = s.found else { return };
	if !in_play || mode != PLAY || SPEC.get().is_none() {
		return;
	}
	s.queue.remove(0);
	s.played.insert(id);
	s.theirs_until = fields + OURS_FOR;
	*PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((entry, id));
	eprintln!("mp: another player's scene {id} plays here too");
}
