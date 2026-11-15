# Archipelago randomizer

This fork plays Ratchet & Clank in an [Archipelago](https://archipelago.gg) multiworld: what the game gives the player is shuffled
with the items of other games, and what they find for you arrives in yours. It is modelled on
[APRac2](https://github.com/evilwb/APRac2) (MIT), the Archipelago world of Ratchet & Clank 2 for PCSX2. APRac2 cannot run here (it is
for the second game, and it works by patching that disc image and reading PCSX2's memory); this is the same design for the first
game, with the game side built into the port.

## Playing

1. Install [Archipelago](https://github.com/ArchipelagoMW/Archipelago/releases) (0.6.0 or newer) and `rac1.apworld` from this
   project's release (double-click it). The release also has `Ratchet & Clank.yaml`, the player options template.
2. Turn on **Archipelago randomizer** on the launcher's Settings page (or run `tools\ap_play.ps1`, or start the game with
   `RAC_AP=28111`). The game then listens on 127.0.0.1:28111 for the client.
3. Start a new game, then **Ratchet & Clank Client** in the Archipelago Launcher, and connect it to the room. It finds the game.

What you find while the client is closed is kept (`player\ap\<seed>.txt`) and sent when it connects. The full guide for players is
[archipelago/rac1/docs/setup_en.md](../archipelago/rac1/docs/setup_en.md); what the randomizer changes is in
[en_Ratchet & Clank.md](../archipelago/rac1/docs/en_Ratchet%20%26%20Clank.md).

## What is shuffled

Every location was found on the disc (the USA image's code and placed objects), not from memory of the game:

| Kind | Count | The game side reports |
| --- | --- | --- |
| Items the story gives (Swingshot, Heli-Pack, Hoverboard, Trespasser, Magneboots, Hologuise, R.Y.N.O. ...) | 21 | `item.give(id)` of an item the player does not own |
| Infobots (each opens a planet) | 17 | `planet.unlock(planet)` of another planet than the one landed on |
| Special items: Zoomerator (Rilgar race), Raritanium (Hoven), Codebot (Drek's Fleet), Premium and Ultra Nanotech (Orxon) | 5 | their bytes at 0x13d388 + index rising |
| Gold bolts | 40 | their bytes at 0x14bec0 + level * 4 + index rising |
| Gadgetron vendor weapons (option) | 11 | `item.give` of a vendor weapon in the vendor |

Items: the 21 gadgets and weapons, 18 planets' coordinates (Novalis and Kerwan are where the game starts; more early ones with
`starting_planets`), the 5 special items, 40 gold bolts (the game counts what it received: `gold_bolt.count_collected`), the
vendor's 11 weapons with the option, and Bolt Packs as filler. The goal is Drek's defeat on Veldin (his mech, class 1422, level 18).

The logic follows the vanilla route (archipelago/rac1/Logic.py): a location asks for the progression the vanilla game has when it
gets there, so every seed can be beaten. The route is one chain with few places before each link, so the world places its own
progression forward as a playthrough would (`pre_fill`), not by Archipelago's assumed fill, which often found no room for the first
links; the rest (gold bolts, weapons, other games' items) is placed by the multiworld.

## The game side (rac-rs/src/ap.rs)

- `ap::init` (Retail::load, with `RAC_AP`): the bridge (`ap/bridge.rs`: 127.0.0.1, one JSON object a line) and the hands. Without
  multiplayer it takes `item.give` and `planet.unlock` itself; with multiplayer, story.rs has them and asks `ap::give_hook` /
  `ap::unlock_hook` first. Its `gold_bolt.count_collected` hand gives the count received.
- A location's reward is not given: the hand returns to the caller (the scene, the shop, the infobot's movie play on), or the byte
  goes back (special items, the Persuader the Pokitaru trader stores directly). Its id goes to the client.
- The items received are given in play, one a field, at a safe point (`MAIN_STACK`), through the same functions (`GRANTING` lets
  them through); special items and bolts are written. Premium and Ultra Nanotech set max health (5, 8) as retail's shop does.
  Coordinates are written as retail's `planet.unlock` writes them (the unlock byte, the planet appended to the galaxy order)
  without its banner: the banner's text is the infobot's, in the text of the level whose infobot gives the planet, and anywhere
  else the game shows "Paradox: This message does not exist".
- Death link: the hero's death goes out; one that comes in leaves him one hit point and deals the game's own `take_damage` (it
  takes at most one a call), so his death plays as an enemy's last blow would make it, and it is not sent back.
- `L1 + L2 + R1 + R2 + Select` restarts the planet at its landing pad (the level request of retail's own Load Game), from play or
  from the pause menu the combo's Select opens.
- With the vendor option, while a shop is open each of the Gadgetron weapons shows as its place in the multiworld stands: bought
  there already, as owned (an ammo row); not bought there yet, as for sale even when the multiworld sent the weapon, so its place
  can always be bought. The shop's list is built again with the game's own `vendor.build_item_list`; the bytes go back when it
  closes.
- The client passes on the locations the server has as checked (`checked`): a co-op partner's finds, or an earlier save's, are
  known to the game too.
- The client's lines are drawn low in the middle of the picture through the mods' text layer (`omni::draw::submit_ap`).
- The game's own prompts where a location sells or trades its reward (Kerwan's Heli-Pack and Swingshot, the Novalis infobot,
  the Grindboots, the R.Y.N.O., the nanotech, the Thruster-Pack, the Persuader trade, the Hydro-Pack, the PDA) name what the
  multiworld put there, as APRac2 renames its vendors' and NPCs' (`ap/text.rs`): the client scouts the locations, the texts
  are read from the player's disc and found in RAM, and their highlighted name is replaced in place (no game text is in the
  code).

## Co-op

Two players of a multiplayer session play one slot: both start the game with Archipelago on and their own client on that slot.
Archipelago keeps the first report of each location; both games get every item, and story.rs's team sharing gives each game only
what it does not have yet. Each client tells its game what the server has as checked, so a place one player found (a vendor's
weapon bought) shows as found in the other's game too.

## Files

| File | What |
| --- | --- |
| `rac-rs/src/ap.rs`, `ap/bridge.rs`, `ap/notice.rs`, `ap/text.rs` | The game side |
| `rac-rs/src/mp/story.rs`, `mp.rs` | The team's hands ask the randomizer first; `in_front`, `story_replaying`, `death_state` |
| `rac-rs/src/omni/mod.rs`, `omni/draw.rs` | `hook_spec`; the randomizer's own draw list |
| `rac-rs/src/retail/worker.rs`, `worker/field_run.rs` | `ap::init`, `ap::tick`, the pad buttons for the escape |
| `archipelago/rac1/` | The apworld: world, options, items, locations, logic, regions, pool, client, docs, tests |
| `tools/build_apworld.ps1`, `tools/ap_play.ps1` | Packs `rac1.apworld`; starts the game with Archipelago on |
| `setup.ps1`, `launcher_pages.ps1` | The launcher's switch (`generated\archipelago.cfg`) |

## Testing

- `archipelago/rac1/test`: Archipelago's world tests (run inside an Archipelago source tree, `worlds/rac1`): counts, ids, options.
- In-game test aids (environment, test runs only; nothing happens without them): `RAC_AP_TEST_DIE=FIELD` (the hero's last blow,
  his death goes out), `RAC_AP_TEST_DREK=FIELD` (Drek's health to 0 on Veldin's return, for the goal),
  `RAC_AP_TEST_UNLOCK=FIELD:PLANET` (the game's `planet.unlock` as an infobot calls it). The game's run log has an `ap:` line for
  each location found, item given, death link, escape, goal and game mode change.
- Archipelago's general tests (`test/general`) pass with this world in.
- Generation was run for hundreds of seeds per option set: every seed fills, is beatable, and reaches every location.
- An end-to-end run: a local server with a two-game multiworld, the client, and the game picking up a gold bolt on Novalis.
