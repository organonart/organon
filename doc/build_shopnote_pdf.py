#!/usr/bin/env python3
"""Render a Workshop "shop note" markdown file into a typeset PDF.

The house letter style (classic serif, brass-ruled heads, a title page, the
Workshop footer) factored into one reusable script, so a new note needs no
per-note build code at all. `doc/shopnotes/README.md` is the authoring
guide; this file is the whole of the machinery.

Usage:
    python3 doc/build_shopnote_pdf.py doc/shopnotes/issue-412-fdtd.md
    # writes doc/shopnotes/issue-412-fdtd.pdf next to the source

The markdown contract (see doc/shopnotes/README.md for the authoring guide):

    # The Title of the Note
    **Author**: James Andrew Walsh
    **Issue**: #412 · open            <- "open" / "shipped" picks the stamp
    **Subject**: italic subtitle shown on the title page
    **In one line**: the whole note in one sentence
    **Date**: July 2026

    ## Sections use h2 (brass-ruled). [[PAGEBREAK]] forces a page break.

Optional vector figures: a sibling file `<note-stem>_figure.py` may define
`figure()` returning a reportlab Drawing (plus a `CAPTION` string), and
`figure_<name>()` / `CAPTION_<NAME>` for extra figures. The markdown then
places them with `[[FIGURE]]` / `[[FIGURE:<name>]]`. Notes without figures
need no such file.
"""
import html
import importlib.util
import re
import sys
from pathlib import Path

from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER, TA_JUSTIFY, TA_LEFT
from reportlab.lib.pagesizes import LETTER
from reportlab.lib.units import inch
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (
    BaseDocTemplate, Frame, HRFlowable, KeepTogether, NextPageTemplate,
    PageBreak, PageTemplate, Paragraph, Spacer,
)

# --- House palette (shared with the hand-built letters). -------------------
CHARCOAL = colors.HexColor("#1A1D21")
BRASS = colors.HexColor("#8A6A2E")
FAINT = colors.HexColor("#8A8F96")
RULE = colors.HexColor("#C9BFA8")
PLATE = colors.HexColor("#E7E0CF")
GLOW = colors.HexColor("#2E8B9E")
GLOW_LT = colors.HexColor("#CDECF2")

# --- Fonts: macOS Times New Roman, else Liberation Serif (metric twin). ---
MAC = Path("/System/Library/Fonts/Supplemental")
LIB = Path("/usr/share/fonts/truetype/liberation")
DJV = Path("/usr/share/fonts/truetype/dejavu")

if (MAC / "Times New Roman.ttf").exists():
    serif = {
        "": MAC / "Times New Roman.ttf",
        "-Bold": MAC / "Times New Roman Bold.ttf",
        "-Italic": MAC / "Times New Roman Italic.ttf",
        "-BoldItalic": MAC / "Times New Roman Bold Italic.ttf",
    }
    mono = MAC / "Courier New.ttf"
    mono_bold = MAC / "Courier New Bold.ttf"
    sym = MAC / "Arial Unicode.ttf"
else:
    serif = {
        "": LIB / "LiberationSerif-Regular.ttf",
        "-Bold": LIB / "LiberationSerif-Bold.ttf",
        "-Italic": LIB / "LiberationSerif-Italic.ttf",
        "-BoldItalic": LIB / "LiberationSerif-BoldItalic.ttf",
    }
    mono = DJV / "DejaVuSansMono.ttf"
    mono_bold = DJV / "DejaVuSansMono-Bold.ttf"
    sym = DJV / "DejaVuSerif.ttf"

for suffix, path in serif.items():
    pdfmetrics.registerFont(TTFont(f"Serif{suffix}", str(path)))
pdfmetrics.registerFontFamily(
    "Serif", normal="Serif", bold="Serif-Bold",
    italic="Serif-Italic", boldItalic="Serif-BoldItalic",
)
pdfmetrics.registerFont(TTFont("Mono", str(mono)))
pdfmetrics.registerFont(TTFont("Mono-Bold", str(mono_bold)))
pdfmetrics.registerFontFamily("Mono", normal="Mono", bold="Mono-Bold",
                              italic="Mono", boldItalic="Mono-Bold")
pdfmetrics.registerFont(TTFont("Sym", str(sym)))


def S(name, **kw):
    from reportlab.lib.styles import ParagraphStyle
    base = dict(fontName="Serif", fontSize=10.5, leading=14.5,
                textColor=CHARCOAL, alignment=TA_JUSTIFY, spaceAfter=6)
    base.update(kw)
    return ParagraphStyle(name, **base)


BODY = S("Body")
LIST = S("List", leftIndent=18, bulletIndent=6, spaceAfter=4)
CODE = S("Code", fontName="Mono", fontSize=8.6, leading=11.5,
         alignment=TA_LEFT, spaceAfter=1, leftIndent=10)
# keepWithNext: a heading alone at the foot of a page reads as a mistake, and
# this builder places flowables one at a time, so without it a section title
# strands itself whenever its first paragraph does not fit whole.
H1 = S("H1", fontName="Serif-Bold", fontSize=15, leading=19, spaceBefore=18,
       spaceAfter=4, alignment=TA_LEFT, textColor=CHARCOAL, keepWithNext=1)
H2 = S("H2", fontName="Serif-Bold", fontSize=12, leading=16, spaceBefore=12,
       spaceAfter=4, alignment=TA_LEFT, textColor=BRASS, keepWithNext=1)
CAPTION = S("Caption", fontSize=9, leading=12.5, alignment=TA_CENTER,
            textColor=FAINT, spaceBefore=3, spaceAfter=4,
            fontName="Serif-Italic")

T_SERIES = S("TSeries", fontName="Serif-Bold", fontSize=11, leading=15,
             alignment=TA_CENTER, textColor=BRASS, spaceAfter=10)
T_TITLE = S("TTitle", fontName="Serif-Bold", fontSize=30, leading=36,
            alignment=TA_CENTER, spaceAfter=6)
T_SUB = S("TSub", fontName="Serif-Italic", fontSize=14.5, leading=20,
          alignment=TA_CENTER, spaceAfter=4)
T_META = S("TMeta", fontSize=11.5, leading=17, alignment=TA_CENTER, spaceAfter=2)
T_SMALL = S("TSmall", fontSize=10, leading=14, alignment=TA_CENTER,
            textColor=FAINT, spaceAfter=2)
T_BRASS = S("TBrass", fontName="Serif-Bold", fontSize=12, leading=16,
            alignment=TA_CENTER, textColor=BRASS, spaceAfter=2)
T_PURPOSE = S("TPurpose", fontSize=9.5, leading=13.5, alignment=TA_CENTER,
              textColor=FAINT, leftIndent=0.7 * inch, rightIndent=0.7 * inch,
              spaceAfter=2)

SYM_RE = re.compile("([←-⇿∀-⋿′″℀-⅏·])")
META_RE = re.compile(r"^\*\*(Author|Issue|Subject|In one line|Date|Status)\*\*:\s*(.+)$",
                     re.M)
TOKEN_RE = re.compile(r"^\s*\[\[(?:PAGEBREAK|FIGURE(?::[A-Za-z0-9_]+)?)\]\]\s*$")


def inline(text):
    text = html.escape(text, quote=False)
    text = re.sub(r"`([^`]+)`", r'<font name="Mono" size="8.6">\1</font>', text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", text)
    text = re.sub(r"(?<![\w*])\*([^*\n]+)\*(?![\w*])", r"<i>\1</i>", text)
    text = SYM_RE.sub(r'<font name="Sym">\1</font>', text)
    return text


def load_figures(md_path):
    """Import the optional sibling `<stem>_figure.py` module, if present."""
    fig_py = md_path.with_name(md_path.stem + "_figure.py")
    if not fig_py.exists():
        return None
    spec = importlib.util.spec_from_file_location(md_path.stem + "_figure", fig_py)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def figure_flowables(figmod, name):
    """Resolve [[FIGURE]] / [[FIGURE:name]] against the figure module."""
    if figmod is None:
        print(f"warning: [[FIGURE{':' + name if name else ''}]] used but no "
              f"figure module found; skipping")
        return []
    fn = getattr(figmod, f"figure_{name}" if name else "figure", None)
    if fn is None:
        print(f"warning: figure function for '{name or 'figure'}' not found; skipping")
        return []
    caption = getattr(figmod, f"CAPTION_{name.upper()}" if name else "CAPTION", None)
    group = [fn()]
    if caption:
        group.append(Paragraph(inline(caption), CAPTION))
    return [Spacer(1, 6), KeepTogether(group), Spacer(1, 8)]


def parse(md, figmod):
    story = []
    lines = [ln[2:] if ln.startswith("> ") else ln.lstrip(">") if ln.startswith(">")
             else ln for ln in md.splitlines()]
    i, first_h1 = 0, True
    while i < len(lines):
        line = lines[i]
        if line.strip() == "[[PAGEBREAK]]":
            story.append(PageBreak())
            i += 1
            continue
        m = re.match(r"^\[\[FIGURE(?::([A-Za-z0-9_]+))?\]\]$", line.strip())
        if m:
            story.extend(figure_flowables(figmod, m.group(1) or ""))
            i += 1
            continue
        if line.startswith("```"):
            i += 1
            while i < len(lines) and not lines[i].startswith("```"):
                story.append(Paragraph(
                    html.escape(lines[i], quote=False).replace(" ", "&nbsp;") or "&nbsp;",
                    CODE))
                i += 1
            i += 1
            story.append(Spacer(1, 6))
            continue
        if line.startswith("# "):
            if first_h1:
                first_h1 = False
                i += 1
                continue
            story.append(Paragraph(inline(line[2:]), H1))
            i += 1
            continue
        if line.startswith("## "):
            # The rule belongs to the heading it introduces, so bind the two
            # together; H1's keepWithNext then carries the pair to the next
            # page rather than leaving either one stranded at the foot.
            story.append(KeepTogether([
                HRFlowable(width="100%", thickness=0.8, color=BRASS,
                           spaceBefore=16, spaceAfter=2),
                Paragraph(inline(line[3:]), H1),
            ]))
            i += 1
            continue
        if line.startswith("### "):
            story.append(Paragraph(inline(line[4:]), H2))
            i += 1
            continue
        if re.match(r"^\s*---+\s*$", line):
            i += 1
            continue
        m = re.match(r"^(\s*)([-*]|\d+\.)\s+(.*)$", line)
        if m:
            indent, marker, text = m.groups()
            j = i + 1
            while j < len(lines) and lines[j].strip() and not re.match(
                    r"^(\s*)([-*]|\d+\.)\s+", lines[j]) and not lines[j].startswith(("#", "|", "```")) \
                    and not TOKEN_RE.match(lines[j]):
                text += " " + lines[j].strip()
                j += 1
            i = j
            bullet = "•" if marker in "-*" else marker
            story.append(Paragraph(f"<bullet>{bullet}</bullet>{inline(text)}", LIST))
            continue
        if line.strip():
            text = line.strip()
            j = i + 1
            while j < len(lines) and lines[j].strip() and not lines[j].startswith(("#", "|", "```", "- ", "* ")) \
                    and not re.match(r"^\s*(\d+\.|[-*])\s+", lines[j]) and not re.match(r"^\s*---+\s*$", lines[j]) \
                    and not TOKEN_RE.match(lines[j]):
                text += " " + lines[j].strip()
                j += 1
            i = j
            if META_RE.match(text.split("  ")[0]) or META_RE.match(text):
                continue
            story.append(Paragraph(inline(text), BODY))
            continue
        i += 1
    return story


def extract_title(md):
    m = re.search(r"^# (.+)$", md, re.M)
    return m.group(1).strip() if m else "Shop Note"


def extract_meta(md):
    return {k: v for k, v in re.findall(META_RE, md)}


def stamp_for(issue_text):
    words = set(re.findall(r"[a-z]+", issue_text.lower()))
    if "open" in words:
        return "Notes on work in progress"
    if words & {"shipped", "closed", "done", "landed", "merged"}:
        return "Notes on finished work"
    return "Notes from the workbench"


def title_page(title, meta):
    subject = meta.get("Subject", "")
    oneline = meta.get("In one line", "")
    issue = meta.get("Issue", "")
    date = meta.get("Date", "")
    author = meta.get("Author", "James Andrew Walsh")
    fl = [Spacer(1, 1.35 * inch)]
    fl.append(Paragraph("SHOP NOTES", T_SERIES))
    fl.append(Paragraph(inline(title.upper()), T_TITLE))
    if subject:
        fl.append(Paragraph(inline(subject), T_SUB))
    fl.append(Spacer(1, 10))
    fl.append(HRFlowable(width="38%", thickness=1.1, color=BRASS,
                         spaceBefore=4, spaceAfter=16, hAlign="CENTER"))
    line = stamp_for(issue)
    if issue:
        line += f" · issue {issue.split('·')[0].strip()}"
    fl.append(Paragraph(inline(line), T_META))
    fl.append(Spacer(1, 26))
    fl.append(Paragraph(inline(author), T_META))
    if date:
        fl.append(Paragraph(inline(date), T_BRASS))
    if oneline:
        fl.append(Spacer(1, 22))
        fl.append(Paragraph(f"In one line: {inline(oneline)}", T_PURPOSE))
    fl.append(Spacer(1, 1.5 * inch))
    fl.append(HRFlowable(width="24%", thickness=0.7, color=RULE,
                         spaceBefore=0, spaceAfter=10, hAlign="CENTER"))
    fl.append(Paragraph("JAMES’ WORKSHOP", T_BRASS))
    fl.append(Paragraph("make the thing · show the thing · write it down plainly",
                        T_SMALL))
    fl.append(NextPageTemplate("body"))
    fl.append(PageBreak())
    return fl


def make_footer(title, date):
    def footer(canvas, doc):
        canvas.saveState()
        canvas.setFont("Serif", 8.2)
        canvas.setFillColor(FAINT)
        canvas.drawString(0.9 * inch, 0.55 * inch, title)
        canvas.drawCentredString(LETTER[0] / 2, 0.55 * inch, str(canvas.getPageNumber()))
        right = "James’ Workshop" + (f" · {date}" if date else "")
        canvas.drawRightString(LETTER[0] - 0.9 * inch, 0.55 * inch, right)
        canvas.setStrokeColor(RULE)
        canvas.setLineWidth(0.5)
        canvas.line(0.9 * inch, 0.72 * inch, LETTER[0] - 0.9 * inch, 0.72 * inch)
        canvas.restoreState()
    return footer


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: build_shopnote_pdf.py <note.md>")
    src = Path(sys.argv[1]).resolve()
    out = src.with_suffix(".pdf")
    md = src.read_text(encoding="utf-8")
    if "—" in md:
        print("warning: the note contains em dashes; house style forbids them")
    title = extract_title(md)
    meta = extract_meta(md)
    figmod = load_figures(src)
    doc = BaseDocTemplate(str(out), pagesize=LETTER,
                          leftMargin=0.9 * inch, rightMargin=0.9 * inch,
                          topMargin=0.85 * inch, bottomMargin=0.95 * inch,
                          title=title, author=meta.get("Author", "James Andrew Walsh"))
    frame = Frame(doc.leftMargin, doc.bottomMargin, doc.width, doc.height, id="f")
    doc.addPageTemplates([
        PageTemplate(id="title", frames=[frame]),
        PageTemplate(id="body", frames=[frame],
                     onPage=make_footer(title, meta.get("Date", ""))),
    ])
    doc.build(title_page(title, meta) + parse(md, figmod))
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
