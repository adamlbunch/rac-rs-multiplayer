# Online multiplayer

This fork adds online play to rac-rs: 2 to 4 players, each running their own copy of the game built from their own disc. Each
player sees the others in the same level. Upstream rac-rs has no multiplayer; everything here is this fork's own code.

## How it works

- **One player hosts** from inside their game. There is no separate server program. The others join the host's address.
- **Each game runs on its own.** Nothing is run in lockstep. Each player's game sends a small snapshot of its own Ratchet
  (level, position, facing, state, health, item) 25 to 30 times a second. The host passes each snapshot on to the others.
- **Each game shows the others smoothed.** Remote players are drawn about 100 ms in the past, blended between two snapshots that
  arrived, so late or lost packets don't show.
- **UDP, unreliable for snapshots.** A newer snapshot replaces a lost one. Things that must arrive (kills, pickups, level
  changes) will get a reliable channel when the phases below need it.
- **The same disc region for everyone.** PAL ticks 50 times a second and USA 60, so the host refuses a player on the other
  region. The protocol version is checked too.
- **Untrusted input is checked.** Every packet comes from someone else's PC. The decoder checks every length and value,
  refuses NaN and infinite floats, and never panics. Hostile input is part of its tests.

## Where the code is

| Path | What |
|---|---|
| `mp/` (crate `rac-mp`) | The network side: wire format (`proto.rs`), host and join sessions (`session.rs`), smoothing (`interp.rs`). No game code and no dependencies. |
| `rac-rs/src/mp.rs` | The game side: reads the local hero from game RAM each tick, hands it to the session, and takes the others back. |

Upstream files touched, one line each, so merging upstream stays cheap:

- `rac-rs/src/main.rs`: `mod mp;`
- `rac-rs/src/retail/worker.rs`: `crate::mp::init()` after `omni::init`
- `rac-rs/src/retail/worker/field_run.rs`: `crate::mp::tick()` after `omni::tick`, once per game tick
- `rac-rs/src/retail/worker/field_front.rs`: `crate::mp::note_front()` next to `omni::note_front`
- `Cargo.toml`, `rac-rs/Cargo.toml`, `Cargo.lock`: the `rac-mp` crate

Hero addresses come from the OmniLoader binding, `rac-rs/src/omni/rac1.json`. Both supported discs use its "ntsc" data
layout (see `omni/layout.rs`).

## Phases

0. **Connect and exchange (now).** The game reads the local hero, sends it and receives the others, and writes where they are
   to the run log. It only reads game RAM and changes nothing in the game.
1. **Ghosts.** Show each remote player as a puppet Ratchet: spawn a moby, then set its position, facing and animation every
   tick. Draw names above heads with the mod system's draw list. First test it offline: a puppet that replays your own moves
   one second late.
2. **Shared world.** Kills, crates, bolts and pickups by a stable object key, decided by the host. Needs the reliable channel.
3. **Enemies fight everyone.** Just before an enemy's update, swap in the nearest player as "the hero", then put the real one
   back, so the original AI chases whoever is closest.
4. **Session polish.** Keep cutscenes, planet travel and pausing in step.

## Trying phase 0

Build the game with `Setup.cmd` as upstream describes, then start it with these environment variables (`build\release\rac-rs.exe`):

```bat
set RAC_MP=host
set RAC_MP_NAME=Host
build\release\rac-rs.exe
```

```bat
set RAC_MP=join:192.168.1.20
set RAC_MP_NAME=Friend
build\release\rac-rs.exe
```

| Variable | Meaning |
|---|---|
| `RAC_MP` | `host` (UDP port 2410), `host:PORT`, `join:ADDR` or `join:ADDR:PORT`. Unset: multiplayer is off and the game runs as upstream's. |
| `RAC_MP_NAME` | The name the others see (default `Ratchet`, at most 24 bytes). |

The host must let UDP port 2410 through its firewall, and through its router to play over the internet. Join, leave and position
lines (`mp: ...`) go to the run log, `player\log.txt`.

## Tests

The network crate tests itself over real sockets on 127.0.0.1, with no game and no disc. That covers joining, relaying, a full
session, the other region, other protocol versions, leaving, timeouts, malformed and random packets, and smoothing.

After setup has generated the game code, run them in place:

```bat
cargo test -p rac-mp
```

Before setup has run, the workspace can't load, because the generated crates are missing. In that case, copy `mp\` somewhere
else, add an empty `[workspace]` line to the copy's `Cargo.toml`, and run `cargo test` there.

## Keeping up with upstream

`upstream` is AsobiCodes/rac-rs (fetch only), and `origin` is this repo:

```bat
git fetch upstream
git merge upstream/main
```
