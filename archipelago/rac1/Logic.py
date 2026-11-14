"""What reaching a location takes.

The rules follow the vanilla route: a location requires the progression items a normal playthrough of Ratchet & Clank has by the
time it gets there (Kerwan's Swingshot and Heli-Pack before Eudora, Blarg's Hydrodisplacer and Grindboots before Rilgar, the
O2 Mask and Thruster-Pack of Pokitaru before Orxon's second half, and so on). A seed is beatable whenever these hold, because the
vanilla game is; the rules ask for more than a location may strictly need, never less. A gold bolt asks for every gadget that
moves Ratchet (many are where the game's late gadgets lead), and some planets' for their own late items (`GOLD_EXTRA`).
"""
from typing import Callable, Iterable

from BaseClasses import CollectionState

from .data import Items

Rule = Callable[[CollectionState, int], bool]

SWING = Items.SWINGSHOT.name
HELI = Items.HELI_PACK.name
THRUST = Items.THRUSTER_PACK.name
HYDRO_PACK = Items.HYDRO_PACK.name
O2 = Items.O2_MASK.name
HELMET = Items.PILOTS_HELMET.name
MAGNE = Items.MAGNEBOOTS.name
GRIND = Items.GRINDBOOTS.name
HYDRO = Items.HYDRODISPLACER.name
HOVER = Items.HOVERBOARD.name
TRESPASS = Items.TRESPASSER.name
HOLO = Items.HOLOGUISE.name
PERSUADER = Items.PERSUADER.name
RARITANIUM = Items.RARITANIUM.name
ZOOM = Items.ZOOMERATOR.name
CODEBOT = Items.CODEBOT.name
SONIC = Items.SONIC_SUMMONER.name

# The kits of the vanilla route, in its order.
K0: frozenset = frozenset()
K_KERWAN = K0 | {SWING, HELI}
K_BLARG = K_KERWAN | {HYDRO, GRIND, TRESPASS}
K_GASPAR = K_BLARG | {HELMET}
K_POKITARU = K_GASPAR | {O2, THRUST}
K_ORXON = K_POKITARU | {MAGNE}
K_HOVEN = K_ORXON | {HYDRO_PACK}
K_GEMLIK = K_HOVEN | {PERSUADER}
FULL = K_GEMLIK | {HOLO, CODEBOT, ZOOM, RARITANIUM, SONIC, HOVER}

PROGRESSION_ITEM_NAMES: frozenset = FULL

# The vanilla route's order of what it needs: the planets as it unlocks them and the items as it gets them (`World.pre_fill`).
ROUTE: tuple[str, ...] = (
    Items.KERWAN_COORDS.name, SWING, HELI, Items.ARIDIA_COORDS.name, TRESPASS, HOVER, Items.EUDORA_COORDS.name,
    Items.BLARG_COORDS.name, HYDRO, GRIND, Items.RILGAR_COORDS.name, ZOOM, SONIC, Items.UMBRIS_COORDS.name,
    Items.BATALIA_COORDS.name, Items.GASPAR_COORDS.name, HELMET, Items.ORXON_COORDS.name, Items.POKITARU_COORDS.name, O2, THRUST,
    MAGNE, Items.HOVEN_COORDS.name, HYDRO_PACK, RARITANIUM, PERSUADER, Items.GEMLIK_COORDS.name, Items.OLTANIS_COORDS.name,
    Items.QUARTU_COORDS.name, Items.KALEBO_COORDS.name, HOLO, Items.FLEET_COORDS.name, CODEBOT, Items.VELDIN_RETURN_COORDS.name,
)

# What every gold bolt asks for: the gadgets that take Ratchet somewhere, and the Pilot's Helmet.
GOLD_KIT: frozenset = frozenset({SWING, HELI, THRUST, HYDRO_PACK, O2, MAGNE, GRIND, HYDRO, HOVER, TRESPASS, HELMET})
# And by planet (its level number): Novalis' sand mouse answers the Sonic Summoner; Drek's Fleet has the Codebot's vault and
# its guards to fool; the late planets the Hologuise.
GOLD_EXTRA: dict[int, frozenset] = {
    1: frozenset({SONIC}),
    13: frozenset({HOLO}),
    14: frozenset({HOLO}),
    15: frozenset({HOLO}),
    16: frozenset({HOLO}),
    17: frozenset({HOLO, CODEBOT}),
    18: frozenset({HOLO, CODEBOT}),
}


def needs(*kits: Iterable[str]) -> Rule:
    """A rule asking for every item of the kits."""
    names = frozenset().union(*kits)
    if not names:
        return lambda state, player: True
    names_list = sorted(names)
    return lambda state, player: state.has_all(names_list, player)
