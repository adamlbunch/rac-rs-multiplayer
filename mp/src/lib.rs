//! rac_mp: the network side of online multiplayer for rac-rs.
//!
//! - `proto`: the wire format. One message per UDP datagram, checked field by field on the way in: everything that arrives
//!   comes from someone else's PC, so `decode` never panics and refuses anything malformed.
//! - `session`: a listen server. One player hosts in their own game (player id 0), up to `MAX_PLAYERS - 1` others join. The
//!   host relays each player's hero state to the others. Hero states are sent unreliably (a newer one replaces a lost one).
//! - `interp`: shows each remote hero a few ticks in the past, between two states that arrived, so jitter and a lost
//!   packet do not show.
//! - `reliable`: what must arrive (a kill, a broken crate) is sent again until acknowledged. The host keeps the list of what is
//!   gone from the world and decides it for everyone (`session`).
//!
//! Nothing here knows the game: rac-rs/src/mp.rs reads the local hero out of the game's RAM and hands it over.

pub mod ids;
pub mod interp;
pub mod proto;
pub mod reliable;
pub mod session;

pub use interp::Track;
pub use proto::{AnimClock, EnemyPose, FxEvent, HeroState, MAX_PARTS, MAX_POSES, MAX_VARS, Msg, NO_SLOT, Part, PlayerId, Progress, RejectReason, Reliable, SPAWNED_ID, WorldKey};
pub use session::{Event, Session};

/// The UDP port a host listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 2410;
