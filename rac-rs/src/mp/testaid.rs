//! Test aids for the pair tests (tools/mp_*.ps1), read from the environment once; without them nothing here runs. They put the
//! local hero at a place and log the placed objects of some classes, so two players at one machine can be set up and seen.
//! - `RAC_MP_TEST_GOTO=FIELD:x,y,z[;FIELD:x,y,z]`: at the first field in play from FIELD on, the hero is put at x,y,z.
//! - `RAC_MP_TEST_WATCH=CLASS[,CLASS]`: every `WATCH_EVERY` fields in play, each placed object of those classes (its id, place,
//!   state, and whether it runs here or shows another game's: its update pointer) and the hero's place go to the run log.
//! - `RAC_MP_TEST_DUMP=ID`: with the watch, the placed object with that id is written out whole (its 0x100 bytes, then 0x100 bytes
//!   of its pvars), in words, to compare two games' copies.

use super::{rd, rd8, rd_f, table, HeroState, Mp, HERO, MASK};
use ee_oracle::cpu::Cpu;
use std::sync::{Mutex, OnceLock};

const WATCH_EVERY: u64 = 30;

struct Aids {
	goto: Vec<(u64, [f32; 3])>,
	watch: Vec<u16>,
	dump: Option<u16>,
}

fn aids() -> &'static Aids {
	static A: OnceLock<Aids> = OnceLock::new();
	A.get_or_init(|| {
		let goto = std::env::var("RAC_MP_TEST_GOTO").unwrap_or_default();
		let mut goto: Vec<(u64, [f32; 3])> = goto
			.split(';')
			.filter_map(|e| {
				let (f, p) = e.trim().split_once(':')?;
				let p: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
				Some((f.trim().parse().ok()?, [*p.first()?, *p.get(1)?, *p.get(2)?]))
			})
			.collect();
		goto.sort_by_key(|g| g.0);
		let watch = std::env::var("RAC_MP_TEST_WATCH").unwrap_or_default().split(',').filter_map(|x| x.trim().parse().ok()).collect();
		let dump = std::env::var("RAC_MP_TEST_DUMP").ok().and_then(|v| u16::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok());
		Aids { goto, watch, dump }
	})
}

/// Whether `RAC_MP_TEST_WATCH` names this class: what happens to its objects is said in the run log.
pub(super) fn watched(class: u16) -> bool {
	aids().watch.contains(&class)
}

/// The next place to go to.
static NEXT: Mutex<usize> = Mutex::new(0);

/// One field, in play in a level whose table is read (`world_tick` ran).
pub(super) fn tick(c: &mut Cpu, mp: &Mp, me: &HeroState) {
	let a = aids();
	if (a.goto.is_empty() && a.watch.is_empty()) || me.level < 0 || me.mode != 0 || mp.table == 0 {
		return;
	}
	let mut next = NEXT.lock().unwrap_or_else(|e| e.into_inner());
	if let Some(&(at, p)) = a.goto.get(*next).filter(|g| mp.fields >= g.0) {
		*next += 1;
		let hero = rd(&c.mem.ram, super::HERO_MOBY) & MASK;
		for k in 0..3u32 {
			c.mem.wr32(HERO + 0x80 + k * 4, p[k as usize].to_bits());
			if hero != 0 {
				c.mem.wr32(hero + 0x10 + k * 4, p[k as usize].to_bits());
			}
		}
		eprintln!("mp test: f{} (asked for {at}) the hero is put at {p:?}", mp.fields);
	}
	if a.watch.is_empty() || mp.fields % WATCH_EVERY != 0 {
		return;
	}
	let ram = &c.mem.ram[..];
	let (cell, _) = table(me.level as u32);
	let slots = ((rd(ram, cell + 4) & MASK).saturating_sub(mp.table) / 0x100).min(4096);
	for s in 1..slots {
		let m = mp.table + s * 0x100;
		let class = (rd(ram, m + 0xa4) >> 16) as u16;
		if rd(ram, m + 0x24) == 0 || rd8(ram, m + 0x20) >= 0x80 || !a.watch.contains(&class) {
			continue;
		}
		let id = mp.ids.id(m).unwrap_or(0);
		// Shown: no update, or one that returns at once (mp/machines.rs `stub`).
		let update = rd(ram, m + 0x74);
		let runs = if update == 0 || rd(ram, update & MASK) == 0x03E0_0008 { "shown from another game" } else { "runs here" };
		let kind = (rd(ram, (rd(ram, m + 0x24) & MASK) + 0x44) >> 16) as i16;
		if a.dump == Some(id) {
			let words = |from: u32| (0..64u32).map(|k| format!("{:08x}", rd(ram, from + k * 4))).collect::<Vec<_>>().join(" ");
			eprintln!("mp test: f{} dump {id:#x} moby {}", mp.fields, words(m));
			eprintln!("mp test: f{} dump {id:#x} pvars {}", mp.fields, words(rd(ram, m + 0x78) & MASK));
		}
		eprintln!("mp test: f{} id {id:#x} class {class} at ({:.2}, {:.2}, {:.2}) state {:#x} {runs} (type {kind}, flags {:#x})", mp.fields, rd_f(ram, m + 0x10), rd_f(ram, m + 0x14), rd_f(ram, m + 0x18), rd8(ram, m + 0x20), rd(ram, m + 0x34));
	}
	eprintln!("mp test: f{} hero at ({:.2}, {:.2}, {:.2}) state {:#x}", mp.fields, me.pos[0], me.pos[1], me.pos[2], me.state);
}
