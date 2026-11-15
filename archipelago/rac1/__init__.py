"""Ratchet & Clank (2002) for Archipelago, played in rac-rs-multiplayer (the PC port of the PS2 game). Modelled on evilwb/APRac2
(MIT), the Archipelago world of Ratchet & Clank 2: see LICENSE.md.

The game side is in the port itself (rac-rs/src/rac1/ap.rs): started with RAC_AP, it turns what the game gives the player into
locations and gives the items the multiworld sends; the client here (Rac1Client.py) connects the two over a local port."""
from typing import Any, Dict, Mapping, Optional

from BaseClasses import Item, ItemClassification, LocationProgressType, Tutorial
from worlds.AutoWorld import WebWorld, World
from worlds.LauncherComponents import Component, Type, components, launch_subprocess

from . import ItemPool, Logic
from .data import Items, Locations
from .data.Items import WeaponData
from .Rac1Options import BOLT_PACK_BOLTS, Rac1Options
from .Regions import create_regions

GAME = "Ratchet & Clank"


def run_client(*args: Any) -> None:
    from .Rac1Client import launch
    launch_subprocess(launch, name="Rac1Client")


components.append(Component("Ratchet & Clank Client", func=run_client, component_type=Type.CLIENT))


class Rac1Web(WebWorld):
    tutorials = [Tutorial(
        "Multiworld Setup Guide",
        "A guide to setting up Ratchet & Clank (rac-rs-multiplayer) for Archipelago",
        "English",
        "setup_en.md",
        "setup/en",
        ["rac-rs-multiplayer"],
    )]


class Rac1Item(Item):
    game: str = GAME


class Rac1World(World):
    """
    Ratchet & Clank is a third-person platformer and shooter, first released on the PlayStation 2 in 2002. Ratchet, a Lombax
    mechanic, and Clank, a small robot, travel the Solana galaxy to stop Chairman Drek from building a new planet out of the
    pieces of others. Played in rac-rs-multiplayer, the port of the PS2 game to the PC.
    """
    game = GAME
    web = Rac1Web()
    options_dataclass = Rac1Options
    options: Rac1Options
    topology_present = True
    item_name_to_id = {item.name: item.item_id for item in Items.ALL}
    location_name_to_id = {loc.name: loc.location_id for loc in Locations.ALL if loc.location_id is not None}
    item_name_groups = Items.get_item_groups()
    location_name_groups = Locations.get_location_groups()
    starting_weapon: Optional[WeaponData] = None
    # This world's progression items, placed by `pre_fill` (not in the item pool).
    progression: list[Item]

    def get_filler_item_name(self) -> str:
        return Items.BOLT_PACK.name

    def create_regions(self) -> None:
        create_regions(self)

    def create_item(self, name: str, override: Optional[ItemClassification] = None) -> Item:
        data = Items.from_name(name)
        return Rac1Item(name, override or ItemPool.get_classification(data), data.item_id, self.player)

    def create_event(self, name: str) -> Item:
        return Rac1Item(name, ItemClassification.progression, None, self.player)

    def create_items(self) -> None:
        pool: list[Item] = []
        pool += ItemPool.create_planets(self)
        pool += ItemPool.create_equipment(self)
        pool += ItemPool.create_collectables(self)
        # The progression stays out of the pool: `pre_fill` places it.
        self.progression = [i for i in pool if i.advancement]
        pool = [i for i in pool if not i.advancement]
        unfilled = [loc for loc in self.multiworld.get_unfilled_locations(self.player) if loc.address is not None]
        remain = len(unfilled) - len(self.progression) - len(pool)
        if remain < 0:
            # More items than places (several items put in the starting inventory can't make room otherwise): gold bolts go.
            for _ in range(-remain):
                gold = next((i for i in pool if i.name == Items.GOLD_BOLT.name), None)
                if gold is None:
                    break
                pool.remove(gold)
            remain = len(unfilled) - len(self.progression) - len(pool)
        pool += [self.create_item(Items.BOLT_PACK.name) for _ in range(max(0, remain))]
        self.multiworld.itempool += pool

    def pre_fill(self) -> None:
        """This world's progression is placed here, forward, as a playthrough finds it: each item goes to a location reachable
        with the items placed before it, so every seed is beatable. Ratchet & Clank's route is one chain (Kerwan's gadgets, then
        Blarg's, Gaspar's, Pokitaru's ...) with few places open before each link, where Archipelago's assumed fill often finds
        no place left for the first links. When places run short, an item that opens new ones goes first."""
        mine = list(self.progression)
        if not mine:
            return
        empties = [loc for loc in self.multiworld.get_unfilled_locations(self.player) if loc.address is not None]
        for _ in range(200):
            placed = self._forward_fill(mine, empties)
            if placed is not None:
                break
        else:
            raise Exception(f"Ratchet & Clank: no forward placement of the progression found for {self.player_name}")
        for loc, item in placed:
            self.multiworld.push_item(loc, item, collect=False)
            loc.locked = True
        self.progression = []

    def get_pre_fill_items(self) -> list[Item]:
        # What `pre_fill` places (Archipelago's state of all items counts them until then).
        return list(self.progression)

    def _forward_fill(self, items: list[Item], empties: list) -> Optional[list]:
        from BaseClasses import CollectionState

        rank = {name: k for k, name in enumerate(Logic.ROUTE)}
        state = CollectionState(self.multiworld)
        remaining = list(items)
        self.random.shuffle(remaining)
        free = [loc for loc in empties if loc.progress_type != LocationProgressType.EXCLUDED]
        placed = []
        while remaining:
            reach = [loc for loc in free if loc.can_reach(state)]
            if not reach:
                return None
            if len(reach) > 5:
                item = remaining[-1]
            else:
                # Few places left now: an item that opens more goes first (more is likelier); when none does alone (the route
                # has gates that take several, as Rilgar's: the Hydrodisplacer, the Grindboots and the Trespasser), the next
                # one of the vanilla route.
                def gain(it: Item) -> int:
                    s = state.copy()
                    s.collect(it, True)
                    return sum(1 for loc in free if loc.can_reach(s)) - len(reach)
                gains = [(it, gain(it)) for it in remaining]
                openers = [(it, g) for it, g in gains if g > 0]
                if openers:
                    item = self.random.choices([it for it, _ in openers], weights=[g for _, g in openers])[0]
                else:
                    item = min(remaining, key=lambda it: (rank.get(it.name, len(rank)), self.random.random()))
            loc = self.random.choice(reach)
            remaining.remove(item)
            free.remove(loc)
            placed.append((loc, item))
            state.collect(item, True)
        return placed

    def set_rules(self) -> None:
        goal = self.multiworld.get_location(Locations.DEFEAT_DREK.name, self.player)
        goal.place_locked_item(self.create_event("Victory"))
        self.multiworld.completion_condition[self.player] = lambda state: state.has("Victory", self.player)

    def get_options_as_dict(self) -> Dict[str, Any]:
        return self.options.as_dict(
            "death_link",
            "starting_planets",
            "random_start_weapon",
            "randomize_vendor",
            "exclude_very_expensive_items",
            "bolt_pack_size",
        )

    def fill_slot_data(self) -> Mapping[str, Any]:
        data: Dict[str, Any] = self.get_options_as_dict()
        data["bolt_pack"] = BOLT_PACK_BOLTS[self.options.bolt_pack_size.value]
        data["starting_weapon"] = self.starting_weapon.name if self.starting_weapon else Items.BOMB_GLOVE.name
        return data
