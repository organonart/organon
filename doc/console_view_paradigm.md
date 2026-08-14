# Organon Console — the view paradigm

> **What this document is.** A **design** document for work that does not exist yet: how the
> console will hold and show things that are not text. It is not a present-state reference —
> `CONSOLE_ARCHITECTURE.md` owns that, and everything below is unbuilt unless this document
> says otherwise and names the file.
>
> **Status: one section has been built — §5's double kind registry is now single (#48 Tier 1),
> and §5 and §8 say so where they said otherwise.** Everything else is unimplemented: no
> exhibit has been rendered, no pane has been split, no image has appeared in the console. The
> measured facts are marked as such; the rest is design. Keep that distinction when quoting
> this file — the console's honesty discipline applies to its plans as much as to its
> readouts.
>
> Decisions marked 📌 were made by James on 2026-08-13 and are settled. Their rejected
> alternatives are recorded because a decision without its alternative reads as an accident.

---

## 0. Why this is not a terminal graphics protocol

The motivating frustration was not being able to see an image without leaving the session,
and the thing that made it feel possible was Kitty, which can put an image inline in a
terminal.

**Kitty had to invent an escape-sequence protocol because a terminal's only input is a byte
stream.** The console's conversation front-end is not a terminal: it spawns Claude Code over
pipes and renders a *structured event stream* natively. It therefore does not need a graphics
protocol at all — it needs a **content type in the stream**. That is strictly more capable: an
escape sequence can carry an encoded bitmap; a stream entry can carry a live, interactive,
still-updating thing.

⚠️ **MEASURED (2026-08-11, recorded in `doc/console_patch_protocol.md` §0): the terminal
front-end cannot be the venue for this.** ConPTY rewrites the console's byte stream — APC
sequences (`ESC _ … ESC \`) are **stripped entirely**, a private OSC number survives
byte-intact but is **hoisted out of stream order** (arriving in its own read *before* the
surrounding text, so it cannot carry a position), and OSC 8 survives inline but has its
params rewritten. A WSL tab is `wsl.exe`, a Windows process under ConPTY, so it is not an
escape hatch. There is no ConPTY-free path on this machine.

So: **rich media belongs to the conversation front-end.** Kitty-protocol support in the
terminal tab is worth having *eventually and for a different reason* — compatibility, so
existing tools like `imgcat` and matplotlib's sixel output work — and it is a lesser goal
that shares none of this design.

---

## 1. Three concepts, and why not more

The failure mode this document exists to prevent is building "inline images", then "a PDF
panel", then "an audio player" — three features with three lifecycles, three eviction
stories and nothing shared. Instead:

**A view** — something that can draw itself into a rectangle and handle input there. A
harness (a terminal, a conversation), an exhibit viewer, the portal, and one day a VR
surface are all views.

**An exhibit** — content with a **kind**, holding **one or more items**. Three generated
design candidates arrive as *one exhibit with three items*, not as three inline images. That
single decision is what turns "a gallery scroller" and "a grid that maximises on tap" into
*presentation choices of one kind* rather than two features.

**A placement** — where an exhibit currently lives: inline in the transcript, docked, in a
pane, full screen. **Promotion** is moving between placements, and it is a gesture, not a
per-media-type feature.

### The reuse that makes this affordable

Placement is the same shape of axis as **posture** (`CONSOLE_ARCHITECTURE.md`, the posture
section). Posture established that the same draw code at a different scalar is a different
form, and that scalars tween. An exhibit growing thumbnail → pane → full screen is that
mechanism again, and it is also the portal's already-scoped *animated grow*. One mechanism,
three uses. If promotion needs its own animation system, something has gone wrong.

---

## 2. 📌 Layout: panes hold views

**Decided.** The console generalises from *a strip of tabs* to **a layout of panes, each
holding a view**. Tabs become one arrangement of that layout rather than the top-level unit.

The forcing observation: **a tab is exclusive, and the viewer is not.** The right-hand viewer
was described as *attached* — beside the transcript, simultaneous with it. Tabs are
one-at-a-time, so the viewer cannot be a tab without either breaking what a tab means or
special-casing it forever.

Rejected alternatives, and why:

- **"The viewer is only a bigger placement for exhibits."** Less code and lands sooner, but
  it forecloses the stated requirement — a panel that is *not* a dumb web-page panel, that
  has the console's own capabilities. A placement cannot hold a harness; a pane can.
- **"Keep tabs on top and let them split."** Least disruptive, but it leaves two container
  concepts that will drift, and the drift will be discovered the first time something needs
  to be both.

⚠️ **This is the largest refactor in this document and it should be sized honestly.** Tabs
today are a strip plus a `TabAction` the host applies (`tabs.rs`, whose module doc argues
that the strip never mutates itself mid-frame — a rule that survives this change and should
be preserved verbatim for panes). Generalising them touches tab lifecycle, focus, the cwd
that a conversation pane resolves, the keyboard routing, and the portal's rect derivation.
It is worth it because it is what makes *"it could look like anything in the future, it could
have a VR mode"* a property of the architecture rather than an aspiration — but it should be
scoped as its own tier, and it should not be smuggled in alongside a media kind.

---

## 3. 📌 An exhibit is a reference, never bytes

**Decided.** An exhibit holds a path or a handle to content that can be read again. Bytes
that arrive inline — an image embedded in a tool result — are **spilled to a
content-addressed cache on arrival** and the exhibit refers to that.

This is a day-one constraint rather than an optimisation, because it is what makes eviction
free. Every rendered image is a GPU texture; a day's scrollback with two hundred of them
exhausts VRAM. The console must be able to **drop an exhibit's texture and re-materialise it
later**, and that is only possible if the source outlives the texture.

The rejected alternative — allowing ephemeral bytes an exhibit alone holds — makes every
eviction decision "drop it and it is gone forever", which is the kind of thing that is
invisible for months and then ruins a long session.

⚠️ Precedent to follow rather than reinvent: `session::Artifact` already exists with its
payloads stored beside the log in the session directory, and `CONSOLE_ARCHITECTURE.md` §2
already names a content-addressed artifact store as a coming seam. The exhibit cache should
be that store, not a second one. **Two stores that can disagree eventually do** — the same
argument that made preferences reuse `SessionLog::store_root()` rather than spell `dirs`
twice.

---

## 4. 📌 Placement authority: kind default, agent suggests, human outranks

**Decided**, and it is the same ranking as everywhere else in this system.

1. **A kind has a preferred placement.** Audio docks rather than sitting inline, because that
   is what audio is. An image arrives inline.
2. **An agent may suggest** a placement — "put these three in the viewer" — and a suggestion
   is a request, not an instruction.
3. **The human outranks both, and their choice sticks.**

This is deliberately the hand-outranks-the-camera rule (`CONSOLE_ARCHITECTURE.md`, the camera
section) and the presence-outranks-ambient rule from the lighting layer, reached a third
time by a different road. Consistency here is not tidiness: it means there is **one thing to
learn** about who wins, and a new capability inherits the answer instead of inventing one.

⚠️ The consequence worth stating plainly: **an agent cannot take over the screen layout.** It
can ask. The rejected alternative — the sender chooses — is the most expressive for something
like a three-candidate gallery and is exactly why it must not be allowed.

---

## 5. Kinds are the extension point

A **kind** is a name that resolves to something that can render an exhibit. The registry of
kinds is the surface along which the console is extended "always, for all time".

✅ **Landed — this section is the one part of this document that is no longer design.** #48
Tier 1 made `organon_core::kind::Kind` the single vocabulary both front-ends resolve from;
`cli::PatchKind` is gone and `conversation::ArtifactContent` answers `kind()` from the same
set. The rest of this section is kept in the past tense it was written in, because it is why
the change was made.

⚠️ **The count below is wrong, and the correction is the more useful fact: there were
THREE.** `block_panel::PatchContent` — the terminal front-end's *paint* target, one layer in
from the wire — is the same two-item taxonomy again, and it carries payloads exactly as
`ArtifactContent` does. So the real shape is one vocabulary and **two payload carriers, one per
placement**, which is a better answer than the two-way merge this section imagined: a patch's
panel owns live widget state pinned to scrollback lines, an artifact's is a description the
view keys state off, and neither could be flattened into the other without losing something.
Both now answer `kind()`, each pinned by its own test.

⚠️ **One thing Tier 1 did NOT do, deliberately: unify the two *words*.** The terminal lane says
`scene` and the composer says `/surface`, and both are typed by humans — so an inert tier
could change neither. `CONSOLE_ARCHITECTURE.md` §1.1 records what unifying them would cost.

🚨 **This already existed in embryo TWICE, the two copies already overlapped, and they had
already begun to drift.** Measured 2026-08-13 by reading the tree:

| Concept | Terminal side — `cli.rs::PatchKind` | Conversation side — `conversation.rs::ArtifactContent` |
|---|---|---|
| a live control panel | `Panel` | `Panel(PanelSpec)` |
| a picture the engine draws | `Scene` (the default) | `Surface(SurfaceSpec)` |

The same two-item taxonomy, written twice, on opposite sides of the front-end split — and
already diverging in shape, since one carries a spec payload and the other is a bare enum.
`organon console patch --kind` is described in the CLI skill as *"a name it resolves, never a
command and never a path"*, which is exactly the right definition; `ArtifactContent` is the
same idea reached independently from the other end.

⚠️ **So the media work does not get to add a kind registry — it has to resolve the one that
is already double.** Adding a third would be the "two resolvers that can disagree eventually
do" failure this repo keeps recording (the store-path rule, the doc-rules table shared by two
hooks, the scene name parsed in one crate and rendered in another). The honest sequencing is
that unifying these two comes *before* the first new kind, not after — it is cheap now, at
two variants each, and expensive once a dozen kinds exist on one side.

That unification is also where the placement question gets answered structurally: a patch is
inline-in-a-terminal, an artifact is inline-in-a-conversation, and both are *the inline
placement* of one thing.

Design constraints on a kind:

- It **declares** its preferred placement, its intrinsic aspect (or that it reflows), and
  whether it is interactive or static.
- It **renders at a size** and must tolerate that size changing every frame during a
  promotion tween.
- It **must not decode on the frame thread** (§6).
- An **unknown kind is refused with the known list**, never approximated — the rule the
  lighting scene protocol already follows, for the reason it follows it: a silent
  approximation is indistinguishable from success.

---

## 6. The two constraints that will actually bite

**Decode goes off the frame thread.** A 4K PNG, a PDF page raster, a video frame — any of
these on the UI thread drops a frame. This needs a worker and a placeholder that says *"not
yet"* honestly rather than showing an empty rectangle. The console's honesty discipline
helps here for once: an exhibit that has not decoded should say so, because a blank box and
a failed decode must not look alike.

**A texture budget with eviction, tied to what already exists.** `surface_budget_bytes` and
`free_portal` are the precedent, and the patch system already evicts a rectangle with the
rows it is pinned to. Exhibits should evict on the same principle. §3 is what makes that
safe.

---

## 7. The ladder is steeper than the wish-list suggests

These were named together and are not peers. The ordering matters because it is the argument
for a kind-agnostic spine: with one, the expensive kinds simply land late and block nothing.

| Kind | Difficulty | Why |
|---|---|---|
| Markdown | easy | pure Rust, no dependency of consequence |
| Images | easy | the `image` crate is pure Rust; animated formats slightly more |
| Audio | moderate | decoding is fine (`symphonia`, pure Rust); the *player* — transport, waveform, docking — and an output device are the work |
| LaTeX | moderate–hard | no strong Rust math typesetter; a subset renderer or shelling out |
| PDF | hard | pdfium/mupdf are large C++ dependencies |
| **Video** | **hardest, by a distance** | decode **plus** audio sync **plus** seeking means ffmpeg or gstreamer |

⚠️ For PDF the repo already has the pattern: llama.cpp lives behind an opt-in feature
(`--with-llm`) so the default build stays C++-free and fast. A heavy media dependency should
follow it exactly rather than becoming a default cost.

⚠️ For **video**, the honest position is that "a good experience watching a clip I just
rendered" may be better served by a **constrained path** — a frame sequence, or handing off
to the OS player — than by embedding a media stack. That is a trade to make deliberately when
it is reached, not a thing to assume away now.

---

## 8. What to build first

**Images as exhibits, with two placements and promotion between them.** The argument is not
that images are easiest; it is that this one tier exercises **every part of the spine** —
kind registry, off-thread decode, texture budget and eviction, placement, promotion — while
needing no heavy dependency. It is also the origin motivation, so it is testable against the
thing that started this.

The gallery and the tap-to-maximise grid fall out for free **if and only if** the exhibit is
a collection from day one. That is the one detail that must not be deferred: retrofitting
"several items" onto a single-item exhibit means touching every kind written before it.

Explicitly **not** in a first tier: the pane refactor (§2), which is its own tier and should
land on a quiet tree; and any kind below "images" on the §7 ladder.

⚠️ **But §5's double registry came first, and it has landed** (#48 Tier 1). It was two variants
each when it was reconciled, which is the only reason it was contained; doing it after a dozen
kinds existed on one side would not have been. The next kind is now a variant in
`organon_core::kind` plus a renderer, which is the property the rest of this document assumes.

---

## 9. Open, and not decided

Stated rather than guessed, per house discipline:

- **How an exhibit reaches the console.** Two candidate paths — an agent sends it via a
  console verb, or the console recognises a file path in a tool result and offers it. The
  second is more magical and more likely to be wrong; neither is chosen.
- **Whether the docked placement is one dock or many.** An audio player docks; if a second
  audio exhibit arrives, does it replace, queue, or stack? Not answered.
- **What promotion does to scroll position.** Posture's tween already has to solve scroll
  anchoring; promotion may inherit that solution or may not, and nobody has looked.
- **Whether a pane can hold a harness *and* an exhibit viewer simultaneously**, or whether
  that is what splitting is for.
- ~~**The re-wrap cost of a layout that changes width.**~~ **Taken 2026-08-13 —
  `doc/console_rewrap_measurement.md`.** It does re-wrap, entirely: egui's galley cache is
  keyed on the wrap width, and nothing culls, so a width that moves by a whole point misses
  on every paragraph in the retained scrollback. ≈ 7 µs per galley laid out against ≈ 0.9 µs
  reused — **6–9× per frame**, i.e. 9.1 ms at a 400-element session and 308 ms at the
  10 000-element cap. **Splitting a pane costs exactly one such frame and nothing after it**
  (7.6 ms at 400, 342.9 ms at the cap). Five options are priced there; none is chosen.

---

## 10. What this document does not claim

Nothing described here has been built, seen, or measured in operation. The measured facts it
rests on are: ConPTY's rewriting of the byte stream (§0, 2026-08-11), and the present
existence of `patch --kind`, the portal, `Body::Artifact`, `session::Artifact` and the
surface budget, all read from source rather than observed running. **Every design claim in
between is an argument, not a result.**
