# Keeping up with upstream rac-rs

rac-rs-multiplayer is upstream [AsobiCodes/rac-rs](https://github.com/AsobiCodes/rac-rs) plus a layer of its own: online
multiplayer (`mp/`, `rac-rs/src/rac1/mp*`), the Archipelago randomizer (`archipelago/`, `rac-rs/src/rac1/ap*`) and the fork's
setup, update and release tools. Upstream's updates are wanted here, every one, so this page says how they arrive, how a merge is
done and checked, and the rules that keep each merge cheap.

## How updates arrive

| What | Where it runs | What it does |
|---|---|---|
| `.github/workflows/upstream-watch.yml` | GitHub Actions, daily (and by hand: Actions > Upstream watch > Run workflow) | Compares `main` with upstream's `main`. When upstream has commits this fork lacks, it opens one issue (label `upstream-sync`) with the upstream version, its commits and the files a merge would conflict in, and keeps it up to date; it closes the issue once `main` has everything of upstream. Needs no disc and changes no code. |
| `tools/upstream_sync.ps1` | This PC (it needs the disc for the game code and the build) | Does the merge in a worktree beside the repository, resolves the bookkeeping files by rule, generates the game code again when upstream changed the generators, checks, tests and builds, commits the merge, and with `-Push` pushes it to `main`. |
| The scheduled task "rac-rs upstream sync" | Claude Code on this PC, daily at 10:00 (while the desktop app runs; a missed run runs at the next start) | Runs `tools/upstream_sync.ps1 -Check`; when upstream has something new, runs the merge, resolves what is left as this page says, verifies, pushes to `main` and writes the release notes. It asks before publishing a release. |

```powershell
powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Check      # anything new? which files would conflict?
powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1             # merge in ..\rac-rs-multiplayer-sync and verify
powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Continue   # after resolving the conflicts it listed
powershell -ExecutionPolicy Bypass -File tools\upstream_sync.ps1 -Push       # verify, commit, push to origin main, remove the worktree
```

The report of the last run is `generated\upstream_sync.md`. Exit codes: 0 done or up to date, 10 (`-Check`) upstream has new commits,
20 conflicts are left, 30 verification failed.

Players get the update when a release is published (`tools\make_release.ps1 -Publish`): the setup window and `setup.ps1 -Launch`
offer the newest release of **this** repository (never upstream's, whose code has no multiplayer), and the Linux game's notice reads
this repository's `VERSION`.

## Versions

A release of this fork is upstream's version plus a fourth number: `0.5.1.1` is the first release on upstream `0.5.1`, then
`0.5.1.2`, ... The sync sets `VERSION` to `<upstream>.1`. Every version reader accepts four numbers (`setup.ps1`,
`tools/update_apply.py`, `rac-rs/src/rac1/update_notice_linux.rs`), and `0.5.1` < `0.5.1.1` < `0.5.2`, so a player is never offered
upstream's bare version as an update.

When a sync changes how the game under multiplayer behaves (a new travel flow, a different engine), raise `VERSION` in
`mp/src/proto.rs`: games of different protocol versions do not join each other, so players are not mixed across the change.

## The rules that keep merges cheap

1. **The fork's code lives in files upstream does not have.** Multiplayer: `rac-rs/src/rac1/mp.rs`, `rac-rs/src/rac1/mp/`, the
   `mp/` crate. Archipelago: `rac-rs/src/rac1/ap.rs`, `rac-rs/src/rac1/ap/`, `archipelago/`. The seams: `rac-rs/src/rac1/fork_pack.rs`
   (the one call the retail worker makes for the fork, and R&C1 game functions the fork calls) and `rac-rs/src/engine/omni/fork.rs`
   (everything the fork needs from the mod host). Tools: `tools/mp_*`, `tools/ap_play.ps1`, `tools/hyperv/`, `tools/vm_*`,
   `tools/upstream_sync.ps1`, `tools/make_release.ps1`, `launcher_pages.ps1`.
2. **An upstream file gets a call, not code.** One line that calls into the fork's files, marked `[MP]` or `[AP]`. Where upstream
   caps a function's length (`rac-rs/SIZE_CAPS.txt`), the call goes on the same line as its neighbour so the function does not grow.
3. **The engine stays game-neutral.** `rac-rs/src/engine` must not name a `rac1` module or hold an R&C1 address (upstream's
   `boundary_tests.rs` checks this). What the engine needs to ask R&C1 for the fork goes through the gates in `engine/omni/fork.rs`,
   which `rac1/fork_pack.rs` sets at start.
4. **Upstream's text stays upstream's.** Docs and comments of upstream files are not reworded; the fork documents itself in its own
   files (`docs/MULTIPLAYER.md`, `docs/ARCHIPELAGO.md`, this page).
5. **A new touch point is written down here** (table below), in the same change that adds it.

## Upstream files the fork changes

Everything else upstream ships is taken as it is. Search for `[MP]` / `[AP]` to find the lines.

| Upstream file | The fork's change |
|---|---|
| `Cargo.toml` | the `mp` workspace member |
| `rac-rs/crates/rac1/Cargo.toml` | the `rac-mp` dependency |
| `.gitignore` | the Python caches of `archipelago/` |
| `setup.ps1` | the fork's own window: Play together, the second desktop shortcut, updates from this repository's releases, no RAC-RS Launcher (the fork's largest touch point) |
| `Setup.cmd` | always the setup window, never the RAC-RS Launcher |
| `tools/update_apply.py` | this repository's release archives, four-number versions, never a branch's code |
| `install-desktop-entry.sh` | the desktop entry's name `rac-rs-multiplayer` |
| `oracle/src/mem.rs` | an unmapped access names the instruction that made it |
| `rac-rs/src/engine/install.rs` | the window title `rac-rs-multiplayer vX` |
| `rac-rs/src/engine/omni/mod.rs` | `mod fork; pub use fork::*;` |
| `rac-rs/src/engine/omni/hooks.rs` | `hook_fn` (a slot's hand), `specs` opened to the omni module |
| `rac-rs/src/engine/omni/funcs.rs` | `locate` (a lookup whose miss is not remembered) |
| `rac-rs/src/engine/omni/draw.rs` | the players' names' and the randomizer's draw lists, kept off movies; the overlay layout kept while multiplayer runs |
| `rac-rs/src/rac1/lib.rs` | `pub mod ap; pub mod fork_pack; pub mod mp;` |
| `rac-rs/src/rac1/omni_bind/rac1_funcs.txt` | the game functions multiplayer calls or hooks |
| `rac-rs/src/rac1/update_notice_linux.rs` | this repository's `VERSION`, four-number versions, the fork's update text |
| `rac-rs/src/rac1/host/app.rs` | Ratchet's and the wrench's models for the puppets (two calls on their neighbours' lines) |
| `rac-rs/src/rac1/host/handler.rs` | the first frame asked for at window creation (X11 test windows); `mp::leave` in `quit_app` |
| `rac-rs/src/rac1/retail/worker.rs` | `fork_pack::init()` after `omni::init` |
| `rac-rs/src/rac1/retail/worker/field_run.rs` | `mp::tick`, `ap::tick`, `ap::note_buttons` |
| `rac-rs/src/rac1/retail/worker/field_front.rs` | `mp::note_front` |
| `rac-rs/src/rac1/retail/worker/field_snap.rs` | the puppets and their effects in each snapshot; `mp::leave` where a test run ends |
| `rac-rs/src/rac1/retail/debug_aids.rs` | `mp::leave` where a smoke run ends |
| `rac-rs/src/rac1/retail/prop_pools.rs` | the puppets' object pools |
| `rac-rs/src/rac1/retail/moby_state.rs` | `euler_from_rows` opened to the crate |
| `rac-rs/src/rac1/retail/fx_queue.rs` | `weapon` (the player's own weapon quads) and `effect_pool_cells` |
| `rac-rs/src/rac1/retail/queue_b.rs` | `fill` marks the weapon quads; `arc_tables_in` |
| `rac-rs/src/rac1/retail/tick.rs` | `mp::dump_recent` when a run stops |
| `rac-rs/src/rac1/retail/vendor.rs` | `rows_built`, `rows` for the randomizer's shop |

### Bookkeeping files

These differ on every release on both sides, and `tools/upstream_sync.ps1` resolves them by rule:

| File | Rule |
|---|---|
| `VERSION` | `<upstream version>.1` |
| `CHANGELOG.md` | a `## v<upstream>.1` section (a stub to finish before releasing), upstream's new sections, then the fork's sections since the last sync: newest version first, so the update window shows a player every section between their version and the new one |
| `Cargo.lock` | upstream's; cargo adds `rac-mp` and the generated crates at the first build |
| `release_files.txt` | written again from the tree (`git ls-files`) before the commit |
| `README.md` | the fork's side of a conflict that names this repository (the download line); any other README conflict is left for a person |

## Resolving the conflicts that are left

1. Take **upstream's** side, then put the fork's marked lines back. `git show origin/main:<file>` shows the fork's old version; the
   `[MP]` / `[AP]` lines in it are what to carry over. Where upstream rewrote the code around a call, put the call at the place that
   does the same job now (example from v0.5.1: window close and Quit to Desktop became one `quit_app`, so `mp::leave` went there and
   now covers both).
2. **Moved files**: git follows renames (the script raises `merge.renameLimit`), so a fork line in a moved upstream file usually
   merges by itself. The fork's own files do not move with upstream's: move them to the matching place (v0.5.0 put the game's code in
   the `rac1` crate, so `src/mp*` and `src/ap*` went to `src/rac1/`) and fix the paths in `docs/`.
3. **New upstream fields and arguments** show as compile errors in the fork's code (v0.5.0: `Moby::additive`); fill them in as the
   upstream code next to them does.
4. **A code generator changed** (`tools/recgen_iso.py`, `tools/share_iso.py`, `tools/vugen_iso.py`): the script generates the game
   code again from the disc, and the release note says the update rebuilds the whole game.
5. **Upstream's URLs**: a new upstream file that checks for updates or downloads releases must point at this repository (v0.5.1's
   `update_notice_linux.rs` read upstream's `VERSION`). `git grep -n AsobiCodes` after the merge: what is left must be on purpose
   (credits, the upscaler download, the RAC-RS Launcher's own project).
6. `git rerere` is on in the sync worktree: a resolution done once is replayed when the same conflict comes back.

Then `tools\upstream_sync.ps1 -Continue` (or `-Continue -Push`).

## Checks before pushing

`tools/upstream_sync.ps1` runs them all; any failure stops the push.

- `cargo check` of `rac1`, `rac-rs` and `rac-mp`.
- `cargo test -p rac-mp`: the network side.
- `cargo test -p rac-rs --lib boundary`: upstream's engine / rac1 boundary.
- The release build of the exe (`-NoBuild` skips it).
- Reported only: `cargo test -p rac-rs --lib size_limits` (see Known debt).

Not covered by the script (run them before publishing a release when the sync touched the game under multiplayer): a real pair
(`tools\mp_play.ps1`, `tools\mp_sweep.ps1`) and an Archipelago session (`tools\ap_play.ps1`).

## Known debt

- **Size caps.** Upstream caps every file at 1500 lines and every function at 150 (`size_limits_tests.rs`). `rac1/mp.rs` is 1745
  lines, and `mp::enemy_tick`, `mp::tick`, `mp::world_tick`, `mp/machines.rs tick` and `mp/spawned.rs tick` are over 150. Until they
  are split, the size test is reported, not a gate.
- **`env_doc_tests`** fails on upstream's public tree too (it reads `rac-rs/ENV.md`, which upstream does not publish).
- **Linux updates**: the fork has no RAC-RS Launcher, so on Linux an update is the new release unpacked over the folder and
  `sh setup.sh` again (the game's notice says so).
