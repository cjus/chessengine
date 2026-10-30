"""Write the print edition's SVG figures into book/print/diagrams.

    python3 book/print/figures.py book/print/diagrams

`scripts/build-print-book.sh` runs this before staging. The figures are hand-drawn SVG in
bookcraft's diagram style: a 640-wide viewBox so no label prints under 8pt, and a label or dash
pattern on every mark that differs from another, so they still read in grey on an e-reader.

Every fact drawn here comes from the chapter it illustrates (diagrams/README.md says where).
When a chapter's numbers change, change them here too: the positions in figure 2.1 were checked
with cb2026's `legal` command, figure 6.1 draws chapter 6's UCI output, and figure 14.1.1 draws
the singular extension code in `crates/engine/src/search.rs`.
"""
import sys
from pathlib import Path

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)

INK, MID, MUTED, HAIR = "#1F2937", "#4B5563", "#6B7280", "#D1D5DB"
BLUE, BLUEF, AMBER, AMBERF, RED = "#2F5FA0", "#EBF0F6", "#C08A1E", "#F7D9A5", "#B4261E"
FONT = '"Helvetica Neue", Helvetica, Arial, sans-serif'
MONO = '"SF Mono", Menlo, Consolas, monospace'

STYLE = f"""<style>
  .hd {{ font: 700 13px {FONT}; letter-spacing: .08em; fill: {INK}; }}
  .lbl {{ font: 400 14px {FONT}; fill: {INK}; }}
  .sm {{ font: 400 13px {FONT}; fill: {MID}; }}
  .cd {{ font: 400 14px {MONO}; fill: {INK}; }}
  .cds {{ font: 400 13px {MONO}; fill: {INK}; }}
  .note {{ font: italic 400 13px {FONT}; fill: {MID}; }}
  .bad {{ font: italic 400 13px {FONT}; fill: {RED}; }}
  .pc {{ font: 700 14px {FONT}; }}
  .coord {{ font: 400 13px {FONT}; fill: {MUTED}; }}
</style>"""


def arrow_defs(p):
    return (f'<defs><marker id="{p}-ar" markerWidth="10" markerHeight="8" refX="9" refY="4" '
            f'orient="auto"><path d="M0,0 L10,4 L0,8 z" fill="{MUTED}"/></marker>'
            f'<marker id="{p}-ari" markerWidth="10" markerHeight="8" refX="9" refY="4" '
            f'orient="auto"><path d="M0,0 L10,4 L0,8 z" fill="{INK}"/></marker></defs>')


def svg(p, w, h, title, desc, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" '
            f'height="{h}" role="img" aria-labelledby="{p}-t {p}-d">\n'
            f'<title id="{p}-t">{title}</title>\n<desc id="{p}-d">{desc}</desc>\n'
            f'{STYLE}\n{arrow_defs(p)}\n<rect width="{w}" height="{h}" fill="#FFFFFF"/>\n'
            f'{body}\n</svg>\n')


FILES = "abcdefgh"


def sq_xy(sq, x0, y0, s):
    f, r = FILES.index(sq[0]), int(sq[1]) - 1
    return x0 + f * s, y0 + (7 - r) * s


def board(x0, y0, s, pieces, marked=(), dots=(), crossed=(), dashed=()):
    """pieces: {sq: (letter, white?)}; marked: amber squares; dots: target dots."""
    o = []
    for r in range(8):
        for f in range(8):
            sq = FILES[f] + str(r + 1)
            x, y = sq_xy(sq, x0, y0, s)
            fill = "#FFFFFF" if (f + r) % 2 else "#E9EBEE"
            o.append(f'<rect x="{x}" y="{y}" width="{s}" height="{s}" fill="{fill}"/>')
    for sq in marked:
        x, y = sq_xy(sq, x0, y0, s)
        o.append(f'<rect x="{x + 1.5}" y="{y + 1.5}" width="{s - 3}" height="{s - 3}" '
                 f'fill="{AMBERF}" stroke="{AMBER}" stroke-width="2"/>')
    for sq in dashed:
        x, y = sq_xy(sq, x0, y0, s)
        o.append(f'<rect x="{x + 2.5}" y="{y + 2.5}" width="{s - 5}" height="{s - 5}" '
                 f'fill="none" stroke="{MUTED}" stroke-width="1.5" stroke-dasharray="4 3"/>')
    for sq in crossed:
        x, y = sq_xy(sq, x0, y0, s)
        m = s * 0.3
        o.append(f'<path d="M{x + m},{y + m} L{x + s - m},{y + s - m} M{x + s - m},{y + m} '
                 f'L{x + m},{y + s - m}" stroke="{MUTED}" stroke-width="2"/>')
    o.append(f'<rect x="{x0}" y="{y0}" width="{8 * s}" height="{8 * s}" fill="none" '
             f'stroke="{MUTED}" stroke-width="1.5"/>')
    for i in range(8):
        o.append(f'<text class="coord" x="{x0 + i * s + s / 2}" y="{y0 + 8 * s + 16}" '
                 f'text-anchor="middle">{FILES[i]}</text>')
        o.append(f'<text class="coord" x="{x0 - 8}" y="{y0 + i * s + s / 2 + 5}" '
                 f'text-anchor="end">{8 - i}</text>')
    for sq in dots:
        x, y = sq_xy(sq, x0, y0, s)
        if sq in pieces:
            continue
        o.append(f'<circle cx="{x + s / 2}" cy="{y + s / 2}" r="{s * 0.16}" fill="{INK}"/>')
    for sq, (letter, white) in pieces.items():
        x, y = sq_xy(sq, x0, y0, s)
        cx, cy, r = x + s / 2, y + s / 2, s * 0.38
        fill, stroke, tc = ("#FFFFFF", INK, INK) if white else (INK, INK, "#FFFFFF")
        ring = (f'<circle cx="{cx}" cy="{cy}" r="{r + 3.5}" fill="none" stroke="{INK}" '
                f'stroke-width="1.5" stroke-dasharray="3 2"/>' if sq in dots else "")
        o.append(f'{ring}<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}" stroke="{stroke}" '
                 f'stroke-width="1.8"/><text class="pc" x="{cx}" y="{cy + 5}" '
                 f'text-anchor="middle" fill="{tc}">{letter}</text>')
    return "\n".join(o)


def write(name, text):
    (out / name).write_text(text, encoding="utf-8")


# ---- 1. Chapter 2: check mask and pin ------------------------------------------------
s = 28
L = board(46, 64, s, {"e8": ("K", False), "e7": ("R", False), "c3": ("N", True),
                      "e1": ("K", True)},
          marked=["e2", "e3", "e4", "e5", "e6", "e7"], dots=["e2", "e4"])
R = board(370, 64, s, {"e8": ("K", False), "a5": ("B", False), "d2": ("B", True),
                       "e1": ("K", True)},
          marked=["b4", "c3", "a5"], dots=["b4", "c3", "a5"])
body = f"""
<text class="hd" x="46" y="34">SINGLE CHECK: THE CHECK MASK</text>
<text class="hd" x="370" y="34">A PIN: ONLY ALONG THE LINE</text>
{L}
{R}
<text class="lbl" x="46" y="322">Rook e7 checks. Shaded: the squares between</text>
<text class="lbl" x="46" y="341">the checker and the king, and the checker.</text>
<text class="lbl" x="46" y="366">Knight c3 may only land there: Ne2, Ne4.</text>
<text class="lbl" x="370" y="322">Bishop d2 stands alone between its king</text>
<text class="lbl" x="370" y="341">and bishop a5. Shaded: the pin line.</text>
<text class="lbl" x="370" y="366">It may only move along it: Bc3, Bb4, Bxa5.</text>
<text class="note" x="320" y="398" text-anchor="middle">Dots mark every legal move of the piece in question, as cb2026's legal command lists them.</text>
"""
write("check-mask-and-pin.svg", svg(
    "cm", 640, 412,
    "In check, a piece may only land on the check mask; a pinned piece may only move along its pin",
    "Two chess boards. Left: a black rook on e7 checks the white king on e1; squares e2 to e7 "
    "are shaded as the check mask, and the white knight on c3 has only two legal moves, e2 and "
    "e4, both inside it. Right: a white bishop on d2 is pinned to its king on e1 by a black "
    "bishop on a5; the squares c3, b4 and a5 are shaded, and they are the bishop's only legal "
    "moves.", body))

# ---- 2. Chapter 3: magic index for a rook on d4 --------------------------------------
s = 26
rel = ["d2", "d3", "d5", "d6", "d7", "b4", "c4", "e4", "f4", "g4"]
B = board(40, 60, s, {"d4": ("R", True)}, marked=rel, dashed=["d1", "d8", "a4", "h4"])
bx, bw = 300, 310


def step(y, h, head, code, note):
    return (f'<rect x="{bx}" y="{y}" width="{bw}" height="{h}" rx="5" fill="{BLUEF}" '
            f'stroke="{BLUE}" stroke-width="1.5"/>'
            f'<text class="hd" x="{bx + 14}" y="{y + 21}">{head}</text>'
            f'<text class="cd" x="{bx + 14}" y="{y + 43}">{code}</text>'
            f'<text class="sm" x="{bx + 14}" y="{y + 62}">{note}</text>')


steps = [
    (40, "1  MASK", "occupied &amp; mask", "keeps the 10 shaded squares"),
    (128, "2  MULTIPLY", "× magic", "gathers the scattered bits at the top"),
    (216, "3  SHIFT", "&gt;&gt; (64 − 10)", "the top 10 bits: an index 0 to 1023"),
    (304, "4  LOOK UP", "table[offset + index]", "the rook's attacks, precomputed"),
]
flow = "".join(step(y, 74, h, c, n) for y, h, c, n in steps)
flow += "".join(f'<path d="M{bx + bw / 2},{y + 74} V{y + 86}" stroke="{MUTED}" stroke-width="2" '
                f'marker-end="url(#mg-ar)"/>' for y, *_ in steps[:-1])
body = f"""
<text class="hd" x="40" y="36">ROOK ON d4</text>
{B}
<rect x="40" y="306" width="18" height="18" fill="{AMBERF}" stroke="{AMBER}" stroke-width="2"/>
<text class="lbl" x="66" y="320">relevant: 10 squares, 1,024 patterns</text>
<rect x="40" y="336" width="18" height="18" fill="none" stroke="{MUTED}" stroke-width="1.5" stroke-dasharray="4 3"/>
<text class="lbl" x="66" y="350">ray ends: attacked either way</text>
{flow}
"""
write("magic-index.svg", svg(
    "mg", 640, 392,
    "A rook on d4 depends on only 10 squares, so its attacks are one of 1,024 table entries",
    "Left: a board with a rook on d4. Ten squares on its rank and file are shaded as relevant "
    "(d2, d3, d5, d6, d7, b4, c4, e4, f4, g4); the four ray ends d1, d8, a4 and h4 are outlined "
    "with dashes because the rook attacks them whether or not they are occupied. Right: four "
    "steps from top to bottom: mask the occupancy, multiply by the magic number, shift right by "
    "64 minus 10 to get an index from 0 to 1023, and look the attacks up in the table.", body))

# ---- 3. Chapter 5: alpha-beta cutoff -------------------------------------------------


def node(x, y, label, shape="circle", dashed=False, fill="#FFFFFF"):
    da = ' stroke-dasharray="5 4"' if dashed else ""
    col = MUTED if dashed else INK
    if shape == "circle":
        g = f'<circle cx="{x}" cy="{y}" r="22" fill="{fill}" stroke="{col}" stroke-width="2"{da}/>'
    else:
        g = (f'<rect x="{x - 22}" y="{y - 22}" width="44" height="44" rx="4" fill="{fill}" '
             f'stroke="{col}" stroke-width="2"{da}/>')
    cls = "sm" if dashed else "lbl"
    return g + f'<text class="{cls}" x="{x}" y="{y + 5}" text-anchor="middle">{label}</text>'


def edge(x1, y1, x2, y2, label, dashed=False, lx=0, ly=0):
    da = ' stroke-dasharray="5 4"' if dashed else ""
    col = MUTED if dashed else INK
    mx, my = (x1 + x2) / 2 + lx, (y1 + y2) / 2 + ly
    return (f'<path d="M{x1},{y1} L{x2},{y2}" stroke="{col}" stroke-width="1.8"{da}/>'
            f'<text class="cds" x="{mx}" y="{my}" text-anchor="middle">{label}</text>')


body = (
    '<text class="hd" x="24" y="76">WHITE</text><text class="sm" x="24" y="94">picks the max</text>'
    '<text class="hd" x="24" y="186">BLACK</text><text class="sm" x="24" y="204">picks the min</text>'
    '<text class="hd" x="24" y="296">LEAVES</text><text class="sm" x="24" y="314">evaluated</text>'
    + edge(330, 106, 210, 158, "a", lx=-14, ly=-4)
    + edge(330, 106, 450, 158, "b", lx=14, ly=-4)
    + edge(195, 200, 145, 268, "a1", lx=-16)
    + edge(225, 200, 275, 268, "a2", lx=16)
    + edge(450, 202, 380, 268, "b1", lx=-16)
    + edge(450, 202, 470, 268, "b2", dashed=True, lx=14)
    + edge(450, 202, 560, 268, "b3", dashed=True, lx=18)
    + node(330, 84, "+1")
    + node(210, 180, "+1", "rect")
    + f'<rect x="428" y="158" width="44" height="44" rx="4" fill="{AMBERF}" stroke="{AMBER}" stroke-width="2"/>'
    + '<text class="lbl" x="450" y="185" text-anchor="middle">≤ −2</text>'
    + node(145, 290, "+1") + node(275, 290, "+3") + node(380, 290, "−2")
    + node(470, 290, "?", dashed=True) + node(560, 290, "?", dashed=True)
    + '<text class="sm" x="210" y="230" text-anchor="middle"></text>'
    + f'<text class="lbl" x="496" y="176">cutoff</text>'
    + '<text class="sm" x="496" y="194">after one reply</text>'
    + '<text class="note" x="515" y="336" text-anchor="middle">never searched</text>'
    + '<text class="lbl" x="360" y="34" text-anchor="middle">After move a, White can already guarantee +1 (alpha = +1).</text>'
    + '<text class="lbl" x="320" y="372" text-anchor="middle">Black\'s first reply to b scores −2, worse for White than a\'s +1,</text>'
    + '<text class="lbl" x="320" y="392" text-anchor="middle">so White will never play b, and its other replies can\'t change that.</text>'
    + '<text class="note" x="320" y="418" text-anchor="middle">At Black\'s node that is a beta cutoff: in negamax, Black\'s +2 is at least its beta of −1.</text>'
)
write("alpha-beta-cutoff.svg", svg(
    "ab", 640, 432,
    "Once one reply refutes a move, alpha-beta skips that move's other replies",
    "A two-ply game tree. White, at the root, has moves a and b. Black's replies to a lead to "
    "leaves scored +1 and +3, so a is worth +1 to White. Black's first reply to b, b1, scores "
    "-2, which is already worse for White than +1, so b is worth at most -2 and its other "
    "replies, b2 and b3, drawn dashed, are never searched. The root is worth +1.", body))

# ---- 4. Chapter 6: iterative deepening against the soft and hard limits ---------------
X0, W, TMAX = 70, 540, 9.0


def tx(t):
    return X0 + W * t / TMAX


rows = [(5, 0.000, 0.005), (6, 0.005, 0.040), (7, 0.040, 0.194), (8, 0.194, 3.096)]
bars = ""
y = 70
for d, t0, t1 in rows:
    x0, x1 = tx(t0), max(tx(t1), tx(t0) + 2)
    bars += (f'<text class="lbl" x="{X0 - 10}" y="{y + 15}" text-anchor="end">depth {d}</text>'
             f'<rect x="{x0}" y="{y}" width="{x1 - x0}" height="20" fill="{BLUEF}" '
             f'stroke="{BLUE}" stroke-width="1.5"/>')
    if d < 8:
        bars += f'<text class="sm" x="{x1 + 8}" y="{y + 15}">done at {t1 * 1000:g} ms</text>'
    else:
        bars += f'<text class="sm" x="{x1 + 8}" y="{y + 15}">done at 3.1 s</text>'
    y += 32
bars += (f'<text class="lbl" x="{X0 - 10}" y="{y + 15}" text-anchor="end">depth 9</text>'
         f'<rect x="{tx(3.096)}" y="{y}" width="{tx(9.0) - tx(3.096)}" height="20" fill="none" '
         f'stroke="{MUTED}" stroke-width="1.5" stroke-dasharray="5 4"/>'
         f'<text class="note" x="{tx(3.3)}" y="{y + 15}">not started: it would take several times longer</text>')
axis_y = y + 44
ticks = "".join(
    f'<path d="M{tx(t)},{axis_y} v6" stroke="{MUTED}"/>'
    f'<text class="coord" x="{tx(t)}" y="{axis_y + 22}" text-anchor="middle">{t} s</text>'
    for t in range(0, 10))
soft, hard = tx(2.75), tx(8.25)
body = f"""
<path d="M{soft},48 V{axis_y}" stroke="{AMBER}" stroke-width="2.5" stroke-dasharray="7 4"/>
<text class="hd" x="{soft - 6}" y="40" text-anchor="end">SOFT LIMIT 2.75 s</text>
<path d="M{hard},48 V{axis_y}" stroke="{INK}" stroke-width="2.5"/>
<text class="hd" x="{hard + 6}" y="40" text-anchor="end">HARD 8.25 s</text>
{bars}
<path d="M{X0},{axis_y} H{X0 + W}" stroke="{MUTED}" stroke-width="1.5"/>
{ticks}
<text class="lbl" x="320" y="{axis_y + 56}" text-anchor="middle">After each depth: past the soft limit, don't start another. The hard limit</text>
<text class="lbl" x="320" y="{axis_y + 76}" text-anchor="middle">stops a depth midway, and here was never needed. 60 s left, 1 s increment.</text>
"""
write("time-limits.svg", svg(
    "tm", 640, axis_y + 92,
    "Depth 8 finished past the soft limit, so depth 9 was never started",
    "A timeline from 0 to 9 seconds for a search from the starting position with 60 seconds "
    "left and a 1-second increment. Depths 5, 6 and 7 finish at 5, 40 and 194 milliseconds, "
    "all near the start. Depth 8 runs from 0.19 to 3.1 seconds, crossing the soft limit at 2.75 "
    "seconds, so depth 9, drawn dashed, is never started. The hard limit at 8.25 seconds is "
    "never reached.", body))

# ---- 5. Chapter 8.1: one slot or a bucket of eight in a cache line -------------------
LX, SW = 40, 70


def line(y, picked, all_looked):
    o = [f'<rect x="{LX - 6}" y="{y - 6}" width="{8 * SW + 12}" height="58" rx="6" '
         f'fill="#F9FAFB" stroke="{HAIR}" stroke-width="1.5"/>']
    for i in range(8):
        x = LX + i * SW
        if all_looked or i == picked:
            o.append(f'<rect x="{x + 3}" y="{y}" width="{SW - 6}" height="46" rx="3" '
                     f'fill="{AMBERF}" stroke="{AMBER}" stroke-width="2"/>')
            o.append(f'<text class="sm" x="{x + SW / 2}" y="{y + 20}" text-anchor="middle" '
                     f'style="fill:{INK}">slot {i}</text>'
                     f'<text class="sm" x="{x + SW / 2}" y="{y + 37}" text-anchor="middle" '
                     f'style="fill:{INK}">read</text>')
        else:
            o.append(f'<rect x="{x + 3}" y="{y}" width="{SW - 6}" height="46" rx="3" '
                     f'fill="#FFFFFF" stroke="{MUTED}" stroke-width="1.5" stroke-dasharray="4 3"/>')
            o.append(f'<text class="sm" x="{x + SW / 2}" y="{y + 20}" text-anchor="middle">slot {i}</text>'
                     f'<text class="sm" x="{x + SW / 2}" y="{y + 37}" text-anchor="middle">unused</text>')
    return "\n".join(o)


body = f"""
<text class="lbl" x="320" y="30" text-anchor="middle">One 128-byte cache line, as Apple Silicon fetches it: eight 16-byte slots.</text>
<text class="hd" x="34" y="72">BUCKETS OFF (CHAPTER 8): THE KEY PICKS ONE SLOT</text>
{line(86, 5, False)}
<text class="note" x="34" y="166">A miss waits for memory, which delivers all eight slots; the probe reads one.</text>
<text class="hd" x="34" y="214">BUCKETS ON: THE KEY PICKS THE LINE</text>
{line(228, None, True)}
<text class="note" x="34" y="308">The other seven reads cost a few instructions each: the line is already in cache.</text>
<text class="lbl" x="34" y="342">A store takes the slot holding this position, else an empty one, else the</text>
<text class="lbl" x="34" y="362">one worth least: depth − 8 × searches since it was stored.</text>
"""
write("tt-bucket.svg", svg(
    "tb", 640, 380,
    "A probe that misses the cache fetches a whole line of eight slots; buckets use all of them",
    "Two rows, each showing a 128-byte cache line divided into eight 16-byte slots. Top row, "
    "buckets off: the key picks slot 5, which is read; the other seven are fetched but unused. "
    "Bottom row, buckets on: the key picks the whole line and all eight slots are read, at "
    "little extra cost since the line is already in the cache. A store goes to the slot with "
    "the same position, an empty slot, or the slot worth least, its depth less 8 for each "
    "search since it was stored.", body))

# ---- 6. Chapter 14.1: outcomes of the singular search --------------------------------
AX0, AX1, AY = 40, 600, 150
xa, xb = 230, 400   # singular_beta − 50, singular_beta
body = f"""
<text class="lbl" x="320" y="30" text-anchor="middle">The node is searched again at half depth without the hash move, with a null</text>
<text class="lbl" x="320" y="50" text-anchor="middle">window at singular_beta = hash score − 3 × depth. Where its score S lands:</text>
<rect x="{AX0}" y="{AY - 62}" width="{xa - AX0 - 4}" height="50" rx="5" fill="{AMBERF}" stroke="{AMBER}" stroke-width="2"/>
<text class="hd" x="{(AX0 + xa) / 2}" y="{AY - 40}" text-anchor="middle">+2 PLIES</text>
<text class="sm" x="{(AX0 + xa) / 2}" y="{AY - 22}" text-anchor="middle" style="fill:{INK}">double extension</text>
<rect x="{xa + 4}" y="{AY - 62}" width="{xb - xa - 8}" height="50" rx="5" fill="{BLUEF}" stroke="{BLUE}" stroke-width="2"/>
<text class="hd" x="{(xa + xb) / 2}" y="{AY - 40}" text-anchor="middle">+1 PLY</text>
<text class="sm" x="{(xa + xb) / 2}" y="{AY - 22}" text-anchor="middle" style="fill:{INK}">singular extension</text>
<rect x="{xb + 4}" y="{AY - 62}" width="{AX1 - xb - 4}" height="50" rx="5" fill="#FFFFFF" stroke="{MUTED}" stroke-width="1.5" stroke-dasharray="5 4"/>
<text class="hd" x="{(xb + AX1) / 2}" y="{AY - 40}" text-anchor="middle">NOT SINGULAR</text>
<text class="sm" x="{(xb + AX1) / 2}" y="{AY - 22}" text-anchor="middle">another move comes close</text>
<path d="M{AX0},{AY} H{AX1}" stroke="{INK}" stroke-width="2" marker-end="url(#sg-ari)"/>
<path d="M{xa},{AY - 8} V{AY + 8}" stroke="{INK}" stroke-width="2"/>
<path d="M{xb},{AY - 8} V{AY + 8}" stroke="{INK}" stroke-width="2"/>
<text class="cds" x="{xa}" y="{AY + 28}" text-anchor="middle">singular_beta − 50</text>
<text class="cds" x="{xb}" y="{AY + 28}" text-anchor="middle">singular_beta</text>
<text class="sm" x="{AX0}" y="{AY + 28}">lower S</text>
<text class="sm" x="{AX1}" y="{AY + 28}" text-anchor="end">higher S</text>
<text class="note" x="{(AX0 + xa) / 2}" y="{AY + 52}" text-anchor="middle">off the PV, at most 6 a line</text>
<path d="M{(xb + AX1) / 2},{AY + 40} V{AY + 64}" stroke="{MUTED}" stroke-width="2" marker-end="url(#sg-ar)"/>
<rect x="{xb - 60}" y="{AY + 70}" width="{AX1 - xb + 60}" height="132" rx="5" fill="#F9FAFB" stroke="{HAIR}" stroke-width="1.5"/>
<text class="lbl" x="{xb - 46}" y="{AY + 94}">singular_beta ≥ beta?</text>
<text class="sm" x="{xb - 30}" y="{AY + 114}" style="fill:{INK}">yes: multi-cut, return singular_beta</text>
<text class="lbl" x="{xb - 46}" y="{AY + 142}">else hash score ≥ beta?</text>
<text class="sm" x="{xb - 30}" y="{AY + 162}" style="fill:{INK}">yes: hash move a ply shallower</text>
<text class="sm" x="{xb - 30}" y="{AY + 182}">(negative extension); no: as usual</text>
<text class="lbl" x="{AX0}" y="{AY + 96}">One search, four outcomes.</text>
<text class="lbl" x="{AX0}" y="{AY + 118}">singular_margin (3) moves</text>
<text class="lbl" x="{AX0}" y="{AY + 140}">both ticks; the double</text>
<text class="lbl" x="{AX0}" y="{AY + 162}">extension margin (50) sets</text>
<text class="lbl" x="{AX0}" y="{AY + 184}">the gap between them.</text>
"""
write("singular-outcomes.svg", svg(
    "sg", 640, AY + 216,
    "Where the search without the hash move scores decides between two extensions, a cutoff and a reduction",
    "A number line of S, the score of the search without the hash move. Below singular_beta "
    "minus 50 the hash move is extended two plies (a double extension, off the principal "
    "variation and at most six per line). Between that and singular_beta it is extended one ply "
    "(a singular extension). At or above singular_beta it is not singular: if singular_beta is "
    "at least beta the node is cut off by multi-cut; otherwise, if the hash score is at least "
    "beta, the hash move is searched a ply shallower (a negative extension); otherwise nothing "
    "changes.", body))

# ---- 7. Chapter 21: updating the accumulator ------------------------------------------


def vec(x, y, n, label, fill, stroke, dashed=False):
    da = ' stroke-dasharray="4 3"' if dashed else ""
    w = 14
    cells = "".join(f'<rect x="{x + i * w}" y="{y}" width="{w}" height="22" fill="{fill}" '
                    f'stroke="{stroke}" stroke-width="1.2"{da}/>' for i in range(n))
    return cells + f'<text class="sm" x="{x + n * w / 2}" y="{y + 40}" text-anchor="middle">{label}</text>'


def view(y, head, before, minus, plus):
    n = 6
    return f"""
<text class="hd" x="30" y="{y}">{head}</text>
{vec(30, y + 14, n, before, BLUEF, BLUE)}
<text class="lbl" x="134" y="{y + 31}" text-anchor="middle" style="font-size:20px">−</text>
{vec(148, y + 14, n, minus, "#FFFFFF", MUTED, dashed=True)}
<text class="lbl" x="252" y="{y + 31}" text-anchor="middle" style="font-size:20px">+</text>
{vec(266, y + 14, n, plus, AMBERF, AMBER)}
<text class="lbl" x="370" y="{y + 31}" text-anchor="middle" style="font-size:20px">=</text>
{vec(384, y + 14, n, "accumulator after", BLUEF, BLUE)}
"""


body = f"""
<text class="lbl" x="320" y="28" text-anchor="middle">White plays Ng1–f3. Each view's 256 hidden values change by two rows of weights.</text>
{view(70, "WHITE'S VIEW", "accumulator before", "row: our N on g1", "row: our N on f3")}
{view(156, "BLACK'S VIEW (BOARD FLIPPED)", "accumulator before", "row: their N on g8", "row: their N on f6")}
<path d="M492,99 H520 V170 H492" stroke="{MUTED}" stroke-width="1.5" fill="none"/>
<path d="M520,134 H540" stroke="{MUTED}" stroke-width="2" marker-end="url(#nn-ar)"/>
<rect x="546" y="104" width="78" height="60" rx="5" fill="#F9FAFB" stroke="{HAIR}" stroke-width="1.5"/>
<text class="sm" x="585" y="126" text-anchor="middle" style="fill:{INK}">SCReLU,</text>
<text class="sm" x="585" y="143" text-anchor="middle" style="fill:{INK}">512 output</text>
<text class="sm" x="585" y="159" text-anchor="middle" style="fill:{INK}">weights</text>
<text class="note" x="30" y="252">Columns shown: 6 of the 256. Rows of the same 768 × 256 weights serve both views.</text>
<text class="lbl" x="30" y="282">Recomputing adds a row for every piece on the board, up to 32 per view.</text>
<text class="lbl" x="30" y="302">A quiet move takes one row off and puts one on; a capture takes two off; castling, two of each.</text>
"""
write("nnue-accumulator.svg", svg(
    "nn", 640, 318,
    "A move changes the accumulator by a few rows of weights instead of recomputing it",
    "For the move knight g1 to f3, two rows of vectors. In White's view, the accumulator before, "
    "minus the weight row for our knight on g1, plus the row for our knight on f3, equals the "
    "accumulator after. In Black's view, with the board flipped, the same move subtracts the row "
    "for their knight on g8 and adds the row for their knight on f6. Both accumulators feed the "
    "SCReLU activation and the 512 output weights. Recomputing would add a row for every piece, "
    "up to 32 per view.", body))

print("figures written:", ", ".join(sorted(p.name for p in out.glob("*.svg"))))
