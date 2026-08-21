# Organon is the product; a layout is what used to be an app

> **Status: RATIFIED as words, 2026-08-21. Not yet executed as code.** The decision named here
> is taken, and `doc/organon_prd.md` is where it now lives as a product definition — writing that
> PRD *is* §5's *"ratify the words first"* step. ⚠️ **Everything else in §5 is still ahead of us,
> and the ordering is unchanged**: no identifier has been renamed, no edition has been collapsed
> and no code has been touched. **#111** owns the restructure and is not started.
>
> ✏️ This block previously read *"a proposal to ratify, not a change already made"*, while #110 and
> #111 both described the position as already ratified. Two documents disagreeing about whether a
> decision had been taken is exactly the drift this tree spends its refusals preventing, so the
> disagreement is resolved here rather than left for a reader to arbitrate.
>
> James, 2026-08-20, on seeing a panel stack and a live 3D viewport in one window:
> *"it's not Organon Console. This is Organon. This is the one thing that it is. It starts
> with this console capability, but we can use it to build outward and build up any sort of
> application that we want."*

---

## 1. The observation that forces it

The console can now divide its pane into regions, and a region can hold an agent, a live 3D
viewport, or a scrolling stack of Organon's own editor panels. Assembling those is a matter of
typing.

Which means a *window full of panels beside an instrument* — the standalone editor — and a
*window full of conversation* — the console — and a *window built around model
visualisation* — Mind — stop being three programs. They are three **arrangements of one
program**, and the arrangement is data.

🚨 **That is the whole claim.** Everything below is consequence.

## 2. What this dissolves: the compile-time `Edition`

`organon-core/src/edition.rs` holds `Edition` = `Full | Mind | Console`, chosen by cargo
features, driving six behaviours (product name, IPC namespace, which tabs show, the visual's
opening state, and so on). Both feature flags default off, and turning both on is a
`compile_error!`.

That mechanism exists to answer *"which product is this binary?"* — and a layout answers the
same question at runtime, better. Three binaries built from one workspace become **one binary
that opens into a named arrangement**.

📌 **The edition system was right for what it was.** It let Mind and the Console exist without
forking a hundred thousand lines. What changes is not that it was wrong, but that the thing it
approximates at build time is now expressible at run time — and a runtime answer can be
switched, saved, shared and extended, which a cargo feature cannot.

## 3. 🚨 What this does NOT dissolve: the plugin

**A VST3/CLAP inside a DAW cannot be a layout, and this is not a matter of effort.** It has a
different lifetime (the host creates and destroys it), a host-owned window, an audio thread
with hard real-time constraints, and a saved-session identity that outlives any of our
decisions.

So the honest shape is:

| | |
|---|---|
| **One Organon application** | whose layouts replace the standalone visualizer, Organon Mind and Organon Console |
| **One plugin artifact** | `Organon.vst3` / `.clap`, unchanged, a separate lifetime |

⚠️ **`CLAUDE.md` invariant 1 is untouched by this and must stay untouched**: the VST3 class ID
and CLAP ID never move, whatever the product is called. Renaming a product is a change to
words a person reads; changing a class ID orphans the device in every saved DAW session. They
are not the same kind of edit and this proposal touches only the first.

⚠️ Likewise the internal identifiers `CLAUDE.md` already protects for *stated reasons* —
`organic-math-visual`'s **file name** (because `spawn_visual()` probes for it), the IPC paths,
the `OrganicMath/` store directory. The test there is "does anything read this?", and this
proposal changes nothing that anything reads.

## 4. What a layout has to be, if it is the unit of identity

Saved layouts are currently deferred in #98 behind the tiers that make regions worth saving.
That ordering stays correct — you cannot usefully save an arrangement of things that do not
exist yet — but the *weight* changes. A layout stops being a convenience and becomes the thing
a person means when they say which program they are running.

Consequences worth stating before anyone builds it:

- **A layout is data, and it must be legible data.** The harness registry
  (`organon-console/src/harness.rs`) is the precedent this repo has already proven: built-ins
  seeded in code, a user's file merged over them by id, serde defaults, unknown fields
  tolerated. `doc/organon_modules_plan.md` §4 calls it *"the closest existing thing, and the
  right precedent"* for extensibility, and it is the right precedent here too.
- **Named layouts are a vocabulary**, so they belong in the one command table (§1.8's *one
  table, four front doors*) rather than beside it.
- ⚠️ **A layout must not be able to produce a window nobody can recover from.** Region
  assignment already refuses by name and keeps the last agent region; a *saved* layout is an
  assignment that arrives all at once, from a file, possibly written by someone else. It needs
  the same refusals and one more: a layout that cannot be drawn must say so and leave the
  current one standing, never half-apply.

## 5. What actually has to move, if this is ratified

Listed so the size is visible. **None of it is urgent**, and none of it should happen before
#98's tiers land.

| | |
|---|---|
| ✅ `doc/organon_prd.md` | **done, 2026-08-21** — the one product definition, absorbing `doc/organon_mind_prd.md`. The words, which §5 says go first |
| `CLAUDE.md` | the naming-convention section, which currently defines *Organon* as the visualizer product and *Organon Console* as a separate thing |
| ⚠️ `README.md`, `doc/how_organon_works.md` (§1 **and** §16), `doc/equations_into_light.md` | **missing from this list until 2026-08-21, and from #111's too** — each carries its own wording of the identity claim, so fixing one leaves four. `doc/organon_prd.md` §1.1 is now the single source they quote |
| `ARCHITECTURE.md` §4.1 | owns the edition mechanism |
| `organon-core/src/edition.rs` | the `Edition` enum and its six behaviours |
| `MIND_ARCHITECTURE.md`, `CONSOLE_ARCHITECTURE.md` | their opening claims about being separate products |
| the binaries | `organon-mind`, `organon-console` and `organon-standalone` collapsing into one, with the plugin and the visual untouched |

🚨 **The order matters and it is the opposite of exciting.** Ratify the words first, let #98
finish, build saved layouts, and only then consider collapsing the editions — because the
editions are what currently make the three products *work*, and a rename that outruns the
mechanism leaves documents describing a thing that does not exist. This repo has paid for that
twice already: a doc claiming a check always prints when it never had, and a seams row
describing a refactor whose premise had already been satisfied elsewhere.

## 6. What is not decided here

- **The name of the collapsed application's default layout**, and whether "Console" survives
  as a layout name.
- **Whether Mind's visual behaviours** (`edition.rs`'s items 3–6) are layout state or something
  narrower.
- **Whether the plugin eventually hosts a layout** of its own inside its editor window. Not
  ruled out; simply not this decision.
