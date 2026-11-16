//! [AP] The game's givers decide what they offer from bytes the multiworld writes too, so an item received before its place was
//! visited could take that place away:
//! - Each item's "had it" mark (`HAD`, beside the owned bytes; retail `item.give` sets it) is read by givers when their level
//!   loads: Eudora's Suck Cannon (class 1120), Orxon's Magneboots (18), Pokitaru's trader (298, the Persuader), Quartu's Bolt
//!   Grabber (1388), Rilgar's R.Y.N.O. seller (925) and more leave or stay silent once it is set. For every item that is a place,
//!   the mark says whether its place was found.
//! - Some read the item's own byte: Orxon's nanotech seller (1326) sells the Premium while its byte is clear, then the Ultra;
//!   Pokitaru's trader plays its trade while the Raritanium's or the Persuader's byte is set; Hoven's Raritanium (282) and the
//!   Fleet's Codebot (1428) leave when the level loads if theirs is set. On those levels the bytes say whether the places were
//!   found (none of them is needed to get around there); elsewhere, what was received.
//! - Eight givers read their item's owned byte, and that item is needed on their own level (the Swingshot on Kerwan, the O2 Mask
//!   and the Pilot's Helmet for gold bolts there): Aridia's Sonic Summoner (class 786), Kerwan's Helga (890, the Swingshot) and
//!   Heli-Pack (909), Gaspar's Pilot's Helmet (1290, its code on every level), Pokitaru's O2 Mask pickup (23) and Thruster-Pack
//!   (90), Hoven's Hydro-Pack (328) and Oltanis' Morph-o-Ray (1354). A hand at each one's update (this port's specs
//!   `ap_giver_update_<class>` in omni_bind/rac1_funcs.txt) runs it with the byte saying whether its place was found, and puts
//!   the byte back after: the rest of the level sees what was received. A giver that sets the byte itself (the O2 Mask pickup)
//!   finds its place; one that calls `item.give` finds it there. The other readers of these bytes use the item (Blarg's water
//!   pads, Kerwan's class 914, Kalebo's 1410, the boots and packs) and see what was received.

use super::{byte, put_byte, received_equip, word, State, ITEM_SPECIAL, LOC_ITEM, LOC_SPECIAL, LEVEL_WORD, MASK, OWNED, SPECIAL};
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::{Reloc, RelocHand};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;

/// Each item's "had it" mark.
const HAD: u32 = 0x13_D4E8;
/// The items that are places of their own (data/Locations.py `item_loc`): their mark says whether the place was found.
const ITEM_PLACES: [u32; 21] = [2, 3, 4, 5, 6, 7, 9, 12, 21, 22, 23, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35];

const fn special(level: u32, k: u32) -> (u32, u32) {
	(SPECIAL + k, LOC_SPECIAL + level * 16 + k)
}

/// (level, the bytes its givers read with the places they stand for).
const VIEWS: [(i32, &[(u32, u32)]); 4] = [
	(10, &[special(10, 4), special(10, 5)]),
	(11, &[(OWNED + 35, LOC_ITEM + 35)]),
	(12, &[special(12, 1)]),
	(17, &[special(17, 2)]),
];

/// What the byte says away from its level: received (and, for a special item, not traded away).
fn received(s: &State, addr: u32) -> bool {
	if addr >= OWNED {
		received_equip(s, addr - OWNED)
	} else {
		let k = addr - SPECIAL;
		s.received.contains(&(ITEM_SPECIAL + k)) && !s.consumed.contains(&k)
	}
}

/// Whether the byte at `addr` shows its place now (the watch counts its change as a find even when it was received, and the
/// grant leaves it alone).
pub(super) fn viewed(s: &State, addr: u32) -> bool {
	s.view.is_some_and(|i| VIEWS[i].1.iter().any(|&(a, _)| a == addr))
}

/// Every field once the seed is known, in any game mode (givers read the bytes when their level loads, before play): the marks
/// say whether the places were found; on a level with givers that read an item's byte, its byte says so too, and once the level
/// is left it says what was received again.
pub(super) fn keep(ram: &mut [u8], s: &mut State) {
	for &id in &ITEM_PLACES {
		let found = s.found.contains(&(LOC_ITEM + id));
		if (byte(ram, HAD + id) != 0) != found {
			put_byte(ram, HAD + id, found as u8);
		}
	}
	let level = word(ram, LEVEL_WORD) as i32;
	let now = VIEWS.iter().position(|v| v.0 == level);
	if s.view != now {
		if let Some(i) = s.view.take() {
			for &(addr, _) in VIEWS[i].1 {
				let v = received(s, addr);
				set(ram, s, addr, v);
			}
		}
		if now.is_some() {
			eprintln!("ap: level {level}: its givers' bytes show their places");
		}
		s.view = now;
	}
	if let Some(i) = now {
		for &(addr, loc) in VIEWS[i].1 {
			let v = s.found.contains(&loc);
			set(ram, s, addr, v);
		}
	}
}

/// The givers that read their item's owned byte: (class, item).
const OWNED_GIVERS: [(u16, u32); 8] = [(786, 5), (890, 12), (909, 2), (1290, 7), (23, 6), (90, 3), (328, 4), (1354, 21)];
const SPEC_PREFIX: &str = "ap_giver_update_";
/// A moby's class (+0xa6).
const CLASS: u32 = 0xa6;
static SPECS: OnceLock<Vec<&'static Reloc>> = OnceLock::new();
const RELOCS: [fn() -> &'static Reloc; 8] = [reloc::<0>, reloc::<1>, reloc::<2>, reloc::<3>, reloc::<4>, reloc::<5>, reloc::<6>, reloc::<7>];
/// A giver's update runs under its hand now (the hand at its entry lets that call through).
static WRAPPING: AtomicBool = AtomicBool::new(false);
/// What each giver's byte was last changed to by the hand, by its place in `OWNED_GIVERS` (bit 0: changed, bit 1: shown as
/// given), for one log line per change.
static SAID: [AtomicU8; 8] = [const { AtomicU8::new(0) }; 8];

fn reloc<const N: usize>() -> &'static Reloc {
	SPECS.get().and_then(|s| s.get(N).copied()).expect("giver spec")
}

/// `Retail::load`, after the multiplayer's hands: a later hand wins at an entry both take (the multiplayer's hand at every
/// `rac1.moby.*.update` only acts on enemies it mirrors, and no giver is an enemy).
pub(super) fn install() {
	let specs = crate::omni::func_relocs(SPEC_PREFIX);
	let specs = &specs[..specs.len().min(RELOCS.len())];
	if SPECS.set(specs.iter().map(|(_, r)| *r).collect()).is_err() {
		return;
	}
	for (i, (id, _)) in specs.iter().enumerate() {
		ee_oracle::rec::register_reloc_hand(Box::leak(Box::new(RelocHand { name: id.clone().leak(), reloc: RELOCS[i], est: ee_oracle::hs::EXACT, f: giver_hand })));
	}
	crate::omni::allow_hook_hands();
	eprintln!("ap: {} givers' updates see whether their places were found", specs.len());
}

/// A giver's update: run with its item's byte saying whether its place was found, when that differs from what it says now.
fn giver_hand(c: &mut Cpu) -> bool {
	if !super::on() || WRAPPING.load(Ordering::Relaxed) {
		return false;
	}
	let moby = c.g(4) as u32 & MASK;
	let class = u16::from_le_bytes([byte(&c.mem.ram, moby + CLASS), byte(&c.mem.ram, moby + CLASS + 1)]);
	let Some(k) = OWNED_GIVERS.iter().position(|g| g.0 == class) else { return false };
	let id = OWNED_GIVERS[k].1;
	let addr = OWNED + id;
	let now = byte(&c.mem.ram, addr);
	let shown = match super::state().as_ref() {
		Some(s) if s.seed.is_some() => s.found.contains(&(LOC_ITEM + id)),
		_ => return false,
	};
	if (now != 0) == shown {
		return false;
	}
	let said = 1 | (shown as u8) << 1;
	if SAID[k].swap(said, Ordering::Relaxed) != said {
		eprintln!("ap: giver class {class} sees item {id} as {} (its place {}found)", if shown { "given" } else { "not given" }, if shown { "" } else { "not " });
	}
	let (entry, ra) = (c.pc, c.g(31) as u32);
	put_byte(&mut c.mem.ram, addr, shown as u8);
	WRAPPING.store(true, Ordering::Relaxed);
	let run = catch_unwind(AssertUnwindSafe(|| crate::omni::frame_wait::call(c, entry, ra)));
	WRAPPING.store(false, Ordering::Relaxed);
	let set = byte(&c.mem.ram, addr) != 0 && !shown;
	put_byte(&mut c.mem.ram, addr, now);
	if set {
		if let Some(s) = super::state().as_mut() {
			super::found(s, LOC_ITEM + id);
		}
	}
	// A wait for the next frame inside left the machine there: the rest of the update runs plain from it.
	if !run.unwrap_or_else(|p| resume_unwind(p)) {
		return true;
	}
	c.pc = ra;
	c.npc = ra.wrapping_add(4);
	c.mem.now = c.icount;
	true
}

/// The byte set, and seen as it is now (the next field's watch must not take it for a find).
fn set(ram: &mut [u8], s: &mut State, addr: u32, v: bool) {
	if (byte(ram, addr) != 0) == v {
		return;
	}
	put_byte(ram, addr, v as u8);
	let seen = if addr >= OWNED { s.seen_owned.as_mut().map(|x| (x, addr - OWNED)) } else { s.seen_special.as_mut().map(|x| (x, addr - SPECIAL)) };
	if let Some((x, i)) = seen {
		if let Some(b) = x.get_mut(i as usize) {
			*b = v as u8;
		}
	}
}
