"""Farm v2 sprite sheet (#242): writes sprites.svg next to this file.

The horse is farm's current one (assets/themes/farm/sprites.svg, Corral's horsesvg.py port): the same body,
neck and leg shapes, coats as --c/--d/--m/--b, legs as `.leg` groups with `--a` (herd.js turns them into the
trot). New: --fk, the project colour (#162's hash), worn as a halter and a saddle cloth; a bent leg; the props.
Every action sprite is drawn in the 132 x 100 box; row sprites are 16 x 16 pixel art.

    python3 docs/design/farm-v2/make_sprites.py
"""

from pathlib import Path

HOOF = "#3a332e"
FK = "var(--fk,#8b949e)"
WOOD, WOOD_D, WOOD_L = "var(--fp,#9c916a)", "var(--fr,#8b825d)", "var(--fh,#e2d7ac)"
HAY, HAY_D, TWINE = "#d9b44a", "#b38a2c", "#7a5a22"
RED, RED_D = "#d2453b", "#9e2a22"


def f(n):
    return f"{n:g}"


def leg(x, near, a, bend=None):
    """One leg hung at (x, 60); `bend` folds it at the knee (the lower half turns by `bend` degrees)."""
    w = 5.0 if near else 4.4
    fill = "var(--c)" if near else "var(--d)"
    cls = (
        ("leg-nf" if x > 64 else "leg-nh")
        if near
        else ("leg-ff" if x > 64 else "leg-fh")
    )
    r = w / 2
    if bend is None:
        inner = (
            f'<rect x="{f(-r)}" y="0" width="{f(w)}" height="34" rx="{f(r)}" fill="{fill}"/>'
            f'<rect x="{f(-r)}" y="29" width="{f(w)}" height="5" rx="2.5" fill="{HOOF}"/>'
        )
    else:
        # herd.js turns `.leg` round its fill box's top centre: an empty 40 px rect keeps that at the hip.
        inner = (
            '<rect x="-20" y="0" width="40" height="1" fill="none"/>'
            f'<rect x="{f(-r)}" y="0" width="{f(w)}" height="19" rx="{f(r)}" fill="{fill}"/>'
            f'<g transform="translate(0 17) rotate({bend})"><rect x="{f(-r)}" y="0" width="{f(w)}" height="17" rx="{f(r)}" fill="{fill}"/>'
            f'<rect x="{f(-r)}" y="12" width="{f(w)}" height="5" rx="2.5" fill="{HOOF}"/></g>'
        )
    return f'<g transform="translate({f(x)} 60)"><g class="leg {cls}" style="--a:{a}deg">{inner}</g></g>'


TAIL = {
    "down": "M 39 43 Q 28 53 27 69",
    "up": "M 40 41 Q 30 35 26 21",
    "flow": "M 40 41 Q 28 38 20 30",
    "swish": "M 39 43 Q 26 47 22 60",
}

HEAD = (
    '<path d="M 80 40 C 92 30 102 22 110 15 L 118 20 Q 124 23 127 30 Q 129 34 127 36 Q 124 39 120 38 Q 112 36 106 31 Q 100 40 96 50 L 84 52 Z" fill="var(--c)"/>'
    '<path d="M 104 28 Q 110 32 116 34 Q 108 38 101 33 Z" fill="var(--d)" opacity="0.3"/>'
    '<path d="M 122 27 Q 128 31 127 36 Q 124 39 120 38 Q 120 32 122 27 Z" fill="#c9958b"/>'
    '<circle cx="124.5" cy="32.5" r="1.1" fill="#2f2a26"/>'
    '<path d="M 109 14 L 111 5 L 116 11 Z" fill="var(--c)"/><path d="M 111 11 L 112 8 L 114 10 Z" fill="#c9958b"/>'
    '<circle cx="116" cy="22.5" r="1.7" fill="#1d1a17"/>'
    '<path d="M 119 21 Q 123 25 125 30 L 123 31 Q 120 26 117 22 Z" fill="var(--b)"/>'
)
MANE = '<path d="M 82 37 Q 95 26 108 15 L 111 18 Q 98 28 86 39 Z" fill="var(--m)"/>'
# The project colour: a halter (crown, cheek and nose straps, a brass ring) on the head.
HALTER = (
    f'<path d="M 108.5 15.5 Q 112 22 118.5 30.5 M 117.5 21.5 Q 119.5 30 121.5 37.6" stroke="{FK}" stroke-width="2.3" fill="none" stroke-linecap="round"/>'
    '<circle cx="119" cy="31" r="1.5" fill="#d9c27a"/>'
)
BODY = (
    '<path d="M 40 40 Q 47 35.5 60 38.5 Q 72 41 80 36 Q 87 33 90 42 Q 92 48 92 54 Q 92 60 88 64 Q 66 68 44 62 Q 31 57 30 54 Q 30 46 34 42 Q 37 39.5 40 40 Z" fill="var(--c)"/>'
    '<path d="M 34 42 Q 30 46 30 54 Q 31 57 44 62 Q 40 61 38 46 Q 37 42 38 40 Z" fill="var(--d)" opacity="0.32"/>'
    '<path d="M 80 36 Q 87 33 90 42 Q 91 47 91 52 Q 85 50 82 44 Q 80 40 80 36 Z" fill="var(--d)" opacity="0.22"/>'
)
# The saddle cloth: the project colour on the back, a light trim along its lower edge.
CLOTH = (
    f'<path d="M 51 37.6 Q 60 39.2 70 40.6 Q 72 48 71 55 Q 61 56.5 52 54.5 Q 50 46 51 37.6 Z" fill="{FK}"/>'
    '<path d="M 52 54.5 Q 61 56.5 71 55" stroke="#f4ecd8" stroke-opacity="0.55" stroke-width="1.2" fill="none"/>'
)


def horse(
    legs=(0, 0, 0, 0),
    bends=(None, None, None, None),
    neck=-6,
    neck_dy=0,
    tail="down",
    wrap="",
    cloth=True,
    extra_back="",
    shadow=True,
    neck_t=None,
):
    """legs/bends: ff, fh, nf, nh. `wrap`: a transform round the whole horse; `extra_back`: drawn on the back."""
    ff, fh, nf, nh = legs
    bff, bfh, bnf, bnh = bends
    neck_t = neck_t or f"rotate({neck} 90 46)" + (
        f" translate(0 {neck_dy})" if neck_dy else ""
    )
    s = ""
    if shadow:
        s += '<ellipse cx="64" cy="97" rx="44" ry="4" fill="#2f2a26" opacity="0.14"/>'
    s += f'<g transform="{wrap or "translate(0 0)"}">'
    s += leg(78, False, ff, bff) + leg(51, False, fh, bfh)
    s += f'<path d="{TAIL[tail]}" stroke="var(--m)" stroke-width="6.5" fill="none" stroke-linecap="round"/>'
    s += f'<g class="neck" transform="{neck_t}">{HEAD}{HALTER}{MANE}</g>'
    s += BODY + (CLOTH if cloth else "") + extra_back
    s += leg(84, True, nf, bnf) + leg(43, True, nh, bnh)
    return s + "</g>"


def g(id_, body, cls="horse"):
    return f'<g id="{id_}" class="{cls} {id_}">{body}</g>'


# --- props (box coordinates) ---


def plough():
    soil = '<path d="M 0 95.5 Q 20 92.5 44 95.5 L 44 99 L 0 99 Z" fill="#6b4a2f" opacity="0.55"/>'
    for x, y in ((6, 94.5), (14, 93.6), (25, 93.8), (35, 94.6)):
        soil += f'<ellipse cx="{x}" cy="{y}" rx="2.2" ry="1.3" fill="#5a3d26" opacity="0.7"/>'
    p = (
        '<path d="M 3 62 L 15 86 M 9 61 L 19 85" stroke="#6f4b2a" stroke-width="2.4" stroke-linecap="round"/>'
        '<path d="M 14 86 L 40 84" stroke="#6f4b2a" stroke-width="3" stroke-linecap="round"/>'
        '<path d="M 19 85 L 30 85 L 26 95 Q 20 96 16 93 Z" fill="#8f949b"/><path d="M 26 95 Q 20 96 16 93 L 19 92 Q 23 94 26 93 Z" fill="#c6cad0"/>'
    )
    return soil + p


TRACES = '<path d="M 40 84 Q 58 72 79.5 55.5" stroke="#3a2b20" stroke-width="1.6" fill="none"/>'
# A collar round the neck base, in the project colour, for the plough's traces.
COLLAR = f'<path d="M 79 37 Q 86 46 86 58" stroke="{FK}" stroke-width="5" fill="none" stroke-linecap="round"/><path d="M 79 37 Q 86 46 86 58" stroke="#3a2b20" stroke-opacity="0.35" stroke-width="1.2" fill="none"/>'


def anvil():
    return (
        '<path d="M 6 97 L 10 88 L 24 88 L 28 97 Z" fill="#5b4a3a"/>'  # the stump
        '<path d="M 12 88 L 14 82 L 21 82 L 23 88 Z" fill="#3d4148"/>'
        '<path d="M 4 76 L 28 76 Q 33 76 36 73 Q 33 80 26 81 L 10 81 Q 6 80 4 76 Z" fill="#4a4f57"/>'
        '<path d="M 4 76 L 28 76 Q 33 76 36 73 L 33 74.5 Q 30 77.2 27 77.4 L 5.6 77.4 Z" fill="#7d838c"/>'
        '<path d="M 13 74.6 a 3.4 3 0 1 1 6.8 0" stroke="#ff9a3c" stroke-width="2" fill="none"/>'  # a hot shoe
        '<g transform="rotate(-35 26 60)"><rect x="25" y="52" width="2.4" height="16" rx="1" fill="#6f4b2a"/><rect x="21.5" y="49" width="9.5" height="5" rx="1" fill="#3d4148"/></g>'
        '<path d="M 18 69 l 1.2 -3 l 1.2 3 l -1.2 3 Z M 9 70 l 1 -2.4 l 1 2.4 l -1 2.4 Z M 27 68 l .9 -2 l .9 2 l -.9 2 Z" fill="#ffd27a"/>'
    )


def fence_back(x0=0, x1=132):
    s = ""
    for x in (x0 + 4, x1 - 10):
        s += f'<rect x="{x}" y="46" width="6" height="51" fill="{WOOD}"/><rect x="{x + 1}" y="46" width="1.4" height="51" fill="{WOOD_L}" opacity="0.5"/>'
    for y in (52, 70):
        s += f'<rect x="{x0}" y="{y}" width="{x1 - x0}" height="5" fill="{WOOD_D}"/><rect x="{x0}" y="{y}" width="{x1 - x0}" height="1.3" fill="{WOOD_L}" opacity="0.6"/>'
    return f'<g opacity="0.85">{s}</g>'


def vet():
    """A kneeling vet in a white coat, holding the lifted front hoof."""
    return (
        '<ellipse cx="111" cy="97" rx="12" ry="2.5" fill="#2f2a26" opacity="0.14"/>'
        '<path d="M 104 97 L 104 88 Q 104 84 108 84 L 118 84 L 120 97 Z" fill="#3e4a5c"/>'  # trousers, knee down
        '<path d="M 106 86 Q 104 74 110 70 L 116 70 Q 121 74 120 88 Z" fill="#eef1f4"/>'  # coat
        '<path d="M 110 70 L 113 78 L 116 70" stroke="#c9d1d9" stroke-width="1" fill="none"/>'
        '<path d="M 109 74 Q 103 77 99 79" stroke="#eef1f4" stroke-width="3.4" stroke-linecap="round"/>'  # arm to the hoof
        '<circle cx="98.6" cy="79.2" r="1.8" fill="#d9a77f"/>'
        '<circle cx="113" cy="65" r="4.6" fill="#d9a77f"/><path d="M 108.2 63.6 Q 109 58.6 113.6 58.8 Q 118 59.2 118 63.4 Z" fill="#4a3a2c"/>'
        '<path d="M 111.4 71 Q 109 78 112 80 Q 115.4 80.6 115.6 77" stroke="#2f3a48" stroke-width="1" fill="none"/><circle cx="115.6" cy="77" r="1.2" fill="#8a96a8"/>'
    )


def bales_on_back():
    s = f'<path d="M 50 38 L 72 40.5 L 72 47 L 50 45 Z" fill="{FK}"/>'
    for x, y, w, h in ((41, 23, 21, 16), (62, 24.5, 21, 16), (51, 9, 21, 15)):
        s += bale_shape(x, y, w, h)
    s += f'<path d="M 66 41 Q 68 52 66 63" stroke="{FK}" stroke-width="2.6" fill="none"/>'  # girth strap
    return s


def bale_shape(x, y, w, h, fill=HAY, shade=HAY_D, twine=TWINE):
    s = f'<rect x="{f(x)}" y="{f(y)}" width="{f(w)}" height="{f(h)}" rx="2" fill="{fill}"/>'
    s += f'<rect x="{f(x)}" y="{f(y + h * 0.62)}" width="{f(w)}" height="{f(h * 0.38)}" rx="2" fill="{shade}" opacity="0.55"/>'
    for k in range(1, 4):
        xx = x + w * k / 4
        s += f'<path d="M {f(xx - 1.5)} {f(y + 2)} l 1.5 {f(h - 4)}" stroke="{shade}" stroke-width="0.7" opacity="0.8"/>'
    for t in (0.3, 0.7):
        s += f'<rect x="{f(x + w * t - 0.6)}" y="{f(y)}" width="1.2" height="{f(h)}" fill="{twine}"/>'
    return s


def grass(x, y, n=5):
    s = ""
    for i in range(n):
        xx = x + i * 2.6
        s += f'<path d="M {f(xx)} {f(y)} q {f(-1 + i % 2 * 2)} -4 {f(-0.5 + i % 3)} -{f(6 + i % 2 * 2)}" stroke="#5f7d36" stroke-width="1.3" fill="none" stroke-linecap="round"/>'
    return s


def stable_front():
    """The stable seen from the side: its facade with a door hole; the horse inside it is dimmed."""
    wall = "M 90 26 L 111 10 L 132 26 L 132 97 L 90 97 Z M 96 97 L 96 38 L 126 38 L 126 97 Z"
    return (
        f'<path d="{wall}" fill="var(--bn,#a86e52)" fill-rule="evenodd"/>'
        '<path d="M 86 28 L 111 7 L 136 28 L 132 30 L 111 13 L 90 30 Z" fill="var(--rf,#536456)"/>'
        '<rect x="96" y="38" width="30" height="59" fill="var(--dr,#344635)" opacity="0.86"/>'
        '<rect x="94.5" y="36.5" width="33" height="2.5" fill="#efe2c8" opacity="0.8"/>'
        '<rect x="94.5" y="36.5" width="2.5" height="60.5" fill="#efe2c8" opacity="0.8"/><rect x="125" y="36.5" width="2.5" height="60.5" fill="#efe2c8" opacity="0.8"/>'
        '<rect x="106" y="17" width="10" height="8" fill="var(--wn,#e5dfb8)" opacity="0.8"/>'
    )


def gate(x=100):
    """A five-bar farm gate with its brace, and the hanging post."""
    s = f'<rect x="{x + 26}" y="44" width="6" height="53" fill="{WOOD}"/><rect x="{x + 27}" y="44" width="1.4" height="53" fill="{WOOD_L}" opacity="0.5"/>'
    s += f'<rect x="{x}" y="56" width="3.4" height="40" fill="{WOOD}"/>'
    for i in range(5):
        y = 56 + i * 9
        s += f'<rect x="{x}" y="{y}" width="27" height="2.8" fill="{WOOD_D}"/>'
    s += f'<path d="M {x + 2} 93 L {x + 25} 58" stroke="{WOOD_D}" stroke-width="2.6"/>'
    return s


def neigh(x, y):
    return f'<path d="M {x} {y} l 7 -5 M {x + 1.5} {y + 4} l 9 -1 M {x + 1} {y + 8} l 7 3" stroke="{RED}" stroke-width="2.2" stroke-linecap="round"/>'


def low_fence(x, h=26, sign=False):
    """The jump: two posts and three rails (the top one in the project colour)."""
    s = ""
    for xx in (x, x + 22):
        s += f'<rect x="{xx}" y="{97 - h}" width="4.4" height="{h}" fill="{WOOD}"/>'
    s += f'<rect x="{x - 4}" y="{97 - h + 2}" width="34" height="3.6" fill="{FK}"/>'
    for k in (0.45, 0.8):
        s += f'<rect x="{x - 4}" y="{f(97 - h + h * k)}" width="34" height="3" fill="{WOOD_D}"/>'
    return s


def puff(x, y, s=1.0):
    return "".join(
        f'<circle cx="{f(x + dx * s)}" cy="{f(y + dy * s)}" r="{f(r * s)}" fill="#d9c9a3" opacity="0.75"/>'
        for dx, dy, r in (
            (0, 0, 4),
            (5, -2, 3.4),
            (-5, -1, 3),
            (9, 1, 2.4),
            (-9, 1, 2.2),
        )
    )


# --- the action sprites ---
SCALE_R = "translate(18 13.6) scale(0.86)"  # the horse shrunk to 86 % against the right edge, for a prop on the left

ACTIONS = {
    # impl: pulling a plough, a working trot (the gait animates `.leg` as today).
    "horse-plough": plough()
    + f'<g transform="{SCALE_R}">'
    + horse(
        legs=(-22, 20, 24, -22),
        neck=-2,
        tail="swish",
        wrap="translate(0 -1) rotate(-3 66 52)",
        extra_back=COLLAR,
        shadow=False,
    )
    + "</g>"
    + TRACES,
    # fix: shoeing at the anvil, the near hind hoof lifted, a hot shoe and sparks.
    "horse-anvil": anvil()
    + f'<g transform="{SCALE_R}">'
    + horse(
        legs=(0, 0, 0, 30),
        bends=(None, None, None, 62),
        neck=4,
        tail="swish",
        shadow=False,
    )
    + "</g>",
    # rev: the vet checking a front hoof at the fence.
    "horse-vet": fence_back()
    + horse(legs=(0, 0, -44, 0), bends=(None, None, -14, None), neck=-10, tail="down")
    + vet(),
    # update (catch-up): hauling hay bales on a pack saddle, a walk.
    "horse-hay": horse(
        legs=(-12, 10, 12, -10),
        neck=-4,
        tail="down",
        cloth=False,
        extra_back=bales_on_back(),
    ),
    # idle: grazing, the head down in the grass.
    "horse-graze": horse(neck_t="translate(-1 3) rotate(96 86 50)", tail="down")
    + grass(104, 97, 7),
    # done: trotting into the stable (the facade covers the front half; inside is dimmed).
    "horse-stable": '<g transform="translate(-6 0)">'
    + horse(
        legs=(-22, 20, 24, -22),
        neck=-6,
        tail="up",
        wrap="translate(0 -1.5) rotate(-4 66 52)",
    )
    + "</g>"
    + stable_front(),
    # blocked / needs you: rearing and neighing at the gate.
    "horse-rear": gate(98)
    + '<g transform="translate(4 12) scale(0.86)">'
    + horse(
        legs=(-40, 28, -30, 24),
        bends=(110, None, 120, None),
        neck=14,
        tail="down",
        wrap="rotate(-30 47 94)",
        shadow=False,
    )
    + "</g>"
    + '<ellipse cx="44" cy="97" rx="24" ry="3.4" fill="#2f2a26" opacity="0.14"/>'
    + neigh(84, 6),
}

# --- moments ---
MOMENTS = {
    # merged: the fence jump. Three frames; `-over` is the static (reduced-motion) frame.
    "jump-takeoff": low_fence(96)
    + horse(
        legs=(-40, 40, -50, 30),
        bends=(None, None, None, None),
        neck=-14,
        tail="flow",
        wrap="rotate(-14 47 94)",
    ),
    "jump-over": low_fence(54, 20)
    + '<ellipse cx="66" cy="97" rx="30" ry="3" fill="#2f2a26" opacity="0.1"/>'
    + horse(
        legs=(-70, 50, -80, 60),
        bends=(120, None, 130, None),
        neck=4,
        tail="flow",
        wrap="translate(2 -18)",
        shadow=False,
    ),
    "jump-land": low_fence(6)
    + horse(
        legs=(-14, 36, 20, 46),
        neck=6,
        tail="up",
        wrap="translate(14 0) rotate(8 84 94)",
    )
    + puff(102, 93, 0.8),
    # fail: refusing the jump, front legs braced, head up, dust; `-plant` is the static frame.
    "refuse-plant": low_fence(98)
    + horse(
        legs=(-28, -18, -34, -14),
        neck=-26,
        tail="up",
        wrap="rotate(7 47 94) translate(-6 0)",
    )
    + puff(96, 93),
    "refuse-turn": low_fence(98)
    + '<g transform="translate(132 0) scale(-1 1)">'
    + horse(
        legs=(-22, 20, 24, -22),
        neck=-4,
        tail="up",
        wrap="translate(14 -1.5) rotate(-4 66 52)",
    )
    + "</g>",
}

# --- the rail: one bale per queued issue (drawn in a 40 x 28 box) ---
RAIL = {
    "bale": '<ellipse cx="20" cy="26.5" rx="17" ry="1.8" fill="#2f2a26" opacity="0.18"/>'
    + bale_shape(3, 5, 34, 21),
    "bale-red": '<ellipse cx="20" cy="26.5" rx="17" ry="1.8" fill="#2f2a26" opacity="0.18"/>'
    + bale_shape(3, 5, 34, 21, fill=RED, shade=RED_D, twine="#f4ecd8")
    + '<circle cx="33" cy="6" r="5.4" fill="#f4ecd8"/><path d="M 33 3 L 33 6.8" stroke="#b3261e" stroke-width="2" stroke-linecap="round"/><circle cx="33" cy="9" r="1.1" fill="#b3261e"/>',
    # The paddock: one fence bay (a post and two rails, the top one in the project colour), tiled along the pasture top.
    "paddock-bay": f'<rect x="0" y="0" width="6" height="30" fill="{WOOD}"/><rect x="1" y="0" width="1.4" height="30" fill="{WOOD_L}" opacity="0.5"/>'
    + f'<rect x="0" y="5" width="52" height="4" fill="{FK}"/><rect x="0" y="5" width="52" height="1.1" fill="#fff" opacity="0.35"/>'
    + f'<rect x="0" y="18" width="52" height="4" fill="{WOOD_D}"/><rect x="0" y="18" width="52" height="1.1" fill="{WOOD_L}" opacity="0.6"/>',
}


# --- the load scene in miniature, for a Host card header (40 x 16): today's weather words, drawn small ---
def mini_cloud(x, y, fill="#e3f1e9", k=1.0):
    return "".join(
        f'<ellipse cx="{f(x + dx * k)}" cy="{f(y + dy * k)}" rx="{f(rx * k)}" ry="{f(ry * k)}" fill="{fill}"/>'
        for dx, dy, rx, ry in ((0, 0, 5, 3), (5, -1.6, 4.4, 3.4), (10, 0, 4.4, 2.8))
    )


MINI_GROUND = '<path d="M 0 13 Q 20 11 40 13 L 40 16 L 0 16 Z" fill="#7f8f50"/>'
MINI = {
    "mini-calm": '<rect width="40" height="16" rx="3" fill="#8ebbc6"/>'
    + '<circle cx="33" cy="4.6" r="2.8" fill="#ffe7a0"/>'
    + mini_cloud(8, 6)
    + MINI_GROUND
    + '<path d="M 24 13 l 0 -3.6" stroke="#5f7d36" stroke-width="1.2"/>',
    "mini-windy": '<rect width="40" height="16" rx="3" fill="#8ebbc6"/>'
    + mini_cloud(4, 5)
    + mini_cloud(24, 7, k=0.8)
    + '<path d="M 3 10 h 12 M 9 12 h 14 M 20 9.6 h 9" stroke="#f4ecd8" stroke-width="0.9" stroke-linecap="round"/>'
    + MINI_GROUND
    + '<path d="M 26 13 q 2 -2 4 -3.4 M 30 13 q 2 -2 4 -3.4" stroke="#5f7d36" stroke-width="1.2" fill="none"/>',
    "mini-storm": '<rect width="40" height="16" rx="3" fill="#4f5a63"/>'
    + mini_cloud(6, 5, "#2a323c")
    + mini_cloud(22, 4.4, "#2a323c")
    + '<path d="M 7 9 l -1.4 3.6 M 13 9 l -1.4 3.6 M 27 9 l -1.4 3.6 M 33 9 l -1.4 3.6" stroke="#d8e6f5" stroke-width="0.8"/>'
    + '<path d="M 19 7 l -2.4 4 l 2 0 l -1.6 3.6 l 4 -5 l -2 0 l 1.4 -2.6 Z" fill="#ffe7a0"/>'
    + '<path d="M 0 13 Q 20 11 40 13 L 40 16 L 0 16 Z" fill="#4c6340"/>',
}

# --- 16 px row sprites: pixel art, one character per pixel ---
# Row sprites take the coat as --rc/--rd/--rm (set inline when the row has a lane), palomino otherwise: the board
# defines --c (the cyan token), so an inherited --c would paint every row horse cyan.
PAL = {
    "c": "var(--rc,#c89a56)",
    "d": "var(--rd,#a37a3c)",
    "m": "var(--rm,#e8dcc0)",
    "k": FK,
    "h": HOOF,
    "e": "#1d1a17",
    "y": HAY,
    "Y": HAY_D,
    "t": TWINE,
    "w": "#8b6b4a",
    "s": "#8f949b",
    "o": "#ff9a3c",
    "i": "#4a4f57",
    "v": "#eef1f4",
    "f": "#d9a77f",
    "b": "#a86e52",
    "r": "#536456",
    "D": "#344635",
    "R": RED,
    "!": "#f4ecd8",
    "g": "#5f7d36",
    "p": "#9c916a",
    "n": "#6b4a2f",
}
ROWS = {
    "row-plough": [  # impl
        "................",
        "................",
        "...........mcc..",
        "..........mccce.",
        "..........mcc.cc",
        "w.....cccmcc....",
        ".w...mckkccc....",
        "..w..ccccccc....",
        "..ww.ccccccc....",
        "..sw..c.c..c.c..",
        "..s..c...c..c..c",
        "....h....h.h...h",
        "nnnnn...........",
        "................",
        "................",
        "................",
    ],
    "row-anvil": [  # fix
        "................",
        "................",
        "...........mcc..",
        "..........mccce.",
        "..........mcc.cc",
        "......cccmcc....",
        ".o...mckkccc....",
        "o.o..ccccccc....",
        ".....ccccccc....",
        "....cc...c.c....",
        "iiii.....c.c....",
        ".ii......h.h....",
        "wwww............",
        "................",
        "................",
        "................",
    ],
    "row-vet": [  # rev
        "................",
        "................",
        "........mcc.....",
        ".......mccce....",
        ".......mcc.cc...",
        "p..cccmcc.....p.",
        "pppmckkccc.pppp.",
        "p..ccccccc...fp.",
        "p..ccccccc..vvp.",
        "pppc.c..ccvvvpp.",
        "p..c.c....cvv.p.",
        "p..h.h......v.p.",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-hay": [  # update / catch-up
        "................",
        ".....yyyy.......",
        "....yytyyy..mcc.",
        "...yyyyyytymccce",
        "...YYYYYYYmcc.cc",
        "...kcccccmcc....",
        "..mckkkkccc.....",
        "..cccccccccc....",
        "...c.c...c.c....",
        "..c...c.c...c...",
        "..h...h.h...h...",
        "................",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-graze": [  # idle
        "................",
        "................",
        "................",
        "................",
        "................",
        "...cccccccm.....",
        "..mckkcccccm....",
        "..cccccccccmc...",
        "..ccccccccc.cc..",
        "...c.c...c.c.ce.",
        "...c.c...c.c..cc",
        "...h.h...h.h.gcg",
        "............g.g.",
        "................",
        "................",
        "................",
    ],
    "row-stable": [  # done
        "...........r....",
        "..........rbr...",
        ".........rbbbr..",
        "........rbbbbbr.",
        ".......rbbDDDbbr",
        "........bbDDDbb.",
        "..ccccccDDDDDbb.",
        ".mckkcccDDDDDbb.",
        ".ccccccDDDDDDbb.",
        ".c.c...cDDDDDbb.",
        "c...c.c.DDDDDbb.",
        "h...h.h.DDDDDbb.",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-rear": [  # blocked / needs you
        "..........R.....",
        "........mcc.R...",
        ".......mccce....",
        "......mccc.cc...",
        ".....mccc.....pp",
        "....ckkc.c....p.",
        "...ccccc..c...pp",
        "..cccccc......p.",
        "..ccccc.......pp",
        "..c.c.........p.",
        "..c..c........pp",
        "..h..h........p.",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-bale": [  # queued
        "................",
        "................",
        "................",
        "................",
        "..yyyyyyyyyyyy..",
        ".yyyytyyyyytyyy.",
        ".yyyytyyyyytyyy.",
        ".yyyytyyyyytyyy.",
        ".YYYYtYYYYYtYYY.",
        ".YYYYtYYYYYtYYY.",
        "..YYYtYYYYYtYY..",
        "................",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-bale-red": [  # queued and needs you
        "................",
        "................",
        "................",
        "................",
        "..RRRRRRRRRRRR..",
        ".RRRR!RRRRR!RRR.",
        ".RRRR!RRRRR!RRR.",
        ".RRRR!RRRRR!RRR.",
        ".RRRR!RRRRR!RRR.",
        ".RRRR!RRRRR!RRR.",
        "..RRR!RRRRR!RR..",
        "................",
        "................",
        "................",
        "................",
        "................",
    ],
    "row-jump": [  # merged (Events)
        "................",
        "......ccccmcc...",
        "....mcckkcmccce.",
        "...m.cccccc..cc.",
        "..m.c.cc.cc.....",
        "...c...cc..c....",
        "................",
        "................",
        "................",
        "......kkkkkkk...",
        "......p.....p...",
        "......ppppppp...",
        "......p.....p...",
        "......p.....p...",
        "................",
        "................",
    ],
    "row-refuse": [  # FAIL (Events)
        "................",
        "......mcc.......",
        ".....mccce......",
        "....mccc.cc.....",
        "...ckkc.........",
        "..cccccc........",
        ".mccccc.c.......",
        ".m.c...c.c......",
        "...c....c..kkkkk",
        "...c.....c.p...p",
        "...h......hppppp",
        "...........p...p",
        "...........p...p",
        "................",
        "................",
        "................",
    ],
}


def pixels(rows):
    s = ""
    for y, row in enumerate(rows):
        assert len(row) == 16, (row, len(row))
        x = 0
        while x < 16:
            ch = row[x]
            if ch == ".":
                x += 1
                continue
            n = 1
            while x + n < 16 and row[x + n] == ch:
                n += 1
            s += f'<rect x="{x}" y="{y}" width="{n}" height="1" fill="{PAL[ch]}"/>'
            x += n
    return f'<g shape-rendering="crispEdges">{s}</g>'


def main():
    out = [
        (
            "<!-- Farm v2 (#242) sprite sheet: generated by make_sprites.py. Horses: Corral ios/tools/herd-art/horsesvg.py +"
            " HerdArt.swift (c) 2026 jirathip-dev, MIT, as assets/themes/farm/sprites.svg. Coats: --c/--d/--m/--b;"
            " project colour: --fk. Actions and moments: 132 x 100; rail: 40 x 28 (bay 52 x 30); row sprites: 16 x 16; Host minis: 40 x 16. -->"
        )
    ]
    out += [g(k, v) for k, v in ACTIONS.items()]
    out += [g(k, v, "moment") for k, v in MOMENTS.items()]
    out += [g(k, v, "rail") for k, v in RAIL.items()]
    out += [g(k, pixels(v), "row") for k, v in ROWS.items()]
    out += [g(k, v, "mini") for k, v in MINI.items()]
    Path(__file__).with_name("sprites.svg").write_text("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
