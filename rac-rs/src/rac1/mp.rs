//! Online multiplayer, the game side (this fork's own; upstream rac-rs has none). The network side is the rac-mp crate (mp/
//! beside rac-rs/: wire format, sessions, interpolation). Here: read the local hero out of the machine's RAM once per field,
//! hand it to the session, take the other players' heroes back and show them as puppets.
//!
//! Off unless RAC_MP is set: without it no socket is opened, nothing is read and no puppet prop exists, so the game runs exactly
//! as upstream's.
//! - `RAC_MP=host` hosts on UDP port 2410, `RAC_MP=host:PORT` on another, `RAC_MP=host:ADDR:PORT` on one address only
//!   (127.0.0.1: this PC); `RAC_MP=join:ADDR` or `join:ADDR:PORT` joins (ADDR an IP address or a host name).
//! - `RAC_MP=mirror` (or `mirror:FIELDS`): no network; one puppet plays the local hero a second (FIELDS) behind. The
//!   offline check of the puppets.
//! - `RAC_MP_NAME`: the name the others see (default "Ratchet").
//! - `RAC_MP_FOLLOW=0`: a client stays where it is when the host travels.
//!
//! Puppets are picture only: each other player in the same level is one more class-0 (Ratchet) moby in the field's snapshot
//! (`puppets`, retail/snapshot.rs), placed, turned, lit and posed by his own animation clock like any moby the renderer
//! mirrors, with the mobys the game keeps on him (Clank, a pack, the held item) as his game placed them (`read_part`). Nothing
//! is written to game RAM, so the game itself does not know they are there (enemies, collision: later phases). The class-0 prop
//! pool they draw with (`pool`) is added to the level's pools (retail/prop_pools.rs) and its model to the level's prop models
//! (viewer/app.rs) only while RAC_MP is set. The wrench draws under a class of its own (`PUPPET_WRENCH`). Names go to the mods'
//! last 2D layer as a list of their own (`draw_names`, omni/draw.rs `submit_mp`).
//!
//! Synced enemies (`enemy_tick` in mp/foes.rs, the hands in mp/enemies.rs): the host gives each enemy to the game of the player
//! nearest to it. That game runs it and streams it, the others mirror it, and a hit from another game travels to it.
//!
//! Sounds and effects (mp/fx.rs, made here by mp/replay.rs): the sound cues and explosions a game makes for what it runs are made
//! again on the others' copies.
//!
//! Spawned objects (mp/spawned.rs): each game reports what it makes during play, the host gives it an id, and the others show it as a
//! stand-in, or wake a copy of their own when the game running it is gone.
//!
//! The shared world (`world_tick`, mp/world.rs): each game watches the objects the level placed that matter (enemies, crates,
//! pickups) in the moby table, where a placed object keeps its slot. One that goes from a player's game near that player is
//! reported; the host takes the first report and tells the others, and each game removes it with the game's own `delete_moby`,
//! again if it comes back (a player's death restores a checkpoint) or if the player arrives later. This is the only thing that
//! changes a game's RAM, and only when another player removed something. The rewards stay with the player who earned them.
//!
//! Staying in step: the team travels together. Clients follow the host to another level (`follow_host`), by the request the game's
//! own level ends make, and when a client takes a ship by itself the host's game goes after it, and the rest after the host. A
//! player in a scene, a movie, a ship or a sprite screen is not drawn, and nobody is drawn over a local scene (`on_stage`). A
//! name says when its player is paused, in a dialog or shopping. A client that lost the host keeps trying to get back in
//! (`run_net`), and tells the host again what it knows is gone.
//!
//! Threads: `init` on the main thread in `Retail::load` (after `omni::init`); `note_front`, `tick` and `puppets` on the retail
//! worker thread: `tick` after each field the EE ran (after `omni::tick`), `puppets` when that field's snapshot is taken. The
//! session runs on a thread of its own ("mp-net", `run_net` in mp/net.rs): the game does not tick in parts of the front end or
//! while a level loads, and the players must keep answering each other meanwhile. `WIRE` is what the two hand each other; what the
//! session reported is taken in by mp/events.rs.
//!
//! Addresses: both our discs (PAL and USA v1.00) have the data layout of the OmniLoader binding's "ntsc" column (omni/layout.rs):
//! the hero moby pointer 0x1413d0 (as retail/snapshot.rs reads it), hero base 0x13f350 (position +0x80, state +0x2084), health
//! 0x1415f8, the first item slot's id 0x140408, level word 0x15ed84. The mode and level-loop exit words are overlay data, read
//! through `disc::va` as omni/events.rs does. What each level links in apart (the hit queue, the scene start) is read from the
//! level's own code: `enemies::hit_ring`, `scenes::find`. Game functions are called by spec (`omni::call_game`), either disc.

mod arcs;
mod arrive;
mod draws;
mod enemies;
mod events;
mod foes;
mod fx;
mod ids;
mod keep;
mod machines;
mod net;
mod notice;
mod particles;
mod quads;
mod replay;
mod scenes;
mod spawned;
mod story;
mod testaid;
mod touch;
mod world;

use foes::enemy_tick;
use net::{open, run_net};
pub use quads::effects;
use quads::{QUADS_SENT, QUADS_SHOWN};
use replay::fx_tick;
use world::world_tick;

use crate::fmath::Fm;
use crate::retail::{disc, euler_from_rows, table, Clock, Culled, Moby};
use ee_oracle::cpu::Cpu;
use rac_mp::{AnimClock, ArcPose, Draws, FxQuad, RopePose, SHOT_ID, EnemyPose, Event, FxEvent, Particle, HeroState, Keep, NO_SLOT, Name, Part, PlayerId, Progress, SPAWNED_ID, Session, Track, WorldKey};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Send the local hero every this many fields (25 or 30 times a second).
const SEND_EVERY: u64 = 2;
/// Show the others this many fields in the past (about 100 ms), so a late or lost state does not show.
const DELAY: f64 = 6.0;
/// The offline mirror's default delay: one second.
const MIRROR_DELAY: f64 = 60.0;
/// Write where the others are to the run log every this many fields.
const LOG_EVERY: u64 = 300;
/// Class types (class record +0x46) the shared world follows: enemies and crates.
const WORLD_TYPES: [i16; 2] = [5, 9];
/// Classes it follows besides those: the gold bolt.
const WORLD_CLASSES: [u16; 1] = [1134];
/// Fields after a level starts before anything counts as gone: the game is still setting the level up.
const WORLD_SETTLE: u32 = 120;
/// Only something this near the local hero (metres) can be his doing.
const WORLD_NEAR: f32 = 80.0;
/// More than this many going at once in one field is the game clearing the level, not a player.
const WORLD_BURST: usize = 8;
/// The most slots a moby table can have that we believe.
const WORLD_SLOTS: u32 = 4096;
/// The class types of an enemy and of a crate.
const ENEMY: i16 = 5;
const CRATE: i16 = 9;
/// An enemy or crate another game finished is finished here too, by a blow from the local hero this strong, with the wrench's mask; if
/// it still stands after this many fields, the blow is posted again. Only within `FINISH_NEAR` of the local hero (metres): further
/// away it is hidden till he comes.
const FINISH_DAMAGE: f32 = 1000.0;
const FINISH_MASK: u32 = 0x1_0000;
const FINISH_WITHIN: u64 = 180;
/// An enemy to finish that is still in its first state here (state byte 0: its update never ran in this game, another game ran it) is
/// woken first, its own update run; one not woken within this many fields is taken away instead.
const WAKE_WITHIN: u64 = 60;
const FINISH_NEAR: f32 = 30.0;
/// Hits and explosions made here between two fields, at most, per field: the game's hit ring takes a few per field, and a full one
/// waits for the next field, which a call between two fields can't.
const BLOWS_PER_FIELD: usize = 2;
/// The host reconsiders who runs which enemy every this many fields, and moves one only to a player this much nearer (metres).
const OWN_EVERY: u64 = 30;
const OWN_MARGIN: f32 = 4.0;
/// Enemies this far (metres) from every other player are not streamed: nobody can see them. A spawned one is, this much less often
/// (in fields), so that another game has it when this one goes.
const STREAM_RANGE: f32 = 120.0;
/// What an enemy we run draws goes to the others when one of them is this near it (metres), for this many enemies at most.
const DRAW_RANGE: f32 = 45.0;
const DRAWS_AT_ONCE: usize = 6;
/// Other games' particles waiting to be put here, at most.
const PARTICLES_KEEP: usize = 64;
const FAR_EVERY: u64 = 15 * SEND_EVERY;
/// Another game's sounds and effects are made this many fields after they come, as its enemies are shown (`DELAY`).
const FX_DELAY: u64 = 6;
/// The classes of another player's sounds (hero, wrench, Clank, what he holds) heard here, at most, and how high above his feet.
const VOICES_EACH: usize = 6;
const VOICE_LIFT: f32 = 0.8;
/// The hero's death states (retail/modcam.rs `GATE_STATES`, death_respawn.md: the pit tumble 0x77 and the deaths). The respawn that
/// follows resets what it finds of the hero's class, and a voice of it there stopped the game: nothing of ours stays through one.
const DEATH_STATES: [i32; 10] = [0x3d, 0x47, 0x77, 0x7a, 0x7b, 0x7c, 0x7d, 0x7f, 0x80, 0x82];
/// Fields after the local hero's death or fall ends in which the game is still left alone (`Mp::held`): the respawn's fade and
/// reset, and the level's objects put back, settle first. Raj's game stopped right after a fall's respawn (v0.4.1.4).
const RESPAWN_HOLD: u32 = 120;
/// Fields without a pose after which an enemy runs here again, whoever owns it: its owner paused, left or is loading.
const POSE_STALE: u64 = 60;
/// How long a client waits between tries to get back to a lost host.
const REJOIN_EVERY: Duration = Duration::from_secs(3);
/// A hero part with an offset (metres), turn (radians) or scale this large or more is not set up yet, and is not sent.
const PART_SANE: f32 = 1.0e3;
/// The network thread held up this long is said in the run log, with what held it: the others drop a player silent for 15 s
/// (`rac_mp::session::TIMEOUT`), and a drop is then explained.
const HELD_UP: Duration = Duration::from_secs(2);
/// Fields between travel requests while a client has not reached the host's level.
const FOLLOW_EVERY: u64 = 600;
/// How far above his feet (metres) a player's name sits, and its text size (1 = the game's 16-pixel line).
const NAME_HEIGHT: f32 = 1.5;
const NAME_SIZE: f32 = 0.75;
/// Puppet props: one per other player.
const PUPPETS: u32 = rac_mp::proto::MAX_PLAYERS as u32 - 1;
/// The puppets' made-up moby addresses (above the 32 MB of RAM, so no real moby has one): the props bind by address.
const PUPPET_ADDR: u32 = 0xF000_0000;

/// The back objects that ride on every hero (607 and Clank, 601), so a level's pools need room for the puppets' too
/// (retail/prop_pools.rs).
/// Also the bodies a hero can have instead of Ratchet's (`HeroState::body`): Clank on his own (87), Giant Clank (419).
pub const PART_CLASSES: [u32; 4] = [607, 601, 87, 419];
/// The class the puppets' wrenches draw as (`puppet_wrench`): no real class has it, and it stays under the pool code's limits.
pub const PUPPET_WRENCH: u32 = 0xFF0;
/// The cells that point at the parts: the back objects, then the held item (retail/snapshot.rs reads the same).
const PART_CELLS: [u32; rac_mp::MAX_PARTS] = [0x14_04D0, 0x14_04D4, crate::retail::HELD_MOBY];

const HERO_MOBY: u32 = 0x14_13D0;
const HERO: u32 = 0x13_F350;
const HEALTH: u32 = 0x14_15F8;
const ITEM: u32 = 0x14_0408;
const LEVEL_WORD: u32 = 0x15_ED84;
const EXIT_WORD: u32 = 0x15_F550;
const MODE_WORD: u32 = 0x15_F5A4;
const MASK: u32 = 0x01ff_ffff;

struct Mp {
	/// The offline mirror's delay in fields (None: a session runs on the network thread).
	mirror: Option<f64>,
	/// Our player id, once we have one.
	my_id: Option<PlayerId>,
	/// Who runs which enemy; an enemy not listed runs on the host.
	owners: BTreeMap<WorldKey, PlayerId>,
	/// Enemies another game runs, by slot: their poses, last health and the field the last pose came.
	poses: BTreeMap<u16, EnemyTrack>,
	/// Spawned objects of this game, by moby address, and stand-ins of other games', by id.
	spawned: BTreeMap<u32, spawned::Spawned>,
	stand_ins: BTreeMap<u16, spawned::StandIn>,
	/// Host: every spawned object in the world, by id.
	live: BTreeMap<u16, spawned::Live>,
	/// Spawned objects this game runs from the next `spawned::tick`; `orphaned`: the host went, so all it holds; `rejoined`: a host took
	/// us back, so it hears again of what this game has.
	takeovers: Vec<WorldKey>,
	orphaned: bool,
	rejoined: bool,
	/// The level those are of, fields of play left in which what shows up came with it, where that was, and the next name for a spawned
	/// object here.
	spawned_level: i8,
	opening: u32,
	start_places: BTreeSet<(u16, [i32; 3])>,
	next_local: u16,
	/// Other games' sounds and effects, with the field they are made and whose they are, and their blows on the level's objects.
	fx_in: Vec<(u64, PlayerId, FxEvent)>,
	/// Hidden mobys the other players' own sounds come from, by player and class, kept at their puppets.
	voices: BTreeMap<(PlayerId, u16), u32>,
	touches_in: Vec<(WorldKey, f32, u32)>,
	/// What another game finished, dying here its own way, by moby address: the field it began.
	dying: BTreeMap<u32, u64>,
	/// The ids of the level's placed objects now, which every game shares (mp/ids.rs); every slot below is one of them.
	ids: ids::Ids,
	/// The moby table of the level now (0: none), and hits from other games waiting for the next `enemy_tick`.
	table: u32,
	hits_in: Vec<(WorldKey, f32, u32)>,
	tracks: BTreeMap<PlayerId, Track>,
	/// This field's smoothed heroes of the others, for `puppets`.
	shown: Vec<(PlayerId, HeroState)>,
	/// Cutscenes, shared (mp/scenes.rs).
	scenes: scenes::Scenes,
	/// The others' progress this game does not have yet (mp/story.rs).
	story_in: Vec<Progress>,
	/// The level's machinery, as this game sees it and the others stream it.
	machines: machines::Machines,
	/// The local hero's level (-1: none) and game mode this field.
	level: i8,
	mode: u8,
	/// The local hero moby's draw distance (+0x32), which the puppets take.
	draw: i16,
	fields: u64,
	/// What the shared world watches in the level now, by slot: its class and class type. `watched_level` is that level, `settle` the fields left
	/// before something going counts.
	watched: BTreeMap<u16, (u16, i16)>,
	/// Objects of the shared world's gone list that ended in this game, in this level, in settled play: one alive again after that
	/// is the level's own (an enemy it sends again), not one to finish.
	gone_here: BTreeSet<u16>,
	/// Objects of the gone list this game is finishing (its blow posted): when one is no longer alive, it ended here.
	finishing: BTreeSet<u16>,
	watched_level: i8,
	settle: u32,
	/// What is gone from the shared world, in every level.
	removed: BTreeSet<WorldKey>,
	/// The level last told to the session (-1: none).
	announced: i8,
	/// The level to go to while this game is not there yet (a client: the host's; the host: one a client went to by itself), and the
	/// field of the next request.
	follow: Option<u8>,
	next_follow: u64,
	/// Client: the level the host last said it is in.
	host_level: Option<u8>,
	/// The local hero dies or falls, or was respawned less than `RESPAWN_HOLD` fields ago (`respawn` counts them down): nothing of
	/// ours is in the game then (no voice, no stand-in), and nothing is made, removed, hit or given in it for the others.
	held: bool,
	respawn: u32,
	/// Every progress of the team this game sent or got: given again to a host it gets back in with (a host that started again has
	/// lost what it held).
	story_seen: Vec<Progress>,
	/// Our player's name as the session has it, every player's things the team keeps (mp/keep.rs), the arcs of enemies other games
	/// run here (by id: the arc and the field it came), and the look for the host after coming (mp/arrive.rs).
	my_name: String,
	keeps: keep::Keeps,
	arcs_in: BTreeMap<u16, (ArcPose, u64)>,
	arrive: arrive::Arrive,
	/// The particles of enemies here (mp/particles.rs): ours sent, others' put in, and what enemies draw (mp/draws.rs).
	particles: particles::Particles,
	particles_in: Vec<Particle>,
	draws: draws::DrawState,
	/// Other games' shots this game fired too (mp/spawned.rs `fired_here_too`): no stand-in.
	shots_skipped: BTreeSet<u16>,
	/// The other players' weapon beams (`effects`): by player, the field they came, their tick, its rope and its quads.
	quads_in: BTreeMap<PlayerId, (u64, u32, Option<RopePose>, Vec<FxQuad>)>,
}

/// An enemy another game runs.
#[derive(Default)]
struct EnemyTrack {
	track: Track,
	class: u16,
	health: f32,
	scale: f32,
	heard: u64,
	/// Its fade and override colour (EnemyPose `alpha`, `ovr`).
	alpha: u8,
	ovr: Option<u32>,
}

static MP: Mutex<Option<Mp>> = Mutex::new(None);

/// What the game and the network thread hand each other.
struct Wire {
	/// The local hero to send (the newest replaces one not sent yet).
	out: Option<HeroState>,
	/// What the session reported since the game last looked.
	events: Vec<Event>,
	/// Everyone in, us included (names for the log).
	roster: Vec<(PlayerId, String)>,
	/// What went from our game, for the session to report.
	gone: Vec<WorldKey>,
	/// What this game brought back after it was gone (`Session::back`).
	back: Vec<WorldKey>,
	/// The level our game is in, for the session to announce.
	level: Option<u8>,
	/// Client: the level our game went to by itself, for the host to follow.
	went: Option<u8>,
	/// Our players' blows on the level's objects.
	touches: Vec<(WorldKey, f32, u32)>,
	/// Sounds and effects our game made for what it runs (level, events).
	fx: Option<(u8, Vec<FxEvent>)>,
	/// What our game spawned (name, level, class, position, whether a copy counts whenever it shows up, whether only we make it).
	spawns: Vec<(u16, u8, u16, [f32; 3], bool, bool)>,
	/// What our game got that the whole team keeps, and the cutscenes it started (level, scene).
	story: Vec<Progress>,
	scenes: Vec<(u8, i32)>,
	/// The variables of objects our game moves (object, state byte, pvar words).
	vars: Vec<(WorldKey, u8, Vec<u32>)>,
	/// The machinery our game has moving near the others (level, tick, poses).
	machines: Option<(u8, u32, Vec<EnemyPose>)>,
	/// The enemies our game runs (level, tick, poses), hits on others', and who runs what (host).
	enemies: Option<(u8, u32, Vec<EnemyPose>)>,
	hits: Vec<(WorldKey, f32, u32)>,
	assign: Vec<(WorldKey, PlayerId)>,
	/// Our player's things to send (mp/keep.rs), and, back in, the others' this game remembers for the host.
	keep: Option<Keep>,
	remembered: Vec<(Name, Keep)>,
	/// The arcs the enemies our game runs keep up (level, tick, arcs).
	arcs: Option<(u8, u32, Vec<ArcPose>)>,
	/// Particles made near the enemies our game runs (level, particles), and what they drew (level, tick, draws).
	particles: Option<(u8, Vec<Particle>)>,
	draws: Option<(u8, u32, Vec<Draws>)>,
	/// Our player's weapon beams this field (level, tick, rope, quads).
	quads: Option<(u8, u32, Option<RopePose>, Vec<FxQuad>)>,
}

static WIRE: Mutex<Wire> = Mutex::new(Wire { keep: None, remembered: Vec::new(), arcs: None, particles: None, draws: None, quads: None, out: None, events: Vec::new(), roster: Vec::new(), gone: Vec::new(), back: Vec::new(), level: None, went: None, spawns: Vec::new(), touches: Vec::new(), fx: None, story: Vec::new(), scenes: Vec::new(), vars: Vec::new(), machines: None, enemies: None, hits: Vec::new(), assign: Vec::new() });
/// Hero states held for a game that does not tick (front end, level load): the older ones are dropped past this many events.
const WIRE_KEEP: usize = 2048;
/// `init` ran (a later `Retail::load`, after Load Game or travel, keeps the session).
static STARTED: AtomicBool = AtomicBool::new(false);
/// A session is open: `tick` and `puppets` do nothing (not even take the lock) until then.
static ON: AtomicBool = AtomicBool::new(false);
/// The title screen / front end is running (field_front.rs, every field).
static FRONT: AtomicBool = AtomicBool::new(false);
/// The game is quitting (`leave`): the network thread says so to the others and ends; `SAID_BYE` when it has.
static QUIT: AtomicBool = AtomicBool::new(false);
static SAID_BYE: AtomicBool = AtomicBool::new(false);
/// A network thread runs (not the offline mirror).
static NET: AtomicBool = AtomicBool::new(false);
/// RAC_MP was set but the session could not start: `tick` draws only the line that says so.
static FAILED: AtomicBool = AtomicBool::new(false);
/// How long a quitting game waits for its goodbye to go out.
const LEAVE_WAIT: Duration = Duration::from_millis(250);

/// RAC_MP is set: decided once, before the first level loads (the puppet pool and model are part of the level's props).
fn wanted() -> bool {
	static W: OnceLock<bool> = OnceLock::new();
	*W.get_or_init(|| std::env::var("RAC_MP").is_ok_and(|v| !v.trim().is_empty()))
}

/// The class-0 props to set aside for puppets (retail/prop_pools.rs `count_pools`, viewer/app.rs): 0 without RAC_MP.
pub fn pool() -> u32 {
	if wanted() { PUPPETS } else { 0 }
}

/// viewer/app.rs, at a level load (after Ratchet's class is taken out of the props): Ratchet's model and animation as the
/// class-0 prop model the puppets draw with. Nothing without RAC_MP, on the title, or outside retail mode.
pub fn puppet_model(
	l: &crate::level::Level,
	hero: Option<&crate::mobies::MobyClass>,
	front: bool,
	meshes: &mut std::collections::HashMap<u32, crate::mesh::Mesh>,
	anims: &mut std::collections::HashMap<u32, (crate::mobies::MobyClass, crate::anim::Animated)>,
) {
	if pool() == 0 || front || !crate::retail::active() {
		return;
	}
	let Some(c) = hero else { return };
	match crate::anim::animation(l, 0) {
		Ok(Some(a)) if a.sequences.iter().any(|s| !s.frames.is_empty()) => {
			let m = &c.mesh;
			let mesh = crate::mesh::Mesh { name: m.name.clone(), positions: m.positions.clone(), uvs: m.uvs.clone(), faces: m.faces.clone(), colours: Vec::new(), lit: None, strip_phase: m.strip_phase.clone(), normals: m.normals.clone() };
			meshes.insert(0, mesh);
			anims.insert(0, (c.clone(), a));
		}
		r => eprintln!("mp: no puppet model: Ratchet's animation {:?}", r.err()),
	}
}

/// viewer/app.rs, at a level load: the wrench as a prop model under `PUPPET_WRENCH`. Class 71 itself stays out of the props, as
/// upstream keeps it: the local hero's loose wrenches are drawn their own way and must not find a prop.
pub fn puppet_wrench(
	l: &crate::level::Level,
	gadgets: &std::collections::HashMap<u32, crate::mobies::MobyClass>,
	meshes: &mut std::collections::HashMap<u32, crate::mesh::Mesh>,
	anims: &mut std::collections::HashMap<u32, (crate::mobies::MobyClass, crate::anim::Animated)>,
) {
	if pool() == 0 {
		return;
	}
	let Some(c) = gadgets.get(&crate::mobies::WRENCH_CLASS) else { return };
	if let Ok(Some(a)) = crate::anim::gadget_animation(l, crate::mobies::WRENCH_CLASS) {
		let m = &c.mesh;
		meshes.insert(PUPPET_WRENCH, crate::mesh::Mesh { name: m.name.clone(), positions: m.positions.clone(), uvs: m.uvs.clone(), faces: m.faces.clone(), colours: Vec::new(), lit: None, strip_phase: m.strip_phase.clone(), normals: m.normals.clone() });
		anims.insert(PUPPET_WRENCH, (c.clone(), a));
	}
}

/// `Retail::load`: opens the session RAC_MP asks for, once per process.
pub fn init() {
	if STARTED.swap(true, Ordering::Relaxed) || !wanted() {
		return;
	}
	let spec = std::env::var("RAC_MP").unwrap_or_default();
	let name = std::env::var("RAC_MP_NAME").unwrap_or_else(|_| "Ratchet".into());
	let hz = disc::field_rate().round() as u8;
	enemies::install();
	fx::install();
	draws::install();
	touch::install();
	story::install();
	scenes::install();
	let mp = |mirror, my_id| Mp { mirror, my_id, owners: BTreeMap::new(), poses: BTreeMap::new(), spawned: BTreeMap::new(), stand_ins: BTreeMap::new(), live: BTreeMap::new(), takeovers: Vec::new(), orphaned: false, rejoined: false, spawned_level: -1, opening: 0, start_places: BTreeSet::new(), next_local: 0, fx_in: Vec::new(), voices: BTreeMap::new(), touches_in: Vec::new(), dying: BTreeMap::new(), machines: machines::Machines::default(), story_in: Vec::new(), scenes: scenes::Scenes::default(), ids: ids::Ids::default(), table: 0, hits_in: Vec::new(), tracks: BTreeMap::new(), shown: Vec::new(), level: -1, mode: 0, draw: 0, fields: 0, watched: BTreeMap::new(), watched_level: -1, gone_here: BTreeSet::new(), finishing: BTreeSet::new(), settle: 0, removed: BTreeSet::new(), announced: -1, follow: None, next_follow: 0, host_level: None, held: false, respawn: 0, story_seen: Vec::new(), my_name: rac_mp::proto::clean_name(&name), keeps: keep::Keeps::default(), arcs_in: BTreeMap::new(), arrive: arrive::Arrive::default(), particles: particles::Particles::default(), particles_in: Vec::new(), draws: draws::DrawState::default(), shots_skipped: BTreeSet::new(), quads_in: BTreeMap::new() };
	let started = match spec.trim().strip_prefix("mirror") {
		Some(rest) => match rest.strip_prefix(':').map(str::parse::<f64>) {
			None if rest.is_empty() => Ok(mp(Some(MIRROR_DELAY), None)),
			Some(Ok(d)) if d >= 1.0 => Ok(mp(Some(d), None)),
			_ => Err(format!("bad mirror delay {rest:?}")),
		},
		None => open(&spec, &name, hz).and_then(|(s, host)| {
			let what = if s.is_host() { "hosting" } else { "joining" };
			eprintln!("mp: {what} as {name:?} at {hz} ticks a second, local address {:?}", s.local_addr().ok());
			// A host's title says what to do (`title_hint`); a client's says where it is joining until it is in.
			if !s.is_host() {
				notice::status(Some(format!("Multiplayer: joining {} ...", spec.trim().trim_start_matches("join:"))), false);
			}
			let my_id = s.is_host().then_some(0);
			let again = host.map(|h| (h, name.clone(), hz));
			std::thread::Builder::new().name("mp-net".into()).spawn(move || run_net(s, again)).map_err(|e| e.to_string())?;
			Ok(mp(None, my_id))
		}),
	};
	match started {
		Ok(m) => {
			if let Some(d) = m.mirror {
				eprintln!("mp: offline mirror, a puppet {d} fields behind");
			}
			*lock() = Some(m);
			ON.store(true, Ordering::Relaxed);
		}
		Err(e) => {
			eprintln!("mp: RAC_MP={spec:?} not started: {e}");
			notice::say(format!("Multiplayer could not start: {e}"), 30);
			FAILED.store(true, Ordering::Relaxed);
		}
	}
}

/// The game is quitting (the window closed, or a test run ended): the others are told at once, so this player leaves their games
/// now instead of standing still until they give him up as silent. Waits a moment for the network thread to say it.
pub fn leave() {
	if !NET.load(Ordering::Relaxed) || QUIT.swap(true, Ordering::Relaxed) {
		return;
	}
	let end = Instant::now() + LEAVE_WAIT;
	while !SAID_BYE.load(Ordering::Relaxed) && Instant::now() < end {
		std::thread::sleep(Duration::from_millis(1));
	}
}

fn wire() -> MutexGuard<'static, Wire> {
	WIRE.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock() -> MutexGuard<'static, Option<Mp>> {
	MP.lock().unwrap_or_else(|e| e.into_inner())
}

/// `retail/worker/field_front.rs`, once per field: whether the front end is running (there is no level to be in).
pub fn note_front(front: bool) {
	FRONT.store(front, Ordering::Relaxed);
}

/// [AP] The front end is up (title, menus before a level).
pub fn in_front() -> bool {
	FRONT.load(Ordering::Relaxed)
}

/// [AP] RAC_MP is set: story.rs then has the hands at `item.give` and `planet.unlock` (ap.rs asks through them).
pub fn wanted_now() -> bool {
	wanted()
}

/// [AP] story.rs is giving the player what the team got (not the player's own find).
pub fn story_replaying() -> bool {
	story::replaying()
}

/// [AP] A hero state that is his death.
pub fn death_state(state: i32) -> bool {
	DEATH_STATES.contains(&state)
}

/// After each field the EE ran, on the worker thread. Changes the game only to remove what the shared world says is gone.
pub fn tick(c: &mut Cpu) {
	if !ON.load(Ordering::Relaxed) {
		if FAILED.load(Ordering::Relaxed) {
			draw_lines(&mut Vec::new(), &mut Vec::new(), true, notice::TOP);
		}
		return;
	}
	let mut g = lock();
	let Some(mp) = g.as_mut() else { return };
	let me = hero_field(c, mp);
	for ev in exchange(mp, me) {
		events::on_event(c, mp, &me, ev);
	}
	if mp.mirror.is_none() {
		play_field(c, mp, &me);
	}
	show_field(c, mp);
}

/// The field begins: the local hero read, and whether he dies, falls or was respawned a moment ago (`Mp::held`).
fn hero_field(c: &Cpu, mp: &mut Mp) -> HeroState {
	mp.fields += 1;
	RECENT_FIELD.store(mp.fields, Ordering::Relaxed);
	let me = read_hero(&c.mem.ram, mp.fields as u32);
	mp.level = me.level;
	mp.mode = me.mode;
	// In play only: on the title and through a level load the health word reads 0.
	let dies = me.level >= 0 && me.mode == 0 && dying(&me);
	mp.respawn = if dies { RESPAWN_HOLD } else { mp.respawn.saturating_sub(1) };
	let was_held = mp.held;
	mp.held = dies || mp.respawn > 0;
	if mp.held != was_held && mp.my_id.is_some() {
		if mp.held {
			eprintln!("mp: field {}: the hero dies or falls (state {:#x}, health {}): the game is left alone till after the respawn ({} stand-ins, {} voices go)", mp.fields, me.state, me.health, mp.stand_ins.len(), mp.voices.len());
		} else {
			eprintln!("mp: field {}: respawned and settled at ({:.1}, {:.1}, {:.1})", mp.fields, me.pos[0], me.pos[1], me.pos[2]);
		}
	}
	// +0x32 is the high half of the word at +0x30.
	mp.draw = hero_moby(&c.mem.ram).map_or(0, |a| (rd(&c.mem.ram, a + 0x30) >> 16) as i16);
	me
}

/// The local hero goes to the session (offline: to the mirror's track), and what the session reported since comes back.
fn exchange(mp: &mut Mp, me: HeroState) -> Vec<Event> {
	if mp.mirror.is_some() {
		mp.tracks.entry(1).or_default().push(me);
		Vec::new()
	} else {
		let mut w = wire();
		if mp.fields % SEND_EVERY == 0 {
			w.out = Some(me);
		}
		std::mem::take(&mut w.events)
	}
}

/// A field of a session (not the offline mirror): the level told, the host followed, then the shared world, spawned objects,
/// enemies, machinery, what the team keeps, the story, scenes, and the others' sounds and effects, in that order.
fn play_field(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if me.level >= 0 && me.level != mp.announced {
		mp.announced = me.level;
		wire().level = Some(me.level as u8);
		// A client that came here by itself (not after the host, and the host is elsewhere) says so: the host's game follows.
		if mp.my_id != Some(0) && mp.follow.is_none() && mp.host_level.is_some_and(|h| h as i8 != me.level) {
			eprintln!("mp: here in level {} ahead of the host: it is told", me.level);
			wire().went = Some(me.level as u8);
		}
	}
	follow_host(c, mp, me);
	world_tick(c, mp, me);
	arrive::tick(c, mp, me);
	testaid::tick(c, mp, me);
	spawned::hand_over(mp, me);
	spawned::tick(c, mp, me);
	if me.mode == 0 {
		// Where this level keeps its hit queue, read from its code in play (logged once a level), before a blow needs it.
		let _ = enemies::hit_ring(c);
	}
	enemy_tick(c, mp, me);
	machines::tick(c, mp, me);
	keep::tick(c, mp, me.mode == 0 && me.level >= 0 && !mp.held);
	story::tick(c, mp, me.mode == 0 && !mp.held);
	scenes::tick(c, mp, me.level, me.mode == 0 && !mp.held);
	fx_tick(c, mp, me);
}

/// The field ends: the others as shown this field, the session's lines and the names over the puppets, and now and then where
/// everyone is, in the run log.
fn show_field(c: &Cpu, mp: &mut Mp) {
	let delay = mp.mirror.unwrap_or(DELAY);
	mp.shown = mp.tracks.iter_mut().filter_map(|(id, t)| Some((*id, t.sample(delay)?))).collect();
	let roster = wire().roster.clone();
	notice::roster(&roster);
	// On the front end the lines show on the title and its menus only (front mode 0 or 3), not over its text screens and boxes.
	let front = FRONT.load(Ordering::Relaxed);
	let menu = front && matches!(rd(&c.mem.ram, crate::retail::title_boot::front_mode()), 0 | 3);
	notice::hint(if menu { title_hint(mp) } else { None });
	draw_names(c, mp, &roster, if front { Some(menu) } else { None });
	if mp.fields % LOG_EVERY == 0 {
		eprintln!("mp: enemy updates mirrored {}, hits sent on {} ({} scored here by a mod), enemies run elsewhere {}, spawned here {}, stand-ins {}, sounds and effects sent {} {:?}, made for others {} {:?}, machinery shown {}, arcs sent {} and shown {}, particles sent {} and put {} (not {:?}), draws sent {} and shown {}, weapon quads sent {} and drawn {}", enemies::MIRRORED.load(Ordering::Relaxed), enemies::FORWARDED.load(Ordering::Relaxed), enemies::SCORED.load(Ordering::Relaxed), mp.poses.len(), mp.spawned.len(), mp.stand_ins.len(), fx::SENT.load(Ordering::Relaxed), fx::kinds(&fx::SENT_KINDS), fx::PLAYED.load(Ordering::Relaxed), fx::kinds(&fx::PLAYED_KINDS), machines::shown(mp), arcs::SENT.load(Ordering::Relaxed), arcs::SET.load(Ordering::Relaxed), particles::SENT.load(Ordering::Relaxed), particles::PUT.load(Ordering::Relaxed), particles::NOT_PUT.iter().map(|a| a.load(Ordering::Relaxed)).collect::<Vec<_>>(), draws::SENT.load(Ordering::Relaxed), draws::SHOWN.load(Ordering::Relaxed), QUADS_SENT.load(Ordering::Relaxed), QUADS_SHOWN.load(Ordering::Relaxed));
		for (id, h) in &mp.shown {
			let who = roster.iter().find(|(i, _)| i == id).map_or("mirror", |(_, n)| n.as_str());
			let parts: Vec<u16> = h.parts.iter().map(|p| p.class).take_while(|&c| c != 0).collect();
			eprintln!("mp: {who} (player {id}) level {} at ({:.1}, {:.1}, {:.1}) seq {} health {} parts {parts:?}", h.level, h.pos[0], h.pos[1], h.pos[2], h.anim.new_seq, h.health);
		}
	}
}

/// Who runs this enemy: the host, unless it gave it away.
fn owner_of(mp: &Mp, key: &WorldKey) -> PlayerId {
	// A shot is its game's, which named it (rac_mp::shot_id).
	rac_mp::shot_owner(key.slot).unwrap_or_else(|| mp.owners.get(key).copied().unwrap_or(0))
}

fn read_clock(ram: &[u8], a: u32) -> AnimClock {
	AnimClock { cur: rd8(ram, a + 0x50), next: rd8(ram, a + 0x51), old_seq: rd8(ram, a + 0x52), new_seq: rd8(ram, a + 0x53), p: rd_f(ram, a + 0x54), m: rd_f(ram, a + 0x58), r: rd_f(ram, a + 0x5c) }
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
	(0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// The enemy at `a`, as a pose for the stream.
fn read_pose(ram: &[u8], a: u32, slot: u16, class: u16) -> EnemyPose {
	let v3 = |a: u32| [rd_f(ram, a), rd_f(ram, a + 4), rd_f(ram, a + 8)];
	let pvar = rd(ram, a + 0x78) & MASK;
	EnemyPose { slot, class, pos: v3(a + 0x10), rot: v3(a + 0x40), anim: read_clock(ram, a), health: if pvar != 0 { rd_f(ram, pvar + 0x20) } else { 0.0 }, scale: rd_f(ram, a + 0x2c), state: rd8(ram, a + 0x20), alpha: rd8(ram, a + 0x23), ovr: (rd(ram, a + 0x34) & 0x10 != 0).then(|| rd(ram, a + 0x90)) }
}

/// RAC_MP_FOLLOW=0 turns following the host off.
fn follows() -> bool {
	std::env::var("RAC_MP_FOLLOW").map_or(true, |v| v != "0")
}

/// A client whose host is in another level, or a host one of whose clients went to another by itself, asks the game to go there, in
/// play only, by the request the game's own level ends make (retail/worker/field_run.rs writes the same three words). The game then
/// runs its own trip.
fn follow_host(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let Some(to) = mp.follow else { return };
	if me.level == to as i8 {
		mp.follow = None;
		return;
	}
	if me.level >= 0 && me.mode == 0 && !mp.held && mp.fields >= mp.next_follow && !disc::extras_off() {
		if mp.next_follow == 0 {
			let who = if mp.my_id == Some(0) { "the team".to_string() } else { notice::name_of(0) };
			notice::say(format!("Taking you to {who} on {} ...", level_name(to)), 8);
		}
		mp.next_follow = mp.fields + FOLLOW_EVERY;
		eprintln!("mp: following the {} to level {to}", if mp.my_id == Some(0) { "team" } else { "host" });
		c.mem.wr32(disc::va(0x15_F5A0u32), to as u32);
		c.mem.wr32(disc::va(0x15_F5B8u32), 1);
		c.mem.wr32(disc::va(EXIT_WORD), 1);
	}
}

/// What to do next on the title screen (mp/notice.rs): the host starts or loads a game, a client does too (or waits while its game
/// is started for it: the setup window's Join does that for a player with no saves, through the title pilot).
fn title_hint(mp: &Mp) -> Option<String> {
	static STARTING: OnceLock<bool> = OnceLock::new();
	let starting = *STARTING.get_or_init(|| std::env::var_os("RAC_TITLE_PILOT").is_some());
	match mp.my_id {
		Some(0) => Some("You are hosting. Choose New Game or Load Game: friends who join come to your planet.".into()),
		Some(_) if starting => Some(format!("Connected to {}. Starting a new game to join them ...", notice::name_of(0))),
		Some(_) => Some(format!("Connected to {}. Choose Load Game or New Game: you then go to their planet.", notice::name_of(0))),
		None => None,
	}
}

/// A level's planet name, for the session's lines.
fn level_name(level: u8) -> &'static str {
	const NAMES: [&str; 20] = ["Veldin", "Novalis", "Aridia", "Kerwan", "Eudora", "Rilgar", "Blarg", "Umbris", "Batalia", "Gaspar", "Orxon", "Pokitaru", "Hoven", "Gemlik", "Oltanis", "Quartu", "Kalebo III", "Drek's Fleet", "Veldin", "the ship"];
	NAMES.get(level as usize).copied().unwrap_or("another planet")
}

/// The last things multiplayer did in the game (made, removed, hit, replayed), with the field: said in the run log when the game
/// stops (`dump_recent`), so a stop can be put down to what came before it.
static RECENT: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
const RECENT_KEEP: usize = 48;
static RECENT_FIELD: AtomicU64 = AtomicU64::new(0);

fn note(what: String) {
	let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
	if r.len() == RECENT_KEEP {
		r.pop_front();
	}
	r.push_back((RECENT_FIELD.load(Ordering::Relaxed), what));
}

/// The game stopped (retail/tick.rs): what multiplayer last did in it goes to the run log.
pub fn dump_recent() {
	let r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
	if r.is_empty() {
		return;
	}
	eprintln!("mp: the last {} things multiplayer did in the game before it stopped (field, what):", r.len());
	for (f, what) in r.iter() {
		eprintln!("mp:   {f} {what}");
	}
}

/// Whether the local hero is dying (a death or a fall), on his way to a respawn.
fn dying(me: &HeroState) -> bool {
	me.health <= 0 || DEATH_STATES.contains(&me.state)
}

/// Whether a player in game mode `mode` stands in the world for the others: in play, or paused, in a dialog or shopping, where his
/// hero waits where he is. Not in a movie, a scene, a ship or a sprite screen (mode 0 gameplay, 1 movie, 2 scene, 3 pause, 4 dialog,
/// 5 vendor, 6 ship, 7 sprite screen).
fn on_stage(mode: u8) -> bool {
	matches!(mode, 0 | 3 | 4 | 5)
}

/// The names above the puppets, as a text list for the last 2D layer (omni/draw.rs `submit_mp`). A list every field, an empty one
/// with nobody to name, so a name leaves with its player. Only in play (mode 0): a scene or a menu has the screen to itself.
fn draw_names(c: &Cpu, mp: &Mp, roster: &[(PlayerId, String)], front: Option<bool>) {
	let (mut cmds, mut pool) = (Vec::new(), Vec::new());
	for (id, h) in mp.shown.iter().filter(|(_, h)| h.level == mp.level && mp.level >= 0 && mp.mode == 0 && on_stage(h.mode)) {
		let name = roster.iter().find(|(i, _)| i == id).map_or("Mirror", |(_, n)| n.as_str());
		let name = format!("{name}{}", match h.mode {
			3 => " (paused)",
			4 => " (dialog)",
			5 => " (shopping)",
			_ => "",
		});
		let head = [h.pos[0], h.pos[1], h.pos[2] + NAME_HEIGHT];
		let Some(([x, y], true)) = crate::omni::screen_of(c, head) else { continue };
		let text_off = pool.len() as u32;
		pool.extend_from_slice(name.as_bytes());
		cmds.push(crate::omni::OmniDrawCmd { kind: crate::omni::draw_quads::KIND_TEXT, layer: 0, align: 1, mod_index: 0, x, y: y - NAME_SIZE * crate::omni::draw_quads::LINE, w: NAME_SIZE, h: 0.0, color: 0xFFFF_FFFF, color2: 0x0000_00C0, value: 0.0, icon: 0, text_off, text_len: name.len() as u32, reserved0: 0, reserved1: 0 });
	}
	// The session's lines (mp/notice.rs): on the front end's title and menus (`front`: Some(on one of them)), below its menu, clear of
	// the logo; in the game at the top, but not over a movie or a scene.
	match front {
		Some(menu) => draw_lines(&mut cmds, &mut pool, menu, notice::TOP_FRONT),
		None => draw_lines(&mut cmds, &mut pool, !matches!(mp.mode, 1 | 2), notice::TOP),
	}
}

/// Adds the session's lines (mp/notice.rs) at the top of the screen, centred, when `show`, and hands the list over.
fn draw_lines(cmds: &mut Vec<crate::omni::OmniDrawCmd>, pool: &mut Vec<u8>, show: bool, top: f32) {
	let step = notice::SIZE * crate::omni::draw_quads::LINE * 1.2;
	for (i, (text, warn)) in notice::lines(show).into_iter().enumerate() {
		let text_off = pool.len() as u32;
		pool.extend_from_slice(text.as_bytes());
		let color = if warn { notice::YELLOW } else { notice::WHITE };
		cmds.push(crate::omni::OmniDrawCmd { kind: crate::omni::draw_quads::KIND_TEXT, layer: 0, align: 1, mod_index: 0, x: 256.0, y: top + step * i as f32, w: notice::SIZE, h: 0.0, color, color2: notice::SHADOW, value: 0.0, icon: 0, text_off, text_len: text.len() as u32, reserved0: 0, reserved1: 0 });
	}
	crate::omni::draw::submit_mp(cmds, pool);
}

/// retail/snapshot.rs, when a field's snapshot is taken: the other players in the local hero's level, as class-0 mobys.
pub fn puppets() -> Vec<Moby> {
	if !ON.load(Ordering::Relaxed) {
		return Vec::new();
	}
	let g = lock();
	let Some(mp) = g.as_ref().filter(|mp| mp.level >= 0) else { return Vec::new() };
	let mut out = Vec::new();
	// Nobody walks through a scene of ours, and a player in a scene of theirs is not here.
	if !on_stage(mp.mode) {
		return out;
	}
	for (id, h) in mp.shown.iter().filter(|(_, h)| h.level == mp.level && on_stage(h.mode)).take(PUPPETS as usize) {
		let addr = PUPPET_ADDR + *id as u32 * 0x100;
		// Clank on his own, Giant Clank: his moby's class (a level without its model draws none).
		out.push(puppet_moby(addr, h.body as u32, h.pos, h.rot, 1.0, &h.anim, h, mp.draw));
		for (k, p) in h.parts.iter().take_while(|p| p.class != 0).enumerate() {
			let pos = [h.pos[0] + p.offset[0], h.pos[1] + p.offset[1], h.pos[2] + p.offset[2]];
			let class = if p.class as u32 == crate::mobies::WRENCH_CLASS { PUPPET_WRENCH } else { p.class as u32 };
			out.push(puppet_moby(addr + 1 + k as u32, class, pos, p.rot, p.scale, &p.anim, h, mp.draw));
		}
	}
	out
}

/// One moby of a puppet, lit as his hero `h`.
fn puppet_moby(addr: u32, class: u32, pos: [f32; 3], rot: [f32; 3], scale: f32, a: &AnimClock, h: &HeroState, draw: i16) -> Moby {
	Moby {
		addr,
		class,
		pos,
		rot,
		mirror: false,
		scale,
		shown: true,
		side_only: None,
		view_culled: Culled::No,
		#[cfg(test)]
		culls: None,
		#[cfg(test)]
		raw: (0, 0, 0),
		rgb: h.rgb,
		light: h.light,
		clock: Clock { cur: a.cur, next: a.next, old_seq: a.old_seq, new_seq: a.new_seq, p: a.p, m: a.m, r: a.r },
		fx: Vec::new(),
		alpha: 0x80,
		// A puppet is Ratchet's body and wrench: never drawn additively (flag +0x34 bit 0x200 is not his).
		additive: false,
		draw,
		ovr: None,
	}
}

fn rd(ram: &[u8], a: u32) -> u32 {
	let i = (a & MASK) as usize;
	ram.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_f(ram: &[u8], a: u32) -> f32 {
	Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0)
}

fn rd8(ram: &[u8], a: u32) -> u8 {
	ram.get((a & MASK) as usize).copied().unwrap_or(0)
}

/// The hero's moby, when the pointer holds one.
fn hero_moby(ram: &[u8]) -> Option<u32> {
	Some(rd(ram, HERO_MOBY) & MASK).filter(|&a| a != 0 && rd(ram, a + 0x24) != 0)
}

/// The local hero out of the machine's RAM (all 32 MB).
fn read_hero(ram: &[u8], tick: u32) -> HeroState {
	// The PS2 has no NaN or infinity: a word that reads as one here is sent as 0, since the others refuse non-finite floats.
	let f = |a: u32| Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0);
	let v3 = |a: u32| [f(a), f(a + 4), f(a + 8)];
	let mut h = HeroState {
		tick,
		level: level(ram),
		mode: rd(ram, disc::va(MODE_WORD)).min(255) as u8,
		pos: v3(HERO + 0x80),
		state: rd(ram, HERO + 0x2084) as i32,
		health: (rd(ram, HEALTH) as i32).clamp(0, 127) as i8,
		item: rd(ram, ITEM) as i32,
		..Default::default()
	};
	if let Some(m) = hero_moby(ram) {
		h.pos = v3(m + 0x10);
		h.rot = v3(m + 0x40);
		h.anim = AnimClock { cur: rd8(ram, m + 0x50), next: rd8(ram, m + 0x51), old_seq: rd8(ram, m + 0x52), new_seq: rd8(ram, m + 0x53), p: f(m + 0x54), m: f(m + 0x58), r: f(m + 0x5c) };
		h.rgb = [rd8(ram, m + 0x3c), rd8(ram, m + 0x3d), rd8(ram, m + 0x3e)];
		h.light = rd(ram, m + 0x38);
		h.body = (rd(ram, m + 0xa4) >> 16) as u16;
		// The game places the parts by the joints of his matrix (rows +0xc0), which with the modern camera lag the drawn yaw
		// +0x48 the puppet takes: they turn about him by the difference, as retail/strafe.rs `turn_parts` does for the local hero.
		let r0 = v3(m + 0xc0);
		let turn = if r0[0] != 0.0 || r0[1] != 0.0 { h.rot[2] - r0[1].fatan2(r0[0]) } else { 0.0 };
		let mut n = 0;
		for cell in PART_CELLS {
			if let Some(p) = read_part(ram, rd(ram, cell) & MASK, h.pos, turn) {
				h.parts[n] = p;
				n += 1;
			}
		}
	}
	h
}

/// The part in the moby at `a`: none when the cell is empty, the slot is free or the game hides it.
fn read_part(ram: &[u8], a: u32, hero: [f32; 3], turn: f32) -> Option<Part> {
	let f = |a: u32| Some(f32::from_bits(rd(ram, a))).filter(|v| v.is_finite()).unwrap_or(0.0);
	let v3 = |a: u32| [f(a), f(a + 4), f(a + 8)];
	let header = rd(ram, a + 0x24) & MASK;
	let class = (rd(ram, a + 0xa4) >> 16) as u16;
	if a == 0 || header == 0 || class == 0 || rd8(ram, a + 0x20) >= 0xfd || rd(ram, a + 0x34) & 0x81 != 0 {
		return None;
	}
	let unit = |v: [f32; 3]| {
		let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
		if l > 0.0 { v.map(|x| x / l) } else { v }
	};
	let (r0, r1, r2) = (unit(v3(a + 0xc0)), unit(v3(a + 0xd0)), unit(v3(a + 0xe0)));
	let mut rot = if r0 == [0.0; 3] { v3(a + 0x40) } else { euler_from_rows(r0, r1, r2) };
	rot[2] += turn;
	let pos = v3(a + 0x10);
	let (sn, cs) = turn.fsin_cos();
	let (x, y) = (pos[0] - hero[0], pos[1] - hero[1]);
	let base = f(header + 0x24);
	let part = Part {
		class,
		offset: [cs * x - sn * y, sn * x + cs * y, pos[2] - hero[2]],
		rot,
		scale: if base != 0.0 { f(a + 0x2c) / base } else { 1.0 },
		anim: AnimClock { cur: rd8(ram, a + 0x50), next: rd8(ram, a + 0x51), old_seq: rd8(ram, a + 0x52), new_seq: rd8(ram, a + 0x53), p: f(a + 0x54), m: f(a + 0x58), r: f(a + 0x5c) },
	};
	// A part the game has not set up yet is not his: on Novalis' arrival one went out with a NaN (every state of that stretch
	// undecodable) and, sent as 0, with a garbage turn the host's angle wrap never finished.
	let sane = |v: &f32| v.is_finite() && v.abs() < PART_SANE;
	(part.offset.iter().chain(&part.rot).all(sane) && sane(&part.scale)).then_some(part)
}

/// The level running (0..=18), or -1: title screen, level loop ending, or no level word.
fn level(ram: &[u8]) -> i8 {
	let l = rd(ram, LEVEL_WORD) as i32;
	if FRONT.load(Ordering::Relaxed) || rd(ram, disc::va(EXIT_WORD)) != 0 || !(0..=18).contains(&l) {
		return -1;
	}
	l as i8
}
