"""Every item of Ratchet & Clank (2002) the randomizer shuffles, with the ids the game side (rac-rs/src/rac1/ap.rs) knows them by.

The game's own item ids (its owned bytes at 0x13d4c0 + id) were read from the disc's text table (the item names) and its code."""
from dataclasses import dataclass
from typing import Dict, Set

from . import Ids


@dataclass(frozen=True)
class ItemData:
    item_id: int
    name: str


@dataclass(frozen=True)
class EquipmentData(ItemData):
    game_id: int


@dataclass(frozen=True)
class WeaponData(EquipmentData):
    pass


@dataclass(frozen=True)
class CoordData(ItemData):
    planet: int


@dataclass(frozen=True)
class SpecialData(ItemData):
    index: int


@dataclass(frozen=True)
class CollectableData(ItemData):
    pass


def _equip(game_id: int, name: str) -> EquipmentData:
    return EquipmentData(Ids.ITEM_EQUIP + game_id, name, game_id)


def _weapon(game_id: int, name: str) -> WeaponData:
    return WeaponData(Ids.ITEM_EQUIP + game_id, name, game_id)


# Gadgets and items
HELI_PACK = _equip(2, "Heli-Pack")
THRUSTER_PACK = _equip(3, "Thruster-Pack")
HYDRO_PACK = _equip(4, "Hydro-Pack")
SONIC_SUMMONER = _equip(5, "Sonic Summoner")
O2_MASK = _equip(6, "O2 Mask")
PILOTS_HELMET = _equip(7, "Pilot's Helmet")
SWINGSHOT = _equip(12, "Swingshot")
HYDRODISPLACER = _equip(22, "Hydrodisplacer")
TRESPASSER = _equip(26, "Trespasser")
METAL_DETECTOR = _equip(27, "Metal Detector")
MAGNEBOOTS = _equip(28, "Magneboots")
GRINDBOOTS = _equip(29, "Grindboots")
HOVERBOARD = _equip(30, "Hoverboard")
HOLOGUISE = _equip(31, "Hologuise")
GADGETRON_PDA = _equip(32, "Gadgetron PDA")
MAP_O_MATIC = _equip(33, "Map-o-matic")
BOLT_GRABBER = _equip(34, "Bolt Grabber")
PERSUADER = _equip(35, "Persuader")

# Weapons
SUCK_CANNON = _weapon(9, "Suck Cannon")
BOMB_GLOVE = _weapon(10, "Bomb Glove")
DEVASTATOR = _weapon(11, "Devastator")
VISIBOMB_GUN = _weapon(13, "Visibomb Gun")
TAUNTER = _weapon(14, "Taunter")
BLASTER = _weapon(15, "Blaster")
PYROCITOR = _weapon(16, "Pyrocitor")
MINE_GLOVE = _weapon(17, "Mine Glove")
WALLOPER = _weapon(18, "Walloper")
TESLA_CLAW = _weapon(19, "Tesla Claw")
GLOVE_OF_DOOM = _weapon(20, "Glove of Doom")
MORPH_O_RAY = _weapon(21, "Morph-o-Ray")
RYNO = _weapon(23, "R.Y.N.O.")
DRONE_DEVICE = _weapon(24, "Drone Device")
DECOY_GLOVE = _weapon(25, "Decoy Glove")

# Planets (their infobots' coordinates): the game's level numbers.
PLANET_NAMES = [
    "Veldin", "Novalis", "Aridia", "Kerwan", "Eudora", "Rilgar", "Blarg Station", "Umbris", "Batalia", "Gaspar", "Orxon",
    "Pokitaru", "Hoven", "Gemlik Base", "Oltanis", "Quartu", "Kalebo III", "Drek's Fleet", "Veldin Return",
]


def _coords(planet: int) -> CoordData:
    return CoordData(Ids.ITEM_COORDS + planet, f"{PLANET_NAMES[planet]} Coordinates", planet)


NOVALIS_COORDS = _coords(1)
ARIDIA_COORDS = _coords(2)
KERWAN_COORDS = _coords(3)
EUDORA_COORDS = _coords(4)
RILGAR_COORDS = _coords(5)
BLARG_COORDS = _coords(6)
UMBRIS_COORDS = _coords(7)
BATALIA_COORDS = _coords(8)
GASPAR_COORDS = _coords(9)
ORXON_COORDS = _coords(10)
POKITARU_COORDS = _coords(11)
HOVEN_COORDS = _coords(12)
GEMLIK_COORDS = _coords(13)
OLTANIS_COORDS = _coords(14)
QUARTU_COORDS = _coords(15)
KALEBO_COORDS = _coords(16)
FLEET_COORDS = _coords(17)
VELDIN_RETURN_COORDS = _coords(18)

# Special items (their bytes at 0x13d388 + index)
ZOOMERATOR = SpecialData(Ids.ITEM_SPECIAL + 0, "Zoomerator", 0)
RARITANIUM = SpecialData(Ids.ITEM_SPECIAL + 1, "Raritanium", 1)
CODEBOT = SpecialData(Ids.ITEM_SPECIAL + 2, "Codebot", 2)
PREMIUM_NANOTECH = SpecialData(Ids.ITEM_SPECIAL + 4, "Premium Nanotech", 4)
ULTRA_NANOTECH = SpecialData(Ids.ITEM_SPECIAL + 5, "Ultra Nanotech", 5)

GOLD_BOLT = CollectableData(Ids.ITEM_GOLD_BOLT, "Gold Bolt")
BOLT_PACK = CollectableData(Ids.ITEM_BOLT_PACK, "Bolt Pack")

GADGETS: tuple[EquipmentData, ...] = (
    HELI_PACK, THRUSTER_PACK, HYDRO_PACK, SONIC_SUMMONER, O2_MASK, PILOTS_HELMET, SWINGSHOT, HYDRODISPLACER, TRESPASSER,
    METAL_DETECTOR, MAGNEBOOTS, GRINDBOOTS, HOVERBOARD, HOLOGUISE, GADGETRON_PDA, MAP_O_MATIC, BOLT_GRABBER, PERSUADER,
)
WEAPONS: tuple[WeaponData, ...] = (
    SUCK_CANNON, BOMB_GLOVE, DEVASTATOR, VISIBOMB_GUN, TAUNTER, BLASTER, PYROCITOR, MINE_GLOVE, WALLOPER, TESLA_CLAW,
    GLOVE_OF_DOOM, MORPH_O_RAY, RYNO, DRONE_DEVICE, DECOY_GLOVE,
)
# What a Gadgetron vendor sells, and the planet whose first visit adds it (retail's table at 0x1c4318).
VENDOR_WEAPONS: dict[WeaponData, int] = {
    PYROCITOR: 1, BLASTER: 3, GLOVE_OF_DOOM: 4, MINE_GLOVE: 5, TAUNTER: 6, DEVASTATOR: 8, WALLOPER: 9, VISIBOMB_GUN: 10,
    DECOY_GLOVE: 11, DRONE_DEVICE: 12, TESLA_CLAW: 14,
}
# A starting weapon in place of the Bomb Glove (option `random_start_weapon`): the vendor's first ones.
STARTING_WEAPONS: tuple[WeaponData, ...] = (PYROCITOR, BLASTER, GLOVE_OF_DOOM, MINE_GLOVE, TAUNTER)
COORDS: tuple[CoordData, ...] = tuple(_coords(p) for p in range(1, 19))
# Planets the game can begin on beside Novalis and Kerwan (option `starting_planets`).
STARTABLE_COORDS: tuple[CoordData, ...] = (ARIDIA_COORDS, EUDORA_COORDS, BLARG_COORDS, RILGAR_COORDS, BATALIA_COORDS)
SPECIALS: tuple[SpecialData, ...] = (ZOOMERATOR, RARITANIUM, CODEBOT, PREMIUM_NANOTECH, ULTRA_NANOTECH)

ALL: tuple[ItemData, ...] = GADGETS + WEAPONS + COORDS + SPECIALS + (GOLD_BOLT, BOLT_PACK)

_BY_ID: Dict[int, ItemData] = {i.item_id: i for i in ALL}
_BY_NAME: Dict[str, ItemData] = {i.name: i for i in ALL}


def from_id(item_id: int) -> ItemData:
    return _BY_ID[item_id]


def from_name(name: str) -> ItemData:
    return _BY_NAME[name]


def coords_for_planet(planet: int) -> CoordData:
    return _BY_ID[Ids.ITEM_COORDS + planet]


def get_item_groups() -> Dict[str, Set[str]]:
    return {
        "Gadgets": {i.name for i in GADGETS},
        "Weapons": {i.name for i in WEAPONS},
        "Coordinates": {i.name for i in COORDS},
        "Special Items": {i.name for i in SPECIALS},
    }
