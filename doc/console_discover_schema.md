# The Discover schema

**What this is.** The wire format the console uses to ask a program *"where am I, what might
we talk about, and what can I put my hands on."* Two documents share one vocabulary: a
**discovery document** (orientation) and a **control descriptor** (manipulation).

**Status:** settled 2026-08-10, before implementation. Issue #3 Tier 3 and issue #4 Tier 3
implement this rather than design it. Amend this file rather than diverge from it — it is a
vocabulary, and a vocabulary that two components spell differently is not one.

**Scope.** This describes a *document format*, not a protocol. The console does not care
whether a document came from invoking a program or from a cache on disk, which is what lets a
CLI that has never heard of Organon get a strip (issue #3, Tier 5).

---

## The two invariants

Everything else is detail. These two are load-bearing, and both fail silently if broken.

### 🚨 I1 — The console never executes anything a payload names

An entry says **what it is**, never **what to run**. The console re-invokes a source it
already knows, with a path.

The tempting design is `"next": "organon explore surface --discover"` on each entry. Once
"any program can emit this" is true, that is arbitrary code execution from data, and it is the
kind of thing that gets designed in by accident and found later. **No field in either document
is ever passed to a shell, `exec`, or a process spawn.** `coverage.full` is a string to
*display* or hand to the agent as text — never to run.

### 🚨 I2 — Descriptors are generated from the parameter table, never hand-written

The console renders knobs from what a descriptor claims. If a descriptor's range or taper
disagrees with the engine, **every control sits in the wrong place and every value an agent
sets lands somewhere other than it meant** — and it will look approximately right, so nobody
will notice for weeks.

The tree already has the machinery and the discipline: `doc/reference/` is generated from
`agent.rs`'s exhaustive matches, `param_desc` and `recipe.rs`, and
`generated_reference_is_current` fails the build on drift. Descriptors are generated the same
way and guarded the same way. See *Tests that must exist*.

🚨 **`agent.rs::id_range` is NOT the source of truth for a descriptor's range.** It and
`clip.rs::RANGES` are hand-written mirrors of `params.rs`, pinned by no test — and as of
2026-08-10, **9 of 45 actuatable ids disagree with `params.rs`**: `trans_amp_x/y/z` by 10×
on the maximum (the published `doc/reference/parameters.md` ships the wrong range), plus
`exposure`, `bloom_intensity`, `sss_power`, `irid_scale`, `cam_damping`, and `cam_path`
(whose declared max admits a 12th `CamPath` variant that does not exist). A descriptor's
`min`/`max`/`default` come from the parameter object — `Param::preview_plain(0.0)` /
`preview_plain(1.0)` / `default_plain_value()` — and
`taper_round_trips_against_the_engine_range` is what keeps them there.

---

## The three legs

The console deals in three different things, and conflating them is how this schema doubles
in size before it is used once.

| | What it is | Where it lives | How big |
|---|---|---|---|
| **Topics** | what we might talk about | the discovery document | 5–7 |
| **Commands** | what can be executed | `--help` on a node | large; never inlined |
| **Controls** | what can be manipulated | the control descriptor | one per parameter |

**A discovery document carries topics, and optionally a descriptor on any entry that is a
control.** It never carries commands. Acting is `--help`, paid for in the turn that needs it.

That is the property that makes the strip cheap enough to fire on every context switch — the
agent's scarce resource is context, not round trips, and the CLI surfaces ~1,370 parameters.
**`--discover` is orientation, not documentation.**

---

## Invocation

**These are subcommands, not flags, and `describe` already exists.** The `organon` CLI is
subcommand-styled throughout — `catalog`, `describe`, `status`, `get`, `set`, `snap`, `recipe`,
`watch` — so a `--discover` flag would be foreign to it. More importantly,
`skills/organon-cli/SKILL.md` already documents `organon describe <query>` returning *"its
kind, range, current value, and what it does."* **That is the control descriptor, in prose.**

```
organon discover [path]            # path omitted = root
organon describe <query> --json    # the descriptor, machine-readable
```

🚨 **`--json` is a second rendering of `describe`, never a second implementation.** One
command, two outputs, one source — the same discipline that makes the CLI, the agent's
catalog and the strip three renderings of one table. If the prose and the JSON can disagree
about a range, this whole schema is decorative.

The source's *invocation* is configuration, never payload (I1). The forms above are Organon's;
a foreign source is invoked however its Tier 5 mapping recorded, and the console does not care
which — it consumes documents, not command lines.

**Requirements on an emitter:**

- JSON to **stdout**, nothing else on stdout. Diagnostics to stderr.
- Exit **0** on success. Any non-zero exit means "no document," not "empty document."
- `<path>` is a dotted id string. Unknown path → exit non-zero with a message on stderr.

**Sources are configuration, never payload** (I1). For Organon the source is the `organon`
binary. For a foreign CLI it is whatever the Tier 5 mapping pass recorded, and the cache key
is `(source id, path)`.

---

## The discovery document

```json
{
  "discover": 1,
  "context": {
    "id": "explore.surface",
    "title": "Surface",
    "summary": "The material the generator is drawn onto."
  },
  "entries": [
    { "id": "roughness",    "label": "Roughness",    "phrase": "roughness: ",       "children": true  },
    { "id": "displacement", "label": "Displacement", "phrase": "displacement: ",    "children": true  },
    { "id": "layers",       "label": "Layers",       "phrase": "layers: "                             },
    { "id": "lighting",     "label": "Lighting",     "phrase": "lighting: ",        "children": true  },
    { "id": "presets",      "label": "Presets",      "phrase": "surface presets: "                    }
  ],
  "coverage": { "shown": 5, "of": 41, "full": "organon explore surface --help" }
}
```

### Fields

| Field | Req | Notes |
|---|---|---|
| `discover` | ● | Schema version. `1` for this document. |
| `context.id` | ● | Dotted path. What `--at` would take to get back here. |
| `context.title` | ● | Human label for the context. |
| `context.summary` | | One line, for the **agent's** orientation. Not rendered in the strip. |
| `entries[]` | ● | 5–7 recommended, **10 hard cap**. May be empty. |
| `entry.id` | ● | Unique within the document. **Stable across versions** — see below. |
| `entry.label` | ● | What the strip shows. Short; it shares a row with 4–9 others. |
| `entry.phrase` | ● | Literal text composed into the input line on tap. |
| `entry.glyph` | | A single character (Nerd Font available). Decorative. |
| `entry.children` | | `true` if `<context.id>.<entry.id>` has its own document. Default `false`. |
| `entry.control` | | A **control descriptor** (below). Present when the entry is a control, not a topic. |
| `coverage` | | `{ shown, of, full }`. See *Coverage is not optional in spirit*. |

Unknown fields **MUST** be ignored. Foreign emitters will get this wrong and should still work.

### `entry.id` is load-bearing beyond the strip

It is stable across versions because it is what makes the promotion ladder possible later —
*"this entry was tapped twelve times; pin it"* requires a name that does not move. Costs
nothing now; impossible to retrofit once documents are cached and usage is recorded.

### `phrase` is literal text, not a template

It is inserted at the cursor in the input line **as visible, editable text**. Not hidden
metadata: invisible injection creates a state where you and the agent disagree about what was
said and neither can tell, whereas visible text degrades perfectly to *"you typed it
yourself."*

**Convention:** a fragment that reads naturally before whatever the user types next, ending in
a separator. `"roughness: "` composes to `roughness: make it uneven toward the edges`. Keep it
short — every tap spends context, and `surface: ` beats `Let us now discuss the topic of
surfaces.`

**If a phrase needs current state in it, the emitter interpolates when it generates the
document.** The emitter knows the state; the console does not. There is no template engine
here and there should not be one.

### Drill-down, and why the input line is the undo stack

Tapping an entry with `children: true` composes its phrase **and** fetches
`<context.id>.<entry.id>`. Two levels read as a path in language:

```
surface: roughness: make it more uneven toward the edges
```

Because the composed text is ordinary editable text, **backing out is backspace.** There is no
separate navigation state to reconcile with what was actually said — which is the failure mode
that kills most drill-down interfaces.

### Static by default; state-dependent when the emitter wants it

A document **SHOULD** be stable for a given path, so it is cacheable and the strip never
surprises you. An emitter **MAY** include state-dependent entries (the presets you actually
have, the generator currently loaded) when they earn their place.

**The console re-fetches only on explicit navigation. It never polls.** Staleness in a subject
selector is cheap; a strip that flickers as state changes underneath your hand is not.

### Coverage is not optional in spirit

A strip showing 5 of 41 things and saying nothing about it **is a lie about the shape of the
system.** This is the deep risk in a summoned interface: spatial UI is *browsable* — you can
find a feature by looking, without knowing its name — and a curated strip is only as
discoverable as its curation.

`coverage` makes the strip honest about being a subset and hands the human and the agent the
route to the rest. It is the same move the tree already makes with the honesty ledger's
measured / derived / proxy / projection marks, which is exactly why it will survive.

⚠️ `coverage.full` is displayed or handed to the agent as text. **Never run** (I1).

---

## The control descriptor

Defined once; carried in two places — inline on a discovery entry that is a control, and in
bulk from `--describe`.

```json
{
  "id": "surface.roughness",
  "label": "Roughness",
  "help": "How much the surface scatters light. Low is mirror, high is chalk.",
  "kind": "float",
  "value": 0.35,
  "default": 0.2,
  "range": { "min": 0.0, "max": 1.0, "taper": "linear" },
  "unit": null,
  "format": { "style": "magnitude" },
  "widget": "knob",
  "writable": true,
  "mapped": { "surface": "midi", "control": "CC 21" }
}
```

### Kinds

`float` · `int` · `bool` · `enum` · `trigger`.

`color` is the obvious next one given the palette, and vector kinds after that. **Reserve the
names; do not build them.**

```json
{
  "id": "synth.cutoff", "label": "Cutoff", "kind": "float",
  "value": 880.0, "default": 1000.0,
  "range": { "min": 20.0, "max": 20000.0, "taper": "log" },
  "unit": "Hz", "format": { "style": "fixed", "decimals": 0 }
}
```

```json
{
  "id": "surface.material", "label": "Material", "kind": "enum",
  "value": "graphite", "default": "slate",
  "variants": [
    { "id": "graphite", "label": "Graphite" },
    { "id": "paper",    "label": "Legal pad" }
  ],
  "widget": "combo"
}
```

A `trigger` carries no `value`, `default` or `range` — it is invocable, not settable.

### ⚠️ `taper` is the field everyone omits and the one that breaks everything

Audio and visual parameters are almost never linear; a cutoff knob is logarithmic. nih-plug
models skew explicitly in its float ranges. **If the descriptor says `linear` and the engine is
skewed, every knob is in the wrong place and every agent-set value is wrong** — approximately
right, so it survives review.

Permitted: `{"taper":"linear"}` · `{"taper":"log"}` · `{"taper":"skewed","factor":0.3}`.
Enough to be truthful about what the engine actually does, and no more.

📌 **Verified 2026-08-10 against `native/src/params.rs`** — not `param_table.rs`, which
declares no ranges (it is the slot-packing table). Every one of the engine's 1372 parameters
is `FloatRange::Linear` or `IntRange::Linear`: `FloatParam::new` is called exactly once in
the tree, inside `flin()`, which hard-codes Linear; `Skewed`, `SymmetricalSkewed`,
`Reversed` and `with_step_size` are unused, and no host parameter is drawn with a non-linear
widget. So `taper` is `"linear"` for every Organon control today; `log` and
`skewed{factor}` remain permitted as headroom for foreign emitters and for a future skewed
range — `taper_round_trips_against_the_engine_range` is what will catch one.

### Values live in the display domain

`min`, `max`, `value` and `default` are what a human would say — *20 to 20000 Hz*, not
normalized 0..1. The normalized form is an implementation detail the console has no business
knowing, and the agent should not have to do the mapping to say "set cutoff to 800 Hz."

`taper` is what lets a knob compute its own position from those four numbers. That is the
entire reason it is in the document.

### `format` says how to print, and `"magnitude"` is the engine's actual rule

`format` is `{ "style": "fixed", "decimals": N }` or `{ "style": "magnitude" }`. `style`
defaults to `"fixed"`; `decimals` is required for `"fixed"` and ignored otherwise.

Every Organon float goes through one magnitude-scaled formatter (`v2s_va` in
`native/src/params.rs`): decimals = 0 when |v| ≥ 1000, 1 when ≥ 100, 2 when ≥ 10, else 3;
trailing zeros and a trailing `.` are trimmed; `""` and `"-0"` render as `"0"`. A fixed
decimal count renders `2160` as `2160.000` and `0.005` as `0.01` — both visibly wrong next
to the editor, which is why `{decimals: N}` alone could not say the truth. Ints are
`{"style": "fixed", "decimals": 0}`.

**Round-trip law:** `format(v)` must equal the parameter's own
`normalized_value_to_string(preview_normalized(v), false)` — asserted inside
`taper_round_trips_against_the_engine_range`; same walk, same params, no second test.

### `unit` is `null` until a parameter declares one

nih-plug's `with_unit` is unused across all 1372 parameters, so `Param::unit()` is `""`
everywhere; units exist only as prose inside names (`"Exposure (EV)"`) and glosses.
Emitting `null` is the honest answer, and I2 forbids inventing one at the emitter. When
units are wanted, they get declared at the parameter — in `flin`/`ilin` or a sibling table
generated from the same slot lists — never typed into a descriptor.

### `mapped` is the promotion ladder made observable

If a descriptor can say *"currently on CC 21,"* the ladder stops being a design idea and
becomes a queryable fact. The agent can see that a control has reached the physical rung and
stop summoning it; a mapped control nobody has touched in a month is a demote candidate.

`surface` is where it sits — `midi`, `strip`, `panel`, or absent for none.

### 🚨 The guardrail: a descriptor describes a parameter, never a layout

The moment someone adds `x`, `y`, `width`, `row` or `order`, this has quietly become a UI
framework description language and it will be a bad one. Layout is the console's business.

`widget` is a **hint**. The console is free to ignore it. That distinction is what keeps this
on the right side of the line.

---

## `describe --json`

The existing `organon describe` rendered as data instead of prose. Returns descriptors for a
node.

```json
{
  "describe": 1,
  "context": { "id": "explore.surface", "title": "Surface" },
  "controls": [ /* control descriptors */ ],
  "coverage": { "shown": 12, "of": 84, "full": "organon explore surface --describe --all" }
}
```

Default page size applies — **a panel with 84 controls is a menu, not an instrument.** `--all`
lifts it for programmatic consumers. `coverage` is required here in practice for the same
reason it matters on the strip.

---

## The skill moves with the CLI

`skills/organon-cli/SKILL.md` is the agent-facing teaching document for this CLI, and it
already makes the correct split: it teaches the loop and the grammar, and defers the surface
to the live catalog — *"the live catalog is the authority … ask the tool, not your memory."*

**Keep that split.** Adding `discover`, `describe --json`, or an `organon console` branch means
the skill gains the *shape* of the new vocabulary; it must never gain an enumeration of what
lives inside it, because an enumeration is what rots.

📌 `.claude/hooks/doc-rules.sh` now lists the skill as accountable for `native/src/bin/ctl.rs`
and `native/src/cli.rs` — the files that define the command *shape*. Parameter-level detail is
deliberately not a trigger: that is already guarded by `generated_reference_is_current`, and a
rule that fires on every parameter change is a rule everyone learns to ignore.

⚠️ The skill currently covers 45 of ~1,370 parameters without saying so. **That is the same
lie `coverage` exists to prevent**, one layer up — worth a sentence in the skill stating it is
a curated teaching subset, not an index.

---

## Console behaviour and failure modes

| Situation | Behaviour |
|---|---|
| Unparseable document | Strip **unchanged**, logged to stderr. Never a partial render — half a strip is worse than none. |
| More than 10 entries | Truncate to the cap, and say so in `coverage`. **Never scroll.** |
| Zero entries | Strip hides entirely, and the reserved row returns to the PTY. |
| Source slow or hanging | Timeout, strip unchanged. **The strip must never block input.** |
| Duplicate `entry.id` | Reject the document. It cannot be addressed unambiguously. |
| Unknown `discover` version | Reject, log the version. Do not guess. |

---

## Tests that must exist

Pure, headless, no GPU, no egui context — the house shape.

- `discovery_document_round_trips` — serde, with every optional field absent.
- `unknown_fields_are_ignored` — the foreign-emitter contract.
- `entry_ids_are_unique_within_a_document`
- `entry_count_never_exceeds_the_cap`
- `coverage_shown_matches_entries_len`
- `a_document_with_no_entries_hides_the_strip`
- **`taper_round_trips_against_the_engine_range`** — the I2 guard. Emit a descriptor for every
  parameter, map its display domain back through the declared taper, and assert it matches the
  engine's own range mapping. This is the test that stops silent wrongness, and it belongs
  beside `generated_reference_is_current`.
- `descriptor_values_are_in_the_display_domain` — a normalized 0..1 value for a parameter whose
  display range is 20..20000 is a bug, and an easy one to ship.

---

## Deferred, and held deliberately

Layout of any kind. Grouping beyond a flat `group` string. Modulation routing and automation
lanes — real, but VST-host territory, not what a console panel needs. Value smoothing. Anything
describing how a control *animates*. Multi-select on the strip. Templating in `phrase`. Inline
nested children in a discovery document.

Each is real work. None of it is this.
