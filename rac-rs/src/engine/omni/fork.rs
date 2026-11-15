//! [MP] [AP] This fork's own seam into the mod host (rac-rs-multiplayer; docs/UPSTREAM_SYNC.md). Everything this fork's online
//! multiplayer (rac1/mp.rs) and Archipelago randomizer (rac1/ap.rs) need from omni lives here, in a file upstream does not have, so
//! a merge of upstream's omni only meets the `mod fork; pub use fork::*;` lines of mod.rs and the two slots of draw.rs.
//!
//! Game-neutral like the rest of engine/ (boundary_tests.rs): no R&C1 address and no rac1 module is named here. What the engine has
//! to ask R&C1 (is multiplayer on, is a movie up) comes in through `set_gates`, which rac1/fork_pack.rs calls before anything runs;
//! the R&C1 game functions with fixed numbers (create_moby, delete_moby) are called from there with `call_game_with`.

use super::{bridge, funcs, hooks, native_host};
use ee_oracle::cpu::Cpu;
use ee_oracle::hs_reloc::Reloc;
use std::sync::OnceLock;

/// A world point in the game's 2D space, for the players' names (mp.rs).
pub use super::draw_w2s::screen_of;
/// The draw command the players' names and the randomizer's lines are made of.
pub use super::cabi::OmniDrawCmd;

/// What the engine asks the game pack for this fork's overlays (draw.rs). Unset (before rac1/fork_pack.rs `init`), both say no.
pub struct Gates {
	/// Multiplayer runs (mp.rs `pool` > 0): the overlay's place is noted for the players' names even when no mod runs.
	pub mp_running: fn() -> bool,
	/// A movie plays (retail/fmv_bridge.rs `waiting`): no field runs then, so the last names and lines would stay up over it.
	pub movie: fn() -> bool,
}

static GATES: OnceLock<Gates> = OnceLock::new();

/// The game pack hands its gates over once (later calls are ignored).
pub fn set_gates(g: Gates) {
	let _ = GATES.set(g);
}

pub(super) fn mp_running() -> bool {
	GATES.get().is_some_and(|g| (g.mp_running)())
}

pub(super) fn movie() -> bool {
	GATES.get().is_some_and(|g| (g.movie)())
}

/// The specs of the hook points `want` accepts that no mod has taken, for this fork's own hands (mp/stand.rs). Two hands cannot share an entry.
pub fn hook_specs(want: impl Fn(&str) -> bool) -> Vec<(String, &'static Reloc)> {
	let taken = |id: &str| (0..hooks::MAX_HOOKS).any(|n| hooks::slot(n).id.get().is_some_and(|i| i == id));
	let mut v: Vec<_> = hooks::specs().iter().filter(|(id, _)| want(id) && !taken(id)).map(|(id, r)| (id.clone(), r)).collect();
	v.sort_by(|a, b| a.0.cmp(&b.0));
	v
}

/// [AP] The spec of a hook point, whether a mod has taken it or not: to find the game function in the code loaded now.
pub fn hook_spec(id: &str) -> Option<&'static Reloc> {
	hooks::specs().get(id)
}

/// A hook point a mod has taken: its spec and the mod's hand, for a hand of this port's at the same entry (registered after the mod's,
/// it is the one the table installs) that passes the mod every call it does not take itself.
pub fn taken_hook(id: &str) -> Option<(&'static Reloc, fn(&mut Cpu) -> bool)> {
	let n = (0..hooks::MAX_HOOKS).find(|&n| hooks::slot(n).id.get().is_some_and(|i| i == id))?;
	Some((hooks::specs().get(id)?, hooks::hook_fn(n)))
}

/// The game functions whose name starts with `prefix` (the game's funcs file, `Game::funcs`), with their specs.
pub fn func_relocs(prefix: &str) -> Vec<(String, &'static Reloc)> {
	funcs::all().iter().filter(|f| f.name.starts_with(prefix)).map(|f| (f.name.clone(), &f.reloc)).collect()
}

/// The code spec of a game function the mods call (the game's funcs file), by its name there: for a hand at its entry.
pub fn func_reloc(name: &str) -> Option<&'static Reloc> {
	funcs::all().iter().find(|f| f.name == name).map(|f| &f.reloc)
}

/// Where the function of `reloc` starts in the code loaded now, if it is there.
pub fn find_entry(c: &Cpu, reloc: &Reloc) -> Option<u32> {
	funcs::find_in(&funcs::code_words(c), funcs::CODE_LO, reloc)
}

/// Whether the function of `reloc` still starts at `entry`.
pub fn entry_holds(c: &Cpu, reloc: &Reloc, entry: u32) -> bool {
	reloc.matches(&c.mem, entry)
}

/// Runs the game's code at `entry` between two fields, with the registers `set` gives it.
pub fn run_game(c: &mut Cpu, entry: u32, set: impl FnOnce(&mut Cpu)) -> Result<(), String> {
	native_host::run_at(c, entry, set, |_| ()).map_err(str::to_string)
}

/// Runs the game function `number` (the game's funcs file, NTSC) between two fields: `set` gives it its registers, `read` takes what it left.
pub fn call_game_with<T>(c: &mut Cpu, number: u32, set: impl FnOnce(&mut Cpu), read: impl FnOnce(&Cpu) -> T) -> Result<T, String> {
	let (entry, _) = funcs::resolve(c, number, true, native_host::level_word(c))?;
	native_host::run_at(c, entry, set, read).map_err(str::to_string)
}

/// Runs the game function `number` (the game's funcs file, NTSC) between two fields, with the registers `set` gives it.
pub fn call_game(c: &mut Cpu, number: u32, set: impl FnOnce(&mut Cpu)) -> Result<(), String> {
	call_game_with(c, number, set, |_| ())
}

/// Where the game function `number` (the game's funcs file, NTSC) is in the code loaded now. A miss is not remembered: the caller
/// paces its own tries (each scans the code).
pub fn game_entry(c: &Cpu, number: u32) -> Option<u32> {
	funcs::locate(c, number, true)
}

/// Whether the game runs inside a call made for it (`run_game`, `call_game`, a hook's dispatch) rather than in its own frame: a hand
/// that sends the game somewhere and back leaves such a call alone (a frame wait there ends the call, and what came back to it later
/// returned to the call's empty return address 0x10).
pub fn in_made_call(c: &Cpu) -> bool {
	native_host::calls_live() > 0 || bridge::in_hook() || c.g(31) as u32 == 0x10
}

/// What a mod's hook needs too: the relocatable hands switched on.
pub fn allow_hook_hands() {
	hooks::allow_reloc_hands();
}
