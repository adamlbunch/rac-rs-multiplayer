"""A small ISO9660 (level 2 names) image of the files in a folder, flat: for the VM's answer disc.  usage: mkiso.py DIR OUT.iso LABEL"""
import os
import struct
import sys

SEC = 2048


def both16(v):
    return struct.pack("<H", v) + struct.pack(">H", v)


def both32(v):
    return struct.pack("<I", v) + struct.pack(">I", v)


def dirrec(name, extent, size, isdir):
    n = len(name)
    rec = bytes([0, 0]) + both32(extent) + both32(size) + bytes([126, 1, 1, 0, 0, 0, 0]) + bytes([2 if isdir else 0, 0, 0]) + both16(1) + bytes([n]) + name
    if len(rec) % 2:
        rec += b"\0"
    return bytes([len(rec)]) + rec[1:]


src, out, label = sys.argv[1], sys.argv[2], sys.argv[3]
files = sorted(f for f in os.listdir(src) if os.path.isfile(os.path.join(src, f)))
datas = [open(os.path.join(src, f), "rb").read() for f in files]
root_lba, first = 20, 21
lbas, lba = [], first
for d in datas:
    lbas.append(lba)
    lba += max(1, -(-len(d) // SEC))
total = lba
root = dirrec(b"\0", root_lba, SEC, True) + dirrec(b"\1", root_lba, SEC, True)
for f, d, l in zip(files, datas, lbas):
    root += dirrec((f.upper() + ";1").encode(), l, len(d), False)
assert len(root) <= SEC
date = b"0" * 16 + b"\0"
pvd = bytearray(SEC)
pvd[0:7] = b"\x01CD001\x01"
pvd[40:72] = label.upper().encode().ljust(32)
pvd[80:88] = both32(total)
pvd[120:124] = both16(1)
pvd[124:128] = both16(1)
pvd[128:132] = both16(SEC)
pvd[132:140] = both32(10)
pvd[140:144] = struct.pack("<I", 18)
pvd[148:152] = struct.pack(">I", 19)
pvd[156:190] = dirrec(b"\0", root_lba, SEC, True)
for o in (190, 318, 446, 574):
    pvd[o:o + 128] = b" " * 128
pvd[813:881] = date * 4
pvd[881] = 1
term = bytearray(SEC)
term[0:7] = b"\xffCD001\x01"
lpt = bytes([1, 0]) + struct.pack("<I", root_lba) + struct.pack("<H", 1) + b"\0\0"
mpt = bytes([1, 0]) + struct.pack(">I", root_lba) + struct.pack(">H", 1) + b"\0\0"
img = bytearray(SEC * 16) + pvd + term + lpt.ljust(SEC, b"\0") + mpt.ljust(SEC, b"\0") + root.ljust(SEC, b"\0")
for d in datas:
    img += d + b"\0" * (-len(d) % SEC or (SEC if not d else 0))
open(out, "wb").write(img)
print(f"{out}: {len(files)} files, {len(img)} bytes")
