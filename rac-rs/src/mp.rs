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
//! Synced enemies (`enemy_tick`, mp/enemies.rs): the host gives each enemy to the game of the player nearest to it. That game runs it
//! and streams it, the others mirror it, and a hit from another game travels to it.
//!
//! Sounds and effects (mp/fx.rs): the sound cues and explosions a game makes for what it runs are made again on the others' copies.
//!
//! Spawned objects (mp/spawned.rs): each game reports what it makes during play, the host gives it an id, and the others show it as a
//! stand-in, or wake a copy of their own when the game running it is gone.
//!
//! The shared world (`world_tick`): each game watches the objects the level placed that matter (enemies, crates, pickups) in the
//! moby table, where a placed object keeps its slot. One that goes from a player's game near that player is reported; the host
//! takes the first report and tells the others, and each game removes it with the game's own `delete_moby`, again if it comes
//! back (a player's death restores a checkpoint) or if the player arrives later. This is the only thing that changes a game's
//! RAM, and only when another player removed something. The rewards stay with the player who earned them.
//!
//! Staying in step: the team travels together. Clients follow the host to another level (`follow_host`), by the request the game's
//! own level ends make, and when a client takes a ship by itself the host's game goes after it, and the rest after the host. A
//! player in a scene, a movie, a ship or a sprite screen is not drawn, and nobody is drawn over a local scene (`on_stage`). A
//! name says when its player is paused, in a dialog or shopping. A client that lost the host keeps trying to get back in
//! (`run_net`), and tells the host again what it knows is gone.
//!
//! Threads: `init` on the main thread in `Retail::load` (after `omni::init`); `note_front`, `tick` and `puppets` on the retail
//! worker thread: `tick` after each field the EE ran (after `omni::tick`), `puppets` when that field's snapshot is taken. The
//! session runs on a thread of its own ("mp-net", `run_net`): the game does not tick in parts of the front end or while a
//! level loads, and the players must keep answering each other meanwhile. `WIRE` is what the two hand each other.
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
mod fx;
mod ids;
mod keep;
mod machines;
mod notice;
mod particles;
mod scenes;
mod spawned;
mod story;
mod testaid;
mod touch;

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

/// The network thread: sends what the game left in `WIRE`, polls the session every few milliseconds whether the game ticks or
/// not, and leaves what came in for the game. A client that lost the host (`again`: the host, the name, the tick rate) keeps
/// joining again; one the host refused stops.
fn run_net(mut s: Session, again: Option<(SocketAddr, String, u8)>) {
	// Whether this game ever got in: a host that never answered is most often one its firewall keeps out.
	let mut ever_in = s.is_host();
	NET.store(true, Ordering::Relaxed);
	loop {
		if QUIT.load(Ordering::Relaxed) {
			s.close();
			eprintln!("mp: at {} leaving: the others are told", clock());
			SAID_BYE.store(true, Ordering::Relaxed);
			return;
		}
		let t0 = Instant::now();
		let touches = std::mem::take(&mut wire().touches);
		let story = std::mem::take(&mut wire().story);
		story.into_iter().for_each(|p| s.progress(p));
		let scenes = std::mem::take(&mut wire().scenes);
		scenes.into_iter().for_each(|(level, id)| s.send_scene(level, id));
		let vars = std::mem::take(&mut wire().vars);
		vars.into_iter().for_each(|(key, state, words)| s.send_vars(key, state, &words));
		touches.into_iter().for_each(|(key, damage, mask)| s.touch(key, damage, mask));
		let (keep, remembered, arcs) = {
			let mut w = wire();
			(w.keep.take(), std::mem::take(&mut w.remembered), w.arcs.take())
		};
		keep.into_iter().for_each(|k| s.keep(k));
		remembered.into_iter().for_each(|(name, k)| s.remembered(name, k));
		if let Some((level, tick, arcs)) = arcs {
			s.send_arcs(level, tick, &arcs);
		}
		let (particles, draws, quads) = {
			let mut w = wire();
			(w.particles.take(), w.draws.take(), w.quads.take())
		};
		if let Some((level, tick, rope, qs)) = quads {
			s.send_quads(level, tick, rope, &qs);
		}
		if let Some((level, ps)) = particles {
			s.send_particles(level, &ps);
		}
		if let Some((level, tick, ds)) = draws {
			ds.iter().for_each(|d| s.send_draws(level, tick, d));
		}
		let (out, gone, level, foes, hits, assign, spawns, fx, machines) = {
			let mut w = wire();
			(w.out.take(), std::mem::take(&mut w.gone), w.level.take(), w.enemies.take(), std::mem::take(&mut w.hits), std::mem::take(&mut w.assign), std::mem::take(&mut w.spawns), w.fx.take(), w.machines.take())
		};
		if let Some(h) = out {
			s.send_state(&h);
		}
		gone.into_iter().for_each(|k| s.report(k));
		std::mem::take(&mut wire().back).into_iter().for_each(|k| s.back(k));
		level.into_iter().for_each(|l| s.travel(l));
		let went = wire().went.take();
		went.into_iter().for_each(|l| s.went(l));
		spawns.into_iter().for_each(|(local, level, class, pos, lasting, solo)| s.report_spawn(local, level, class, pos, lasting, solo, Instant::now()));
		if let Some((level, events)) = fx {
			s.send_fx(level, &events);
		}
		if let Some((level, tick, poses)) = foes {
			s.send_enemies(level, tick, &poses);
		}
		if let Some((level, tick, poses)) = machines {
			s.send_machines(level, tick, &poses);
		}
		hits.into_iter().for_each(|(k, damage, mask)| s.send_hit(k, damage, mask));
		assign.into_iter().for_each(|(k, owner)| s.assign(k, owner));
		let t1 = Instant::now();
		let events = s.poll(t1);
		let t2 = Instant::now();
		// Said here, as it happens (the game may be loading and not read the events for a while), with the time of day: a drop
		// on one PC is matched with the other's log.
		for e in &events {
			let st = s.stats();
			let read = format!("(read {}, undecodable {} (last {:?}), from strangers {})", st.read, st.undecodable, st.last_undecodable, st.from_strangers);
			match e {
				Event::Lost => eprintln!("mp: at {} the host was given up, silent for {:.1} s (0: it said this game is not in) {read}", clock(), s.silent_for(t2).as_secs_f32()),
				Event::Left { id } => eprintln!("mp: at {} player {id} left or went silent {read}", clock()),
				Event::Joined { id, .. } | Event::Connected { id } => eprintln!("mp: at {} player {id} in {read}", clock()),
				Event::Stranger(a) => eprintln!("mp: at {} packets from {a}, which is not a player here: told it is not in {read}", clock()),
				_ => {}
			}
		}
		let refused = events.iter().any(|e| matches!(e, Event::Rejected(_)));
		ever_in |= events.iter().any(|e| matches!(e, Event::Connected { .. }));
		{
			let mut w = wire();
			w.events.extend(events);
			if w.events.len() > WIRE_KEEP {
				// Nobody read them for a long while: the hero states are stale, the joins and leaves still count.
				w.events.retain(|e| !matches!(e, Event::State { .. } | Event::Enemies { .. } | Event::Machines { .. } | Event::Fx { .. }));
			}
			if w.roster.as_slice() != s.roster() {
				w.roster = s.roster().to_vec();
			}
		}
		let t3 = Instant::now();
		if t3 - t0 >= HELD_UP {
			let secs = |a: Instant, b: Instant| (b - a).as_secs_f32();
			eprintln!("mp: the network thread was held up {:.1} s (handing over and sending {:.1} s, reading {:.1} s, handing back {:.1} s)", secs(t0, t3), secs(t0, t1), secs(t1, t2), secs(t2, t3));
		}
		if s.is_over() {
			let Some((host, name, hz)) = again.as_ref().filter(|_| !refused) else { return };
			if !ever_in {
				eprintln!("mp: no answer from {host} in a minute: is a game hosting there, and do that PC's firewall (and its router, over the internet) let UDP port {} in? Trying again", host.port());
				notice::status(Some(format!("No answer from {host}. Is your friend's game hosting? Is Tailscale on for both of you? Still trying ...")), true);
			}
			std::thread::sleep(REJOIN_EVERY);
			if let Ok(n) = Session::join(*host, name, *hz, Instant::now()) {
				s = n;
			}
		}
		let before = Instant::now();
		std::thread::sleep(Duration::from_millis(2));
		let slept = before.elapsed();
		if slept >= HELD_UP {
			eprintln!("mp: the network thread did not run for {:.1} s (this machine was too busy or stalled)", slept.as_secs_f32());
		}
	}
}

/// The time of day (UTC) as HH:MM:SS.mmm, for the network lines of two PCs' logs to be put side by side.
fn clock() -> String {
	let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 86_400_000;
	format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// The session RAC_MP asks for, and the host's address when it is a join.
fn open(spec: &str, name: &str, hz: u8) -> Result<(Session, Option<SocketAddr>), String> {
	let spec = spec.trim();
	if spec == "host" || spec.starts_with("host:") {
		// host, host:PORT (every network), or host:ADDR:PORT (one address: 127.0.0.1 keeps it to this PC).
		let bind: SocketAddr = match spec.strip_prefix("host:") {
			None => ([0, 0, 0, 0], rac_mp::DEFAULT_PORT).into(),
			Some(p) if !p.contains(':') => ([0, 0, 0, 0], p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?).into(),
			Some(a) => a.parse().map_err(|_| format!("bad address {a:?}"))?,
		};
		return Session::host(bind, name, hz).map(|s| (s, None)).map_err(|e| e.to_string());
	}
	if let Some(at) = spec.strip_prefix("join:") {
		let at = if at.contains(':') { at.to_string() } else { format!("{at}:{}", rac_mp::DEFAULT_PORT) };
		// The host listens on IPv4 (0.0.0.0): a name with both kinds of address (Tailscale's MagicDNS names have both) is joined at
		// its IPv4 one, whichever the system lists first.
		let mut addrs: Vec<SocketAddr> = at.to_socket_addrs().map_err(|e| e.to_string())?.collect();
		addrs.sort_by_key(|a| !a.is_ipv4());
		let addr = addrs.first().copied().ok_or_else(|| format!("no address for {at:?}"))?;
		return Session::join(addr, name, hz, Instant::now()).map(|s| (s, Some(addr))).map_err(|e| e.to_string());
	}
	Err("expected host, host:PORT, host:ADDR:PORT, join:ADDR, join:ADDR:PORT, mirror or mirror:FIELDS".into())
}

fn lock() -> MutexGuard<'static, Option<Mp>> {
	MP.lock().unwrap_or_else(|e| e.into_inner())
}

/// `retail/worker/field_front.rs`, once per field: whether the front end is running (there is no level to be in).
pub fn note_front(front: bool) {
	FRONT.store(front, Ordering::Relaxed);
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
	let events = if mp.mirror.is_some() {
		mp.tracks.entry(1).or_default().push(me);
		Vec::new()
	} else {
		let mut w = wire();
		if mp.fields % SEND_EVERY == 0 {
			w.out = Some(me);
		}
		std::mem::take(&mut w.events)
	};
	for ev in events {
		match ev {
			Event::State { id, hero } => mp.tracks.entry(id).or_default().push(hero),
			Event::Joined { id, name } => {
				eprintln!("mp: {name:?} joined as player {id}");
				// The host a client joined is named by the title's line (`title_hint`); everyone else is announced.
				if id != 0 || mp.my_id == Some(0) {
					notice::say(format!("{name} joined"), 6);
				}
			}
			Event::Left { id } => {
				mp.tracks.remove(&id);
				eprintln!("mp: player {id} left");
				notice::say(format!("{} left", notice::name_of(id)), 6);
			}
			Event::Connected { id } => {
				eprintln!("mp: in, as player {id}");
				notice::status(None, false);
				notice::say(if mp.my_id.is_some() { "Back in with the host." } else { "Connected." }, 6);
				mp.my_id = Some(id);
				// Back after losing the host, perhaps a new one: it hears again what this game knows is gone. Ids of spawned objects were the old one's.
				mp.removed.retain(|k| k.slot < SPAWNED_ID);
				wire().gone.extend(mp.removed.iter().copied());
				mp.live.clear();
				mp.rejoined = true;
				// The host may be a new one: it hears every player's things this game remembers, and the team's progress (it keeps
				// what it has none of).
				wire().remembered = keep::remembered(mp, &mp.my_name.clone());
				if id != 0 {
					wire().story.extend(mp.story_seen.iter().copied());
				}
				arrive::connected(mp);
			}
			Event::Rejected(why) => {
				eprintln!("mp: the host refused us: {why:?}");
				notice::status(Some(match why {
					rac_mp::RejectReason::Version => "Could not join: your friend's game is another version. Update both games to the same release.",
					rac_mp::RejectReason::Region => "Could not join: your friend's disc is from the other region (a PAL and a USA game can't play together).",
					rac_mp::RejectReason::Full => "Could not join: that game already has 4 players.",
				}.into()), true);
			}
			Event::Stranger(_) => {}
			Event::Lost => {
				mp.tracks.clear();
				mp.owners.clear();
				mp.poses.clear();
				mp.orphaned = true;
				machines::stop(c, mp);
				if mp.my_id.is_some() {
					eprintln!("mp: the host is gone, trying to get back in");
					notice::status(Some("Lost the host. Trying to get back in ...".into()), true);
				}
			}
			Event::Removed(key) => {
				mp.removed.insert(key);
			}
			Event::Back(key) => {
				mp.removed.remove(&key);
			}
			Event::Travel(level) => {
				mp.host_level = Some(level);
				if follows() {
					mp.follow = Some(level);
					mp.next_follow = 0;
				}
			}
			// A team travels together: a client took a ship by itself, and the host's game goes after it. The others then follow the host.
			Event::Went { id, level } if follows() && mp.my_id == Some(0) => {
				eprintln!("mp: player {id} went to level {level}: the team goes too");
				mp.follow = Some(level);
				mp.next_follow = 0;
			}
			Event::Went { .. } => {}
			Event::Owner { key, owner } => {
				mp.owners.insert(key, owner);
				mp.poses.remove(&key.slot);
				if key.slot >= SPAWNED_ID {
					spawned::new_owner(mp, key, owner);
				}
			}
			Event::Hit { key, damage, mask } => mp.hits_in.push((key, damage, mask)),
			Event::Enemies { id, level, tick, poses } if level as i8 == me.level => {
				for p in poses {
					let key = WorldKey { level, slot: p.slot, class: p.class };
					if !mp.removed.contains(&key) && owner_of(mp, &key) == id {
						let t = mp.poses.entry(p.slot).or_default();
						t.track.push(HeroState { tick, level: me.level, pos: p.pos, rot: p.rot, anim: p.anim, ..Default::default() });
						(t.class, t.health, t.scale, t.heard, t.alpha, t.ovr) = (p.class, p.health, p.scale, mp.fields, p.alpha, p.ovr);
					}
				}
			}
			Event::Enemies { .. } => {}
			Event::Spawned { id, owner, local, level, class } => spawned::answered(mp, id, owner, local, level, class),
			Event::Duplicate { local, of } => spawned::copy_of(mp, local, of),
			Event::Fx { id, level, events } if level as i8 == me.level => mp.fx_in.extend(events.into_iter().map(|e| (mp.fields + FX_DELAY, id, e))),
			Event::Fx { .. } => {}
			Event::Touch { key, damage, mask } => mp.touches_in.push((key, damage, mask)),
			Event::Machines { id, level, tick, poses } if level as i8 == me.level => machines::heard(mp, id, tick, poses),
			Event::Machines { .. } => {}
			Event::Vars { key, state, words } => machines::vars_heard(mp, key, state, words),
			Event::Scene { level, id } => scenes::heard(mp, level, id),
			Event::Story(p) => {
				if !mp.story_in.contains(&p) {
					mp.story_in.push(p);
				}
				if !mp.story_seen.contains(&p) {
					mp.story_seen.push(p);
				}
			}
			Event::Keep { name, keep, live } => {
				let me = mp.my_name.clone();
				keep::heard(mp, name, keep, live, &me);
			}
			Event::Arcs { id, level, arcs, .. } if level as i8 == me.level => {
				for a in arcs {
					if owner_of(mp, &WorldKey { level, slot: a.slot, class: mp.poses.get(&a.slot).map_or(0, |t| t.class) }) == id || a.slot >= SPAWNED_ID {
						mp.arcs_in.insert(a.slot, (a, mp.fields));
					}
				}
			}
			Event::Arcs { .. } => {}
			Event::Particles { level, particles, .. } if level as i8 == me.level => {
				mp.particles_in.extend(particles);
				let over = mp.particles_in.len().saturating_sub(PARTICLES_KEEP);
				mp.particles_in.drain(..over);
			}
			Event::Draws { id, level, draws, .. } if level as i8 == me.level => {
				let class = mp.poses.get(&draws.slot).map_or(0, |t| t.class);
				if draws.slot >= SPAWNED_ID || owner_of(mp, &WorldKey { level, slot: draws.slot, class }) == id {
					let now = mp.fields;
					mp.draws.got(draws, now);
				}
			}
			Event::Quads { id, level, tick, part, rope, quads, .. } if level as i8 == me.level => {
				let now = mp.fields;
				let e = mp.quads_in.entry(id).or_insert((now, tick, None, Vec::new()));
				if e.1 != tick {
					*e = (now, tick, None, Vec::new());
				}
				e.0 = now;
				if part == 0 {
					e.2 = rope;
				}
				e.3.extend(quads);
			}
			Event::Particles { .. } | Event::Draws { .. } | Event::Quads { .. } => {}
		}
	}
	if mp.mirror.is_none() {
		if me.level >= 0 && me.level != mp.announced {
			mp.announced = me.level;
			wire().level = Some(me.level as u8);
			// A client that came here by itself (not after the host, and the host is elsewhere) says so: the host's game follows.
			if mp.my_id != Some(0) && mp.follow.is_none() && mp.host_level.is_some_and(|h| h as i8 != me.level) {
				eprintln!("mp: here in level {} ahead of the host: it is told", me.level);
				wire().went = Some(me.level as u8);
			}
		}
		follow_host(c, mp, &me);
		world_tick(c, mp, &me);
		arrive::tick(c, mp, &me);
		testaid::tick(c, mp, &me);
		spawned::hand_over(mp, &me);
		spawned::tick(c, mp, &me);
		if me.mode == 0 {
			// Where this level keeps its hit queue, read from its code in play (logged once a level), before a blow needs it.
			let _ = enemies::hit_ring(c);
		}
		enemy_tick(c, mp, &me);
		machines::tick(c, mp, &me);
		keep::tick(c, mp, me.mode == 0 && me.level >= 0 && !mp.held);
		story::tick(c, mp, me.mode == 0 && !mp.held);
		scenes::tick(c, mp, me.level, me.mode == 0 && !mp.held);
		fx_tick(c, mp, &me);
	}
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

/// One field of synced enemies (mp/enemies.rs): the hits that came and went, who runs what (host), the stream of what this game runs,
/// and the mirror of what other games run.
fn enemy_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let (Some(my), true) = (mp.my_id, me.level >= 0 && mp.table != 0) else {
		enemies::set_driven(BTreeMap::new());
		return;
	};
	let level = me.level as u8;
	// Not while the local hero dies or respawns: the blow is dealt by him (his moby the attacker), and the respawn resets the level.
	let hits: Vec<_> = if mp.held { Vec::new() } else { mp.hits_in.drain(..mp.hits_in.len().min(BLOWS_PER_FIELD)).collect() };
	for (key, damage, mask) in hits {
		let moby = if key.slot >= SPAWNED_ID {
			mp.spawned.iter().find(|(_, s)| s.id == Some(key.slot)).map(|(a, _)| *a)
		} else {
			mp.ids.addr(key.slot).filter(|_| owner_of(mp, &key) == my)
		};
		if let Some(moby) = moby.filter(|_| key.level == level) {
			note(format!("another player's hit on id {:#x} class {} at {moby:#x}", key.slot, key.class));
			let hero = enemies::hero_moby(&c.mem.ram);
			if let Err(e) = enemies::post_hit(c, moby, hero, damage, mask) {
				eprintln!("mp: could not post a hit on slot {}: {e}", key.slot);
			}
		}
	}
	for h in enemies::take_hits() {
		let stand_in = mp.stand_ins.iter().find(|(_, s)| s.moby == h.moby).map(|(id, s)| (*id, s.class));
		let Some((slot, class)) = stand_in.or_else(|| Some((mp.ids.id(h.moby)?, (rd(&c.mem.ram, h.moby + 0xa4) >> 16) as u16))) else { continue };
		// A mod that scores blows (Arsenal Mastery) scores this one here, for this player; what it came to goes to the enemy's game.
		let damage = enemies::resolve_here(c, &h).unwrap_or(h.damage);
		if damage > 0.0 {
			wire().hits.push((WorldKey { level, slot, class }, damage, h.mask));
		}
	}
	let ram = &c.mem.ram[..];
	// The other players here who can run an enemy: in play and alive.
	let others: Vec<(PlayerId, [f32; 3])> = mp.shown.iter().filter(|(_, h)| h.level == me.level && h.mode == 0 && h.health > 0).map(|(id, h)| (*id, h.pos)).collect();
	let foes: Vec<(u16, u16)> = mp.watched.iter().filter(|(_, v)| v.1 == ENEMY).map(|(s, v)| (*s, v.0)).collect();
	// Who gives out the enemies here: the host, or in a level the host isn't in, the first player in it.
	let leader = std::iter::once(my).chain(mp.shown.iter().filter(|(_, h)| h.level == me.level).map(|(id, _)| *id)).min().unwrap_or(my);
	if leader == my && mp.fields % OWN_EVERY == 0 {
		let mut players = others.clone();
		if me.mode == 0 && me.health > 0 {
			players.push((my, me.pos));
		}
		for &(slot, class) in &foes {
			let key = WorldKey { level, slot, class };
			let Some(a) = mp.ids.addr(slot) else { continue };
			let (cur, at) = (owner_of(mp, &key), read_pose(ram, a, slot, class).pos);
			let d = |p: &(PlayerId, [f32; 3])| dist2(at, p.1).sqrt();
			let now = players.iter().find(|p| p.0 == cur).map_or(f32::MAX, d);
			let best = players.iter().min_by(|a, b| d(a).total_cmp(&d(b)));
			if let Some(b) = best.filter(|b| b.0 != cur && d(b) + OWN_MARGIN < now) {
				mp.owners.insert(key, b.0);
				mp.poses.remove(&slot);
				wire().assign.push((key, b.0));
			}
		}
	}
	let ours: BTreeMap<u32, u16> = foes.iter().filter(|&&(slot, class)| owner_of(mp, &WorldKey { level, slot, class }) == my).filter_map(|&(slot, _)| Some((mp.ids.addr(slot)?, slot))).chain(mp.spawned.iter().filter_map(|(a, s)| Some((*a, s.id?)))).collect();
	let player: Vec<u32> = std::iter::once(enemies::hero_moby(ram)).chain(PART_CELLS.iter().map(|&c| rd(ram, c) & MASK)).filter(|&a| a != 0).collect();
	fx::set_scene(ours.clone(), player, !others.is_empty() && me.mode == 0, mp.ids.by_addr(), mp.fields);
	touch::set_scene(!others.is_empty() && me.mode == 0, mp.ids.by_addr(), mp.fields);
	for (slot, class, damage, mask) in touch::take() {
		eprintln!("mp: a blow on slot {slot} class {class}, for the others too");
		wire().touches.push((WorldKey { level, slot, class }, damage, mask));
		// What a player's blow breaks (Novalis' and Kerwan's breakable props) is then gone for everyone, as a crate is, a player who
		// comes later or was too far for the blow to land included: watched from the blow on.
		if let Some(a) = mp.ids.addr(slot) {
			let kind = (rd(ram, (rd(ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
			mp.watched.entry(slot).or_insert((class, kind));
		}
	}
	// Theirs here, a few a field, and those still waiting for their object.
	let hero = enemies::hero_moby(&c.mem.ram);
	let theirs: Vec<_> = if mp.held { Vec::new() } else {
		touch::keep_up(c, hero, BLOWS_PER_FIELD);
		mp.touches_in.drain(..mp.touches_in.len().min(BLOWS_PER_FIELD)).collect()
	};
	for (key, damage, mask) in theirs {
		let Some(a) = mp.ids.addr(key.slot) else { continue };
		let ram = &c.mem.ram[..];
		if key.level == level && rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 && (rd(ram, a + 0xa4) >> 16) as u16 == key.class {
			match touch::take_blow(c, a, hero, damage, mask) {
				Ok(()) => eprintln!("mp: another player's blow on slot {} class {}", key.slot, key.class),
				Err(e) => eprintln!("mp: could not deal another player's blow: {e}"),
			}
		}
	}
	let ram = &c.mem.ram[..];
	let near = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < STREAM_RANGE * STREAM_RANGE);
	let caught: Vec<FxEvent> = fx::take()
		.into_iter()
		.filter(|(moby, e)| match e {
			FxEvent::Explode { pos, .. } | FxEvent::Spark { pos, .. } | FxEvent::Glow { pos, .. } | FxEvent::Burst { pos, .. } | FxEvent::Flash { pos, .. } => near([pos[0], pos[1], pos[2]]),
			FxEvent::Cue { .. } | FxEvent::PlayerCue { .. } => near([rd_f(ram, moby + 0x10), rd_f(ram, moby + 0x14), rd_f(ram, moby + 0x18)]),
		})
		.map(|(_, e)| e)
		.collect();
	if !caught.is_empty() {
		fx::SENT.fetch_add(caught.len() as u64, Ordering::Relaxed);
		caught.iter().for_each(|e| {
			fx::SENT_KINDS[fx::kind(e)].fetch_add(1, Ordering::Relaxed);
		});
		wire().fx = Some((level, caught));
	}
	if mp.fields % SEND_EVERY == 0 && !others.is_empty() {
		let held_arcs = arcs::read(ram, me.level, &ours);
		if !held_arcs.is_empty() {
			wire().arcs = Some((level, mp.fields as u32, held_arcs));
		}
		let near = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < STREAM_RANGE * STREAM_RANGE);
		let placed = foes
			.iter()
			.filter(|&&(slot, class)| owner_of(mp, &WorldKey { level, slot, class }) == my)
			.filter_map(|&(slot, class)| Some(read_pose(ram, mp.ids.addr(slot)?, slot, class)));
		let far = mp.fields % FAR_EVERY == 0;
		let made = mp.spawned.iter().filter_map(|(a, s)| Some((read_pose(ram, *a, s.id?, s.class), s.kind))).filter(|(p, kind)| near(p.pos) || (far && *kind == ENEMY)).map(|(p, _)| p);
		let poses: Vec<EnemyPose> = placed.filter(|p| near(p.pos)).chain(made).collect();
		if !poses.is_empty() {
			wire().enemies = Some((level, mp.fields as u32, poses));
		}
	}
	// Particles and draws of ours that others see (mp/particles.rs, mp/draws.rs).
	let seen = |p: [f32; 3]| others.iter().any(|o| dist2(p, o.1) < DRAW_RANGE * DRAW_RANGE);
	let in_play = me.mode == 0 && !mp.held && !others.is_empty();
	let ram = &c.mem.ram[..];
	let ps = if in_play { mp.particles.take(ram, me.level, mp.fields, &ours, Some(enemies::hero_moby(ram)).filter(|&h| h != 0), seen, &mp.dying) } else { Vec::new() };
	if !ps.is_empty() {
		wire().particles = Some((level, ps));
	}
	// What they draw only for those a player is near enough to see it (DRAW_RANGE), the nearest first, DRAWS_AT_ONCE at most.
	let mut near_draws: Vec<(f32, u32, u16)> = if in_play {
		ours.iter().filter(|(m, _)| !mp.dying.contains_key(*m)).map(|(m, id)| {
			let at = [rd_f(ram, *m + 0x10), rd_f(ram, *m + 0x14), rd_f(ram, *m + 0x18)];
			(others.iter().map(|o| dist2(at, o.1)).fold(f32::MAX, f32::min), *m, *id)
		}).filter(|d| d.0 < DRAW_RANGE * DRAW_RANGE).collect()
	} else {
		Vec::new()
	};
	near_draws.sort_by(|a, b| a.0.total_cmp(&b.0));
	let watch: BTreeMap<u32, u16> = near_draws.iter().take(DRAWS_AT_ONCE).map(|d| (d.1, d.2)).collect();
	let table_end = rd(ram, table(me.level as u32).0 + 4) & MASK;
	let ds = mp.draws.take(c, me.level, mp.table, table_end, watch, mp.fields % SEND_EVERY == 0);
	if !ds.is_empty() {
		wire().draws = Some((level, mp.fields as u32, ds));
	}
	// The local hero dies or respawns: every enemy runs here, its own way, through the respawn's reset.
	if mp.held {
		enemies::set_driven(BTreeMap::new());
		mp.draws.show(c, mp.fields, &BTreeMap::new());
		return;
	}
	let fields = mp.fields;
	let watched = &mp.watched;
	let stand_ins = &mp.stand_ins;
	let owners = &mp.owners;
	let dying = &mp.dying;
	let ids = &mp.ids;
	let mut driven = BTreeMap::new();
	// The copies shown from another game, with their id and where they are, for their arcs.
	let mut shown_at: Vec<(u32, u16, [f32; 3])> = Vec::new();
	for (slot, t) in mp.poses.iter_mut() {
		if let Some(s) = stand_ins.get(slot) {
			// A stand-in has no update to skip: it is placed from here, while it is still ours (making another can have taken it away).
			if let Some(h) = t.track.sample(DELAY).filter(|_| enemies::still_ours(&c.mem.ram, s.moby, s.class)) {
				let d = enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: t.health, scale: t.scale, look: Some((t.alpha, t.ovr)) };
				enemies::apply(&mut c.mem.ram, s.moby, &d);
				driven.insert(s.moby, d);
				shown_at.push((s.moby, *slot, h.pos));
			}
			continue;
		}
		let Some(&(class, _)) = watched.get(slot) else { continue };
		let Some(a) = ids.addr(*slot) else { continue };
		if dying.contains_key(&a) {
			continue;
		}
		let owner = owners.get(&WorldKey { level, slot: *slot, class }).copied().unwrap_or(0);
		if owner == my || fields.saturating_sub(t.heard) > POSE_STALE {
			continue;
		}
		if let Some(h) = t.track.sample(DELAY) {
			driven.insert(a, enemies::Driven { pos: h.pos, rot: h.rot, anim: h.anim, health: t.health, scale: t.scale, look: Some((t.alpha, t.ovr)) });
			shown_at.push((a, *slot, h.pos));
		}
	}
	enemies::set_driven(driven);
	// What they draw, and the particles they made near them.
	let shown: BTreeMap<u16, u32> = shown_at.iter().map(|&(m, s, _)| (s, m)).collect();
	mp.draws.show(c, fields, &shown);
	let ps = std::mem::take(&mut mp.particles_in);
	if !ps.is_empty() && me.mode == 0 {
		let ram = &c.mem.ram[..];
		let local = |id: u16| shown.get(&id).copied().filter(|&a| rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80);
		let local: BTreeMap<u16, u32> = ps.iter().filter_map(|p| Some((p.slot, local(p.slot)?))).collect();
		mp.particles.put(&mut c.mem.ram, me.level, fields, &ps, |id| local.get(&id).copied());
	}
	// Their arcs, set for this field (mp/arcs.rs): one no longer heard is left to the level's helper, which ends it.
	mp.arcs_in.retain(|_, (_, t)| fields.saturating_sub(*t) <= arcs::STALE);
	for (moby, slot, at) in shown_at {
		if let Some((a, _)) = mp.arcs_in.get(&slot) {
			arcs::set(&mut c.mem.ram, me.level, moby, at, a);
		}
	}
}


/// Other games' sounds and effects that are due, made on this game's copies, in play.
fn fx_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let now = mp.fields;
	// Cues all; explosions a few a field, the rest the next.
	let mut blasts = 0;
	let mut due = Vec::new();
	mp.fx_in.retain(|(f, id, e)| {
		let blast = matches!(e, FxEvent::Explode { .. } | FxEvent::Spark { .. } | FxEvent::Glow { .. } | FxEvent::Burst { .. } | FxEvent::Flash { .. });
		if *f > now || (blast && blasts == BLOWS_PER_FIELD) {
			return true;
		}
		blasts += blast as usize;
		due.push((*id, *e));
		false
	});
	if dying(me) {
		for ((id, class), m) in std::mem::take(&mut mp.voices) {
			if enemies::still_ours(&c.mem.ram, m, class) {
				note(format!("voice of player {id} class {class} at {m:#x} removed: the hero dies"));
				let _ = crate::omni::delete_moby(c, m);
			}
		}
	}
	if mp.held {
		return;
	}
	voices_tick(c, mp, me);
	if due.is_empty() || me.mode != 0 || mp.table == 0 {
		return;
	}
	let mut made = 0;
	for (id, e) in due {
		// This game's copy of what a slot or id names, alive.
		let copy = |mp: &Mp, ram: &[u8], slot: u16| {
			match slot {
				NO_SLOT => None,
				s if s >= SPAWNED_ID => spawned::local_of(mp, s),
				s => mp.ids.addr(s),
			}
			.filter(|&a| rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80)
		};
		let voice = match e {
			FxEvent::PlayerCue { class, .. } => voice_of(c, mp, id, class, &mut made),
			// Made for something this game has no copy of: a hidden one of its class stands in.
			FxEvent::Explode { slot, owner, .. } | FxEvent::Glow { slot, owner, .. } | FxEvent::Burst { slot, owner, .. } if owner != NO_SLOT && copy(mp, &c.mem.ram, slot).is_none() => voice_of(c, mp, id, owner, &mut made),
			_ => None,
		};
		let ram = &c.mem.ram[..];
		let local = |slot: u16| copy(mp, ram, slot);
		let raw = match e {
			FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Glow { slot, .. } | FxEvent::Burst { slot, .. } if slot != NO_SLOT && slot < SPAWNED_ID => mp.ids.addr(slot),
			_ => None,
		};
		if raw.is_some_and(|a| mp.dying.contains_key(&a)) {
			continue;
		}
		let named: BTreeMap<u16, u32> = match e {
			FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Glow { slot, .. } | FxEvent::Burst { slot, .. } => local(slot).map(|a| (slot, a)).into_iter().collect(),
			FxEvent::PlayerCue { .. } | FxEvent::Spark { .. } | FxEvent::Flash { .. } => BTreeMap::new(),
		};
		note(format!("player {id}'s {e:?} replayed (voice {voice:x?}, named {named:x?})"));
		if !fx::play(c, &e, |s| named.get(&s).copied(), voice) {
			mp.fx_in.push((now + 1, id, e));
		}
	}
}

/// The hidden moby player `id`'s sounds of class `class` come from here, made the first time (two a field at most).
fn voice_of(c: &mut Cpu, mp: &mut Mp, id: PlayerId, class: u16, made: &mut usize) -> Option<u32> {
	// Checked at each use: the call before this one can have taken it away (an explosion clears out older effects of its class).
	if let Some(&v) = mp.voices.get(&(id, class)) {
		if enemies::still_ours(&c.mem.ram, v, class) {
			return Some(v);
		}
		mp.voices.remove(&(id, class));
	}
	if *made == 2 || mp.voices.keys().filter(|(p, _)| *p == id).count() >= VOICES_EACH {
		return None;
	}
	*made += 1;
	match crate::omni::create_moby(c, class as u32) {
		Ok(m) if m != 0 => {
			let m = m & MASK;
			enemies::no_update(&mut c.mem.ram, m);
			enemies::hide(&mut c.mem.ram, m);
			eprintln!("mp: player {id}'s class {class} sounds come from a voice here");
			note(format!("voice of player {id} class {class} made at {m:#x}"));
			mp.voices.insert((id, class), m);
			Some(m)
		}
		_ => None,
	}
}

/// The voices stay at their players' puppets, and go with them.
fn voices_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	let at: BTreeMap<PlayerId, [f32; 3]> = mp.shown.iter().filter(|(_, h)| h.level == me.level).map(|(id, h)| (*id, h.pos)).collect();
	let gone: Vec<(PlayerId, u16)> = mp.voices.keys().copied().filter(|(id, _)| !at.contains_key(id)).collect();
	// A voice the game took away (a respawn) is made again when it is next needed.
	mp.voices.retain(|&(_, class), m| enemies::still_ours(&c.mem.ram, *m, class));
	for k in gone {
		if let Some(m) = mp.voices.remove(&k) {
			note(format!("voice of player {} class {} at {m:#x} removed: its player went", k.0, k.1));
			if let Err(e) = crate::omni::delete_moby(c, m) {
				eprintln!("mp: could not remove a voice: {e}");
			}
		}
	}
	for (&(id, _), &m) in &mp.voices {
		let p = at[&id];
		for k in 0..3u32 {
			c.mem.wr32(m + 0x10 + k * 4, (p[k as usize] + if k == 2 { VOICE_LIFT } else { 0.0 }).to_bits());
		}
	}
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

/// One field of the shared world, in the level the local hero is in: walks the placed slots of the moby table, reports what went
/// from this game (near the hero, in play, alive, not a whole level clearing) and removes what the host says is gone.
fn world_tick(c: &mut Cpu, mp: &mut Mp, me: &HeroState) {
	if me.level < 0 {
		mp.watched.clear();
		mp.watched_level = -1;
		mp.table = 0;
		return;
	}
	if mp.watched_level != me.level {
		mp.watched.clear();
		mp.dying.clear();
		// The game cleared the table, voices too; the arc tables are the new level's.
		mp.voices.clear();
		mp.arcs_in.clear();
		arcs::forget();
		fx::forget();
		mp.particles.forget();
		mp.particles_in.clear();
		mp.draws.forget();
		touch::forget();
		mp.watched_level = me.level;
		mp.settle = WORLD_SETTLE;
		mp.gone_here.clear();
		mp.finishing.clear();
		mp.poses.clear();
	}
	// A death clears and refills the table through the respawn: what goes then is nobody's doing, and the table settles again after.
	mp.settle = if dying(me) { WORLD_SETTLE } else { mp.settle.saturating_sub(1) };
	let level = me.level as u8;
	let ram = &c.mem.ram[..];
	let (cell, _) = table(level as u32);
	// The table: the placed objects from `start`, the spawned ones from `spawned` (omni/mobys.rs reads the same cells).
	let (start, spawned) = (rd(ram, cell) & MASK, rd(ram, cell + 4) & MASK);
	let slots = spawned.wrapping_sub(start) / 0x100;
	if start == 0 || spawned <= start || (spawned - start) % 0x100 != 0 || slots > WORLD_SLOTS {
		mp.table = 0;
		return;
	}
	mp.table = start;
	mp.ids.update(ram, start, slots);
	// What a rebuilt table (a respawn) no longer has went with it, not by a player's doing.
	mp.watched.retain(|s, _| mp.ids.addr(*s).is_some());
	let (mut gone, mut delete) = (Vec::new(), Vec::new());
	// Slot 0 is the hero.
	for slot in 1..slots {
		let a = start + slot * 0x100;
		let Some(s) = mp.ids.id(a) else { continue };
		let class = (rd(ram, a + 0xa4) >> 16) as u16;
		if rd(ram, a + 0x24) != 0 && rd8(ram, a + 0x20) < 0x80 {
			let key = WorldKey { level, slot: s, class };
			// Not while the table settles (a respawn of the hero refills it): what comes back then is finished again, as before.
			if mp.settle == 0 && mp.removed.contains(&key) && mp.gone_here.remove(&s) {
				// Ended here, and alive again: the level sent it again (Novalis' class 660 and 459 come back elsewhere). It lives, in
				// every game, and its next end is reported anew.
				mp.removed.remove(&key);
				wire().back.push(key);
				eprintln!("mp: back here: level {level} id {s:#x} class {class}, sent again by the level");
			} else if mp.removed.contains(&key) {
				delete.push((a, s));
				continue;
			}
			if !mp.watched.contains_key(&s) {
				let kind = (rd(ram, (rd(ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
				if WORLD_TYPES.contains(&kind) || WORLD_CLASSES.contains(&class) {
					mp.watched.insert(s, (class, kind));
				}
			}
		} else if let Some((class, kind)) = mp.watched.remove(&s) {
			let (x, y, z) = (rd_f(ram, a + 0x10) - me.pos[0], rd_f(ram, a + 0x14) - me.pos[1], rd_f(ram, a + 0x18) - me.pos[2]);
			if x * x + y * y + z * z < WORLD_NEAR * WORLD_NEAR {
				gone.push((WorldKey { level, slot: s, class }, kind, [rd_f(ram, a + 0x10), rd_f(ram, a + 0x14), rd_f(ram, a + 0x18)]));
			}
		}
	}
	// In play, alive and settled: anything else that takes objects away (a scene, a death, a level starting) is not a player's doing.
	if mp.settle == 0 && me.mode == 0 && me.health > 0 && gone.len() <= WORLD_BURST {
		for (k, kind, at) in gone {
			if mp.removed.insert(k) {
				eprintln!("mp: gone here: level {} id {:#x} class {} type {kind} at {at:.1?}", k.level, k.slot, k.class);
				wire().gone.push(k);
				mp.gone_here.insert(k.slot);
			}
		}
	}
	// Not while the local hero dies or respawns: the respawn puts the level's objects back, and what comes back is finished after.
	if me.mode == 0 && !mp.held {
		let hero = enemies::hero_moby(&c.mem.ram);
		let listed: BTreeSet<u32> = delete.iter().map(|d| d.0).collect();
		// What this game was finishing and is alive no more ended here: alive again later, the level sent it again.
		let alive: BTreeSet<u16> = delete.iter().map(|d| d.1).collect();
		let ended: Vec<u16> = mp.finishing.iter().filter(|s| !alive.contains(s)).copied().collect();
		for s in ended {
			mp.finishing.remove(&s);
			if mp.settle == 0 {
				mp.gone_here.insert(s);
			}
		}
		mp.dying.retain(|a, _| listed.contains(a));
		let mut blows = 0;
		for (a, s) in delete {
			mp.watched.remove(&s);
			// An enemy or a crate goes here its own way: its death or break, its sounds, what it drops and what counts on it.
			let kind = (rd(&c.mem.ram, (rd(&c.mem.ram, a + 0x24) & MASK) + 0x44) >> 16) as i16;
			let finishing = kind == ENEMY || kind == CRATE;
			if finishing {
				let at = [rd_f(&c.mem.ram, a + 0x10), rd_f(&c.mem.ram, a + 0x14), rd_f(&c.mem.ram, a + 0x18)];
				let near = dist2(at, me.pos) < FINISH_NEAR * FINISH_NEAR;
				// Still in its first state here: hit so, the game takes paths it never set up (Novalis' birdbots, class 459, then divided
				// by a path count of 0 and stopped). It is woken first, its own update run, and the blow lands once it is awake.
				let asleep = kind == ENEMY && rd8(&c.mem.ram, a + 0x20) == 0;
				match mp.dying.get(&a).copied() {
					None if near && blows < BLOWS_PER_FIELD => {
						blows += 1;
						mp.dying.insert(a, mp.fields);
						mp.poses.remove(&s);
						mp.fx_in.retain(|(_, _, e)| !matches!(e, FxEvent::Cue { slot, .. } | FxEvent::Explode { slot, .. } | FxEvent::Burst { slot, .. } if *slot == s));
						enemies::release(a);
						if asleep {
							eprintln!("mp: waking id {s:#x} here, to finish it for another player");
						} else {
							mp.finishing.insert(s);
							match enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK) {
								Ok(()) => eprintln!("mp: finishing for another player: level {level} id {s:#x} at {at:.1?}"),
								Err(e) => eprintln!("mp: could not finish id {s:#x}: {e}"),
							}
						}
					}
					// Woken: the blow now. Not woken in time: taken away.
					Some(t) if near && !mp.finishing.contains(&s) => {
						if !asleep {
							mp.finishing.insert(s);
							match enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK) {
								Ok(()) => eprintln!("mp: finishing for another player (woken here): level {level} id {s:#x} at {at:.1?}"),
								Err(e) => eprintln!("mp: could not finish id {s:#x}: {e}"),
							}
						} else if mp.fields - t >= WAKE_WITHIN {
							mp.dying.remove(&a);
							match crate::omni::delete_moby(c, a) {
								Ok(()) => {
									eprintln!("mp: removed for another player (it did not wake here): level {level} id {s:#x}");
									mp.gone_here.insert(s);
								}
								Err(e) => eprintln!("mp: could not remove slot {s}: {e}"),
							}
						}
					}
					// Far from this game's hero: left as it is until he comes, not hit, hidden or taken away from there. What carries it (one
					// of Kerwan's trains its cargo crates) broke on each of those; near, the blow breaks it as a player's would.
					None if !near => {
						mp.poses.remove(&s);
						enemies::release(a);
					}
					// Near and still standing: the blow again, now and then, till it reads it.
					Some(t) if near && (mp.fields - t) % FINISH_WITHIN == 0 && mp.fields != t => {
						let _ = enemies::post_hit(c, a, hero, FINISH_DAMAGE, FINISH_MASK);
					}
					_ => {}
				}
				continue;
			}
			mp.dying.remove(&a);
			note(format!("id {s:#x} at {a:#x} removed for another player"));
			match crate::omni::delete_moby(c, a) {
				Ok(()) => {
					eprintln!("mp: removed for another player: level {level} id {s:#x}");
					if mp.settle == 0 {
						mp.gone_here.insert(s);
					}
				}
				Err(e) => eprintln!("mp: could not remove slot {s}: {e}"),
			}
		}
	}
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

/// Weapon quads sent each other field, at most (8 datagrams), and fields a player's are shown after they came.
const QUADS_MAX: usize = 8 * rac_mp::proto::MAX_QUADS;
const QUADS_LAST: u64 = 6;
/// Weapon quads sent and drawn, for the run log.
static QUADS_SENT: AtomicU64 = AtomicU64::new(0);
static QUADS_SHOWN: AtomicU64 = AtomicU64::new(0);
/// Their corners in 2 mm steps (as far as 65 m from him), their UVs in 1/1024.
const QUAD_STEP: f32 = 0.002;
const UV_STEP: f32 = 1.0 / 1024.0;

/// retail/worker/field_snap.rs, when a field's snapshot is taken: our player's weapon beams (the quads `queue_b::fill` marks his)
/// and Swingshot rope go to the others, relative to where he is, every other field while another player is here; the other
/// players' come into the snapshot's effects, placed where their puppets are. The game itself is not touched.
pub fn effects(fx: &mut Option<crate::retail::RetailFx>) {
	if !ON.load(Ordering::Relaxed) {
		return;
	}
	let Some(fx) = fx.as_mut() else { return };
	let mut g = lock();
	let Some(mp) = g.as_mut().filter(|mp| mp.level >= 0 && mp.mode == 0) else { return };
	let others = mp.shown.iter().any(|(_, h)| h.level == mp.level && on_stage(h.mode));
	// Ours.
	if let (true, Some(me), true) = (others, fx.hero, mp.fields % SEND_EVERY == 0) {
		let q = |v: f32| (v / QUAD_STEP).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
		let u = |v: f32| (v / UV_STEP).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
		let c = |v: f32| (v * 128.0).round().clamp(0.0, 255.0) as u8;
		let quads: Vec<FxQuad> = fx
			.weapon
			.iter()
			.flat_map(|(b, r)| if *b { fx.qb.get(r.clone()) } else { fx.qa.get(r.clone()) }.unwrap_or(&[]).iter())
			.take(QUADS_MAX)
			.map(|w| FxQuad {
				pos: w.pos.map(|p| [q(p[0] - me[0]), q(p[1] - me[1]), q(p[2] - me[2])]),
				uv: w.uv.map(|t| [u(t[0]), u(t[1])]),
				colour: w.colour.map(|k| k.map(c)),
				tex: w.tex as u32,
				additive: w.additive,
			})
			.collect();
		let rope = fx.rope.map(|(_, r)| RopePose { hook: [r.hook[0] - me[0], r.hook[1] - me[1], r.hook[2] - me[2]], phase: r.phase, wave: r.wave, amp: r.amp });
		if !quads.is_empty() || rope.is_some() {
			QUADS_SENT.fetch_add(quads.len() as u64 + rope.is_some() as u64, Ordering::Relaxed);
			wire().quads = Some((mp.level as u8, mp.fields as u32, rope, quads));
		}
	}
	// Theirs, where their puppets are.
	let now = mp.fields;
	mp.quads_in.retain(|_, e| now.saturating_sub(e.0) <= QUADS_LAST);
	for (id, (_, _, rope, quads)) in &mp.quads_in {
		let Some((_, h)) = mp.shown.iter().find(|(i, h)| i == id && h.level == mp.level && on_stage(h.mode)) else { continue };
		let at = h.pos;
		QUADS_SHOWN.fetch_add(quads.len() as u64 + rope.is_some() as u64, Ordering::Relaxed);
		for w in quads {
			fx.qb.push(crate::particles::WorldQuad {
				pos: w.pos.map(|p| [at[0] + p[0] as f32 * QUAD_STEP, at[1] + p[1] as f32 * QUAD_STEP, at[2] + p[2] as f32 * QUAD_STEP]),
				uv: w.uv.map(|t| [t[0] as f32 * UV_STEP, t[1] as f32 * UV_STEP]),
				colour: w.colour.map(|k| k.map(|v| v as f32 / 128.0)),
				tex: w.tex as usize,
				additive: w.additive,
			});
		}
		// His rope, from the hook to his hand (about 1 m above his feet), facing up toward where the local camera looks from.
		if let Some(r) = rope {
			let hook = [at[0] + r.hook[0], at[1] + r.hook[1], at[2] + r.hook[2]];
			let cam = fx.hero.map_or([at[0], at[1], at[2] + 4.0], |m| [m[0], m[1], m[2] + 2.0]);
			let s = crate::retail::queue_b::SwingRope { hook, cam, phase: r.phase, wave: r.wave, amp: r.amp };
			fx.qa.extend(crate::retail::queue_b::rope_quads(&s, [at[0], at[1], at[2] + 1.0]));
		}
	}
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
