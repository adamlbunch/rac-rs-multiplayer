# Online multiplayer

This fork adds online play to rac-rs: 2 to 4 players, each running their own copy of the game built from their own disc. Each
player sees the others in the same level. Upstream rac-rs has no multiplayer; everything here is this fork's own code.

**Status:** all four phases are built, and enemies are synced, both placed and spawned, along with enemy shots. Players connect and see each other as animated Ratchets with
names, Clank and what they hold. Each placed enemy runs in one game and is mirrored in the others, so all players see it in the
same place doing the same thing. An enemy or shot spawned during play appears in the other games as a stand-in, and an enemy carries on in another game when the one running it leaves. Clients follow the host between planets, and a client that loses the host gets back in. Tested
with two games on one PC, on the USA disc; not yet tested across the internet or on the PAL disc. The limits are listed under
each phase below.

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
- **Each enemy runs in exactly one game.** The host gives every placed enemy to the game of the player nearest to it, and moves
  it when another player gets clearly closer. That game runs its real AI against a hero who is really there, so attacks,
  collision and damage are the game's own and nothing is faked.
  - That game streams the enemy's position, rotation, animation and health 25 to 30 times a second.
  - Every other game skips the enemy's update and mirrors it, about 100 ms behind.
  - A weapon's swing or shot is tested against where mirrored enemies really are, and a hit travels to the game running that enemy.
  - If the owner pauses, leaves or stops streaming for a second, the enemy runs locally again until the host reassigns it.
- **A spawned object runs in the game that made it, until that game goes.** Enemies and enemy shots made during play have no slot in
  common across games, so each game reports what it makes.
  - The host gives the object an id, or says it is a copy: the same class another game just reported near the same place, or, for
    an enemy or what came with the level, reported whenever a game gets there. A copy stands down, hidden and no longer updated.
  - The game that made it streams it with the placed enemies, and spawned enemies more slowly even when nobody is near. The others
    keep a stand-in, made by the game's own `create_moby`, which never updates and goes when the object is gone.
  - When that game leaves the session or the level for two seconds, the host gives each of its objects to the nearest player still
    in the level. That game wakes its copy where the object last was. With no copy, it starts its stand-in: the class's first update
    runs where it stands, and it keeps its health. A shot isn't handed over: it ends with its game.
  - An object left with nobody in its level is gone. If the host goes, each client keeps running what it holds and reports it to
    the next host.
  - A shot's stand-in that reaches the local hero in flight hits him through the game's own damage, knockback included.
- **Players stay in step.** Clients follow the host to another planet, through the same request the game's level ends make.
  Nobody is drawn over a cutscene, and a player in one isn't drawn. A name says when its player is paused, in a dialog or
  shopping. A client that loses the host keeps trying to get back in, and tells the host again what it knows is gone.
- **What multiplayer changes in a game's memory:** it removes what another player removed, follows the host's travel, and writes
  mirrored enemies. With nobody else connected it changes nothing.
- **The same disc region for everyone.** PAL ticks 50 times a second and USA 60, so the host refuses a player on the other
  region. The protocol version is checked too.
- **Untrusted input is checked.** Every packet comes from someone else's PC. The decoder checks every length and value,
  refuses NaN and infinite floats, and never panics. Hostile input is part of its tests.

## Where the code is

| Path | What |
|---|---|
| `mp/` (crate `rac-mp`) | The network side: wire format (`proto.rs`), host and join sessions and the host's world list (`session.rs`), the reliable channel (`reliable.rs`), smoothing (`interp.rs`). No game code, no dependencies. |
| `rac-rs/src/mp.rs` | The game side: reads the local hero from game RAM, runs the network thread, builds the puppets, keeps the shared world, follows the host, and provides the offline mirror. |
| `rac-rs/src/mp/enemies.rs` | Synced enemies: the hands that skip an enemy's update and mirror it, the weapon test against mirrored enemies, posting a forwarded hit, and what makes a stand-in or a stood-down copy. |
| `rac-rs/src/mp/spawned.rs` | Spawned objects: reporting them, copies, stand-ins, and changing hands when a game goes. |
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
| `rac-rs/src/omni/mod.rs` | `screen_of` and `OmniDrawCmd` opened to the crate, for the names; `delete_moby`, which runs the game's function with no mod loaded; `hook_specs` and `allow_hook_hands`, for the enemy hands; `hit_ring_write`, to post a forwarded hit; `create_moby`, for stand-ins |
| `rac-rs/src/omni/hooks.rs` | `specs` opened to the omni module |
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

   Not shared yet: dropped bolts and other things spawned during play, except enemies and their shots (phase 3), and other pickups
   such as weapons.

   Known risk: a boss or an enemy tied to a story trigger is removed like any other. Killing one in one game may not run its
   script in the others, which could leave a door shut or a scene unplayed there. Not tested.
3. **Enemies fight everyone, in sync.** Done for placed enemies, spawned enemies and enemy shots.

   Seen working:
   - A mirrored toad ends on the same coordinates as the real one, about 100 ms behind.
   - When a player gets nearer, that player's game takes the toad over, and it chases and bites them for real.
   - A wrench swing in one game kills a toad running in the other.
   - Kerwan's lobbed bombs show in the other game on the same coordinates, shrinking as the real ones do.
   - A bomb's stand-in reaching a hero takes an orb of health, and the game's invincibility then absorbs the next hits.
   - Rilgar's level-start objects count once, including with a client arriving 40 seconds after the host.
   - An enemy carries on when its game goes, in each of these: the owner quits with a copy in the other game, or with only a stand-in
     there; the owner flies to another planet; the host does, and a client takes it on; the host quits, then a new host comes up and
     the client reports what it holds. It kept its position, animation and health in the logs, with no crash. The enemies
     in these tests were made by the game's `create_moby` from a test switch, not by a real spawner, which also sets them up.
   - A blow on an enemy's stand-in reaches the game that runs it. This trip was tested by queuing the blow directly: a real wrench
     swing on a stand-in was not seen landing.

   Not synced yet:
   - Enemy effects (flashes, beams, particles) and bursts on death. They exist only in the game running the enemy.
   - Handover of spawned objects during play: while its game stays in the level, a spawned enemy stays there too and goes for that
     game's hero, however near another player is.
   - A shot's blast radius in other games: a stand-in hits within one fixed reach, in flight only.
   - Blast weapons (bombs, rockets) on an enemy another game runs, unless the game's own blast list finds it. Its collision is
     stale there, so expect misses.
   - Enemy sounds in mirroring games.
   - Two clients in a level the host isn't in: only the host assigns enemies, so each runs its own. Following the host avoids this.

   How it behaves now:
   - An enemy class with no hook point (135 update functions have one), or one a mod has taken, runs in every game.
   - A class that keeps its own numbers in the animation bytes (one the class has no sequence for) doesn't mirror its animation.
   - Two shots of one class really fired within 12 m and 8 seconds of each other, in different games, count as one. So do two enemies
     of one class within 12 m, at any time: the second game's stands down.
   - A stand-in that takes over starts as new, without the settings a spawner gives it, and its AI starts again. A woken copy carries
     on from where it stood down. A class with no update, moved by a parent, stays where it is unless its parent comes with it.
   - A spawned enemy holds still in other games while its game is paused, and for up to two seconds after its game has gone.
   - A taken-over enemy keeps its AI state from when its new owner last ran it, so it may reset its behaviour at the handover.
   - Health is carried at the usual offset, which is class dependent: a class that keeps it elsewhere won't carry it over.
   - A bigger enemy can be hit a little further out or less far than its real shape: the test is one fixed reach.

4. **Session polish.** Done.

   Seen working: a client following the host to Kerwan, the `(paused)` and `(dialog)` tags, and a client getting back in after
   the host was killed and a new one started on the same port.

   How it behaves now:
   - Following is one way: a client that travels alone stays there until the host moves. `RAC_MP_FOLLOW=0` turns it off.
   - Following can take a client to a planet their save hasn't reached.
   - A host that restarts starts a new world list, and rejoining clients fill it in from what they remember.

   Not seen working yet: hiding puppets during cutscenes, and the `(shopping)` tag.

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
| `RAC_MP_FOLLOW` | `0`: a client stays where it is when the host travels. |

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
`-ShotField`) and run hidden. A forced `-Travel` tests level changes. Novalis leaves Ratchet in a default pose on arrival; Kerwan works, after Cross dismisses the
first-save notice, and it's where Clank shows on a new game.
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
