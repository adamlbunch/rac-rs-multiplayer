import typing

from BaseClasses import CollectionState, Location, LocationProgressType, Region

from .data import Items, Locations
from .data.Locations import LocationData

if typing.TYPE_CHECKING:
    from . import Rac1World


class Rac1Location(Location):
    game: str = "Ratchet & Clank"


def create_regions(world: "Rac1World") -> None:
    """Menu, then one region a planet, reached with its coordinates (Veldin, where the game begins, is the menu)."""
    menu = Region("Menu", world.player, world.multiworld)
    world.multiworld.regions.append(menu)
    options = world.get_options_as_dict()
    by_planet: dict[int, list[LocationData]] = {}
    for loc in Locations.active(options):
        by_planet.setdefault(loc.planet, []).append(loc)

    for planet, locs in sorted(by_planet.items()):
        region = Region(Items.PLANET_NAMES[planet], world.player, world.multiworld)
        world.multiworld.regions.append(region)
        coords = Items.coords_for_planet(planet).name

        def planet_rule(state: CollectionState, name: str = coords) -> bool:
            return state.has(name, world.player)

        menu.connect(region, f"Fly to {Items.PLANET_NAMES[planet]}", planet_rule)
        for data in locs:
            location = Rac1Location(world.player, data.name, data.location_id, region)

            def rule(state: CollectionState, d: LocationData = data) -> bool:
                return d.rule(state, world.player)

            location.access_rule = rule
            if data.expensive and world.options.exclude_very_expensive_items:
                location.progress_type = LocationProgressType.EXCLUDED
            region.locations.append(location)
