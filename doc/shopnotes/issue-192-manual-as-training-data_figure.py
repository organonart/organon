"""Figure for the issue-192 shop note.

One chain, left to right, from the descriptions in the source to a model that
can drive the program. Solid boxes are things that exist in the repository
today; dashed boxes do not exist at all. The row underneath says which
artifacts get committed and which deliberately do not, because that split is
the part of the plan people are most likely to argue with.

Palette copied from doc/build_shopnote_pdf.py.
"""
from reportlab.lib import colors
from reportlab.graphics.shapes import Drawing, Line, Polygon, Rect, String

CHARCOAL = colors.HexColor("#1A1D21")
BRASS = colors.HexColor("#8A6A2E")
FAINT = colors.HexColor("#8A8F96")
RULE = colors.HexColor("#C9BFA8")
PLATE = colors.HexColor("#E7E0CF")
GLOW = colors.HexColor("#2E8B9E")
GLOW_LT = colors.HexColor("#CDECF2")

W = 468.0

CAPTION = (
    "The chain from a sentence in the source code to a model that can operate the program. "
    "The two shaded boxes are built and have tests: the program writes its own manual and a test "
    "fails if the manual and the code disagree, and the reader that measures what a training run "
    "moved is finished, with 40 tests, along with 30 more for the connection to the training "
    "application. Everything drawn with a broken outline is not built, which is five of the "
    "six remaining boxes and all five of the steps in the issue. The line underneath is the "
    "storage decision: the corpus, the recipe and the measurement are small, readable and "
    "committed, while the trained adjustment is about 40 megabytes and the base model belongs "
    "to somebody else, so neither of those is kept here."
)


def _box(d, x, y, w, h, title, sub, built):
    if built:
        d.add(Rect(x, y, w, h, fillColor=GLOW_LT, strokeColor=GLOW, strokeWidth=1.1))
        tc, sc = CHARCOAL, CHARCOAL
    else:
        d.add(Rect(x, y, w, h, fillColor=None, strokeColor=FAINT, strokeWidth=0.9,
                   strokeDashArray=[3, 2.6]))
        tc, sc = FAINT, FAINT
    lines = title.split("|")
    top = y + h - 12 if len(lines) == 1 else y + h - 11
    for i, ln in enumerate(lines):
        d.add(String(x + w / 2, top - i * 9.4, ln, fontName="Times-Bold", fontSize=7.4,
                     fillColor=tc, textAnchor="middle"))
    if sub:
        for j, ln in enumerate(sub.split("|")):
            d.add(String(x + w / 2, y + 16 - j * 8.4, ln, fontName="Times-Italic",
                         fontSize=6.8, fillColor=sc, textAnchor="middle"))


def _arrow(d, x0, x1, y):
    d.add(Line(x0, y, x1 - 4, y, strokeColor=RULE, strokeWidth=1.0))
    d.add(Polygon([x1, y, x1 - 5, y + 2.6, x1 - 5, y - 2.6],
                  fillColor=RULE, strokeColor=RULE))


def figure():
    H = 212.0
    d = Drawing(W, H)

    d.add(String(0, H - 10, "From a sentence in the source to a model that can drive the program",
                 fontName="Times-Bold", fontSize=8.5, fillColor=CHARCOAL))

    bw, bh, gap = 104.0, 52.0, 17.0
    row1_y = H - 74
    row2_y = row1_y - 78

    # Row one: what the repository does with its own descriptions today.
    _box(d, 0, row1_y, bw, bh, "the descriptions,|in the source",
         "100 catalog entries|about 10,600 words", True)
    _arrow(d, bw, bw + gap, row1_y + bh / 2)
    _box(d, bw + gap, row1_y, bw, bh, "the manual,|written by the program",
         "pinned by a test|that fails on drift", True)
    _arrow(d, 2 * bw + gap, 2 * bw + 2 * gap, row1_y + bh / 2)
    _box(d, 2 * (bw + gap), row1_y, bw, bh, "the corpus|(step one)",
         "same definitions,|question and answer pairs", False)
    _arrow(d, 3 * bw + 2 * gap, 3 * bw + 3 * gap, row1_y + bh / 2)
    _box(d, 3 * (bw + gap), row1_y, bw, bh, "the recipe|(step two)",
         "seed, size, steps:|a file, not a click path", False)

    # The turn down to row two.
    x_last = 3 * (bw + gap) + bw / 2
    x_first = bw / 2
    d.add(Line(x_last, row1_y, x_last, row1_y - 18, strokeColor=RULE, strokeWidth=1.0))
    d.add(Line(x_last, row1_y - 18, x_first, row1_y - 18, strokeColor=RULE, strokeWidth=1.0))
    d.add(Line(x_first, row1_y - 18, x_first, row2_y + bh + 4, strokeColor=RULE,
               strokeWidth=1.0))
    d.add(Polygon([x_first, row2_y + bh, x_first - 2.6, row2_y + bh + 5,
                   x_first + 2.6, row2_y + bh + 5], fillColor=RULE, strokeColor=RULE))

    # Row two: the training run and what comes out of it.
    _box(d, 0, row2_y, bw, bh, "the training run",
         "one overnight window|on a shared card", False)
    _arrow(d, bw, bw + gap, row2_y + bh / 2)
    _box(d, bw + gap, row2_y, bw, bh, "the fingerprint|(step three)",
         "336 sites, under 50 KB|reader built, file not", False)
    _arrow(d, 2 * bw + gap, 2 * bw + 2 * gap, row2_y + bh / 2)
    _box(d, 2 * (bw + gap), row2_y, bw, bh, "the report|(step four)",
         "shown in review,|never blocking a change", False)
    _arrow(d, 3 * bw + 2 * gap, 3 * bw + 3 * gap, row2_y + bh / 2)
    _box(d, 3 * (bw + gap), row2_y, bw, bh, "the operator|(step five)",
         "driving the program|offline, no key", False)

    # Key.
    ky = row2_y - 24
    d.add(Rect(0, ky, 16, 9, fillColor=GLOW_LT, strokeColor=GLOW, strokeWidth=1.0))
    d.add(String(21, ky + 1.5, "built, with tests", fontName="Times-Italic", fontSize=7.0,
                 fillColor=CHARCOAL))
    d.add(Rect(112, ky, 16, 9, fillColor=None, strokeColor=FAINT, strokeWidth=0.9,
               strokeDashArray=[3, 2.6]))
    d.add(String(133, ky + 1.5, "not built", fontName="Times-Italic", fontSize=7.0,
                 fillColor=FAINT))

    # What is kept and what is not.
    sy = ky - 24
    d.add(Line(0, sy + 15, W, sy + 15, strokeColor=RULE, strokeWidth=0.5))
    d.add(String(0, sy + 2,
                 "Committed to the repository: the manual, the corpus, the recipe, the "
                 "measurement.  Not committed: the trained",
                 fontName="Times-Roman", fontSize=7.2, fillColor=CHARCOAL))
    d.add(String(0, sy - 8,
                 "adjustment of about 40 megabytes, and the base model it attaches to, "
                 "which is published elsewhere.",
                 fontName="Times-Roman", fontSize=7.2, fillColor=CHARCOAL))

    return d
