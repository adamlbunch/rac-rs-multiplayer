"""The lines the game shows for the multiworld (an item sent or received): plain ASCII, short, as the game font draws them."""
from typing import TYPE_CHECKING, Any, Optional

if TYPE_CHECKING:
    from .Rac1Client import Rac1Context


def _clean(s: str, limit: int = 28) -> str:
    s = "".join(c if " " <= c <= "~" else "?" for c in s)
    return s if len(s) <= limit else s[:limit - 3] + "..."


def plain_item_message(ctx: "Rac1Context", args: dict[str, Any]) -> Optional[str]:
    """A PrintJSON ItemSend packet as one line, when it concerns this slot."""
    item = args.get("item")
    receiving = args.get("receiving")
    if item is None or ctx.slot is None:
        return None
    sender = item.player
    if receiving != ctx.slot and sender != ctx.slot:
        return None
    receiver_game = ctx.slot_info[receiving].game if receiving in ctx.slot_info else ctx.game
    name = _clean(ctx.item_names.lookup_in_game(item.item, receiver_game))
    if receiving == ctx.slot and sender == ctx.slot:
        return f"Found your {name}"
    if receiving == ctx.slot:
        return f"Got {name} from {_clean(ctx.player_names.get(sender, '?'), 16)}"
    return f"Sent {name} to {_clean(ctx.player_names.get(receiving, '?'), 16)}"
