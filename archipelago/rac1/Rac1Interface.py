"""The game's side of the connection: rac-rs-multiplayer started with RAC_AP listens on a local port (rac-rs/src/ap/bridge.rs) for
one JSON object a line, both ways. This takes the place of APRac2's PCSX2 memory interface (PINE): the game itself reports what
the player found and makes what the multiworld sends."""
import asyncio
import json
from typing import Any, Callable, Optional

from .data.Ids import DEFAULT_GAME_PORT


class Rac1Interface:
    def __init__(self, logger, port: int = DEFAULT_GAME_PORT) -> None:
        self.logger = logger
        self.port = port
        self.reader: Optional[asyncio.StreamReader] = None
        self.writer: Optional[asyncio.StreamWriter] = None
        self.level: int = -1
        self.mode: int = 0
        self.version: str = ""

    @property
    def connected(self) -> bool:
        return self.writer is not None

    async def connect(self) -> bool:
        try:
            self.reader, self.writer = await asyncio.open_connection("127.0.0.1", self.port)
            return True
        except OSError:
            self.reader = self.writer = None
            return False

    def disconnect(self) -> None:
        if self.writer is not None:
            try:
                self.writer.close()
            except Exception:
                pass
        self.reader = self.writer = None
        self.level = -1

    async def send(self, msg: dict[str, Any]) -> None:
        if self.writer is None:
            return
        try:
            self.writer.write((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
            await self.writer.drain()
        except (OSError, ConnectionError):
            self.disconnect()

    async def receive(self, handle: Callable[[dict[str, Any]], Any]) -> None:
        """Reads the game's lines until it goes away; each goes to `handle`."""
        assert self.reader is not None
        while True:
            line = await self.reader.readline()
            if not line:
                self.disconnect()
                return
            try:
                msg = json.loads(line.decode("utf-8", "replace"))
            except json.JSONDecodeError:
                self.logger.warning(f"The game sent a line that is not JSON: {line[:80]!r}")
                continue
            if msg.get("t") == "state":
                self.level = int(msg.get("level", -1))
                self.mode = int(msg.get("mode", 0))
            elif msg.get("t") == "hello":
                self.version = str(msg.get("version", ""))
            result = handle(msg)
            if asyncio.iscoroutine(result):
                await result

    # What the client tells the game.
    async def send_seed(self, seed: str, slot: str, options: dict[str, Any]) -> None:
        await self.send({"t": "seed", "seed": seed, "slot": slot, "options": options})

    async def send_items(self, item_ids: list[int]) -> None:
        await self.send({"t": "items", "items": item_ids})

    async def send_checked(self, locations: list[int]) -> None:
        await self.send({"t": "checked", "loc": locations})

    async def send_names(self, names: dict[int, str]) -> None:
        await self.send({"t": "names", "names": {str(k): v for k, v in names.items()}})

    async def show(self, text: str) -> None:
        await self.send({"t": "msg", "text": text})

    async def kill(self, text: str) -> None:
        await self.send({"t": "kill", "text": text})
