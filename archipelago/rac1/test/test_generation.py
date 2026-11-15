from ..data import Ids, Items, Locations
from . import Rac1TestBase


class TestDefault(Rac1TestBase):
    def test_counts(self) -> None:
        story = len(Locations.STORY)
        self.assertEqual(story, 43)
        self.assertEqual(len(Locations.GOLD_BOLTS), 40)
        locations = [loc for loc in self.multiworld.get_locations(self.player) if loc.address is not None]
        self.assertEqual(len(locations), story + 40)
        # The progression is placed before the general fill (`pre_fill`): the rest is in the pool.
        pool = [i for i in self.multiworld.itempool if i.player == self.player]
        placed = [loc.item for loc in locations if loc.item is not None]
        self.assertEqual(len(pool) + len(placed), len(locations))
        for loc in locations:
            if loc.item is not None:
                self.assertTrue(loc.item.advancement)

    def test_ids_are_the_game_sides(self) -> None:
        # rac-rs/src/rac1/ap.rs reports these numbers.
        self.assertEqual(Locations.KERWAN_SWINGSHOT.location_id, Ids.LOC_ITEM + 12)
        self.assertEqual(Locations.NOVALIS_INFOBOT_ARIDIA.location_id, Ids.LOC_INFOBOT + 2)
        self.assertEqual(Locations.RILGAR_ZOOMERATOR.location_id, Ids.LOC_SPECIAL + 5 * 16 + 0)
        self.assertEqual(Locations.FLEET_CODEBOT.location_id, Ids.LOC_SPECIAL + 17 * 16 + 2)
        self.assertEqual(Items.SWINGSHOT.item_id, Ids.ITEM_EQUIP + 12)
        self.assertEqual(Items.coords_for_planet(5).item_id, Ids.ITEM_COORDS + 5)
        self.assertEqual(Items.GOLD_BOLT.item_id, Ids.ITEM_GOLD_BOLT)

    def test_unique_ids(self) -> None:
        ids = [loc.location_id for loc in Locations.ALL if loc.location_id is not None]
        self.assertEqual(len(ids), len(set(ids)))
        names = [loc.name for loc in Locations.ALL]
        self.assertEqual(len(names), len(set(names)))

    def test_novalis_first(self) -> None:
        starts = [i.name for i in self.multiworld.precollected_items[self.player]]
        self.assertIn(Items.NOVALIS_COORDS.name, starts)

    def test_goal_needs_the_late_game(self) -> None:
        self.assertFalse(self.can_reach_location(Locations.DEFEAT_DREK.name))
        self.collect_all_but(["Victory"])
        self.assertTrue(self.can_reach_location(Locations.DEFEAT_DREK.name))


class TestVendor(Rac1TestBase):
    options = {"randomize_vendor": True, "random_start_weapon": True, "starting_planets": 2}

    def test_vendor_locations(self) -> None:
        names = {loc.name for loc in self.multiworld.get_locations(self.player)}
        for loc in Locations.VENDOR:
            self.assertIn(loc.name, names)
        pool = [i.name for i in self.multiworld.itempool if i.player == self.player]
        self.assertIn(Items.BOMB_GLOVE.name, pool)
        self.assertIsNotNone(self.world.starting_weapon)


class TestFewestStarts(Rac1TestBase):
    options = {"starting_planets": 2, "exclude_very_expensive_items": False}

    def test_novalis_and_kerwan(self) -> None:
        coords = [i.name for i in self.multiworld.precollected_items[self.player] if i.name.endswith("Coordinates")]
        self.assertEqual(sorted(coords), sorted([Items.NOVALIS_COORDS.name, Items.KERWAN_COORDS.name]))


class TestManyStarts(Rac1TestBase):
    options = {"starting_planets": 5, "randomize_vendor": True}
