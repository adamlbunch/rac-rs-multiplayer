#!/usr/bin/env python3
"""mp_disc_check.py - checks, on a disc image, the two things multiplayer reads from each level's own code (this fork's test helper):
the hit queue in `hit_ring_write` (mp/enemies.rs `hit_ring`) and the level's "start scene" (mp/scenes.rs `find`). It builds each
level's RAM as the generator does (boot ELF + overlay records, tools/recgen_iso.py) and looks for both as the game does.

  python -I tools/mp_disc_check.py DISC.iso

On the USA disc it also places both functions in the port's PAL v1.00 -> USA map (rac-rs/src/retail/disc_map.rs): wholly inside
one aligned run, the PAL v1.00 disc has the same code there, so the check holds for it without its image. On a PAL v1.00 image it
looks there directly and compares with the map's prediction. Nothing is written; no disc bytes are kept.
"""
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SEC = 0x800
# hit_ring_write in level 0 (Veldin) on each disc: the copy the other levels' are matched against.
VELDIN_HIT_RING = {"SCUS_971.99": 0x259A88, "SCES_509.16": 0x259698}
# The words of the scene start mp/scenes.rs looks for: addiu v0, zero, 2; addiu a0, zero, 100; addiu a1, zero, 2.
SCENE_WORDS = (0x24020002, 0x24040064, 0x24050002)
KIND = {0: "run", 1: "gap", 2: "ext", 3: "tail", 4: "ambig"}


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def wad_decompress():
    """The generator's own WAD decoder (tools/recgen_iso.py `iso_wad_decompress`), taken from its source: one copy of it."""
    src = open(os.path.join(HERE, "recgen_iso.py"), encoding="utf-8").read()
    a = src.index("def iso_wad_decompress")
    ns = {"iso_u32": u32}
    exec(src[a:src.index("\ndef ", a + 10)], ns)
    return ns["iso_wad_decompress"]


class Disc:
    def __init__(self, path):
        self.f = open(path, "rb")
        self.dec = wad_decompress()
        self.boot_name, self.boot = self._boot()

    def bytes(self, lba, n):
        self.f.seek(lba * SEC)
        return self.f.read(n)

    def _boot(self):
        pvd = self.bytes(16, SEC)
        assert pvd[1:6] == b"CD001", "not an ISO9660 image"
        dd = self.bytes(u32(pvd, 158), u32(pvd, 166))
        o = 0
        while o < len(dd):
            ln = dd[o]
            if ln == 0:
                o = (o // SEC + 1) * SEC
                continue
            name = dd[o + 33:o + 33 + dd[o + 32]].decode("latin1").split(";")[0]
            if name.upper() in VELDIN_HIT_RING:
                return name.upper(), self.bytes(u32(dd, o + 2), u32(dd, o + 10))
            o += ln
        sys.exit("no SCUS_971.99 or SCES_509.16 on this disc")

    def level_ram(self, lvl):
        ram = bytearray(0x2000000)
        elf = self.boot
        phoff, (phentsize, phnum) = u32(elf, 28), struct.unpack_from("<HH", elf, 42)
        for i in range(phnum):
            t, off, va, pa, fs, ms, fl, al = struct.unpack_from("<8I", elf, phoff + i * phentsize)
            if t == 1:
                ram[va:va + fs] = elf[off:off + fs]
        toc = self.bytes(1500, 0x2960)
        assert u32(toc, 0) == 1 and u32(toc, 4) == 0x2960, "the table of contents is not where v1.00 has it (another revision?)"
        hdr = self.bytes(u32(toc, 0x28C8 + lvl * 8), 0x2434)
        assert u32(hdr, 0) == lvl and u32(hdr, 4) == 0x2434, f"level {lvl}: no header"
        data = self.bytes(u32(hdr, 8), u32(hdr, 12) * SEC)
        off, size = u32(data, 0), u32(data, 4)
        ov = data[off:off + size]
        ov = self.dec(ov) if ov[:3] == b"WAD" else ov
        p = 0
        while p < len(ov):
            load, sz, _, _ = struct.unpack_from("<4I", ov, p)
            ram[load:load + sz] = ov[p + 16:p + 16 + sz]
            p += 16 + sz
        return ram


def shape(w):
    """What two builds of one function share: a jump keeps its opcode, the rest opcode and registers (immediates are relocated)."""
    op = w >> 26
    return (op,) if op in (2, 3) else (w >> 16,) if op else (w & 0xFC1FFFFF,)


def func(ram, a):
    """The words from `a` to its jr ra and delay slot."""
    out = []
    while True:
        out.append(u32(ram, a))
        a += 4
        if out[-1] == 0x03E00008:
            out.append(u32(ram, a))
            return out


def find(ram, want):
    # Cheap first test on the first word, then all.
    first = want[0]
    for a in range(0x100000, 0x400000, 4):
        if shape(u32(ram, a)) == first and all(shape(u32(ram, a + 4 * k)) == want[k] for k in range(1, len(want))):
            return a
    return None


def hit_queue(ram, entry):
    """As mp/enemies.rs reads it: lui v0 / addiu v0, v0 the block (index at +0x14), lui s0 / addiu a0, s0 the queue."""
    at = lambda op: next((a for a in range(entry, entry + 0x80, 4) if u32(ram, a) >> 16 == op), None)
    ws = [at(op) for op in (0x3C02, 0x2442, 0x3C10, 0x2604)]
    if None in ws:
        return None
    lo = lambda w: ((w & 0xFFFF) ^ 0x8000) - 0x8000
    lv, bl, ls, ql = (u32(ram, a) for a in ws)
    return ((ls & 0xFFFF) << 16) + lo(ql), ((lv & 0xFFFF) << 16) + lo(bl) + 0x14


def scene_start(ram):
    """As mp/scenes.rs finds it: the three words, then the entry (addiu sp, sp, -n) before them."""
    for a in range(0x1F0000, 0x380000, 4):
        if (u32(ram, a), u32(ram, a + 4), u32(ram, a + 8)) == SCENE_WORDS:
            return next((p for p in range(a - 4, a - 0x400, -4) if u32(ram, p) >> 16 == 0x27BD and u32(ram, p) & 0x8000), None)
    return None


def pal_map():
    dm = open(os.path.join(HERE, "..", "rac-rs", "src", "retail", "disc_map.rs"), encoding="utf-8").read()
    out = {}
    for m in re.finditer(r"pub static L(\d+): &\[Seg\] = &\[(.*?)\n\];", dm, re.S):
        out[int(m.group(1))] = [tuple(int(x, 0) for x in g) for g in
                                re.findall(r"lo: (0x[0-9a-f]+), hi: (0x[0-9a-f]+), d: (-?\d+), d_hi: (-?\d+), kind: (\d+)", m.group(2))]
    return out


def in_run(segs, lo_usa, hi_usa):
    """The PAL v1.00 address of USA [lo, hi), when wholly inside one aligned run of the map; else what covers it."""
    cover = [(lo, hi, d, k) for lo, hi, d, _, k in segs if lo + d < hi_usa and hi + d > lo_usa]
    if len(cover) == 1 and cover[0][3] == 0:
        return lo_usa - cover[0][2], None
    return None, ", ".join(f"{KIND[k]} d={d:+d}" for _, _, d, k in cover) or "no segment"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    disc = Disc(sys.argv[1])
    usa = disc.boot_name == "SCUS_971.99"
    print(f"{sys.argv[1]}: {disc.boot_name} ({'USA' if usa else 'PAL'})")
    ref = [shape(w) for w in func(disc.level_ram(0), VELDIN_HIT_RING[disc.boot_name])]
    segs = pal_map()
    bad = 0
    for lvl in range(19):
        ram = disc.level_ram(lvl)
        ring = find(ram, ref)
        scene = scene_start(ram)
        q = ring and hit_queue(ram, ring)
        line = f"level {lvl:2d}: hit_ring_write {ring and hex(ring)}, queue {q and hex(q[0])}, index {q and hex(q[1])}; start scene {scene and hex(scene)}"
        ok = bool(q) and scene is not None
        if usa and ok:
            for name, a in (("hit_ring_write", ring), ("start scene", scene)):
                pal, why = in_run(segs[lvl], a, a + 4 * len(func(ram, a)))
                line += f"; {name} on PAL v1.00 at {pal:#x}" if pal else f"; {name} NOT in one aligned run ({why})"
                ok &= pal is not None
        elif ok:
            # PAL v1.00: where the map says the USA ones come from.
            line += " (PAL v1.00 directly)"
        bad += not ok
        print(line + ("" if ok else "   <- FAILS"))
    print("all 19 levels pass" + (", and for PAL v1.00 through the map" if usa else "") if not bad else f"{bad} level(s) fail")
    sys.exit(1 if bad else 0)


main()
