# Organon Mind — Visual Reference

> **Status: adopted (2026-07-29).** The canonical **visual identity** for Organon Mind: the
> reference image, the colour system, and — just as load-bearing — the *subtle* material
> treatments that give it its character. Companion to `doc/organon_mind_prd.md` (§5.5 references
> this file) and to `#542`, which is the implementation programme. This is a product decision,
> not concept art.
>
> **This revision supersedes the warm "instrument / observatory" shell** adopted 2026-07-24
> (warm near-black `#141210`, taupe hairlines, gold accents). The shell is now **blue slate**.
> The *data* language — the magma/inferno colormap and the teal analytical accent — carries
> forward unchanged; see §11. The superseded direction is kept in §18 for provenance.
>
> **The canonical image:** [`doc/ui/reference-images/BlueSlateUILook.png`](ui/reference-images/BlueSlateUILook.png)
> (1498 × 1050) — a restyle of a real Organon Mind screenshot (the shipping Mind tab, not an
> invented layout), which is why it is usable as a spec rather than as inspiration.

---

## 0. How to read this document

Two warnings that determine how much to trust the numbers below.

**The hex values are *sampled from a generated raster*, not authored design tokens.** The
reference image was produced by an image model, so local values wander by a few RGB levels and
no value is sacred to ±3. What *is* canonical is the **structure**: the relationships between
surfaces, the direction of each gradient, the presence of grain, the discipline about where
chroma is permitted. Implement the structure; treat the numbers as the starting point of a
tuning pass on the Mac.

**The subtleties are the design, not decoration on top of it.** The single most common way to
get this wrong is to implement the palette table in §2, ship flat fills, and conclude the
palette was the point. It was not. A flat `#212A30` panel with a 1 px border is a *cheap* dark
UI; the reference is not, and the entire difference lives in §3–§6 — the weak gradients, the
2–3% grain, the one-pixel top highlight, the compressed tonal steps. Budget accordingly.

---

## 1. What the image is of

A dark, low-saturation **blue-slate** instrument. Almost every surface is gray with a small
excess of blue in the RGB balance — a typical background pixel is around `R 30 / G 39 / B 46`.
The blue is present only to make the gray read cool, technical, and metallic.

The governing metaphor, and the sentence to re-read whenever a decision is unclear:

> **A dark powder-coated scientific chassis with shallow brushed-titanium control surfaces,
> viewed under dim, cool laboratory light.**

Two consequences worth stating up front, because they rule out most "dark theme" instincts:

- **It is lit from outside, not from within.** There is no bloom, no neon edge, no luminous fog,
  no glow on any control. Brightness comes from a surface catching ambient light. The only
  things permitted to *emit* are live data (§11).
- **Most of it would survive being converted to grayscale.** Saturation is doing very little
  work. That is precisely why it reads as an instrument rather than a themed entertainment UI.

---

## 2. The base palette

| Role | Approx. | Character |
|---|---:|---|
| Deepest wells | `#121A20` | Nearly-black blue-charcoal |
| Input wells | `#141C22` | Recessed field interior |
| Outer shell | `#192228` | Dark slate body |
| Main background | `#1D252C` | Primary cool-gray field |
| Panel body | `#212A30` | Slightly elevated slate |
| Raised surface | `#273139` | Buttons, lighter regions |
| Header midpoint | `#2F3941`–`#35414A` | Muted silver-slate |
| Border | `#3A464F` | Cool graphite outline |
| Strong border / highlight | `#52616B` | Brushed-steel edge |
| Selected blue | `#5D788C` | Desaturated instrument blue |
| Primary text | `#D1D6D9` | Silver-white, **never** pure white |
| Secondary text | `#8E999F` | Cool neutral gray |
| Disabled text | `#606B72` | Recessive steel gray |
| Teal data indicator | `#389394` | Small analytical accent |

**The rule that generates this table:** keep green between red and blue, and keep blue above
red by **8–15 levels on the background surfaces** (`#121A20` → `#212A30`), *widening* as
surfaces brighten — 18 at `#273139`, 21 at `#3A464F`, 25 at `#52616B`. Derive any new colour
that way rather than picking by eye, or the family drifts.

> **Corrected 2026-07-29.** An earlier revision stated the 8–15 band as holding on *every*
> surface. It does not, and implementing it as written would have desaturated the brighter
> chrome: holding a constant channel gap while luminance rises *changes the hue*. What is
> actually constant is the cast's direction and its proportion, so the absolute gap grows with
> brightness. Caught by `theme.rs`'s `palette_is_cool` test failing against the real tokens —
> which is the argument for asserting a palette rule in code rather than only in prose.

**Pure white is essentially banned.** Even the brightest text sits at `#D1D6D9`. This reduces
glare, and — more importantly — it reserves the top of the value range for live data, so real
luminance means something when it finally appears.

---

## 3. Surface construction — the three layers

**No large region is a flat fill.** Every one is three stacked layers, and skipping any of them
is what makes a dark UI look cheap:

1. a dark slate **base colour** (§2),
2. a broad, weak **lighting gradient**,
3. a very subtle **surface texture** (§4).

**The shell gradient** — a weak top-to-bottom darkening across the whole application:

```css
linear-gradient(180deg, #202A31 0%, #1D252C 50%, #192128 100%)
```

**The ambient key** — a single broad, extremely faint illumination centred near the upper middle
of the window, raising central surfaces by only a few RGB levels:

```css
radial-gradient(ellipse at 48% 5%, rgba(125, 150, 165, 0.055), transparent 52%)
```

This is what stops the interface reading as one uniformly black rectangle. It should read as the
front face of an instrument catching dim ambient light — never as a visible "gradient".

**Edge falloff.** The extreme left, right, and bottom edges are quieter than the central
workspace. Closer to a soft optical vignette than a graphic effect; if it is noticeable as a
vignette, it is too strong.

---

## 4. The material layer — grain and mottling

Large dark areas carry fine luminance variation rather than perfectly uniform fill. **Two
scales**, and both matter:

- **Fine grain** — very small, nearly monochromatic noise, only **1–3 RGB values** in strength.
  It breaks up digital flatness without ever looking "textured".
- **Broad mottling** — much softer, low-frequency variation spanning tens or hundreds of pixels,
  making parts of a panel fractionally lighter or darker. This is what produces the
  powder-coated / anodized character.

```css
/* fine grain */          opacity: 0.025;  mix-blend-mode: soft-light;  /* monochrome only */
/* broad mottling */      opacity: < 0.04;
```

**Constraints that are easy to violate:**

- **Monochrome only.** Coloured speckle destroys the effect instantly.
- **Finer than paper grain, less directional than brushed metal.** The target is coated metal or
  precision polymer, not raw aluminium.
- **Static, not animated.** It is a material, not an effect.

---

## 5. Silver panel headings

The headings (`Neural Network`, `Model / Specimen`, `Chat / Agent`) carry one of the image's most
important effects, and they are **not** simply lighter rectangles. They use a restrained
**three-stage metallic gradient** — dark upper lip, brighter silver-slate middle band, slightly
darker lower portion:

```css
linear-gradient(180deg, #202930 0%, #303B43 42%, #2B353D 100%)
```

Because the **middle is lighter than both top and bottom**, the header reads as a soft *convex*
surface — rolled metal — without ever looking glossy. A two-stop gradient will not do this; the
three stops are the whole trick.

**On the apparent diagonal.** There *is* a weak left-to-right and upper-left-to-lower-right drift
in several surfaces, but the dominant effect on these headings is **vertical**. The diagonal
impression is emergent, from four things at once: slight horizontal luminance variation, the
low-frequency mottling of §4, unequal illumination across adjacent panels, and brighter top-left
/ darker lower-right edges. **The ambiguity is the point** — it must not announce itself as a
45° gradient. Implementing a literal clean diagonal here would be a misreading.

---

## 6. Borders, bevels, and elevation

The elevation hierarchy is deliberately **very shallow**. Nothing floats far above anything else.

A typical card:

```css
background: linear-gradient(180deg, #242E35, #1D252B);
border: 1px solid #39454D;
border-radius: 5px;                    /* 4–6 px throughout */
box-shadow:
    inset 0  1px rgba(225, 240, 248, 0.045),   /* top highlight — critical */
    inset 0 -1px rgba(0, 0, 0, 0.38),          /* lower inner seam */
    0 1px 2px rgba(0, 0, 0, 0.28);             /* minimal drop */
```

**The inset top highlight is the single most important line here.** It is what creates the
sensation that the UI has been *machined into a physical faceplate*. Everything else in this
section is refinement; that one line carries the metaphor.

**Dividers are cool and dark** — nearer `#303A41` than black. Because they differ only slightly
from the panel surfaces, the layout stays dense without becoming a grid of bright boxes.

---

## 7. Title bar

One of the brighter large surfaces, with a more pronounced vertical roll: about `#2D383F` near
the top, `#273138` through the middle, `#222C32` toward the bottom, plus a thin bright line along
the very top and a darker seam along the bottom. Graphite-coloured anodized hardware.

Title text is moderately bright silver-gray, not white — which keeps the macOS traffic-light
controls the only strongly saturated elements in the bar.

---

## 8. Buttons and inputs — opposite treatments

**Buttons** use the §5 material family at small scale: slightly lighter upper face, dark lower
edge, cool 1 px outline, very small inner highlight, almost no drop shadow. Shallow membrane
switches or machined instrument keys, not web buttons.

**No button gets an accent fill.** `Reset All`, `Release MIDI clip`, `Open HDR Environment…` are
all neutral slate. Function is communicated through **value and border contrast, never hue**.

**Inputs are treated oppositely — recessed**, darker than the surrounding card, with the upper
interior darker still:

```css
background: #141C22;
border: 1px solid #303B43;
box-shadow:
    inset 0  1px 2px rgba(0, 0, 0, 0.65),
    inset 0 -1px      rgba(120, 145, 160, 0.035);
```

Raised = actionable, recessed = editable. Depth without exaggerated skeuomorphism.

---

## 9. Tabs

Inactive tabs are nearly integrated into the background, with little or no visible container —
they rely on text alone. The active tab gets a lighter blue-slate face, a subtle vertical
gradient, a thin cool-blue lower line, brighter text, and a slightly raised profile. The accent
is deliberately desaturated — oxidized steel or a dim blue status lamp, not a brand blue.

This generalises into the system's most useful selection rule:

> **Selection is communicated first by luminance, second by outline, and only third by colour.**

---

## 10. The preset column

The preset tiles carry the **strongest and most obvious material gradients** in the interface —
a broad metallic face, lighter around the upper/central region, darkening toward the lower edge,
with mild side-to-side variation so each tile feels like an individual physical module:

```css
background:
    linear-gradient(135deg,
        rgba(89, 106, 117, 0.20),
        rgba(39, 49, 57, 0.08) 48%,
        rgba(15, 22, 28, 0.22)),
    linear-gradient(180deg, #303A42, #222B32);
```

**This is the one place the diagonal component is genuinely present** (unlike §5, where it is
emergent). It produces a subdued brushed / satin-metal reflection.

The **selected** preset gets a brighter blue-gray border, a slightly lighter face, a faint cool
interior halo, higher title contrast, and a stronger lower/side outline. Critically: **it does
not glow.** It reads as illuminated *by* reflected blue light, not as emitting it.

The small `R / D / U` controls are dark miniature keys with restrained steel outlines. Their
repeated geometry is what makes the column read as a **rack-mounted preset memory bank** — worth
protecting in any redesign.

---

## 11. Telemetry, and where chroma is allowed

Telemetry cards use the §6 outer construction but contain very dark **plotting wells**
(`#151D23`–`#1B242A`) with: a dark inset shadow, a subtle border, a weak vertical gradient, very
faint blue-gray Cartesian grid lines, and **no empty-state glow**. The grid lines sit
intentionally close in value to their background — visible on inspection, never competing with
data that has not arrived yet.

**The data language survives the shell change unaltered.** The miniature colour ramps are among
the only high-chroma elements in the entire interface, and they are the magma/inferno family
already adopted in the previous revision:

- **Continuous scalars** (entropy, activation heat, attention weight) — magma/inferno,
  perceptually uniform and recognised by practitioners:
  `#0A0510` → `#3B0F52` (aubergine) → `#8C2981` (plum) → `#DE4968` (crimson) → `#FE9F6D`
  (burnt orange) → `#FCFDBF` (pale gold)
- **Discrete categoricals** (heads / experts / lenses) — muted and editorial:
  teal `#3E7C7B` · ochre `#C08A2E` · terracotta `#B5533C` · sage `#7C8B5A` · dusty coral `#C97B6A`
- **Analytical indicators** — teal `#389394`, used for the `=` provenance marks and status
  glyphs. A recurring accent that never turns the shell cyan.

**This contrast is a designed effect, not an accident of the restyle.** Because the shell is
nearly monochromatic, these tiny scientific colormaps read as *meaningful*. The discipline that
buys that meaning: strong chroma is reserved for **data, selection states, status indicators,
and the traffic lights** — nothing else. Every saturated pixel spent on chrome devalues the
data.

This is also the one place where §1's "no glow" rule is eventually negotiable: **live** telemetry
may warrant luminance that empty telemetry never does.

---

## 12. Typography

A clean humanist or neo-grotesque sans — open counters, medium x-height, modest stroke contrast,
no futuristic styling. Narrow spacing in controls, slightly more breathing room in headings.
(Inter, already vendored at `native/src/overlay/font_{regular,bold}.ttf`, satisfies this.)

Four levels:

| Level | Treatment |
|---|---|
| Application title | Bright silver-gray, medium weight |
| Panel titles | Near-white silver, regular-to-medium |
| Labels and control values | Cool light gray |
| Explanatory / disabled copy | Low-contrast blue-gray |

The explanatory prose in `Model / Specimen` and `Chat / Agent` is **deliberately low contrast**.
That is what lets dense help text coexist with operational controls without competing with them —
a pattern to preserve rather than "fix".

---

## 13. Sliders and numeric controls

Extremely subdued: recessed dark slate track, slightly lighter neutral-steel fill, cool mid-gray
thumb with a 1 px border or highlight, **no saturated active colour**.

The numeric entry boxes align into a rigid column, and the reset buttons repeat at the same
x-position with the same circular-arrow glyph. That repetition is doing real work: it is what
makes a dense panel read as **calibrated** rather than merely crowded. Alignment discipline is
part of the visual identity, not just layout hygiene.

---

## 14. Spatial hierarchy

Hierarchy comes from **value stepping**, not colour changes:

```
Application shell → Main workspace → Panel bodies → Silver headers → Buttons / active tabs → Text / live data
```

**The steps are only 5–15 RGB values.** That compressed tonal range is a major source of the
sophistication: cheap dark interfaces jump from black panels to bright gray controls, and the
jump is what makes them look cheap. Resist the urge to "improve contrast" between adjacent
levels — local contrast differentiates each control from its immediate surroundings, while the
whole interface stays tonally unified.

The strong horizontal bands — title bar, navigation row, main modules, telemetry heading,
telemetry grid — are what make it read as test equipment with clearly defined functional stages.

---

## 15. What actually gives it its character

Not one dramatic treatment; the interaction of several restrained ones. If the implementation
feels wrong, it is almost always one of these that is missing:

1. **Cold-neutral chromatic bias** — every gray leans blue, almost nothing *is* blue.
2. **Silver value hierarchy** — headers and raised controls read metallic because they are
   brighter *and* three-stop.
3. **Material irregularity** — grain and mottling stop large surfaces looking synthetically flat.
4. **Shallow physical depth** — 1 px highlights, inner shadows, dark seams; a faceplate, not
   skeuomorphism.
5. **Local rather than global contrast** — differentiate each control from its neighbours, keep
   the whole tonally unified.
6. **Colour discipline** — chroma reserved for data, selection, status, traffic lights.
7. **No bloom** — illuminated from outside, never internally radioactive.
8. **Slightly imperfect lighting** — the weak diagonal and radial drifts are what stop it looking
   like a sterile CSS mockup. *Perfect uniformity is the failure mode.*
9. **Dense alignment** — repeated controls, consistent columns, rigorous baselines.

---

## 16. Condensed reconstruction recipe

1. Build the shell from `#192228`, `#1D252C`, `#212A30`.
2. Keep blue **8–15 RGB values above red on the backgrounds**, widening to ~25 on the
   brightest edges — a constant *proportion*, not a constant difference (§2).
3. Add a very weak upper-centre radial illumination.
4. Add a soft top-to-bottom darkening across the application.
5. Use **three-stop** silver-slate gradients for headers and buttons.
6. Add 1 px cool borders with darker inner seams.
7. Apply monochromatic noise at **2–3%** opacity.
8. Add broad mottling under **4%** opacity.
9. Make input wells nearly black and visibly inset.
10. Keep primary text below pure white.
11. Reserve stronger chroma for data, selected states, and status indicators.
12. No glow except where live telemetry eventually warrants it.

---

## 17. Implementation map → `native/src/theme.rs`

`#542` Tier 1 landed the *structure* that makes this implementable: every colour, font size, and
row width in the editor already resolves through `theme.rs`, so re-pointing the palette is a
change to one token block rather than to 112 card sites and ~1057 control rows.

> **Every number below is now a *default*, not a constant (#551 Tier 1).** The `◐ UI` panel
> edits the whole palette and all the material/depth/light treatments live, persisted to
> `ui_theme.json`. Treat this document as the specification of the shipped default and the
> reasoning behind it — not as a description of what any given session is looking at.
>
> **The first Mac pass changed one default:** grain strength now ships at **0.28** of the baked
> tile. §4's 1–3 RGB levels remains the tile's ceiling and the arithmetic was right; what it
> could not predict is that 1–3 levels across *every* surface at once still reads as too much
> texture. The lesson generalises to everything in §3–§6: these are weak effects whose sum is
> the design, and their sum can only be judged on a display.

**Status (#542 Tiers 1–2, PR #543): implemented.** `theme.rs` carries the blue-slate tokens
below *and* the painted chrome §3–§6 call for — `theme::paint` builds the gradient meshes, the
material grain, the bevels, and the ambient key entirely from `epaint`, so none of it waits on
the wgpu backend Tier 3 would bring. **Not yet judged on a real display**; every number here is
a starting point for a tuning pass on the Mac.

| `theme.rs` token | Warm value (superseded) | **Blue-slate (shipped)** |
|---|---|---|
| `SUNKEN` | `#0E0C0B` | `#141C22` (input wells) / `#121A20` (deepest) |
| `SHELL` | `#141210` | `#192228` |
| *(new)* `WORKSPACE` | — | `#1D252C` |
| `PANEL` | `#1A1714` | `#212A30` |
| `CARD` | `#201C18` | `#212A30` |
| *(new)* `RAISED` | — | `#273139` |
| `CARD_HEADER` | `#26221D` | `#2F3941` (mid-stop of the §5 gradient) |
| `HAIRLINE` | `#3A342E` | `#3A464F` |
| *(new)* `EDGE_STRONG` | — | `#52616B` |
| `BONE` | `#ECE6DB` | `#D1D6D9` |
| `TITANIUM` | `#C9C1B4` | `#8E999F` |
| `MUTED` | `#8A8175` | `#606B72` |
| `GOLD` / `WARM_WHITE` | `#F4D58A` / `#FFF6E6` | → replaced by `SELECTED` `#5D788C` |
| `AMBER` | `#FFB547` | retained **only** for live data / status (§11) |
| *(new)* `TEAL` | — | `#389394` |

**The warmth test inverts.** `theme.rs` asserted `c.r() > c.b()` on every surface under the warm
shell. It now asserts the §2 rule in two parts: `b − r ∈ 8..=15` across the background family,
plus a second test that the cast **never narrows as luminance rises** (`8..=26` overall,
monotonic). Together these pin the *shape* of the ramp rather than a single number, which is
what caught the over-general rule §2 now records as corrected.

**How §3–§6 are realised** (all `epaint`, no shader, no backend change):

| Spec | `theme::paint` | Note |
|---|---|---|
| multi-stop gradients (§3, §5, §6) | `gradient_v`, `silver_face`, `card_face`, `shell_face` | vertex-coloured `Shape::mesh`; corners **chamfered** so a square mesh sits under a rounded border without poking out |
| diagonal sheen (§10) | `diagonal_sheen` | corner-interpolated quad; used on preset tiles **only** — §5's diagonal is emergent, not painted |
| grain + mottling (§4) | `grain`, `grain_image`, `mottle_image` | two baked monochrome tiles: fine grain `Repeat`-tiled 1:1, mottling stretched with linear filtering |
| inset bevel (§6) | `bevel`, `well` | paired hairline strokes; `well` inverts it for recessed inputs |
| ambient key (§3) | `ambient_key` | one triangle fan, painted **once** per window by `workspace_surface` |

⚠️ **The grain alphas are counter-intuitive, and two successive cuts got them wrong.** egui_glow
blends in **gamma** space — `disable(FRAMEBUFFER_SRGB)`, `srgb_textures = false`, unchanged across
0.31 and 0.33 — so nothing is decoded to linear. What matters is the *premultiply*: 0.31's
`from_rgba_unmultiplied` was gamma-aware and inflated light texels at low alpha (the first cut came
out ~8 levels too bright); **0.33 made it a plain multiply**, collapsing the model to
`deviation ≈ strength × (α/255) × (colour − base)`. Light texels therefore run `α 1..=9` and dark
ones `1..=18` — roughly 1:2, because a light texel travels `colour − base` per unit alpha while a
black one travels only `base`. Peak 2.89 levels at full strength; the shipped 0.28 default lands
under 1 level, which is the subtlety this document has been asking for since §4.

**The lesson is procedural, not numeric.** The intermediate revision was tuned against a
*linear-compositing* model that did not describe the pipeline, and it looked plausible enough to
ship. Only the 0.31 → 0.33 bump — which changed the premultiply out from under it — forced the
re-derivation. `theme.rs`'s `grain_is_balanced` now models the real pipeline end to end, so the
next egui bump fails a test instead of quietly re-texturing the whole interface.

**Still flat, pending a look pass:** button faces (§8) carry correct value/border treatment but
not the small three-stop gradient — egui's built-in `Button` has no paint hook, so giving every
button a gradient face means replacing the widget, which is worth doing only once the palette is
confirmed on a real display. §10's *selected* preset tile is also unwired: Organon has no "last
recalled preset" state to light.

---

## 18. Superseded — the warm direction (2026-07-24 → 2026-07-29)

> **Still selectable (#551 Tier 2).** The warm shell ships as a built-in theme, **Warm
> Instrument**, in the `◐ UI` panel's gallery. It was a considered direction rather than a
> mistake, and a look is judged by comparison — being able to flip between warm and cool in one
> click is worth more than arguing about either from memory. `theme_config::warm_instrument()`
> holds the values; a test asserts it is genuinely warm (red leads blue on every surface, the
> exact inverse of §2's rule).


Kept for provenance. The previous revision specified a **warm** "instrument / observatory" shell:
warm near-black `#141210` / `#1A1714`, charcoal panels `#201C18`, taupe hairlines `#3A342E`, bone
`#ECE6DB` / titanium `#C9C1B4` / muted `#8A8175` type, with gold `#F4D58A` and warm white
`#FFF6E6` for selection. It explicitly forbade blue-black — a rule this revision reverses.

**What carried forward:** the magma/inferno data colormap, the earthy categoricals, the teal
analytical accent, the provenance discipline, the "premium lab instrument, not cyberpunk" stance,
and the anti-bloom rule. Only the **shell** changed, warm → cool.

`doc/assets/organon_mind_concepthero.png` is the warm-era hero shot and is **historical**: its
layout ideas (four linked lens panes, left Project dock, right Properties dock, full-width bottom
dashboard) remain the target for #542 Tiers 4–5, but its colour is superseded by §2.

Its generation prompt, and the companion screen-render prompt, were removed from this file in
the same change that adopted §2. They live **only in git history** — recover either with
`git log -p -- doc/organon_mind_visual_reference.md` if wanted again.
