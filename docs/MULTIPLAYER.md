# Online multiplayer

This fork adds online play to rac-rs: 2 to 4 players, each running their own copy of the game built from their own disc. Each
player sees the others in the same level. Upstream rac-rs has no multiplayer; everything here is this fork's own code.

**Status:** phases 0, 1 and 2 work. Players connect, and each sees the others as animated Ratchets in the same level, with a name
above each head and what they hold in hand. Enemies killed and crates broken by one player are gone for everyone. Tested with
two games on one PC; not yet tested across the internet. Clank on
puppets is written but not yet seen working: a new game doesn't have him, so it needs a save from past the first level.

## How it works

- **One player hosts** from inside their game. There is no separate server program. The others join the host's address.
- **Each game runs on its own.** Nothing is run in lockstep. Each player's game sends a small snapshot of its own Ratchet
  25 to 30 times a second: level, position, rotation, animation clock, state, health, item and lighting. The host passes
  each snapshot on to the others.
- **The others are puppets.** Each other player in your level is drawn as one more Ratchet object. The renderer places,
  lights and animates it from that player's own animation clock, exactly as it does every other object.
  - Puppets are picture only: nothing is written to game memory. The game itself doesn't know they exist.
  - A run with and without multiplayer ends with the same game RAM.
  - What a player carries travels with their snapshot. The game places Clank, a pack and the held item on Ratchet's joints
    itself, so each player sends those objects' placement, relative to their Ratchet. Smoothing moves them with him.
  - Names are drawn in the game font through the mod system's text layer, only during normal play.
- **Remote players are smoothed.** They are shown about 100 ms in the past, blended between two snapshots that arrived, so
  late or lost packets don't show.
- **Networking runs on its own thread.** The game doesn't tick in parts of the menus or while a level loads, but players
  must keep answering each other meanwhile. A client that has no state to send pings the host every second.
- **UDP, unreliable for snapshots.** A newer snapshot replaces a lost one. What must arrive (a kill, a broken crate) goes over a
  reliable, ordered channel: numbered, resent until acknowledged.
- **The host decides the shared world.** Every game watches the objects the level placed that matter: enemies, crates and gold
  bolts. A placed object keeps its slot in the game's object table, so `(level, slot, class)` names it in every game.
  - When one goes from a player's game near that player, in normal play, that game reports it.
  - The host takes the first report of each object, keeps the list, and tells everyone else. A player who joins later gets the
    whole list.
  - Each game removes the object with the game's own `delete_moby`, and again if it comes back, as it does after a player's
    death restores a checkpoint.
  - This is the only time multiplayer changes a game's memory.
- **The same disc region for everyone.** PAL ticks 50 times a second and USA 60, so the host refuses a player on the other
  region. The protocol version is checked too.
- **Untrusted input is checked.** Every packet comes from someone else's PC. The decoder checks every length and value,
  refuses NaN and infinite floats, and never panics. Hostile input is part of its tests.

## Where the code is

| Path | What |
|---|---|
| `mp/` (crate `rac-mp`) | The network side: wire format (`proto.rs`), host and join sessions and the host's world list (`session.rs`), the reliable channel (`reliable.rs`), smoothing (`interp.rs`). No game code, no dependencies. |
| `rac-rs/src/mp.rs` | The game side: reads the local hero from game RAM, runs the network thread, builds the puppets, and provides the offline mirror. |
| `tools/mp_build.ps1` | Rebuilds the game after a code change, with setup's exact settings, so only what changed is recompiled. |
| `tools/mp_play.ps1` | Starts a game and walks it into play on Veldin with no hands on the pad: for tests and screenshots. |

Upstream files touched, each by a line or a few, so merging upstream stays cheap:

| File | Change |
|---|---|
| `rac-rs/src/main.rs` | `mod mp;` |
| `rac-rs/src/retail/worker.rs` | `crate::mp::init()` after `omni::init` |
| `rac-rs/src/retail/worker/field_run.rs` | `crate::mp::tick()` after `omni::tick`, once per game tick; it gets the machine mutably, to remove objects |
| `rac-rs/src/retail/worker/field_front.rs` | `crate::mp::note_front()` next to `omni::note_front` |
| `rac-rs/src/retail/worker/field_snap.rs` | the puppets join each snapshot's object list |
| `rac-rs/src/retail/prop_pools.rs` | the puppets' object pools (only with `RAC_MP`) |
| `rac-rs/src/retail/moby_state.rs` | `euler_from_rows` opened to the crate, to turn a part's matrix into its rotation |
| `rac-rs/src/viewer/app.rs` | Ratchet's and the wrench's models for the puppets (only with `RAC_MP`). On the same lines as their neighbours, because upstream caps that function's length (`SIZE_CAPS.txt`). |
| `rac-rs/src/omni/draw.rs` | a text list of the names' own, merged like the debug dots; the window layout is kept with `RAC_MP` too |
| `rac-rs/src/omni/mod.rs` | `screen_of` and `OmniDrawCmd` opened to the crate, for the names; `delete_moby`, which runs the game's function with no mod loaded |
| `Cargo.toml`, `rac-rs/Cargo.toml`, `Cargo.lock` | the `rac-mp` crate |

Addresses come from the OmniLoader binding, `rac-rs/src/omni/rac1.json`. Both supported discs use its "ntsc" data layout
(see `omni/layout.rs`).

## Phases

0. **Connect and exchange.** Done.
1. **Puppets.** Done:
   - Remote players show as animated Ratchets, with names above their heads and what they hold.
   - Offline mirror mode exists for testing.

   Still to check: Clank and packs on puppets, on a save from past the first level, and weapons other than the wrench.
2. **Shared world.** Done for enemies and crates the level placed. Written but not yet seen working: gold bolts.

   How it behaves now:
   - Rewards stay with the player who earned them: bolts drop in that player's game only.
   - In the other games the object just vanishes, with no death animation, and it goes when the killer's copy is deleted, so
     a little after the hit.
   - Enemies move on their own in each game, so the same enemy can be in different places. Phase 3 narrows that.

   Not shared yet: anything spawned during play (enemies from spawners, dropped bolts), and other pickups such as weapons.

   Known risk: a boss or an enemy tied to a story trigger is removed like any other. Killing one in one game may not run its
   script in the others, which could leave a door shut or a scene unplayed there. Not tested.
3. **Enemies fight everyone.** Just before an enemy's update, swap in the nearest player as "the hero", then put the real one
   back, so the original AI chases whoever is closest.
4. **Session polish.** Keep cutscenes, planet travel and pausing in step. Let a lost client rejoin.

## Playing

Build the game with `Setup.cmd` as upstream describes. After a code change, run
`powershell -ExecutionPolicy Bypass -File tools\mp_build.ps1`. Then start the game with these environment variables:

```bat
set RAC_MP=host
set RAC_MP_NAME=Host
Play.cmd
```

```bat
set RAC_MP=join:192.168.1.20
set RAC_MP_NAME=Friend
Play.cmd
```

| Variable | Meaning |
|---|---|
| `RAC_MP` | `host` (UDP port 2410), `host:PORT`, `host:ADDR:PORT` (one address only; `127.0.0.1` keeps it to this PC), `join:ADDR` or `join:ADDR:PORT`. Or `mirror` / `mirror:FIELDS`: no network, one puppet replays your own Ratchet a second (FIELDS) behind. Unset: multiplayer is off and the game runs as upstream's. |
| `RAC_MP_NAME` | The name the others see (default `Ratchet`, at most 24 bytes). |

The host must let UDP port 2410 through its firewall, and through its router to play over the internet. Join, leave and position
lines (`mp: ...`) go to the run log, `player\log.txt`.

## Testing

**Network tests.** The network crate tests itself over real sockets on 127.0.0.1, with no game and no disc. That covers:
joining, relaying, a full session, the other region, other protocol versions, leaving, timeouts, a host that's still
starting, keepalive while loading, malformed and random packets, and smoothing. `tools\mp_build.ps1 -Test` runs them after a
build.

**Scripted world test.** On Veldin the USA disc has four crates straight ahead of the spawn. A joiner with this pad runs into them
and breaks them with the wrench, and the host's log shows the same slots removed:
`-Pad "1640-2500:0:80,00;1960-1965:80:80,00;1984-1989:80:80,00;..."`, swinging every 24 fields.

**In-game tests.** `tools\mp_play.ps1` boots the game from the disc into play on Veldin by itself: the title pilot answers the
menus and key taps skip the movies. It can hold the stick (`-Pad`), travel to a level (`-Travel`), take a screenshot (`-Shot`,
`-ShotField`) and run hidden. A forced `-Travel` tests level changes, but leaves Ratchet in a default pose on arrival.
For example, two players on one PC, where the joiner runs forward-right and the host takes a picture:

```bat
powershell -ExecutionPolicy Bypass -File tools\mp_play.ps1 -Mp host:127.0.0.1:2410 -Name Host -Hidden -Shot %TEMP%\host.png -ShotField 1830
powershell -ExecutionPolicy Bypass -File tools\mp_play.ps1 -Mp join:127.0.0.1:2410 -Name Joiner -Fields 2100 -Pad "1640-1680:0:ff,00"
```

Play starts at about field 1530 on the USA disc. Pad entries are `from-to:buttons:lx,ly` in hex, counted in fields from the
start; `80,80` is a centred stick.

**Running the tests without a built game.** The workspace can't load until setup has generated the game code. Before that,
copy `mp\` somewhere else, add an empty `[workspace]` line to the copy's `Cargo.toml`, and run `cargo test` there.

## Keeping up with upstream

`upstream` is AsobiCodes/rac-rs (fetch only), and `origin` is this repo:

```bat
git fetch upstream
git merge upstream/main
```

Setup rewrites `Cargo.lock` for your disc's generated crates. Don't commit those changes.
