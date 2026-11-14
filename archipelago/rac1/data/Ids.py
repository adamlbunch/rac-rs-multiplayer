"""The numbers the game side uses (rac-rs/src/ap.rs): the same here, or items and locations go astray."""

# Items: the game's own item id, a planet's number, a special item's byte, a gold bolt, a bolt pack.
ITEM_EQUIP = 1000
ITEM_COORDS = 2000
ITEM_SPECIAL = 3000
ITEM_GOLD_BOLT = 4000
ITEM_BOLT_PACK = 4001

# Locations: the item the game gives there, the planet an infobot unlocks, a special item (level * 16 + byte),
# a gold bolt (level * 4 + index), a weapon bought at a Gadgetron vendor.
LOC_ITEM = 10000
LOC_INFOBOT = 20000
LOC_SPECIAL = 30000
LOC_GOLD = 40000
LOC_VENDOR = 50000

# The port the game listens on for the client (RAC_AP).
DEFAULT_GAME_PORT = 28111
