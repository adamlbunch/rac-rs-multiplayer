# Setup Guide for Ratchet & Clank (rac-rs-multiplayer) in Archipelago

## Requirements

- [Archipelago](https://github.com/ArchipelagoMW/Archipelago/releases) 0.6.0 or higher.
- [rac-rs-multiplayer](https://github.com/adamlbunch/rac-rs-multiplayer/releases), set up with your own Ratchet & Clank disc
  (USA or PAL), as its README says.
- `rac1.apworld`, from the same release (or built with `tools/build_apworld.ps1`).

## Installing the world

Double-click `rac1.apworld` (or put it in the `custom_worlds` folder of your Archipelago install).

## Your YAML

`Ratchet & Clank.yaml` (next to the apworld in the release) is a template: put your name in it, choose the options, and give it to
whoever generates the multiworld. Generation needs no disc.

## Playing

1. Start the game with Archipelago on: in the RAC-RS Launcher, tick **Archipelago**, or run `tools/ap_play.ps1`, or set the
   environment variable `RAC_AP=28111` before starting `rac-rs.exe`. The game then listens for the client on 127.0.0.1:28111
   (another port: `RAC_AP=<port>`, and give the client `--game-port <port>`).
2. Start a new game (or load the save you play this seed in).
3. Open the Archipelago Launcher and start **Ratchet & Clank Client**. Connect it to the room (`archipelago.gg:<port>` or your own
   server). It finds the game by itself.

Keep the client running while you play. What you find while it is closed is kept by the game and sent when it connects again.

## Co-op (rac-rs-multiplayer)

Both players start the game with Archipelago on and their own client, connected to the same slot. A gadget, planet or gold bolt
one of them finds counts once, and both games get every item the multiworld sends.

## Troubleshooting

- The client says the game is not running with Archipelago: the game was started without `RAC_AP`, or another program uses the
  port (the game's run log `player/log.txt` says `ap: could not open`).
- The game's run log has an `ap:` line for each location found and each item given.
