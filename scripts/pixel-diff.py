#!/usr/bin/env python3
"""#266 gate 2: the screenshots of a migrated tab match the pre-migration ones. For every PNG in BEFORE, the share of
pixels that differ in the same-named PNG in AFTER (any channel off by more than 8) must be at most THRESHOLD (0.1 %).
Usage: scripts/pixel-diff.py BEFORE AFTER [THRESHOLD]; shoot both with scripts/ui-audit.sh on the same machine."""
import os, struct, sys, zlib


def png(path):  # 8-bit RGB(A), non-interlaced: Chrome's screenshots
    b = open(path, "rb").read()
    i, idat = 8, b""
    while i < len(b):
        n, t = struct.unpack(">I4s", b[i:i + 8])
        if t == b"IHDR": w, h, _, ct = struct.unpack(">IIBB", b[i + 8:i + 18])
        if t == b"IDAT": idat += b[i + 8:i + 8 + n]
        i += n + 12
    return w, h, (4 if ct == 6 else 3), zlib.decompress(idat)


def rows(w, h, bpp, raw):
    out, prev, n = [], bytearray(w * bpp), w * bpp
    for y in range(h):
        f, line = raw[y * (n + 1)], bytearray(raw[y * (n + 1) + 1:(y + 1) * (n + 1)])
        for x in range(n):
            a, c, u = line[x - bpp] if x >= bpp else 0, prev[x - bpp] if x >= bpp else 0, prev[x]
            p = a + u - c
            line[x] = (line[x] + [0, a, u, (a + u) // 2, a if abs(p - a) <= abs(p - u) and abs(p - a) <= abs(p - c) else u if abs(p - u) <= abs(p - c) else c][f]) & 255
        out.append(bytes(line))
        prev = line
    return out


before, after = sys.argv[1], sys.argv[2]
limit, fail = float(sys.argv[3]) if len(sys.argv) > 3 else 0.001, 0
for name in sorted(f for f in os.listdir(before) if f.endswith(".png")):
    if not os.path.exists(os.path.join(after, name)):
        print(f"{name}: missing in {after}"); fail = 1; continue
    a, b = png(os.path.join(before, name)), png(os.path.join(after, name))
    if a[:2] != b[:2]:
        print(f"{name}: {a[0]}x{a[1]} vs {b[0]}x{b[1]}"); fail = 1; continue
    w, h, bpp, _ = a
    if a == b:
        print(f"{name}: identical"); continue
    ra, rb, d = rows(*a), rows(*b), 0
    for la, lb in zip(ra, rb):
        if la != lb:
            d += sum(1 for x in range(0, w * bpp, bpp) if any(abs(la[x + k] - lb[x + k]) > 8 for k in range(3)))
    share = d / (w * h)
    print(f"{name}: {d} of {w * h} pixels differ ({share:.4%})")
    fail |= share > limit
sys.exit(fail)
