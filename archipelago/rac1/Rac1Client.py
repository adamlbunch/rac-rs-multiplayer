"""The Ratchet & Clank client: connects the game (rac-rs-multiplayer with RAC_AP, Rac1Interface.py) and an Archipelago server.

From the game it gets the locations the player found, his deaths (death link) and the goal; to the game it sends the seed, every
item received (the game makes what it does not have yet) and the lines to show (an item sent or received, a death link)."""
import asyncio
import traceback
from typing import Any, Optional

import Utils
from CommonClient import ClientCommandProcessor, CommonContext, get_base_parser, gui_enabled, handle_url_arg, logger, server_loop
from NetUtils import ClientStatus

from .data import Items, Locations
from .data.Ids import DEFAULT_GAME_PORT
from .Rac1Interface import Rac1Interface
from .TextManager import plain_item_message

GAME = "Ratchet & Clank"


class Rac1CommandProcessor(ClientCommandProcessor):
    def _cmd_game(self) -> None:
        """Show the connection to the game."""
        if isinstance(self.ctx, Rac1Context):
            gi = self.ctx.game_interface
            state = f"connected (level {gi.level}, mode {gi.mode})" if gi.connected else "not connected"
            logger.info(f"Game on 127.0.0.1:{gi.port}: {state}")

    def _cmd_test_hud(self, *args: str) -> None:
        """Show a line in the game."""
        if isinstance(self.ctx, Rac1Context):
            Utils.async_start(self.ctx.game_interface.show(" ".join(args)))

    def _cmd_deathlink(self) -> None:
        """Toggle death link (overrides the seed's option)."""
        if isinstance(self.ctx, Rac1Context):
            self.ctx.death_link_enabled = not self.ctx.death_link_enabled
            Utils.async_start(self.ctx.update_death_link(self.ctx.death_link_enabled), name="Update Deathlink")
            Utils.async_start(self.ctx.send_seed_to_game())
            logger.info(f"Death link {'on' if self.ctx.death_link_enabled else 'off'}")


class Rac1Context(CommonContext):
    command_processor = Rac1CommandProcessor
    game = GAME
    items_handling = 0b111
    slot_data: Optional[dict[str, Any]] = None
    death_link_enabled: bool = False
    game_task: Optional[asyncio.Task] = None
    goal_sent: bool = False

    def __init__(self, server_address: Optional[str], password: Optional[str], game_port: int = DEFAULT_GAME_PORT) -> None:
        super().__init__(server_address, password)
        self.game_interface = Rac1Interface(logger, game_port)
        self.pending_found: set[int] = set()

    async def server_auth(self, password_requested: bool = False) -> None:
        if password_requested and not self.password:
            await super().server_auth(password_requested)
        await self.get_username()
        await self.send_connect()

    def on_package(self, cmd: str, args: dict) -> None:
        if cmd == "Connected":
            self.slot_data = args.get("slot_data") or {}
            self.death_link_enabled = bool(self.slot_data.get("death_link", False))
            Utils.async_start(self.update_death_link(self.death_link_enabled))
            Utils.async_start(self.send_seed_to_game())
            Utils.async_start(self.send_items_to_game())
            Utils.async_start(self.flush_found())
            # What each location holds, for the game's prompts (APRac2 scouts its locations the same way).
            Utils.async_start(self.send_msgs([{"cmd": "LocationScouts", "locations": sorted(self.server_locations), "create_as_hint": 0}]))
        elif cmd == "LocationInfo":
            Utils.async_start(self.send_names_to_game())
        elif cmd == "ReceivedItems":
            Utils.async_start(self.send_items_to_game())
        elif cmd == "RoomUpdate" and "checked_locations" in args:
            Utils.async_start(self.send_checked_to_game())
        elif cmd == "PrintJSON" and args.get("type") == "ItemSend":
            text = plain_item_message(self, args)
            if text:
                Utils.async_start(self.game_interface.show(text))

    def on_deathlink(self, data: dict[str, Any]) -> None:
        super().on_deathlink(data)
        if self.death_link_enabled:
            cause = data.get("cause") or f"Death link from {data.get('source', '?')}"
            Utils.async_start(self.game_interface.kill(cause))

    async def send_seed_to_game(self) -> None:
        if self.slot_data is None or self.slot is None:
            return
        options = dict(self.slot_data)
        options["death_link"] = int(self.death_link_enabled)
        # The game keeps what it found by seed and slot (player/ap/<seed>.txt).
        seed = f"{self.server_seed_name or self.seed_name or 'seed'}_{self.slot}"
        await self.game_interface.send_seed(seed, self.player_names.get(self.slot, ""), options)
        await self.send_checked_to_game()

    async def send_checked_to_game(self) -> None:
        """The locations the server has as checked: found by this game, a co-op partner's on the same slot, or an earlier save's."""
        if self.slot is not None:
            await self.game_interface.send_checked(sorted(self.checked_locations))

    def scouted_names(self) -> dict[int, str]:
        """What each scouted location holds, as the game's prompts show it ("Swingshot", "Hookshot for Link")."""
        names: dict[int, str] = {}
        for loc, item in self.locations_info.items():
            game = self.slot_info[item.player].game if item.player in self.slot_info else self.game
            name = self.item_names.lookup_in_game(item.item, game)
            if item.player != self.slot:
                name = f"{name} for {self.player_names.get(item.player, '?')}"
            names[loc] = name
        return names

    async def send_names_to_game(self) -> None:
        if self.slot is not None and self.locations_info:
            await self.game_interface.send_names(self.scouted_names())

    async def send_items_to_game(self) -> None:
        if self.slot is None:
            return
        await self.game_interface.send_items([item.item for item in self.items_received])

    async def flush_found(self) -> None:
        """The locations the game reported, to the server (it keeps the first of each)."""
        if not self.pending_found or self.slot is None or self.server is None:
            return
        known = set(self.server_locations) if self.server_locations else set()
        found = sorted(loc for loc in self.pending_found if loc in known) if known else sorted(self.pending_found)
        self.pending_found.clear()
        if found:
            await self.send_msgs([{"cmd": "LocationChecks", "locations": found}])

    async def from_game(self, msg: dict[str, Any]) -> None:
        kind = msg.get("t")
        if kind == "hello":
            logger.info(f"Connected to Ratchet & Clank ({msg.get('version', '?')})")
            await self.send_seed_to_game()
            await self.send_items_to_game()
            await self.send_names_to_game()
        elif kind == "found":
            self.pending_found.update(int(v) for v in msg.get("loc", []))
            await self.flush_found()
        elif kind == "death":
            if self.death_link_enabled and self.slot is not None:
                await self.send_death(f"{self.player_names.get(self.slot, 'Ratchet')} ran out of nanotech.")
        elif kind == "goal":
            if not self.goal_sent and self.slot is not None:
                self.goal_sent = True
                await self.send_msgs([{"cmd": "StatusUpdate", "status": ClientStatus.CLIENT_GOAL}])
                logger.info("Drek is defeated: goal complete!")

    def run_gui(self) -> None:
        from kvui import GameManager

        class Rac1Manager(GameManager):
            logging_pairs = [("Client", "Archipelago")]
            base_title = "Archipelago Ratchet & Clank Client"

        self.ui = Rac1Manager(self)
        self.ui_task = asyncio.create_task(self.ui.async_run(), name="UI")


async def game_loop(ctx: Rac1Context) -> None:
    """Keeps the connection to the game, and reads it."""
    logger.info(f"Waiting for Ratchet & Clank on 127.0.0.1:{ctx.game_interface.port} (start the game with Archipelago on)...")
    said = False
    while not ctx.exit_event.is_set():
        try:
            if not ctx.game_interface.connected:
                if await ctx.game_interface.connect():
                    said = False
                else:
                    if not said:
                        logger.info("The game is not running with Archipelago yet; trying again every few seconds.")
                        said = True
                    await asyncio.sleep(3)
                    continue
            await ctx.game_interface.receive(ctx.from_game)
            logger.info("The game went away; waiting for it again.")
        except (OSError, ConnectionError):
            ctx.game_interface.disconnect()
            await asyncio.sleep(3)
        except Exception:
            logger.error(traceback.format_exc())
            ctx.game_interface.disconnect()
            await asyncio.sleep(3)


def launch(*args: str) -> None:
    Utils.init_logging("Rac1Client", exception_logger="Client")

    async def main() -> None:
        parser = get_base_parser()
        parser.add_argument("--game-port", type=int, default=DEFAULT_GAME_PORT, help="the port the game listens on (RAC_AP)")
        parser.add_argument("--name", default=None, help="the slot name")
        parser.add_argument("url", default=None, nargs="?", help="archipelago://name:pass@host:port, as the launcher gives it")
        parsed = handle_url_arg(parser.parse_args(args or None), parser)
        ctx = Rac1Context(parsed.connect, parsed.password, parsed.game_port)
        if parsed.name:
            ctx.auth = parsed.name
        ctx.server_task = asyncio.create_task(server_loop(ctx), name="Server Loop")
        if gui_enabled:
            ctx.run_gui()
        ctx.run_cli()
        ctx.game_task = asyncio.create_task(game_loop(ctx), name="Game Loop")
        await ctx.exit_event.wait()
        ctx.server_address = None
        await ctx.shutdown()
        if ctx.game_task:
            ctx.game_task.cancel()

    import colorama
    colorama.just_fix_windows_console()
    asyncio.run(main())
    colorama.deinit()


if __name__ == "__main__":
    launch()
