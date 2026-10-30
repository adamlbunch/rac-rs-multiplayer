//! rac_mp: the network side of online multiplayer for rac-rs.
//!
//! - `proto`: the wire format. One message per UDP datagram, checked field by field on the way in: everything that arrives
//!   comes from someone else's PC, so `decode` never panics and refuses anything malformed.
//! - `session`: a listen server. One player hosts in their own game (player id 0), up to `MAX_PLAYERS - 1` others join. The
//!   host relays each player's hero state to the others. Hero states are sent unreliably (a newer one replaces a lost one).
//! - `interp`: shows each remote hero a few ticks in the past, between two states that arrived, so jitter and a lost
//!   packet do not show.
//!
//! Nothing here knows the game: rac-rs/src/mp.rs reads the local hero out of the game's RAM and hands it over.

pub mod interp;
pub mod proto;
pub mod session;

pub use interp::Track;
pub use proto::{HeroState, Msg, PlayerId, RejectReason};
pub use session::{Event, Session};

/// The UDP port a host listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 2410;
