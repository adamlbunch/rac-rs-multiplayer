//! [MP] [AP] This fork's own entry into the R&C1 pack (rac-rs-multiplayer; docs/UPSTREAM_SYNC.md): the one call the retail
//! worker makes for it (`init`, in `Retail::load` after the mod host), and the R&C1 game functions it calls by number. A file
//! upstream does not have, so a merge of upstream's rac1 only meets the one-line calls into `mp` / `ap` at their hook points.

/// `Retail::load`, after `omni::init`: the engine's gates for this fork's overlays (engine/omni/fork.rs), then the randomizer and
/// the multiplayer (each does nothing unless its RAC_AP / RAC_MP setting asks for it).
pub fn init() {
	crate::omni::set_gates(crate::omni::Gates { mp_running: || crate::mp::pool() > 0, movie: crate::retail::fmv_bridge::waiting });
	crate::ap::init();
	crate::mp::init();
}

/// R&C1's `create_moby` (NTSC function number, omni_bind/rac1_funcs.txt).
const CREATE_MOBY: u32 = 0x24_E448;
/// R&C1's `delete_moby`.
const DELETE_MOBY: u32 = 0x24_E778;

/// Runs the game's own `create_moby` between two fields: a moby of `class`, or 0 when the pool is full.
pub fn create_moby(c: &mut ee_oracle::cpu::Cpu, class: u32) -> Result<u32, String> {
	crate::omni::call_game_with(c, CREATE_MOBY, |c| c.s(4, class as u64), |c| c.g(2) as u32)
}

/// Runs the game's own `delete_moby` on the moby at `a`, between two fields (no mod needs to be loaded).
pub fn delete_moby(c: &mut ee_oracle::cpu::Cpu, a: u32) -> Result<(), String> {
	crate::omni::call_game(c, DELETE_MOBY, |c| c.s(4, a as u64))
}
