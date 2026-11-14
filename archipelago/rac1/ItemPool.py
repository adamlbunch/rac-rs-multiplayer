from typing import TYPE_CHECKING

from BaseClasses import Item, ItemClassification

from .data import Items
from .data.Items import ItemData
from .Logic import PROGRESSION_ITEM_NAMES

if TYPE_CHECKING:
    from . import Rac1World


def get_classification(item: ItemData) -> ItemClassification:
    if isinstance(item, Items.CoordData) or item.name in PROGRESSION_ITEM_NAMES:
        return ItemClassification.progression
    if item is Items.BOLT_PACK:
        return ItemClassification.filler
    return ItemClassification.useful


def create_planets(world: "Rac1World") -> list[Item]:
    """The coordinates: Novalis and Kerwan, where the vanilla game goes first, and more early planets (option `starting_planets`)
    to start with."""
    precollected = {i.code for i in world.multiworld.precollected_items[world.player]}
    starting = [Items.NOVALIS_COORDS, Items.KERWAN_COORDS]
    candidates = [c for c in Items.STARTABLE_COORDS if c.item_id not in precollected and c not in starting]
    world.random.shuffle(candidates)
    starting += candidates[:max(0, world.options.starting_planets.value - len(starting))]
    for coords in starting:
        if coords.item_id not in precollected:
            world.multiworld.push_precollected(world.create_item(coords.name))
    starting_ids = {c.item_id for c in starting} | precollected
    return [world.create_item(c.name) for c in Items.COORDS if c.item_id not in starting_ids]


def create_equipment(world: "Rac1World") -> list[Item]:
    """The gadgets and the weapons the story gives, the vendor's with its option, the starting weapon."""
    equipment: list[Items.EquipmentData] = list(Items.GADGETS) + [Items.SUCK_CANNON, Items.RYNO, Items.MORPH_O_RAY]
    if world.options.randomize_vendor:
        equipment += list(Items.VENDOR_WEAPONS)
    if world.options.random_start_weapon:
        weapon = world.random.choice(Items.STARTING_WEAPONS)
        world.starting_weapon = weapon
        world.multiworld.push_precollected(world.create_item(weapon.name))
        equipment.append(Items.BOMB_GLOVE)
    precollected = [i.code for i in world.multiworld.precollected_items[world.player]]
    out: list[Item] = []
    for e in equipment:
        if e.item_id in precollected:
            precollected.remove(e.item_id)
            continue
        out.append(world.create_item(e.name))
    return out


def create_collectables(world: "Rac1World") -> list[Item]:
    """The special items and the 40 gold bolts (less any put in the starting inventory)."""
    precollected = [i.code for i in world.multiworld.precollected_items[world.player]]
    out = [world.create_item(s.name) for s in Items.SPECIALS if s.item_id not in precollected]
    gold = 40 - precollected.count(Items.GOLD_BOLT.item_id)
    out += [world.create_item(Items.GOLD_BOLT.name) for _ in range(max(0, gold))]
    return out
