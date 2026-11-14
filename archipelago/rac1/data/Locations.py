"""Every location of the randomizer, by planet. Each was found on the disc: the object that gives it is placed in that level (its
class and its call of `item.give` / `planet.unlock`, or its store of a special item's byte), and the game side (rac-rs/src/ap.rs)
reports it with the id here."""
from typing import Any, Callable, Dict, NamedTuple, Optional

from . import Ids, Items
from ..Logic import (
    CODEBOT, GOLD_EXTRA, GOLD_KIT, HOLO, HOVER, K0, K_BLARG, K_GASPAR, K_GEMLIK, K_HOVEN, K_KERWAN, K_ORXON, K_POKITARU,
    RARITANIUM, SWING, ZOOM, Rule, needs,
)


class LocationData(NamedTuple):
    location_id: Optional[int]
    name: str
    planet: int
    rule: Rule = needs(K0)
    enable_if: Optional[Callable[[Dict[str, Any]], bool]] = None
    # Excluded from progression when the option says so (the R.Y.N.O.'s 150,000 bolts).
    expensive: bool = False


def item_loc(planet: int, what: str, item: Items.EquipmentData, rule: Rule = needs(K0), expensive: bool = False) -> LocationData:
    return LocationData(Ids.LOC_ITEM + item.game_id, f"{Items.PLANET_NAMES[planet]}: {what}", planet, rule, expensive=expensive)


def infobot(planet: int, what: str, to: int, rule: Rule = needs(K0)) -> LocationData:
    return LocationData(Ids.LOC_INFOBOT + to, f"{Items.PLANET_NAMES[planet]}: {what}", planet, rule)


def special(planet: int, what: str, item: Items.SpecialData, rule: Rule = needs(K0)) -> LocationData:
    return LocationData(Ids.LOC_SPECIAL + planet * 16 + item.index, f"{Items.PLANET_NAMES[planet]}: {what}", planet, rule)


def gold_bolt(planet: int, index: int, n: int) -> LocationData:
    rule = needs(GOLD_KIT, GOLD_EXTRA.get(planet, frozenset()))
    return LocationData(Ids.LOC_GOLD + planet * 4 + index, f"{Items.PLANET_NAMES[planet]}: Gold Bolt {n}", planet, rule)


def vendor(weapon: Items.WeaponData) -> LocationData:
    planet = Items.VENDOR_WEAPONS[weapon]
    return LocationData(Ids.LOC_VENDOR + weapon.game_id, f"Gadgetron Vendor: {weapon.name}", planet, needs(K0),
                        enable_if=lambda o: bool(o.get("randomize_vendor")))


# Novalis
NOVALIS_INFOBOT_ARIDIA = infobot(1, "Infobot to Aridia", 2)
NOVALIS_INFOBOT_KERWAN = infobot(1, "Infobot Sold for 500 Bolts", 3)
# Aridia
ARIDIA_HOVERBOARD = item_loc(2, "Sandshark Hunt - Hoverboard", Items.HOVERBOARD, needs(K_KERWAN))
ARIDIA_TRESPASSER = item_loc(2, "Trespasser", Items.TRESPASSER, needs(K_KERWAN))
ARIDIA_SONIC_SUMMONER = item_loc(2, "Skid's Agent - Sonic Summoner", Items.SONIC_SUMMONER, needs(K_KERWAN, {HOVER, ZOOM}))
# Kerwan
KERWAN_SWINGSHOT = item_loc(3, "Helga - Swingshot", Items.SWINGSHOT)
KERWAN_HELI_PACK = item_loc(3, "Heli-Pack", Items.HELI_PACK, needs({SWING}))
KERWAN_INFOBOT_EUDORA = infobot(3, "Infobot to Eudora", 4, needs(K_KERWAN))
# Eudora
EUDORA_SUCK_CANNON = item_loc(4, "Suck Cannon", Items.SUCK_CANNON, needs(K_KERWAN))
EUDORA_INFOBOT_BLARG = infobot(4, "Infobot to Blarg Station", 6, needs(K_KERWAN))
# Rilgar
RILGAR_ZOOMERATOR = special(5, "Hoverboard Race - Zoomerator", Items.ZOOMERATOR, needs(K_BLARG, {HOVER}))
RILGAR_RYNO = item_loc(5, "R.Y.N.O. Salesman", Items.RYNO, needs(K_BLARG), expensive=True)
RILGAR_INFOBOT_UMBRIS = infobot(5, "Infobot to Umbris", 7, needs(K_BLARG))
# Blarg Station
BLARG_HYDRODISPLACER = item_loc(6, "Hydrodisplacer", Items.HYDRODISPLACER, needs(K_KERWAN))
BLARG_GRINDBOOTS = item_loc(6, "Grindboots", Items.GRINDBOOTS, needs(K_KERWAN))
BLARG_INFOBOT_RILGAR = infobot(6, "Infobot to Rilgar", 5, needs(K_BLARG))
# Umbris
UMBRIS_INFOBOT_BATALIA = infobot(7, "Infobot to Batalia", 8, needs(K_BLARG))
# Batalia
BATALIA_METAL_DETECTOR = item_loc(8, "Commando - Metal Detector", Items.METAL_DETECTOR, needs(K_BLARG))
BATALIA_INFOBOT_GASPAR = infobot(8, "Infobot to Gaspar", 9, needs(K_BLARG))
BATALIA_INFOBOT_ORXON = infobot(8, "Deserter's Infobot", 10, needs(K_BLARG))
# Gaspar
GASPAR_PILOTS_HELMET = item_loc(9, "Pilot's Helmet", Items.PILOTS_HELMET, needs(K_BLARG))
# Orxon
ORXON_INFOBOT_POKITARU = infobot(10, "Clank's Infobot to Pokitaru", 11, needs(K_GASPAR))
ORXON_MAGNEBOOTS = item_loc(10, "Magneboots", Items.MAGNEBOOTS, needs(K_POKITARU))
ORXON_PREMIUM_NANOTECH = special(10, "Premium Nanotech", Items.PREMIUM_NANOTECH, needs(K_ORXON))
ORXON_ULTRA_NANOTECH = special(10, "Ultra Nanotech", Items.ULTRA_NANOTECH, needs(K_ORXON))
ORXON_INFOBOT_HOVEN = infobot(10, "Infobot to Hoven", 12, needs(K_ORXON))
# Pokitaru
POKITARU_THRUSTER_PACK = item_loc(11, "Thruster-Pack", Items.THRUSTER_PACK, needs(K_GASPAR))
POKITARU_O2_MASK = item_loc(11, "O2 Mask", Items.O2_MASK, needs(K_GASPAR))
POKITARU_PERSUADER = item_loc(11, "Raritanium Trade - Persuader", Items.PERSUADER, needs(K_POKITARU, {RARITANIUM}))
# Hoven
HOVEN_HYDRO_PACK = item_loc(12, "Hydro-Pack", Items.HYDRO_PACK, needs(K_ORXON))
HOVEN_RARITANIUM = special(12, "Raritanium", Items.RARITANIUM, needs(K_HOVEN))
HOVEN_INFOBOT_GEMLIK = infobot(12, "Infobot to Gemlik Base", 13, needs(K_HOVEN))
# Gemlik Base
GEMLIK_INFOBOT_OLTANIS = infobot(13, "Infobot to Oltanis", 14, needs(K_GEMLIK))
# Oltanis
OLTANIS_PDA = item_loc(14, "Gadgetron PDA", Items.GADGETRON_PDA, needs(K_GEMLIK))
OLTANIS_MORPH_O_RAY = item_loc(14, "Morph-o-Ray", Items.MORPH_O_RAY, needs(K_GEMLIK))
OLTANIS_INFOBOT_QUARTU = infobot(14, "Infobot to Quartu", 15, needs(K_GEMLIK))
# Quartu
QUARTU_BOLT_GRABBER = item_loc(15, "Bolt Grabber", Items.BOLT_GRABBER, needs(K_GEMLIK))
QUARTU_INFOBOT_KALEBO = infobot(15, "Infobot to Kalebo III", 16, needs(K_GEMLIK))
QUARTU_INFOBOT_FLEET = infobot(15, "Infobot to Drek's Fleet", 17, needs(K_GEMLIK))
# Kalebo III
KALEBO_MAP_O_MATIC = item_loc(16, "Hoverboard Race - Map-o-matic", Items.MAP_O_MATIC, needs(K_GEMLIK, {HOVER}))
KALEBO_HOLOGUISE = item_loc(16, "Hologuise", Items.HOLOGUISE, needs(K_GEMLIK))
# Drek's Fleet
FLEET_CODEBOT = special(17, "Codebot", Items.CODEBOT, needs(K_GEMLIK, {HOLO}))
FLEET_INFOBOT_VELDIN = infobot(17, "Infobot to Veldin", 18, needs(K_GEMLIK, {HOLO}))
# Veldin, the return: the goal (an event, no id).
DEFEAT_DREK = LocationData(None, "Veldin Return: Defeat Drek", 18, needs(K_GEMLIK, {HOLO, CODEBOT}))

# The 40 gold bolts: (planet, index of its byte), numbered within the planet.
GOLD_BOLTS: tuple[LocationData, ...] = tuple(
    gold_bolt(p, i, n + 1)
    for p, indices in (
        (1, (0, 1, 2)), (2, (0, 1, 2, 3)), (3, (0, 1, 2)), (4, (0,)), (5, (0, 1)), (6, (0, 1)), (7, (0, 1)), (8, (0, 1)),
        (9, (0, 1)), (10, (0, 1)), (11, (0,)), (12, (0, 1)), (13, (0,)), (14, (0, 1, 2, 3)), (15, (0, 1)), (16, (0, 1)),
        (17, (0, 1)), (18, (0, 1, 2)),
    )
    for n, i in enumerate(indices)
)

VENDOR: tuple[LocationData, ...] = tuple(vendor(w) for w in Items.VENDOR_WEAPONS)

STORY: tuple[LocationData, ...] = (
    NOVALIS_INFOBOT_ARIDIA, NOVALIS_INFOBOT_KERWAN,
    ARIDIA_HOVERBOARD, ARIDIA_TRESPASSER, ARIDIA_SONIC_SUMMONER,
    KERWAN_SWINGSHOT, KERWAN_HELI_PACK, KERWAN_INFOBOT_EUDORA,
    EUDORA_SUCK_CANNON, EUDORA_INFOBOT_BLARG,
    RILGAR_ZOOMERATOR, RILGAR_RYNO, RILGAR_INFOBOT_UMBRIS,
    BLARG_HYDRODISPLACER, BLARG_GRINDBOOTS, BLARG_INFOBOT_RILGAR,
    UMBRIS_INFOBOT_BATALIA,
    BATALIA_METAL_DETECTOR, BATALIA_INFOBOT_GASPAR, BATALIA_INFOBOT_ORXON,
    GASPAR_PILOTS_HELMET,
    ORXON_INFOBOT_POKITARU, ORXON_MAGNEBOOTS, ORXON_PREMIUM_NANOTECH, ORXON_ULTRA_NANOTECH, ORXON_INFOBOT_HOVEN,
    POKITARU_THRUSTER_PACK, POKITARU_O2_MASK, POKITARU_PERSUADER,
    HOVEN_HYDRO_PACK, HOVEN_RARITANIUM, HOVEN_INFOBOT_GEMLIK,
    GEMLIK_INFOBOT_OLTANIS,
    OLTANIS_PDA, OLTANIS_MORPH_O_RAY, OLTANIS_INFOBOT_QUARTU,
    QUARTU_BOLT_GRABBER, QUARTU_INFOBOT_KALEBO, QUARTU_INFOBOT_FLEET,
    KALEBO_MAP_O_MATIC, KALEBO_HOLOGUISE,
    FLEET_CODEBOT, FLEET_INFOBOT_VELDIN,
)

ALL: tuple[LocationData, ...] = STORY + GOLD_BOLTS + VENDOR + (DEFEAT_DREK,)


def active(options: Dict[str, Any]) -> list[LocationData]:
    """The locations a seed with these options has (the goal event included)."""
    return [loc for loc in ALL if loc.enable_if is None or loc.enable_if(options)]


def get_location_groups() -> Dict[str, set[str]]:
    groups: Dict[str, set[str]] = {
        "Gold Bolts": {loc.name for loc in GOLD_BOLTS},
        "Vendor": {loc.name for loc in VENDOR},
        "Infobots": {loc.name for loc in STORY if Ids.LOC_INFOBOT <= (loc.location_id or 0) < Ids.LOC_SPECIAL},
    }
    for p, name in enumerate(Items.PLANET_NAMES):
        names = {loc.name for loc in ALL if loc.planet == p and loc.location_id is not None and not loc.name.startswith("Gadgetron")}
        if names:
            groups[name] = names
    return groups
