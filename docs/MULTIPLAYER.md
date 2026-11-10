# Online multiplayer

This fork adds online play to rac-rs: 2 to 4 players, each running their own copy of the game built from their own disc. Each
player sees the others in the same level. Upstream rac-rs has no multiplayer; everything here is this fork's own code.

**Status:** all five phases are built: the fight, the level's machinery and the team's progress are shared. Players connect and
see each other as animated Ratchets with names, Clank and what they hold, and hear them. Each placed enemy runs in one game and is
mirrored in the others, so all players see it in the same place doing the same thing, with its sounds, explosions and sparks. An enemy
or crate one player finishes dies in every game its own way, bolts and all. Shots, the enemies' and the players', show in every game,
a blow on a switch is dealt in every game, and a prop a blow breaks is gone for everyone. Doors, lifts and platforms move as the
game of the player who set them going has them, in every game, and a partner can ride a lift the other started. An item the game
gives a player, a planet it unlocks and a gold bolt collected count for the whole team. The team travels between planets together,
a level the host isn't in has its enemies shared too, a player who quits leaves the others' games at once, and a client that loses
the host gets back in. Tested with two games
on one PC, on the USA disc, directly and through a relay that delays, reorders and drops packets like the internet, and on two
machines: the host on Windows, the joiner a Linux build of the game on a second machine (the WSL VM: its own kernel, network
address, build and copy of the disc, drawing in software at a quarter of the speed), reaching the host through its firewall, and
Windows to Windows: the joiner a Windows 11 machine in Hyper-V, at about 1 field a second. Not run on the PAL v1.00 disc (no image of
it here), but checked for it: the two things multiplayer reads from each level's code are the same code on PAL v1.00 in all 19
levels (Testing, "Both discs"), and the rest is the port's own PAL addresses and hooks. The PAL image here is v2.00, which rac-rs
itself does not run (it stays in the title menus with no recompiled code). The limits are listed under each phase below.

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
  bolts. Every game names a placed object the same way: by its UID (the number the game's own "gone for good" bitmap uses) and where
  the level places it, or its class and place if it has no UID (rac-mp `ids.rs`). Its slot in the object table won't do: a respawn
  rebuilds the table without what is gone for good, and the slots after it move down (Novalis' bolt cranks went from 296 and 297 to
  294 and 295 when the hero died), and two crates of a class can share a UID.
  - When one goes from a player's game near that player, in normal play, that game reports it.
  - The host takes the first report of each object, keeps the list, and tells everyone else. A player who joins later gets the
    whole list.
  - Each game removes the object with the game's own `delete_moby`, and again if it comes back, as it does after a player's
    death restores a checkpoint.
- **Each enemy runs in exactly one game.** The host gives every placed enemy to the game of the player nearest to it, and moves
  it when another player gets clearly closer. In a level the host isn't in, the first player there does it, through the host. That game runs its real AI against a hero who is really there, so attacks,
  collision and damage are the game's own and nothing is faked.
  - That game streams the enemy's position, rotation, animation and health 25 to 30 times a second.
  - Every other game skips the enemy's update and mirrors it, about 100 ms behind.
  - A weapon's swing or shot is tested against where mirrored enemies really are, and a hit travels to the game running that enemy.
  - If the owner pauses, leaves or stops streaming for a second, the enemy runs locally again until the host reassigns it.
- **A spawned object runs in the game that made it, until that game goes.** Enemies and enemy shots made during play have no slot in
  common across games, so each game reports what it makes.
  - Only what appears during play is shared, with an update of its own, and nothing the level started with. The class types can't
    tell more: a cutscene's Qwark, a lit pillar and a race bike are all "enemies" to them, and every game has its own and needs it.
    In all 19 levels what this leaves is shots: the game places its enemies, and has them already when the level starts.
  - The host gives the object an id, or says it is a copy: the same class another game just reported near the same place, or, for
    an enemy, reported whenever a game gets there. A copy stands down, hidden and no longer updated.
  - The game that made it streams it with the placed enemies, and spawned enemies more slowly even when nobody is near. The others
    keep a stand-in, made by the game's own `create_moby`, which never updates and goes when the object is gone.
  - When that game leaves the session or the level for two seconds, the host gives each of its objects to the nearest player still
    in the level. That game wakes its copy where the object last was. With no copy, it starts its stand-in: the class's first update
    runs where it stands, and it keeps its health. A shot isn't handed over: it ends with its game.
  - An object left with nobody in its level is gone. If the host goes, each client keeps running what it holds and reports it to
    the next host.
  - A shot's stand-in that reaches the local hero in flight hits him through the game's own damage, knockback included.
  - Players' own shots (bombs, rockets, Blaster shots) are shared the same way, so each player sees his partner's in flight. They
    are nobody's copy, and their stand-ins hurt no one: what they hit, they hit in the game that fired them.
- **Sounds and explosions travel.** Hands at the game's `moby_cue` (an object's sound cue) and its explosion function catch what a
  game makes for what it runs, and the other games make the same calls on their copies, about 100 ms later to match the mirror.
  - An enemy's sounds play on its mirrored copy or stand-in; an explosion is made again where it was, whatever made it.
  - An explosion or glow is made for this game's copy of what it was made for. With no copy, it is made for a hidden object of the
    same class kept at that player's puppet, or for nothing if it was made for nothing; never for the local hero, whose memory the
    game would read as a bomb's and break on.
  - They are made inside this game's own frame, never between two fields: the game's functions for them can wait for a field, and
    a call made between two fields that does stops halfway and leaves the game broken (pair runs faulted so, after an explosion and
    after a cue). The hand at the hero's `update_motion_input` runs each first, as the game's own code calls it, on a stack area
    just below the caller's, and has it return to that entry, where the call goes on as it came, or the next one runs (24 a field
    at most). Inside a call made for the game (a mod's hook, `run_game`), the hand does nothing.
  - A player's own sounds (his hero, his wrench, what he holds) come from a hidden object of the same class kept at his puppet.
  - Something a game makes for itself just after making it for another game is not made twice.
- **What one player finishes dies everywhere.** An enemy or a crate gone in another game is dealt a finishing blow here by the local
  hero, so it dies or breaks its own way: its animation, sounds, explosion, the bolts it drops and whatever the level counts on it.
  The blow is dealt once the local hero is within 30 m, as a player there would, and again every three seconds till it falls. Further
  away the object is left as it is until he comes: on Kerwan, a crate the trains carry, hit, hidden or taken away from afar, broke
  the trains' update.
  - A copy still in its first state here (another game ran it, so its update never did here) is woken first: its own update runs,
    and the blow lands once it is awake. Hit asleep, Novalis' birdbots (class 459) took a flight path they never set up, divided by
    its point count of 0 and stopped the game. One not awake within a second is taken away instead.
  - Some enemies the level sends again after they fall (Novalis' class 459 and 660 come back elsewhere). A game that saw one end
    and sees it alive again tells everyone it is back (`Reliable::Back`): it lives in every game, is not finished again, and its
    next end is reported anew. The host forgets it, so whoever joins later is not told it is gone.
- **A blow on the level's objects is dealt everywhere.** A hand at the game's `get_hit`, where every object reads the hit pending on
  it, sees a placed object that is no enemy and no crate (a switch, a breakable prop, a target) take a player's blow, and every other
  game posts the same hit on its own. What such a blow breaks is then watched as a crate is: gone for everyone once it is gone in
  that player's game, a player too far for the blow to land, or one who comes later, included.
- **The level's machinery shows as the nearest player has it.** Doors, lifts, platforms, gates and traffic are placed objects that
  move and are neither enemies nor crates. Nobody is given them: each game runs its own, and a door opens for whoever comes to it.
  - The game of the player nearest to one that moves streams its pose, animation and state 25 to 30 times a second, and its
    variables (pvars: how far a crank was turned) once it comes to rest.
  - Another game shows it so, its update skipped, while its own hero is more than 5 m from it and that player is clearly nearer: a
    lift another player rides goes up with him, a door he opened stands open. When the stream stops at rest, it stays as last shown.
  - When a game's own hero comes near, that game runs the object itself again: from the other game's object at rest (pose, state
    and variables), or else from its own pose when it was taken, as if it had only paused.
  - Only what has not moved by itself in that game is shown: traffic, critters and a lift this player used stay his game's own.
    What keeps moving for 10 s is not streamed, and a class whose objects start again by themselves after resting sends no variables.
    Kerwan's traffic broke three ways before these rules: another car's variables written into one, one paused while the others went
    on, and one woken in another game's place.
- **The team's progress is one.** Hands at the game's own `item.give` and `planet.unlock`, and a watch on its gold bolt and mission
  bytes, see a player's game give him an item, unlock a planet, collect a gold bolt or complete a mission. Every other game then does
  the same, through the same function, when its player is in play: an item it doesn't have, with its banner, a planet still locked,
  a gold bolt not counted, a mission byte raised (0x14c050 + level * 16 + slot; 0xff is complete, and every level's code reads it in
  play). A player who joins later gets everything so far. What a player buys at a vendor stays his.
- **Cutscenes are shared.** Each level's code has its own "start scene N" (USA Veldin 0x298840): it keeps N in the level's scene
  block and sets the game mode to 2. Every game finds it in the loaded level by its code. When a player's game drops into a scene in
  play, it sends N; every other game in that level starts the same scene, in play, inside its own frame: a hand at the hero's
  `update_motion_input` turns that one call into "start scene N", which returns to the hero's update as the game's own scene starts
  do (called between two fields, the function waits for a frame and stops halfway). A scene a game already played in this visit to
  the level is not played again, so two players arriving together each see the arrival scene once.
  A sweep of all 19 USA levels found the scene start in every one (level 5's lies at 0x2c0f90, past where the search used to stop).
- **Hits are posted the way the game posts them.** A hit from another game goes straight into the game's 64-entry hit queue, as
  `hit_ring_write` writes it, with no call into the game: such a call between two fields could stop halfway and break the game.
  Each level links its own queue, at its own address (USA: 0x178100 on Veldin, 0x178580 on Novalis, 0x178200 on Kerwan, and so
  on), so the queue and its index word are read from `hit_ring_write`'s code in the level that is loaded. Found in all 19 levels.
- **Players stay in step.** Clients follow the host to another planet, through the same request the game's level ends make.
  Nobody is drawn over a cutscene, and a player in one isn't drawn. A name says when its player is paused, in a dialog or
  shopping. A client that loses the host keeps trying to get back in, and tells the host again what it knows is gone.
- **Nobody is dropped for a slow stretch.** A player is gone after 15 s with nothing heard from it, so a game that stalls a while (a
  level loading on a slow PC) stays in; a packet that would not decode still counts as heard. A player the host already dropped is
  told so by its next packet (`Bye`) and joins again at once. No float that is not finite goes out: the decoder refuses NaN and
  infinities so they never reach a game, and a hero part's NaN during Novalis' arrival made every hero state of that stretch
  undecodable; the joiner, which pings only when it sends nothing else, went unheard for 20 s and was dropped. A hero part the
  game has not set up yet (a turn, offset or scale of 1000 and more) is not sent at all, and an angle from another game is wrapped
  in one step: a loop taking turns off never ended on such a garbage turn (1e20 less a turn is 1e20), and the host's game stopped
  in it for good. Joins, drops and strangers are logged with the time of day (UTC) and what the session read, and the network
  thread says when it was held up, so the logs of two PCs line up.
- **What multiplayer changes in a game's memory:** it removes what another player removed, follows the host's travel, writes
  mirrored enemies and machinery, and gives what the team got. With nobody else connected it writes nothing. A 1800-field run without
  multiplayer always ends with the same RAM (c6bf2ae4…); one hosting alone ended so in 5 of 8 runs and in one other state
  (cb79ad38…) in 3, with the same multiplayer log. The only difference is the test's own key tap that skips the opening scene: it
  watches the game through the pipelined snapshot, and under the host's extra load it landed on field 1515 instead of 1514; the
  RAM differs from there (the hero's place, the pad, the camera). Multiplayer writes nothing. The PC's own load does the same: with other
  programs busy, every run without multiplayer tapped at 1515 and ended at cb79ad38…, the build from before this phase's
  changes (43892c7) exactly as the build after, so the two states are the key tap's two fields, not multiplayer's doing.
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
| `rac-rs/src/mp/spawned.rs` | Spawned objects: what is shared, reporting it, copies, stand-ins, and changing hands when a game goes. |
| `rac-rs/src/mp/fx.rs` | Sounds and explosions: the hands that catch them, and making them again for another game. |
| `rac-rs/src/mp/touch.rs` | Blows on the level's objects: the hand that catches them, and dealing another game's. |
| `rac-rs/src/mp/machines.rs` | The level's machinery: what moves, streaming it, showing another game's (a hero can ride it), and running it again. |
| `rac-rs/src/mp/testaid.rs` | Test aids for the pair tests: put the hero at a place at a field (`RAC_MP_TEST_GOTO`), log the placed objects of some classes (`RAC_MP_TEST_WATCH`) and dump one (`RAC_MP_TEST_DUMP`). |
| `rac-rs/src/mp/scenes.rs` | Shared cutscenes: finding the level's scene start, catching a scene this game starts, starting another game's. |
| `rac-rs/src/mp/story.rs` | The team's progress: the hands at `item.give` and `planet.unlock`, the gold bolt watch, and doing what another game did. |
| `tools/mp_build.ps1` | Rebuilds the game after a code change, with setup's exact settings, so only what changed is recompiled. `-Test` runs the network tests after, `-TestOnly` only those. |
| `tools/mp_play.ps1` | Starts a game and walks it into play on Veldin with no hands on the pad: for tests and screenshots. |
| `tools/mp_ride.ps1` | Two players at one machine: both travel to a level, each hero is put at a place at a field, and the machine and both heroes are printed side by side from the two run logs. |
| `tools/mp_sweep.ps1` | A host and a joiner on this PC in every level, one pair after another: both travel there, walk about and swing, and each game must run to its last field. |
| `tools/desk_run.ps1` | Starts a program on a desktop of its own, where its windows never show or take the focus: test games. |
| `tools/vm_relay.py`, `tools/vm_pair.ps1` | A UDP relay with delay, jitter and loss, run on another machine, and a pair whose joiner reaches the host only through it. |
| `tools/vm_game.sh` | Starts a game on a Linux machine, on a virtual screen of its own (Xvfb), and walks it into play as `mp_play.ps1` does. |
| `tools/vm_game_pair.ps1` | A pair on two machines: the host here, the joiner on the WSL Linux VM (`vm_game.sh`). |
| `tools/hyperv/` | A Windows 11 test VM in Hyper-V (`hv_create.ps1`, unattended `autounattend.xml`), the game in it (`hv_game.ps1`), a Windows-to-Windows pair (`hv_pair.ps1`), and `hv.ps1` to look at it or run a script in it. |
| `tools/mp_disc_check.py` | Finds, on a disc image, what multiplayer reads from each level's code, and for the USA disc where it lies on PAL v1.00. |

Upstream files touched, each by a line or a few, so merging upstream stays cheap:

| File | Change |
|---|---|
| `rac-rs/src/main.rs` | `mod mp;` |
| `rac-rs/src/retail/worker.rs` | `crate::mp::init()` after `omni::init` |
| `rac-rs/src/retail/worker/field_run.rs` | `crate::mp::tick()` after `omni::tick`, once per game tick; it gets the machine mutably, to remove objects |
| `rac-rs/src/retail/worker/field_front.rs` | `crate::mp::note_front()` next to `omni::note_front` |
| `rac-rs/src/retail/worker/field_snap.rs` | the puppets join each snapshot's object list; `crate::mp::leave()` where a test run ends |
| `rac-rs/src/retail/debug_aids.rs` | `crate::mp::leave()` where a test run ends, on the line of its neighbour |
| `rac-rs/src/retail/prop_pools.rs` | the puppets' object pools (only with `RAC_MP`) |
| `rac-rs/src/retail/moby_state.rs` | `euler_from_rows` opened to the crate, to turn a part's matrix into its rotation |
| `rac-rs/src/viewer/app.rs` | Ratchet's and the wrench's models for the puppets (only with `RAC_MP`). On the same lines as their neighbours, because upstream caps that function's length (`SIZE_CAPS.txt`). |
| `rac-rs/src/omni/draw.rs` | a text list of the names' own, merged like the debug dots; the window layout is kept with `RAC_MP` too |
| `rac-rs/src/omni/mod.rs` | `screen_of` and `OmniDrawCmd` opened to the crate, for the names; `delete_moby`, which runs the game's function with no mod loaded; `hook_specs` and `allow_hook_hands`, for the enemy hands; `create_moby`, for stand-ins and voices; `func_reloc`, `find_entry`, `entry_holds`, `run_game` and `call_game`; `game_entry` (where a game function is in the loaded code, a miss not remembered: `funcs::locate`) and `in_made_call`, for calls made in the game's own frame |
| `rac-rs/src/omni/hooks.rs` | `specs` opened to the omni module |
| `rac-rs/src/viewer/handler.rs` | the first frame is asked for when the window is made, not left to the system's first paint: X11 never paints a window placed off-screen (a test window under Xvfb), and the game waited for ever; `crate::mp::leave()` when the window closes, so the others are told |
| `Cargo.toml`, `rac-rs/Cargo.toml`, `Cargo.lock` | the `rac-mp` crate |

Addresses come from the OmniLoader binding, `rac-rs/src/omni/rac1.json`. Both supported discs use its "ntsc" data layout
(see `omni/layout.rs`).

## Phases

0. **Connect and exchange.** Done.
1. **Puppets.** Done:
   - Remote players show as animated Ratchets, with names above their heads and what they hold.
   - Offline mirror mode exists for testing.

   - On Kerwan each game shows the other player with his pack, Clank and the wrench (parts 607, 601 and 71 in both run logs), and
     a held Bomb Glove (192) comes across as the wrench does.

   Still to look at in a picture: a weapon other than the wrench in a puppet's hand.
2. **Shared world.** Done for enemies, crates and gold bolts the level placed (a gold bolt counted in the other game: phase 5).

   How it behaves now:
   - Rewards stay with the player who earned them: bolts drop in that player's game only.
   - In the other games the object just vanishes, with no death animation, and it goes when the killer's copy is deleted, so
     a little after the hit.
   - Enemies move on their own in each game, so the same enemy can be in different places. Phase 3 narrows that.

   Not shared yet: dropped bolts and other things spawned during play, except enemies and their shots (phase 3), and other pickups
   such as weapons.

   Known risk: a boss or an enemy tied to a story trigger is removed like any other. Killing one in one game may not run its
   script in the others, which could leave a door shut or a scene unplayed there. Not tested.
3. **Enemies fight everyone, in sync.** Done for placed enemies, shots, sounds, explosions, deaths and blows on the level's objects.

   Seen working:
   - A mirrored toad ends on the same coordinates as the real one, about 100 ms behind.
   - When a player gets nearer, that player's game takes the toad over, and it chases and bites them for real.
   - A wrench swing in one game kills a toad running in the other.
   - Kerwan's lobbed bombs show in the other game on the same coordinates, shrinking as the real ones do. Both games fire them, so
     one game's three stand down and show the other's.
   - Gaspar's shots (class 1886), which only one game fired, show in the other.
   - A bomb's stand-in reaching a hero takes an orb of health, and the game's invincibility then absorbs the next hits.
   - Each game keeps its own of what a level starts with, or a scene brings: Qwark's helicopter in Umbris's intro, Gemlik's pillars,
     Rilgar's race bikes.
   - A spawned enemy carries on when its game goes, in each of these: the owner quits with a copy in the other game, or with only a
     stand-in there; the owner flies to another planet; the host does, and a client takes it on; the host quits, then a new host
     comes up and the client reports what it holds. It kept its position, animation and health in the logs, with no crash. No such
     enemy was found in the game (below), so these were made by `create_moby` from a test switch. The hand-over waits for one.

   What the game spawns: a sweep of all 19 levels, on a 16 m grid of where objects stand, with the hero kept alive, found no enemy
   spawned in normal play. What showed up as one was a cutscene's prop, or a part some controller moves. On Kerwan, Eudora, Umbris
   and Gaspar the game's own class list also shows every enemy class it loaded is placed, and that only shots are not; that list was
   not complete on the other levels. A spawner behind a trigger the sweep never pulled, such as a story step or a gadget, can't be ruled out.
   - A blow on an enemy's stand-in reaches the game that runs it. This trip was tested by queuing the blow directly: a real wrench
     swing on a stand-in was not seen landing.
   - Sounds and explosions: on Veldin and Kerwan, each game made 200 to 300 of the other's in a run (toads' and Kerwan's enemies' cues,
     death explosions), and the voices of the other player's hero and wrench were made.
   - Deaths: a toad or crate finished in one game dies or breaks in the other its own way; across three Veldin pairs, 23 to 40 a game
     did, and the few left were too far from that player to update.
   - Bomb Glove blasts from one game land on enemies the other game runs: 29 bomb hits travelled in one run.
   - Across machines: the host hosting on every network (`host`), the joiner given only the address of a relay on another machine
     (the WSL VM, `vm_relay.py`), which forwarded each way with 40 ms delay, 15 ms jitter and 2 % loss, so the host was reached
     through its firewall from another machine's address. On Veldin the pair joined, stayed connected through 4000 to 5000 fields
     with no drop, and mirrored over 12000 enemy updates each way; each game made 512 and more of the other's sounds and effects in
     its own frame, the team's item and the arrival scene came across, and the joiner's four broken crates and one killed toad were
     finished in the host's game when its hero walked up. Until `rac-rs.exe` was allowed through the firewall, the joiner logged
     `no answer from ... in a minute` (Playing, above). The joiner's game ran on this PC there; the next item has it on another.
   - Two machines: the host on Windows here, the joiner a Linux build of the game in the WSL VM (its own kernel, address, build and
     copy of the disc, at 8 to 15 fields a second: no GPU there), reaching the host at its address on the VM's network, through its
     firewall (`tools\vm_game_pair.ps1`). On Veldin the joiner walked to the four crates ahead of the spawn and broke them; the host,
     walking there later, finished the same four (ids 0x1875, 0x2076, 0x3877, 0x180f). Over 63000 enemy updates were mirrored to the
     host and over 8000 back, each game made the other's sounds and effects in its own frame (2084 and 511), the team's item and the
     arrival scene came across, and one session lasted the whole run (16634 packets read, none undecodable), with no fault in either
     game. On Novalis it found three faults, now fixed (How it works, "Nobody is dropped for a slow stretch"): a hero part not set up
     yet made every state of the arrival undecodable, and the joiner was dropped as silent and joined again; with that part sent as
     0 its garbage turn stopped the host's game in the angle wrap; and the 5 s timeout was short for a slow machine's level load.
     After the fixes, an 18000-field host and a 6000-field joiner on Novalis ran with no drop and no hang (9564 packets read, none
     undecodable). The single-player check still ends at c6bf2ae4…. The X11 window under Xvfb found one more: the game waited for a
     first paint that X11 never sends to a window off-screen (`viewer/handler.rs`).
   - Windows to Windows: the host here, the joiner a Windows 11 machine in Hyper-V (`tools\hyperv`), reaching the host at its address
     on Hyper-V's Default Switch, through its firewall. It draws on the CPU, so it played at about 1 field a second against the host's
     60: a very slow PC. The host broke Veldin's four crates first; the joiner, walking there later, finished the same four (ids
     0x1875, 0x180f, 0x3877, 0x2076). Over 107000 enemy updates were mirrored to the host and over 4000 back, each game made the
     other's sounds and effects in its own frame (11795 and 222), the item and the arrival scene came across, and one session lasted
     the 35 minutes (130013 packets read by the joiner, none undecodable), with no fault in either game.
   - Effects in the game's own frame: four Novalis pairs, a Veldin pair and a Kerwan pair, the host killing an enemy every 150
     fields with the joiner beside him, ran to the end with no fault; each game made 256 to over 1000 of the other's sounds and
     effects in its own frame, none left halfway. Before, two of those runs stopped the joiner right after a cue or explosion made
     between two fields could not return.
   - Finishing across levels: the joiner finished 47 of the host's kills on Veldin, 8 to 17 on Novalis and 17 on Kerwan, waking a
     sleeping copy first 1 to 2 times a run. Enemies the level sent again came back in both games (Novalis 10 to 12 a run, Kerwan 7)
     and were not finished again; one killed a second time was reported anew and finished in the other game.
   - A player's bombs show in flight in the other game.
   - Hits outside Veldin: until the hit queue was read from each level's code, a hit from another game was written at Veldin's
     queue in every level, where no other level reads it. On Novalis, 23 hits posted on the nearest enemy, one every 120 fields:
     with Veldin's address the same enemy took all of them and lived; with Novalis' own queue, 15 enemies died one after the other.
     In a pair on Novalis, a joiner's wrench hit on an enemy the host runs killed it in the host's game. Earlier results for blows
     on Novalis' and Eudora's objects were taken with the wrong queue and are not counted.
   - Two clients on Kerwan with the host on Veldin share their enemies (each mirrors the other's).

   - Sparks and glows (`spark_spawn`, `glow_flare_spawn`) are caught and made again like explosions. None came up in the runs
     (the run log counts each kind), so this is not yet seen working.

   Not synced yet:
   - Beams and particles an update makes on its own, without one of the functions above.
   - Cutscenes are shared (phase 5).
   - Handover of spawned objects during play: while its game stays in the level, a spawned enemy stays there too and goes for that
     game's hero, however near another player is.
   - What moves with an enemy but has no update of its own, such as a part some controller drives: it stays each game's own, so it
     can stand still where the enemy is mirrored.
   - A shot's blast radius in other games: a stand-in hits within one fixed reach, in flight only.

   How it behaves now:
   - An enemy class with no hook point (135 update functions have one), or one a mod has taken, runs in every game.
   - A class that keeps its own numbers in the animation bytes (one the class has no sequence for) doesn't mirror its animation.
   - Two shots of one class really fired within 12 m and 8 seconds of each other, in different games, count as one. So do two enemies
     of one class within 12 m, at any time: the second game's stands down.
   - A shot fired in a level's first two seconds of play isn't shared: it can't be told from what the level started with.
   - A stand-in that takes over starts as new, without the settings a spawner gives it, and its AI starts again. A woken copy carries
     on from where it stood down. A class with no update, moved by a parent, stays where it is unless its parent comes with it.
   - A spawned enemy holds still in other games while its game is paused, and for up to two seconds after its game has gone.
   - A taken-over enemy keeps its AI state from when its new owner last ran it, so it may reset its behaviour at the handover.
   - Health is carried at the usual offset, which is class dependent: a class that keeps it elsewhere won't carry it over.
   - A bigger enemy can be hit a little further out or less far than its real shape: the test is one fixed reach.
   - The finishing blow is the wrench's: a class only something else can hurt is taken away after three seconds instead.
   - Both players get the bolts of what is finished: each game drops its own.
   - A player's sounds, and what his explosions were made for when this game has no copy, come from at most six classes a player.
   - While the local hero dies or falls, on the way to a respawn, the voices are taken away, no stand-in is made, machinery runs
     locally and no progress is given: the respawn resets what it finds of the hero's class. They come back in play.
   - Every stand-in and voice is checked to be still this game's before each pose written into it or call made with it.

4. **Session polish.** Done.

   Seen working: a client following the host to Kerwan, the `(paused)` and `(dialog)` tags, and a client getting back in after
   the host was killed and a new one started on the same port.
   - The team travels together: a joiner flew from Veldin to Kerwan alone, told the host (`here in level 3 ahead of the host`), and
     the host's game went after it (`player 1 went to level 3: the team goes too`) and played on with it there.
   - A game that quits says so: the joiner's game ended 2000 fields before the host's, and the host had it gone 1 ms later
     (`leaving: the others are told` at 01:16:26.739, `player 1 left` at 01:16:26.740). Before, a player who quit stood still in
     the others' games for the 15 s it takes to give a silent one up.

   How it behaves now:
   - The team travels together. A client follows the host to another planet, and when a client takes a ship by itself, the host's
     game goes after it and the other clients after the host. `RAC_MP_FOLLOW=0` turns it off for the game it is set in: a client
     with it stays where it is, and a host with it lets a client travel alone.
   - Following can take a client to a planet their save hasn't reached.
   - A host that restarts starts a new world list, and rejoining clients fill it in from what they remember.
   - Quitting (the window closed) tells the others at once; a game that is killed or loses its network is given up after 15 s.

   Not seen working yet: hiding puppets during cutscenes, and the `(shopping)` tag.

5. **Machinery and the team's progress.** Done.

   Seen working:
   - Veldin: the host's moving objects of class 1564 show in the joiner's game on the coordinates the host streamed (slot 201 at
     the host's field 1800 is where the joiner shows it), and the joiner's in the host's.
   - Novalis: the pistons (class 715) and lily pads (695) stream from the nearer player's game, and the other shows them; one is run
     locally again as soon as that game's hero comes within 10 m.
   - A bolt crank: the host turned Novalis' crank 297 to 0.42 of a turn (its pvars hold it at +0x18) while the joiner was 70 m away;
     when the joiner walked up later without touching it, its own crank held exactly the same.
   - Cutscenes: the host's game started Veldin's scene 1 (the level's own function, asked for at field 3000) and the joiner's game
     played the same scene through: 564 fields in each, back to play after. Each game's arrival scene played once in each.
   - Mission bytes: Kerwan's own game completed mission slot 1 in one game, and the other got it; a slot set in the host's reached the
     joiner.
   - Kerwan with deaths: a joiner breaking crates, killed, respawning (its table rebuilt two slots shorter) and breaking more, and a
     host that fell into a pit: every object both games named was the same one (same coordinates in both logs), and four games ran
     to the end.
   - Through the relay (60 ms each way, 25 ms jitter, 3% loss): a planet unlocked and an item given in the host's game (Kerwan,
     Eudora and Rilgar's coordinates, the Suck Cannon and the Trespasser, made by calling the game's own functions) were given in the
     joiner's game, and a gold bolt counted there. The Bomb Glove each game gives itself at level load is not given twice.
   - Riding a partner's lift, on Aridia's elevator 0x103 (class 651, `tools\mp_ride.ps1`): the host stepped in at the top and it went
     down; the joiner, dropped into the shaft after it had left, landed on the host's elevator as his game showed it and rode it the
     last 9 m down standing on it (0.02 to 0.3 m above its floor); at the bottom his game ran it again from the host's state and
     variables, and it stayed there with him on it. The host then stepped out and back in, and the elevator went up: the joiner,
     dropped onto it on the way, rode it 23 m up to the top, where both games' elevators ended at 34.00 m, in the same state, a hero
     on each. Two players who step in together each send their own down, side by side (both arrived at 5.50 m on the same field).

   How it behaves now:
   - Machinery is shown, not owned, and the player who sets it going drives it: while it moves, every other game shows it as his
     game has it, his partner's too if standing at it or on it, who rides it with him (a shown object keeps its collision). Two
     players who set it going together each ride their own. At rest, each game near it runs it again from where it stopped there.
   - A puzzle's progress reaches a partner whose game shows the object (within 120 m and further than 5 m when it moved). One who
     comes later from further away finds it as his own game left it, unless the result is a mission byte, an item or a planet.
   - What moves on its own is each game's: Kerwan's traffic is not in step between games.
   - Skill points and the save are each player's own.

   Switches and breakable props, all levels: every placed object whose update calls the game's `get_hit` (directly or one call
   down) was found from its code (on Novalis 23 besides enemies and crates, on Kerwan 26), the hero stood by it, and after a
   window without a hit it took one with the mask it asks for. In 16 levels so tested (Umbris was not reached, Gemlik and
   Kalebo III only in part), what one blow changes is:
   - A break (state 0xfd): breakable props on most levels, such as Novalis' classes 704, 729, 754, 1042 and 1813 and Kerwan's
     817. In a pair on Novalis every blow the host's game caught was dealt in the joiner's (13), and each of the 9 props the host
     broke was broken or removed in the joiner's game; on Kerwan, 17 blows and its breakable likewise.
   - A switch: Oltanis' class 1417 (state 1 to 3) and Kalebo III's class 546 (1 to 2), both timed: they go back by themselves.
     With the joiner beside the host, each switch the host hit was thrown in the joiner's game too, and went back after as
     long in both (499 fields on Oltanis, 300 on Kalebo III).
   No object was found that stays thrown from one blow, so nothing of a switch is kept for a player who comes later.

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
| `RAC_MP_FOLLOW` | `0`: this game does not follow the others to another planet (a client stays when the host travels; a host stays when a client does). |

The host must let UDP port 2410 through its firewall, and through its router to play over the internet. Join, leave and position
lines (`mp: ...`) go to the run log, `player\log.txt`.

Windows asks once whether `rac-rs.exe` may take connections. If that question was cancelled or never seen (a game started
hidden), Windows adds rules that block it, and nobody on another PC gets in: their log says `mp: no answer from ADDR in a minute`.
To check, in PowerShell:

```
Get-NetFirewallApplicationFilter | Where-Object Program -match 'rac-rs' | Get-NetFirewallRule | Format-Table DisplayName, Direction, Action, Profile
```

A `Block` there is the reason. Change it in Windows Security > Firewall & network protection > Allow an app through firewall
(tick `rac-rs.exe` for the network you play on), or in an administrator PowerShell:

```
Get-NetFirewallApplicationFilter | Where-Object Program -match 'rac-rs' | Get-NetFirewallRule | Where-Object Action -eq Block | Set-NetFirewallRule -Action Allow
```

Joining needs nothing of the kind: the joiner's own firewall lets the host's answers back in.

On Linux it is the same with `./play.sh`, for example `RAC_MP=join:192.168.1.20 RAC_MP_NAME=Friend ./play.sh`. A Linux game and a
Windows game play together.

## Testing

**Network tests.** The network crate tests itself over real sockets on 127.0.0.1, with no game and no disc. That covers:
joining, relaying, a full session, the other region, other protocol versions, leaving, timeouts, a host that's still
starting, keepalive while loading, malformed and random packets, and smoothing. `tools\mp_build.ps1 -Test` runs them after a
build.

**Across machines.** `tools\vm_relay.py` runs on another machine and forwards UDP to the host with delay, jitter and loss; in
WSL: `python3 -I vm_relay.py 2411 HOST_ADDR 2410 40 15 0.02 900` (HOST_ADDR: this PC's address on the WSL network). Then
`tools\vm_pair.ps1 -Relay RELAY_ADDR` starts a host here on every network and a joiner given only the relay's address
(`-JoinPad`, `-HostPad`: their stick and buttons). The host's firewall must let `rac-rs.exe` in (Playing, above).

**Two machines.** The WSL VM stands in for a second PC: a Linux build of the game of its own, with its own copy of the disc and its
own network address. Once: copy the branch in (`git bundle create b.bundle mp-machinery-story`, then in WSL
`git clone -b mp-machinery-story b.bundle ~/racvm/game`), copy the disc to `~/racvm/usa.iso`, and run `./setup.sh --iso
~/racvm/usa.iso` there (Rust and the rest come with it; nothing is installed system-wide). Then `tools\vm_game_pair.ps1` starts
the joiner in the VM (`tools/vm_game.sh`: a virtual screen of its own, muted, run log in `~/racvm/join-PORT/log.txt`) and, once it
nears play, the host here; both logs end up in `%TEMP%\racmp\vmg-host-PORT`. `-Level`, `-HostPad`, `-JoinPad`, `-HostFields` and
`-JoinFields` as for the other pairs. With no GPU in the VM the software Vulkan driver draws, at 8 to 15 fields a second.

**Windows to Windows.** `tools\hyperv\hv_create.ps1 -WinIso <a Windows 11 ISO>` makes a VM in Hyper-V (your account in Hyper-V
Administrators) and installs Windows unattended; it touches nothing else in Hyper-V (its own VM on the Default Switch, half the
default CPU weight, no GPU partition), and keeps the test account's generated password in `%USERPROFILE%\vm\hyperv`, outside the
repo. `hv.ps1 wait` waits for it, `hv_game.ps1 -Copy -Iso <the disc>` puts the game in it over PowerShell Direct (no network share),
and `hv_pair.ps1` hosts here and joins from it. The VM has no GPU: Windows' own software one (WARP) lost its device and ran the game
out of memory, so it draws through Vulkan on the CPU, with the SwiftShader driver and Vulkan loader Microsoft Edge ships. QEMU with
the Windows hypervisor was tried first: it stopped or froze the guest in every configuration tried.

**Both discs.** `python -I tools\mp_disc_check.py DISC.iso` builds each level's RAM from the disc as the generator does and looks
for the two things multiplayer reads from a level's own code, as the game does: the hit queue in `hit_ring_write` and the level's
"start scene". On the USA disc it finds both in all 19 levels, at the places the game logs (Veldin's queue 0x178100, Novalis'
0x178580, level 5's scene start 0x2c0f90), and places both functions in the port's PAL v1.00 -> USA map
(`retail/disc_map.rs`, made from facts about both discs): each lies wholly inside one aligned run, so PAL v1.00 has the same code
there (Veldin's `hit_ring_write` at PAL 0x259698, the very number `mp/enemies.rs` names it by). Given a PAL v1.00 image it looks
there directly. Everything else multiplayer touches is the port's own: game addresses in PAL terms (`disc::va` moves them only on
USA), and hooks and game functions found by their code, from specs made on PAL v1.00. A PAL game runs at 50 fields a second and
says so when it joins; a PAL and a USA game don't play together (the host refuses the other region). The PAL v2.00 disc is a
revised build: its "start scene" has an early check added and its data moved (the mode word at 0x15f6a8, 0x15f5a4 on v1.00).

**Every level.** `tools\mp_sweep.ps1` runs a host and a joiner on this PC in each of the 19 levels, three pairs at a time: both
travel there, walk about with a swing every 24 fields and a jump now and then, turning every 300, for 6000 fields (the joiner
5800). In all 19 both games ran to their last field, with no fault, no drop and nothing that could not be done (`mp: could not`).
In 14 of them a player broke or killed something (2 to 13 objects a level), and the other game finished the same ones when its
hero was near. The one drop it showed was Gaspar's host giving the joiner up 15 s after the joiner's game had ended, which is why
a game that quits now says so (phase 4).

**Two players at one machine.** `tools\mp_ride.ps1` sends a host and a joiner to a level and puts each hero at a place at a
field of its own (`RAC_MP_TEST_GOTO`, mp/testaid.rs), then prints the watched object and both heroes side by side every 30
fields from the two run logs (`RAC_MP_TEST_WATCH`: shown from another game, or run here). The joiner's game runs about 110
fields behind the host's, so the same field number is a later moment for it. On Aridia the elevator 0x103 at (172, 276) goes down
from 34 m when a hero steps in, and up again when he steps out at the bottom (at 174.4, 266.45) and back in:

```bat
powershell -ExecutionPolicy Bypass -File tools\mp_ride.ps1 -Level 2 -Watch 651 -Id 0x103 -HostGoto "3200:172,276,34.6;3650:174.4,266.45,5.6;3800:172,276,6.2" -JoinGoto "3720:172,276,14"
```

`RAC_MP_TEST_DUMP=ID` adds the object's memory, to compare two games' copies: that is how the elevator's destination was found
past the 32 pvar words a message carried then (`MAX_VARS` is 64 since).

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

**Like the internet.** A small UDP relay between the joiner and the host (a script, not part of the repo) can delay each packet
(60 ms each way), add jitter that reorders them (25 ms) and drop some (3%). Runs of 5000 to 6000 fields through it, with the
players touring enemies and crates and throwing bombs, stayed connected, mirrored enemies, finished kills and shared effects and
progress. These soaks found three ways the game broke, now fixed: a replayed explosion made for the local hero, poses written into
a stand-in or voice the game had already taken away (which corrupted the free list `create_moby` walks), and a voice of the hero's
own class left in place through a respawn. After the fixes, 16 relayed games in a row with 20 to 60 replayed explosions each ran
to the end, where before 2 or 3 in 12 broke.

**Without the games taking the screen.** `tools\mp_play.ps1` starts every test game (`-Hidden` or `-Fields`) on a Windows desktop of
its own (`tools\desk_run.ps1`, desktop `racmp-test`): its window never shows on the user's desktop or takes the focus, and it runs
and renders the same (an 1800-field run ends with the same RAM). `-OnDesktop` starts it on the user's desktop as before. `-Iso PATH` plays another disc image than setup's.

**Running the tests without a built game.** The workspace can't load until setup has generated the game code. Before that,
copy `mp\` somewhere else, add an empty `[workspace]` line to the copy's `Cargo.toml`, and run `cargo test` there.

## Keeping up with upstream

`upstream` is AsobiCodes/rac-rs (fetch only), and `origin` is this repo:

```bat
git fetch upstream
git merge upstream/main
```

Setup rewrites `Cargo.lock` for your disc's generated crates. Don't commit those changes.
