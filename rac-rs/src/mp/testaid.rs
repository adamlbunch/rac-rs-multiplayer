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
	census: Option<u64>,
}

/// `RAC_MP_TEST_CENSUS=FIELDS`: that many fields into play in a level, its classes go to the run log once: every class the level
/// has loaded with its type (the class tables `create_moby` reads), and every class of an object in the table with its update.
fn census(c: &Cpu, mp: &Mp, me: &HeroState) {
	static DONE: Mutex<i32> = Mutex::new(-1);
	let Some(after) = aids().census else { return };
	let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
	if *done == me.level as i32 || mp.fields < after {
		return;
	}
	*done = me.level as i32;
	let ram = &c.mem.ram[..];
	// create_moby's first call sets up the new moby: its first two built addresses are the class's index bytes and the headers.
	let Some(create) = crate::omni::game_entry(c, 0x24_E448) else {
		eprintln!("mp census: no create_moby in level {}", me.level);
		return;
	};
	let Some(init) = (0..64).map(|k| rd(ram, create + 4 * k)).find(|w| w >> 26 == 3).map(|w| (create & 0xf000_0000) | (w & 0x03ff_ffff) << 2) else { return };
	let mut hi = [0u32; 32];
	let mut built = Vec::new();
	for k in 0..40 {
		let w = rd(ram, init + 4 * k);
		let (op, rs, rt, imm) = (w >> 26, (w >> 21 & 31) as usize, (w >> 16 & 31) as usize, w & 0xffff);
		if op == 0x0f {
			hi[rt] = imm << 16;
		} else if op == 0x09 && rs == rt && hi[rs] != 0 {
			built.push(hi[rs].wrapping_add(imm as u16 as i16 as i32 as u32));
			hi[rs] = 0;
		}
	}
	let Some(&index) = built.first() else {
		eprintln!("mp census: create_moby {create:#x} sets up at {init:#x}, which builds {built:x?}");
		return;
	};
	// The objects in the table: their class, update, joint records and class header.
	let mut present = std::collections::BTreeMap::new();
	let slots = ((rd(ram, table(me.level as u32).0 + 4) & MASK).saturating_sub(mp.table) / 0x100).min(4096);
	for s in 0..slots {
		let m = mp.table + s * 0x100;
		if rd(ram, m + 0x24) == 0 || rd8(ram, m + 0x20) >= 0x80 {
			continue;
		}
		let class = (rd(ram, m + 0xa4) >> 16) as u16;
		let (mut joints, mut r) = (0, rd(ram, m + 0x64) & MASK);
		while r != 0 && joints < 64 {
			joints += 1;
			r = rd(ram, r + 8) & MASK;
		}
		present.entry(class).or_insert((rd(ram, m + 0x74), joints, rd(ram, m + 0x24) & MASK));
	}
	// The header table: where every present class's header is, by its index byte.
	let idx = |class: u32| rd8(ram, index + class) as u32;
	let fits = |t: u32| present.iter().filter(|(c, v)| v.2 != 0 && idx(**c as u32) != 0xff).all(|(c, v)| rd(ram, t + 4 * idx(*c as u32)) & MASK == v.2);
	let probe = present.iter().find(|(c, v)| v.2 != 0 && idx(**c as u32) != 0xff).map(|(c, v)| (idx(*c as u32), v.2));
	let headers = probe.and_then(|(i, h)| (0x0010_0000u32..0x0040_0000).step_by(4).find(|&a| rd(ram, a) & MASK == h && a >= 4 * i && fits(a - 4 * i)).map(|a| a - 4 * i));
	eprintln!("mp census: level {} index {index:#x} headers {headers:x?} (create_moby {create:#x}, built {built:x?})", me.level);
	if let Some(headers) = headers {
		for class in 0..0x800u32 {
			let i = idx(class);
			let h = rd(ram, headers + 4 * i) & MASK;
			if i == 0xff || h == 0 || h as usize + 0x48 > ram.len() {
				continue;
			}
			let update = built.get(1).map_or(0, |&u| rd(ram, u + 4 * i));
			eprintln!("mp census: level {} class {class} type {} update {update:#x}", me.level, (rd(ram, h + 0x44) >> 16) as i16);
		}
	}
	for (class, (update, joints, h)) in &present {
		eprintln!("mp census: level {} moby class {class} update {update:#x} joints {joints} type {}", me.level, (rd(ram, h + 0x44) >> 16) as i16);
	}
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
		let census = std::env::var("RAC_MP_TEST_CENSUS").ok().and_then(|v| v.trim().parse().ok());
		Aids { goto, watch, dump, census }
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
	if (a.goto.is_empty() && a.watch.is_empty() && a.census.is_none()) || me.level < 0 || me.mode != 0 || mp.table == 0 {
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
	census(c, mp, me);
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
