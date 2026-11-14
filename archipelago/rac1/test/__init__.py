from typing import ClassVar

from BaseClasses import get_seed
from test.bases import WorldTestBase


class Rac1TestBase(WorldTestBase):
    game = "Ratchet & Clank"
    player: ClassVar[int] = 1
    seed = get_seed(1)

    def world_setup(self, *args, **kwargs):
        super().world_setup(seed=self.seed)
        if self.constructed:
            self.world = self.multiworld.worlds[self.player]
