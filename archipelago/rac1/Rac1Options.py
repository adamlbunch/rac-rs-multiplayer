from dataclasses import dataclass

from Options import Choice, DeathLink, DefaultOnToggle, PerGameCommonOptions, Range, StartInventoryPool, Toggle


class StartingPlanets(Range):
    """How many planets the ship can fly to at the start: always Novalis and Kerwan (where the vanilla game goes first), and
    more picked at random among the early ones."""
    display_name = "Starting Planets"
    range_start = 2
    range_end = 5
    default = 3


class RandomStartWeapon(Toggle):
    """Start with a random weapon from the first vendors' stock (Pyrocitor, Blaster, Glove of Doom, Mine Glove or Taunter)
    instead of the Bomb Glove, which goes into the item pool."""
    display_name = "Random Starting Weapon"


class RandomizeVendor(Toggle):
    """The Gadgetron vendors' weapons become locations: buying one sends its item, and the weapons go into the item pool. A weapon
    a vendor sells appears once you land on the planet that adds it (as in the vanilla game). Ammo is bought as usual."""
    display_name = "Randomize Gadgetron Vendor"


class ExcludeVeryExpensiveItems(DefaultOnToggle):
    """No progression item is put where it costs a fortune: the R.Y.N.O. salesman on Rilgar (150,000 bolts)."""
    display_name = "Exclude Very Expensive Items"


class BoltPackSize(Choice):
    """How many bolts a Bolt Pack (the filler item) gives."""
    display_name = "Bolt Pack Size"
    option_small = 0
    option_medium = 1
    option_large = 2
    default = 1


@dataclass
class Rac1Options(PerGameCommonOptions):
    start_inventory_from_pool: StartInventoryPool
    death_link: DeathLink
    starting_planets: StartingPlanets
    random_start_weapon: RandomStartWeapon
    randomize_vendor: RandomizeVendor
    exclude_very_expensive_items: ExcludeVeryExpensiveItems
    bolt_pack_size: BoltPackSize


# The bolts of a Bolt Pack by `bolt_pack_size`.
BOLT_PACK_BOLTS = {0: 500, 1: 1000, 2: 2500}
