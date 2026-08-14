# Changelog

Organon was built in the open for about a year before this repository existed, in a
private monorepo, across ~430 changes. **This file is a reconstruction of that arc** —
what got built, roughly in order — rather than a replay of it. Individual PR entries, and
the issue numbers they reference, stayed private with the original.

From here on, this file gets an entry per meaningful change, newest first.

---

## Unreleased

### The last six params between `world.rs` and the plugin crate

organon#49 Tier 4a. `FdtdSource`, `FieldVolSource`, `ColourMode`, `CalColourSource`,
`FieldKind` and `FluxAxis` join core, each with a `Host*` mirror and an element-wise pin —
the T1/T2 machine, run a third time. Eighteen variants in total, against `GeneratorMode`'s
27 alone.

**What this closes.** `world.rs` names 26 things from `crate::params`; every one is a
*value* type and none is `OrganicMathParams`. With these six in core, **all 26 resolve
below the plugin crate.** The parameters are no longer what holds `World` upstream — the
modules it imports are (`cli`, `agent`, `scene_input`, `frame_ring`, `egui_platform`).

📌 `FluxAxis::normal` travelled with the semantic type rather than staying on the mirror,
because `world.rs` calls it (`axis.normal(center)`, twice) and it is pure glam.

⚠️ **`CalColourSource::Auto` carries no `#[name]`**, so nih-plug *derives* its display
string rather than reading one — the only name in this wave written down nowhere. Core's
`as_str` has to state that derived string literally, and
`host_cal_colour_source_mirrors_core` now asserts it explicitly. Get it wrong and the DAW's
dropdown and the CLI's resolver disagree about a name neither file spells out, silently.
### Posture's desktop margin is symmetric — the transcript is centred, not shoved right

The `Form` token `gutter` becomes **`margin`**, and `Form::gutter_margin` becomes
**`Form::content_margin`**, answering `Margin::symmetric(margin, 0)` — the same 90 points on
the left and the right at desktop posture, so the conversation sits centred in its pane. Both
ends and the midpoint are pinned as before (`0` / `45` / `90`), and the terminal end still
answers `Option::None` rather than a zero margin, so the scrollback's walk runs in the scroll
area's own `Ui` with no wrapping `Frame` at all — unchanged by construction rather than by
arithmetic that ought to agree.

**It was left-only because the specification was describing a picture.** The sentence the
tier was built from — "add a narrow empty left margin column, about 90px wide" — was written
to prompt an image generator into restyling a screenshot, and was implemented literally.
Drawn on a real window for the first time, it read as the console pushed off its own left
edge. Renaming the token is part of the fix rather than tidying after it: a field called
`gutter` that produces a symmetric inset is a lie the next reader has no way to catch.

⚠️ **Every posture test passed the whole time**, because they all asserted the *scalar*
(`f.gutter == 90.0`) and none of them asserted the `Margin` it became — which is where the
asymmetry lived. `the_content_margin_is_symmetric_at_every_posture` now walks the axis and
checks the shape at each step: `left == right`, no vertical inset, and the value is the token.

⚠️ **A margin, not a measure.** The text column is whatever the pane has left — 920 points at
the console's default 1100, but 2320 at 2500, which is wider than prose wants. Claude Desktop
caps the *measure* instead. That was considered and deferred, and `SHELL_ARCHITECTURE.md` §1.6
records why it is a bigger change than it looks: **"uncapped" is not a width**, so a cap
cannot be one more scalar lerping between the two ends — every single-token spelling is either
non-monotone at the midpoint or makes the terminal end depend on the window size. The honest
form needs `t` itself and an available width at the call site, and it should wait for somebody
to have looked at the desktop posture on a wide window. Nobody has.
### Slash commands in the console — one command registry, four front doors

A command a **person** types now runs at once, locally, and costs no inference. Typing
`/background slate` into a conversation tab changes the backdrop on the next frame; before
this, the equivalent went to the agent as a message, was understood by inference, was located
by a tool-search call, came back as a tool call, and raised an approval card asking the human
to approve his own command — about **thirteen seconds and a chunk of context for a command he
had already decided on**, measured on 2026-08-13.

Nothing in that chain was a bug. It is what the console's earlier architecture forced: it
composited *around* a harness it did not own, so it had no way to hear a human's intent except
through that harness. The conversation front-end ended that assumption — the console owns the
composer — and nobody revisited the consequence.

**One table, four spellings.** `organon-shell/src/registry.rs` holds the console's vocabulary
as a hierarchy — a group, a verb, and its argument choices — built from the same
`Vec<CommandSpec>` the MCP tool schemas are generated from. So `organon console background
slate` from a terminal, `mcp__organon__console_background` from an agent, and `/background
slate` from a person are three renderings of one definition, and all three produce the same
`cli::ConsoleOp`. A pie menu is the fourth and is not built; the hierarchy is carried
explicitly so that it can be generated rather than restated. All three existing surfaces stay:
they serve callers with genuinely different routes in.

- Typeable now: `/background`, `/rig`, `/block`, `/patch`, `/portal`, `/camera`,
  `/camera.read`, plus the view's own `/surface` (unchanged) and a generated `/help`.
- **The typed line, minus its slash, is the sidecar line** — `/camera reset distance 40` is
  what the CLI already prints as `queued: camera reset distance 40`, so there is no third
  spelling to learn.
- **Still audited.** A slash command is handed to the same dispatch an agent's tool call
  reaches, onto the same sidecar, drained through the same `CommandService` — it leaves a
  `CommandRun` record. It skips the agent, not the discipline, and reports *accepted* rather
  than *applied*.
- **The approval model is untouched**, because it answers a different question — may this
  *agent* act on my behalf — and a person's own keystroke was never that question.
- An unknown command is **refused with the known list** instead of being forwarded as chat, and
  a refusal does **not** clear the composer, so nothing a person typed can vanish. A line that
  merely *mentions* a command still reaches the agent, and `//` sends one that really does
  begin with a slash.
### Console: a tool call that worked stops taking a whole card

The conversation view was *"a list of bevel-bordered status updates"* — five or six tool
calls on a typical screen, each one rendering its full arguments and full output forever, at
full weight. A turn's mechanical work occupied the transcript in proportion to how much work
it was, rather than to how much attention it deserved.

New module `native/organon-shell/src/card_density.rs` (no egui, like `text_diff`). **Success
is quiet; only a departure from normal takes weight.** A settled success becomes one line —
the verb, the object and a magnitude (`Read src/lib.rs · 120 lines`, `Edit … · +3 -1`) — a
consecutive run of three or more inside one turn becomes one row with a count, and a
**failure is untouched: open, bordered, loud, and structurally incapable of being collapsed
or of joining a group.** Nothing is deleted; everything is one click from the card it was.

Three things the design is built around. **An authorised call is never anonymous** — an
approval and its result share only a `toolu_` id, so a gated call keeps its own row and draws
that id. **Nothing above a reader can change height while they are reading** — an automatic
collapse is applied only while the view is following the live edge, and a manual toggle can
only change content at or below the row that was clicked, so scroll position is stable by
construction rather than by compensation. And **a hand outranks the machine permanently**: a
card the reader opened stays open through every later event.

⚠️ **No group row carries a duration**, which the design asked for. `ToolCard` holds no timing
and `conversation.rs` has no clock by design; a number the view timed itself would be its own
stopwatch wearing the agent's voice. A tool with nothing to measure renders **no** magnitude
rather than a zero. `SHELL_ARCHITECTURE.md` §1.1 owns the full rule, and its honesty ledger
records the part that matters: whether the collapsed transcript actually *reads* better is
unverified — nobody has seen it.

### `organon-scene` — the substrate moves below the plugin

organon#49 Tier 3. Five modules — `substrate_scene`, `substrate_materials`,
`substrate_camera`, `substrate_epochs` and `overlay_meta`, 5 972 lines — leave
`organic-math-native` for a new crate carrying **no nih-plug, no wgpu, no egui, no
winit**. `cargo tree -p organon-scene` is the acceptance test, the same bar `organon-core`
holds one layer down. Every `crate::substrate_*::…` path still resolves through a named
re-export.

**Why a third crate rather than more `organon-core`.** Everything here would compile in
core — it adds no dependency core lacks. The split is on identity: core is the *spine*
(the wire format, the algorithm, the param vocabulary) and is the crates.io-published
face, so its public API is a standing commitment; six thousand lines of substrate look and
camera arithmetic makes that commitment larger and its identity vaguer. `organon-render`
was the other candidate and is worse: its own manifest insists, at length, that it is
`world::render` and **not** the world, because #626 conflated those once. This is scene
*state*, not drawing.

⚠️ **Two things deliberately stayed behind, and both were surprises.**

`scene_input` looks like it belongs — same Console Spike lineage, same subject — and
#49 scoped it as zero-coupling. It isn't: **68 of its lines reach `egui`**, because it
turns pointer gestures into `CameraInput`. The original measurement counted upward
`crate::` references and never asked what *external* crates each module named. It travels
with `world.rs` in Tier 4. (`overlay_meta` failed the same grep and is innocent — the hits
were the substring `egui` inside `AxonWaveguide`.)

`substrate_scene`'s and `substrate_materials`'s **test modules** also stayed, relocated to
`native/tests/substrate.rs` byte-for-byte. Their baseline is
`OrganicMathParams::default().to_shared()` — the *plugin's* default parameter set, named
as such in their own fixtures. `Shared::default()` is a deliberately different thing (core
calls it "the web app's helix defaults"), so substituting it would change what every
assertion is measured against **without changing whether it passes**. #626 Tier 3 hit this
exact problem when `math.rs` moved and answered it the same way.

### An `Edit` card stops re-deriving its diff sixty times a second

`tool_card` called `edit_diff` from inside its own body, so **every frame, for every `Edit`
card in the transcript**, `serde_json::from_str` walked the whole arguments blob and
`text_diff::line_diff` re-ran the alignment — and threw both away. The scrollback is not
virtualised, so a card two thousand lines off screen paid in full.

Found while taking the re-wrap measurement, which named it and deliberately excluded it (its
corpus is `Read` cards). So the first job was to measure it, and the result decided the shape
of the fix rather than the other way round.

Measured two ways that share no code — `edit_diff` called directly with no egui, and whole
frames of the real `scrollback` differenced against the same corpus with the cache cleared
each frame. They land within a few percent on all five shapes:

| one `Edit` card | per call | 400 cards, per frame |
|---|---:|---:|
| an ordinary one-line edit | 1.5 µs | 0.12 ms — below the noise |
| a function-sized hunk | 5.6 µs | 0.52 ms |
| the largest `MAX_CELLS` allows | 43.9 µs | 3.9 ms |
| a 400-line common prefix | 78.2 µs | 6.2 ms |
| *a stated one large edit in ten* | — | **2.4 ms**, 15 % of a 60 Hz budget |

🚨 **The common case was never the problem, and that is the finding.** On ordinary edits the
honest answer would have been to leave it alone. The tail is what justified a field: a session
of large edits cost **61 ms per frame — 16 fps sitting still**, and afterwards the mixed
corpus is indistinguishable from a `Read`-only control.

`ConversationPane::diffs` now holds the result, in the idiom the pane already had for
`artifacts`: computed in `scrollback`'s walk, read by `tool_card` (which takes the diff rather
than deriving it), pruned against the transcript beside the artifacts `retain`. `Body::Tool`
moved out of `draw_element` into `scrollback`'s match for the reason `Body::Artifact` was
already there. `edit_diff` itself is unchanged and still uncached — the cache is at the call
site so the pure function stays pure, and a test fails if anyone memoises it as well.

🚨 **Invalidation is by eviction on `Change::Updated`, because `Arguments::complete` is not a
promise of immutability.** A second `ToolCall` for an id that is not yet *resolved* replaces
the arguments text wholesale, so a cache keyed on "complete" would have shown the first
arguments' diff forever under a card displaying the second arguments' path. A fingerprint
cheap enough to take every frame can collide; hashing 58 KB per card per frame costs a large
fraction of a 78 µs saving. The fold already names the element it changed.

⚠️ **One measurement gap is now recorded rather than closed**, in `text_diff`'s own doc and in
§4 of the finding: `MAX_CELLS` is checked *after* the common prefix is trimmed, so a 400-line
prefix costs zero cells, passes the budget meant to stop large inputs, and then allocates an
owned `String` per prefix line before `elide` discards them. It is the most expensive shape
measured and the one that draws the fewest rows. The cache makes it happen once per card
instead of once per frame; it does not make it cheap.

Also corrected in place: `text_diff`'s module doc and `SHELL_ARCHITECTURE.md` both asserted
the alignment "is recomputed every frame" and offered that as what `MAX_CELLS` was sized
against. Both are now false and both are rewritten — the claim was defensible about one card
and never survived being multiplied by a session.

The instrument stays in the tree (`conversation_view/edit_diff_bench.rs`), including a
`Cache::Off` mode that reproduces the old code exactly, so the before-and-after can be
re-taken rather than believed. Full write-up: `doc/console_edit_diff_cost.md`.
### The product is called Organon Console, and the tree now says so

The rename to **Organon Console** was made a while ago and only ever reached the binary's
name; everything behind it still said *Shell*. This finishes it. The crate
`native/organon-shell` is now **`native/organon-console`**, the cargo feature
`shell-edition` is **`console-edition`**, `native/src/shell_main.rs` is
**`console_main.rs`**, the app state `Shell` is **`Console`**, `Edition::Shell` is
**`Edition::Console`** (with `is_shell` → `is_console` and `SHELL_TABS` →
`CONSOLE_TABS`), `ShellApp` is **`ConsoleApp`**, the provisional command `shell.echo` is
**`console.echo`**, the PR label `shell` is **`console`**, and `SHELL_ARCHITECTURE.md` is
**`CONSOLE_ARCHITECTURE.md`** — moved with `git mv`, so its history follows. Every
citation of the old names across `CLAUDE.md`, `ARCHITECTURE.md`, `MIND_ARCHITECTURE.md`,
`CONTRIBUTING.md`, `README.md`, `LICENSING.md`, `doc/`, the `organon-cli` skill, the
`.claude` hooks and `.github/workflows/ci.yml` moved with them. **CI moved in this same
change** — a feature rename that lands without it fails every PR.

🚨 **Three words spelled "shell" live in this tree and only one of them was ours.** The
product; **a shell** (bash, WSL, `cmd`, the program a terminal harness runs); and a
**geometric** shell (`math::outer_shell`, `PhylSurface::Shell`, the Plexus overlay rind,
a free-slip spherical boundary). The third is the largest by far — 149 occurrences in
`math.rs` alone — and `ORGANON_SHELL_TABS=shell-wsl,shell` carries two of the three senses
on one line. Nothing was renamed that a shell or a solid still means; the harness ids
`shell` / `shell-wsl`, the label `Shell (WSL)`, `default_shell`, `shell_dash_c` and
`$SHELL` are all untouched. Organon **Mind**'s `mind_shell.rs` is untouched too: it is a
different product's UI scaffold, not this one's name.

⚠️ **Two things deliberately still say "shell", because both are read from outside this
repository and a rename here does not reach the far side:**

1. **The IPC namespace value `"organon-shell"`** (`organon-core/src/edition.rs`). The
   `organon` CLI joins on that exact string to find a running console, and the launch
   shims set `ORGANON_IPC_NS=organon-shell` to fork a second one into its own namespace.
   It is a **wire identifier, not a name** — the same class of frozen string as
   `Edition::Full`'s `"organic-math"`. The `Edition` variant renamed *around* it, which is
   exactly the distinction: a variant is ours, a wire value is a contract.
   `console_edition_identity_and_tabs` pins the string, so a find-and-replace fails there
   loudly rather than at a user's keyboard silently.
2. **The `ORGANON_SHELL_*` environment variables** — all nine. They are a shipped flag
   surface that the workstation's `organon-console.cmd` / `oc.cmd` shims already set.

A third stayed for the same reason with a smaller blast radius: the private-annex
citations `doc/organon_shell_prd.md` and `doc/organon_shell_buildplan.md` name files in a
tree this repository cannot see, so renaming the citation would only dangle it. Rename the
annex first. And `%APPDATA%\OrganonShell` stays, because an existing install reads it.

⚠️ **One behaviour genuinely changed, and it is not cosmetic.** `CommandService`'s catalog
is sorted by name, so `console.echo` now precedes `session.note` where `shell.echo`
followed it. `catalog_list_spec_and_suggest` caught it and its expectations moved with the
rename — the catalog doing what it says, not a regression.

`doc/arch/topology.md` was brought true in the same pass, which the rename only exposed:
`CLAUDE.md` says that file owns *"the crate graph and what may depend on what"* and it did
not state one. It now carries the graph read from the manifests (five members, the three
leaf crates siblings rather than a stack, `nih_plug` and the window stack confined to the
root crate) and corrects the claim that the console crate is "organon-core + egui only" —
it has taken `serde`/`serde_json`, `dirs`, `portable-pty` and `alacritty_terminal` since,
so the *publishability* claim survives and the *smallness* claim did not. Its module list
gained `theme.rs`, `posture.rs` and `prefs.rs`, and records that `kind.rs` sits in
`organon-core` rather than here for a topology reason. `.claude/hooks/doc-rules.sh` now
makes topology.md accountable for `native/organon-console/Cargo.toml` as well, and the
now-permanently-absent `SHELL_ARCHITECTURE.md` entry was removed from
`.claude/settings.json` — a listed doc that can never exist is false reassurance.

**Verified:** `cargo test -p organon-console --lib` 526 passed / 1 ignored,
`cargo test -p organon-core` 556 passed, `cargo check --features console-edition --bin
organon-console` and `cargo check --tests -p organic-math-native --features
console-edition` both clean, and all four doc hooks run green. Not seen running — no
window was opened.

### `cli.rs` and `agent.rs` stop needing a plugin host

organon#49 Tier 2. Both files reached `nih_plug::prelude::Enum` to do three things: list
an enum's variant names, look one up by index, and count them. None of that is a
plugin-host concern — it is *"this enum has an ordered set of variants with display
names"*, which the wire format already requires of these types. `organon-core::params`
now owns that vocabulary as **`IndexedEnum`** (`all` / `label` / `labels` / `index` /
`from_index`), and both files use it instead.

Four more semantic enums move to core to make that possible — `SurfaceMode`,
`MaterialType`, `CamPath` and `Palette` — each with a `Host*` mirror and a
`host_*_mirrors_core` pin, exactly as Tier 1 did. Eight now live in core.

⚠️ **The scope was set by a transitive fact, and the issue had under-scoped it.** Tier 2
was written as "a small index trait"; it needed four enum moves as well, because
`cli.rs`'s three selectors are generator/surface/material and `agent.rs`'s feature
fingerprint adds palette and camera path. The reason those two files matter at all is
that they sit on `world.rs`'s dependency path — `world.rs` imports `agent`, and
`shell_main.rs` imports both — so they have to travel to a lower crate when Tier 4 moves
`World`.

📌 **The tier's acceptance bar is now a test, not a grep.**
`cli_and_agent_are_free_of_nih_plug_outside_tests` reads both files and fails on any
`nih_plug` outside the `#[cfg(test)]` block. `cli.rs`'s test block is exempt on purpose:
`engine_ranges` walks the plugin's own `Params` tree through `ParamPtr`, which is
host-side by nature — Tier 2 predicted that risk, and it landed on a test rather than on
shipped code, where it blocks nothing.

Nothing about the built product changes: same variants, same order, same display names,
same wire indices.
### A palette and a posture a person can reach — `organon console theme` / `posture`

The Console has shipped four palettes (`organon`, `light`, `dark`, `chocolate`) and a
terminal↔desktop posture axis for a while, and **a human could select none of them**:
`shell_main.rs` hardcoded `Theme::organon()` and `Posture::TERMINAL`, and the preferences
file nothing had ever written was never read. Two verbs and a startup read close that.

**`organon console theme <name>` repaints the running window and stores the choice.** Live
is the requirement rather than a convenience: what is being judged is a wash of colour
across a window full of real text, and that judgement is made by looking back and forth,
which comparing four palettes across four relaunches destroys. The write goes to
`preferences.json` — the first thing the Console has ever persisted on a person's behalf —
so the next launch opens where you left it.

**`organon console posture <terminal|desktop|0.0-1.0>` snaps.** The axis is a scalar and
every form token lerps along it, so a bare `0.5` is a real console rather than a rounding of
one end. ⚠️ There is no animation, deliberately: a tween moves the transcript's wrap width
continuously, and `doc/console_rewrap_measurement.md` prices one such width change at
~7.6 ms at 400 elements with five options and no decision taken. A snap pays that once, in a
frame nobody reads as a jump. ⚠️ The posture is **not** remembered — a palette is what the
console is made of, a posture (at this tier, undrawn on any real screen) is a view you take
to look at something, and closing the window is a free undo worth keeping.

**Startup precedence: `ORGANON_SHELL_THEME` → `preferences.json` → `organon`.** 📌 That
first rung **amends a recorded decision**. `SHELL_ARCHITECTURE.md` §1.5 said no environment
variable may override a stored preference, and its reasoning — a variable baked into a
launch shim wins *silently*, which is the evaporation the preferences file exists to end —
still stands. It was taken when nothing could select a theme at all, and it named its own
escape hatch: a one-launch override "belongs in a CLI flag that can say so in the console's
own output". `organon-console` has no flags; it is launched by shims, and an environment
variable *is* its argument surface. So the objection is answered directly instead: the
override **announces itself every launch**, naming the variable, the palette and the stored
palette it stands in front of, and it **never writes** — unset it and the stored choice is
back. A loan, not a takeover. §1.5 carries the amendment.

⚠️ **An unknown name is refused out loud and never approximated**, at both ends: `bin/ctl.rs`
builds its `--help` list and its clap gate from `Theme::NAMES` itself, and the console
resolves again on arrival for a line hand-written onto the sidecar. At startup an unknown
name **falls through to the next source** rather than resetting to `organon`, so a typo in a
shim cannot silently discard a stored choice.

⚠️ **Both verbs are routed before `console_step`**, beside `block`/`patch`/`portal`/`camera`.
Everything after that point reaches `record_look_change`, which snapshots the backdrop into
the Tier-4 epoch ledger — and a palette is not a substrate look. Neither verb writes
`backdrop_source` or changes a pixel behind the glyphs, so banding the transcript for one
would record a change that did not happen.

`console.theme` and `console.posture` join the dispatch catalog, and `--help` now lists both
verbs, `ORGANON_SHELL_THEME`, and **both** scrim floors — quoting one was true exactly as
long as no palette could be selected.

🚨 **Still nobody has seen `light`, `dark`, `chocolate` or any `t > 0`.** This removes the
obstacle, not the gap: `cargo test -p organon-shell --lib` is 534 green and the two
`shell-edition` checks are clean, which is a claim about tests and not about a window. The
first `organon console theme light` anybody types is also the first real exercise of the
preferences file — store-root resolution, `create_dir_all`, temp-then-rename, and the read
back at the next launch. A failed save says so on the console's own stderr and nowhere else.

### The three enums standing between `world.rs` and a Console that isn't a plugin binary

`GeneratorMode`, `BoidsForm` and `OscDivision` move to `organon-core::params`, joining
`FuncName` and `ParamValues`. Each keeps a `Host*` mirror in `params.rs` carrying
nih-plug's `#[derive(Enum)]`, because the orphan rule forbids the native crate from
implementing a foreign trait for a foreign type — the `HostFuncName` split from #626 T3,
applied three more times.

The reason is organon#49: **Organon Console is currently a GPL-3.0-or-later binary of the
VST3 crate.** `shell_main.rs` lives in `organic-math-native`, so the Console links
nih-plug and inherits its licence from a plugin binding it never calls. `world.rs` is what
has to move below the plugin crate for that to change, and these were the last three
things it reached for through `crate::params` — the other two were already in core.

⚠️ **Nothing about the built product changes.** The variant lists, their order, their
display names and their wire indices are identical; the plugin's automation lanes and
every saved preset are untouched. Each pair is pinned by a `host_*_mirrors_core` test
comparing the two lists element-wise by name in both directions, because the index **is**
the wire format and a same-length reordering is the failure that silently recalls the
wrong generator. `GeneratorMode`'s once-re-seated ordinals (`None` = 17, `AxonWaveguide`
= 18, `NeuralField` = 19) get a test of their own that asserts by name, not by position.

`organon-core/src/params.rs`'s "what deliberately did NOT move" note said `GeneratorMode`
stays, and it was right on the reason it gave — `math.rs` only needed it in a test. That
note is now rewritten rather than deleted: the reason was about `math.rs`, and `math.rs`
was simply never the only caller that mattered.
### Three palettes beside `organon` — `light`, `dark` and `chocolate`

`Theme` gains three constructors and a `Theme::by_name` resolver. `organon` stays the
default and is byte-unchanged. Nothing selects a palette yet: no picker, no CLI verb, no
startup read — `by_name` is the seam a picker and `prefs.rs`'s stored `theme` name will
share, and an unknown name is `None` rather than a panic or a substitution.

Two things had to move first, because a palette alone could not have reached either.

**The legibility scrim's floor is the palette's, not one constant.** `SCRIM_FLOOR = 96` is a
mandatory near-black wash over the whole rect whenever a backdrop is live, so a light theme
was not reachable by swapping colours — it would have sat under a compulsory dark veil
however its fields were set. The floor is now `Theme::scrim_floor`, and `term_view` carries
two: `SCRIM_FLOOR = 96` for a dark page, `SCRIM_FLOOR_LIGHT = 192` for a light one. ⚠️ PRD
§4.6's rule was always *the glyphs stay legible*; what is dropped is the assumption that
legibility means darkness. **No setting can still cross a floor** — `ORGANON_SHELL_SCRIM` is
clamped up to the active palette's, and the exhaustive test now runs over every palette's own
answer rather than one constant.

**egui's own chrome is derived from the palette.** `set_visuals(egui::Visuals::dark())` was
one hardcoded call colouring sliders, popup frames, the `TextEdit` selection wash and
scrollbars — roughly half the pixels, which would have left `light` reading as broken rather
than as light. `Theme::visuals()` derives them; ⚠️ for `organon` it returns `Visuals::dark()`
byte-for-byte, pinned by test, so adding palettes cannot restyle the console that ships. It
writes colours only — corner radii, widget expansion, stroke widths and shadows come from the
egui base untouched.

Each spec names about ten roles and `Theme` has about fifty fields; every derived field
carries its rule at the site. ⚠️ Notably **none of the three has an amber** (no spec names
one, so "a tool is running" is primary text), and `ansi16` is **chosen, not specified**, for
all three and marked so — the specs were written against the conversation view and say nothing
about a terminal.

⚠️ **Nobody has seen any of them.** 508 tests green, `cargo check --features shell-edition`
clean — that is the whole claim. A palette that passes its hex test can still look wrong.
### Posture — terminal ⟷ desktop as a second axis, orthogonal to the palette

Organon Shell gains `posture.rs`: a scalar `t ∈ [0,1]` and the fourteen form tokens
resolved at it. The theme is what the console is *made of*; posture is *how it holds
itself* — flush and tight and square like a terminal, or inset and open and ruled like a
desktop document. The two are independent on purpose, so `organon` at desktop posture and a
light palette at terminal posture are both real consoles.

It is implementable rather than merely appealing because **every form token is a scalar,
and scalars lerp**: the desktop state is not a second renderer, it is the same draw code
reading different numbers. `Form::at(t)` interpolates the gutter, the corner radii, the
paddings, the line height, the card gap, the label tracking and three alphas; `Shell` holds
the `Posture` beside its `Theme` and `redraw` resolves one `&Form` per frame.

- 🚨 **Nothing on screen changes.** The console ships at `Posture::TERMINAL`, and every
  terminal-end value was read out of the code before it moved — pinned by
  `form_at_terminal_is_the_form_that_shipped`, with the source of each in its assertion.
  The two `Option` returns (`gutter_margin`, `body_line_height`) make that structural: at
  `t = 0` the scrollback wraps in nothing and the text is laid out by the font, exactly as
  before, rather than by a number that ought to agree.
- 🚨 **Posture owns the scalars; the palette owns whether a card has a visible edge.** The
  four-sided border fades out and a left rule fades in over one shared lerp, with no
  per-theme branch at any draw site — a palette that separates surfaces by fill alone gives
  the new `Theme::card_left_rule` zero alpha. The rejected alternative, a
  `Box | LeftRule | None` enum per theme, puts a branch in every card draw and makes the
  tween discontinuous where the enum flips.
- ⚠️ **One token's terminal end disagrees with the spec, and is recorded rather than
  reconciled.** The design gives square corners at terminal posture; the console has drawn
  `CornerRadius::same(6)` since its cards were written, so squaring them would be a visible
  change at the posture that is supposed to *be* today's console — and this tier had no
  window to check it in. The shipped terminal end is `6`; flipping it is one number and a
  matching one in the test.
- ⚠️ **Font family and label case are left out rather than faked.** There is no half-mono
  face and no half-capital letter, so neither is a field: an interpolation that claimed
  otherwise would be a lie that compiles.
- No animation (that is a later tier — `t` is set once and held), no new palette, no
  ordinals in the gutter, and the terminal host is untouched: a character grid's form is
  the font's, with no padding or corner for a scalar to move.
### One kind registry, where the console had two (#48 Tier 1)

The console has two front-ends, and the same two-item taxonomy had been written **three**
times: `cli::PatchKind` (`Scene`/`Panel`) on the wire, `block_panel::PatchContent`
(`Scene`/`Panel`) as the terminal's paint target, and `conversation::ArtifactContent`
(`Surface`/`Panel`) in the conversation. Reached independently, in different crates, and
**already diverged in spelling** — the shape this repo keeps recording as "two resolvers that
can disagree eventually do". Adding a media kind would have made it four.

`organon_core::kind::Kind` is now the one vocabulary, in the spine because that is the only
crate all three copies can see (`cli.rs` is the root crate, the other two are `organon-shell`)
and because a closed set of words needs no host, GPU or UI.

**One vocabulary, two payload carriers — one per placement.** A patch is inline-in-a-terminal
and an artifact is inline-in-a-conversation; `PatchContent` and `ArtifactContent` each answer
`kind()`, and each has a test that fails on an arm with no kind or a kind with no arm. They
are deliberately **not** merged: a patch's panel owns live widget state pinned to scrollback
lines, an artifact's is a description the view keys state off, and the `Kind` on the wire must
be able to carry neither — `doc/console_patch_protocol.md`'s whole point is that a program
which can print cannot drive the machine. The kind is the half that is genuinely shared; the
description is the half that genuinely differs.

⚠️ The design doc this came from (`doc/console_view_paradigm.md` §5) counted **two** copies.
The third turned up while unifying them, and it is the one that settled the shape — a two-way
merge of a bare enum and a payload-carrying one would have had no obvious answer to "where do
the payloads go", while three copies made the placement/vocabulary split read off the tree.

Inert by contract: `scene` and `panel` behave exactly as before, and `--kind scene` is
unchanged in `--help`, in the `organon-cli` skill and on the sidecar wire.

- **An unknown kind is refused with the known list.** `Kind::resolve` returns
  `` `hologram` is not a kind — known kinds: scene, panel `` — no nearest match, no case
  folding, no prefix rule. The three arrival paths now give three deliberate answers: clap
  refuses at the CLI boundary, the command service refuses with that sentence (an agent there
  has no other way to ask what the build can draw), and the sidecar skips the line in silence,
  because nobody is listening and a guess would paint the wrong object into a rectangle
  someone else's output is holding open.
- ⚠️ **The two *words* were deliberately not unified.** `scene` is public CLI surface and
  `/surface` is a composer command a human already types, so an inert tier could break
  neither. `ArtifactContent::Surface` answers `Kind::Scene` through one documented, tested
  crossing; `SHELL_ARCHITECTURE.md` §1.1 states what unifying the spellings would cost, so the
  next tier decides rather than rediscovers.
- **`Kind` has no `Default`.** The "a `patch` line with no third word means `scene`" rule is a
  wire-compatibility fact about that one verb, not a claim that `scene` is the natural kind, so
  it stayed behind as `cli::PATCH_DEFAULT_KIND` — the conversation front-end must not inherit
  an answer to a question it never asked.
### The re-wrap cost is measured: 6–9× per frame, and nothing culls

`doc/console_rewrap_measurement.md`, with the instrument that produced it at
`native/organon-shell/src/conversation_view/rewrap_bench.rs`. Posture's tween (issue #38)
and pane splitting both change the conversation transcript's available width, and both were
scoped without knowing what that costs; `console_view_paradigm.md` §9 had said in as many
words that nobody had taken the measurement.

**egui's galley cache is keyed on the wrap width** (`epaint-0.33.3/src/text/fonts.rs:884` →
`text_layout_types.rs:439`), so a width that moves by a whole point is a *total* miss across
the entire retained scrollback — and nothing culls, because `egui::Label::ui` builds its
galley before it tests `is_rect_visible`. Measured through the real `scrollback` draw path,
headless, release, on `ORGANON-ONE`: **≈ 7 µs to lay out a wrapped galley against ≈ 0.9 µs to
reuse one.** Per frame that is 2.4 ms at a 100-element session, 9.1 ms at 400, 50.5 ms at
2 000 and 308.6 ms at the 10 000-element cap. A *single* change — pane splitting, or snapping
a tween at its end — costs exactly one such frame and nothing after it.

📌 **The larger finding is the steady-state column.** The transcript is not virtualised, so
its layout cost is linear in scrollback length with nothing animating at all: 8.1 ms per frame
at 2 000 elements, 51.6 ms at the cap. The tween does not create that; it multiplies it by
eight. Window-resize drags already pay the animating figure today.

Five options are priced — tween as designed, animate the chrome only, snap once, quantise the
tween, virtualise the scrollback — and **none is chosen**; that is downstream of the number,
not contained in it. §8 lists what was not measured, including anything on a GPU, the
`Edit`-card diff that recomputes every frame, and posture itself, which does not exist yet.

Two tests hold the parts that can rot in silence and run in the default suite: one pins
epaint's cache keying against a version bump, one pins that the whole scrollback is laid out
rather than the visible slice. The benchmark is `#[ignore]`d —
`cargo test --release -p organon-shell --lib -- --ignored --nocapture rewrap`.

### A CRLF checkout silently un-installs the `organon-cli` skill

`SKILL.md` is now pinned to LF in `.gitattributes`. Claude Code reads a skill's YAML
frontmatter from between two `---` fences and its parser does not accept `---\r\n`, so on
a Windows working tree the frontmatter fails to parse and the skill degrades without
reporting anything: `name` falls back to the directory (so it still appears in
`slash_commands` and looks installed), `description` falls back to the body's first
heading, and the skill is **never offered to the model**.

Measured against a real session with three controls in the same directory: `organon-cli`
was the only one of four skills with CRLF and the only one missing from the offered
`skills` array; converting a byte-identical copy to LF took the count 22 → 23 and
restored its real description. Ruled out first, each by experiment: the junction, file
size, description length, a BOM, duplicate copies, a colliding slash command, a disabling
setting, and the skill's own name.

⚠️ The index was already LF (`i/lf w/crlf`) — nothing was ever committed wrong, which is
why review could not see it. This is the third fix in the same place: the skill was a git
symlink (unusable on Windows), then a real tracked file, which is what gave it a CRLF
working copy. Each fix uncovered the next failure.
### The portal recon lands, with the one claim the tree has since falsified marked as false

- **`doc/console_portal_recon.md`.** The site-by-site investigation the portal was built from —
  written before it existed, merged after it. `SHELL_ARCHITECTURE.md` has cited it by branch name
  since §1.2 landed; that citation now points at a file in the tree instead of at a branch, which
  is the whole reason to land a document nobody will change again.
- 🚨 **"Immersive is landed" was the recon's first headline row and it is wrong, so it now says
  so.** The backdrop's *rendering* really is already there; the inference — that immersive is
  therefore nearly free — is not, because `paint_portal` paints the portal **over** the front-end
  and immersive needs its texture **under** the glyphs with the scrim over it, through
  `term_view::draw`'s `Some(bands)` seam that the portal does not touch. A merged investigation
  asserting something false is worse than an unmerged one: it acquires the authority of the tree.
  Corrected in the headline table and argued in §1.1, with the measurement kept — it was right,
  and it is what immersive will be built out of.
- **Amended in place, never overwritten**, on the execution-plan convention this repo already
  uses. `render_source`'s quoted body is marked superseded by `engine_plan` (the third input §6
  predicted, added where §6 said it belonged); `surface_budget_bytes should gain a portal term` is
  marked solved differently, by `free_portal` reporting the two quantities separately; §4's
  texture-churn finding is marked **confirmed live**, since `render_portal` carries the identical
  free-and-realloc body and a window-resize drag now exercises it every frame.
- **Which recommendations were taken is recorded at the top**, so the document can be read as
  history rather than as a to-do list: §2, §3, §5, §6's risk 1, §8 and §9 were adopted as written;
  immersive, full screen, the animated grow, `scene_viewport`'s `Sense` parameter and
  state-conditional Escape remain the open work. §7's four states and five events shipped as two
  and three, deliberately — *an event nothing can raise is an untested arm pretending to be a
  design*.
- **No source file changed, and no cargo run was needed.** The branch is documentation only.
### Console — the first thing it remembers about you

- 🚨 **The console persisted nothing a user chose.** Measured by reading the crate: the only
  writer was the append-only session event log, which is evidence of what happened rather than
  a statement of what is wanted; the only user-*configuration* path was a **read with no
  matching write** (`harnesses.json`); everything else was an `ORGANON_SHELL_*` variable
  sampled once at startup. A colour-theme picker on top of that could offer a choice and lose
  it at exit, which makes the picker pointless.
- **`prefs.rs` — `preferences.json`, at the store root beside `harnesses.json`.** A struct with
  serde defaults, so adding a preference is one field and never a migration: an older file
  missing a newer key still loads, and a newer file with an unknown key does not break an older
  binary. It ships holding the theme **by name**, a plain `String`, so storage and the `Theme`
  type being built separately stay independent.
- **The store root is `SessionLog::store_root()`, called rather than re-derived.** Resolving it
  again through `dirs` would satisfy the letter of the one-resolver rule and still be wrong —
  two resolvers that can disagree eventually do, and the failure is preferences written beside
  a `harnesses.json` the console reads from somewhere else.
- ⚠️ **A write cannot destroy what is already stored**: temp file in the same directory, renamed
  over the target. Same directory because a rename is only atomic within one volume. The reason
  it is in version one rather than later is the read posture — a half-written file fails to
  parse, and "malformed ⇒ no stored preferences" would then silently reset everything the user
  had.
- ⚠️ **Never written with a BOM**, because `serde_json` refuses one outright and the total read
  posture turns that refusal into silence: a file that is present, looks right in an editor, and
  does nothing. Both halves are pinned by test.
- 📌 **No environment variable overrides the file, deliberately.** An override would defeat the
  point: a pick stores correctly, then a variable baked into a launch shim wins silently next
  launch — indistinguishable from the evaporation this exists to end.
- Nothing reads it yet. This is the storage half; the picker is a separate change.
### Organon Shell — an agent can ask where the camera is

- 🚨 **`console.camera.read`: the console's first read verb.** The console gave an agent
  camera *verbs* and no way to ask what the camera was doing, so framing an object cost five
  round trips — set a distance blind, shell out to `organon snap`, read the PNG back off disk,
  judge it, go round again — each with its own approval prompt. Measured live, 2026-08-13. The
  framing verbs are absolute *because* nothing could read; a read is what makes a relative move
  computable at all. One call now returns the three axes, whether anything on screen is showing
  them, who moved them last, and whether a hand is holding them.
- 🚨 **It reports the camera, never the last command.** A hand on the portal outranks an
  agent, so the value an agent last set is routinely *not* where the camera is — echoing it
  back would be a lie the console told confidently. `Shell::redraw` publishes from
  `World::camera_framing()` (the world's own three fields, after its own clamps) at the one
  point in the frame where both writers have run: the agent's drained framing and the hand's
  drained gesture.
- **A separate verb, not a zero-argument `console.camera`.** Every axis on the write is already
  optional, so `{}` is a shape it can be called with — and it currently earns *"needs at least
  one of […]"*, the right answer to a model that forgot its arguments. Overloading would turn
  that mistake into a silent success, and would hand the approval layer one name for two acts
  that deserve different answers to "may I?".
- **MCP only, deliberately — the CLI still cannot read.** The MCP server runs inside the
  console process, so it has somewhere for an answer to arrive; `organon console …` is
  fire-and-forget with no return path and giving it one needs a request/reply sidecar that is
  not built. So the served table is `mcp_specs()` = `console_specs()` + this one verb, and a
  test pins that the difference is exactly that.
- **Small honesty rules, each with a test.** `hand_holds` is settled at *read* time, not
  publish time (the hold is two seconds; a snapshot can be older). The axes are widened
  exactly rather than rounded, so a caller can write one straight back and land on the same
  `f32`. A non-finite axis is omitted rather than serialised as `null`. And a cell nobody has
  published to answers as a *failure* — before the first frame there is genuinely no
  measurement, and `{"yaw":0,…}` is a viewpoint a caller would act on.
- ⚠️ **Nothing here has been seen running.** `cargo test -p organon-shell --lib` (486) and
  `cargo check --features shell-edition --bin organon-console` are green; that an agent's
  reading matches the shot on screen is unverified, and is in the honesty ledger.

### The doc-coherence hook stops crying wolf, and starts watching the console's doc

- 🚨 **`doc-coherence.sh` fired on every single Stop, and had done for as long as §18 has
  existed.** Its duplicate-table-key check was scoped to the whole FILE, so
  `ARCHITECTURE.md` naming the same eleven `World` clusters in an inventory table and again
  in a binding-measurement table read as eight stale rows. Both tables are correct; the hook
  was wrong. A hook that always fires is worse than no hook — it is the thing that teaches a
  session to dismiss hooks in general, including the ones that are right.
- **Scoped per table, where a table ends at its next `|---|` separator row — not at the
  first non-table line.** That distinction is the whole reason file scope was chosen
  originally: the #593 T3 defect hid behind a second defect, a stray `---` that split §19's
  file map in half and carried the two duplicate rows into opposite halves. A stray rule
  opens no new header, so both halves stay one scope and the pair is still caught, while two
  genuinely separate tables — which always carry their own separator — do not collide.
  Verified against that exact shape.
- **`SHELL_ARCHITECTURE.md` and `CONSOLE_ARCHITECTURE.md` are both checked now**, and only
  one of them exists at a time. `[ -f "$f" ] || continue` was already in the loop, so a
  listed-but-absent doc is a silent skip and the list needs no edit when the rename lands.
  Shell's living-state doc had never been on the list at all.
- Findings now carry their line numbers straight out of the awk pass rather than a second
  `grep` over the file, which is also what makes per-table line numbers possible.
### Console — colour became a value the console owns, and nothing on screen moved

- **`organon-shell/src/theme.rs`: one `Theme`, a plain struct of `Color32` fields.** The
  console's palette was ~50 `const Color32` declarations across six files plus a handful of
  literals written inline at the draw site. Most already carried a *semantic* name — `RUNNING`,
  `CONTEXT_ARC`, `COMPOSER_EDGE_DEAD` — so the hard half of theming was done; what a `const`
  cannot do is hold a second answer. `Theme::organon()` is the look that shipped and the only
  palette this build has.
- **One owner, no globals.** `Shell` holds it and every draw site gets `&Theme` as an argument
  — no `static`, no `thread_local!`, no `OnceCell`. That is the point rather than an
  aesthetic: a palette reachable from anywhere stops being state, and a per-tab theme or a
  live preview then becomes a rewrite instead of a second value.
- 🚨 **The pinning test is the whole safety net, and its values were read out of `main` before
  a line was moved.** A wrong shade compiles and draws, so nothing else in the suite could
  tell whether the extraction changed one. `theme_organon_is_the_look_that_shipped` asserts
  every field, `ANSI16`'s sixteen entries and `PANEL_FILL`'s premultiplied alpha included.
- 🚨 **Roles that share bytes today are kept as separate fields.** `term_fg`, `human_text`,
  `tab_active`, `tab_menu_installed` and `panel_text` are one colour in five roles;
  `context_arc_high` was written `= MODE_ALERT`; `timeline_status_denied` equals
  `timeline_status_failed`. Deduplicating by value is exactly what would stop a second palette
  diverging — a lighter one almost certainly wants a terminal foreground and a human's typed
  line to part company. A second test asserts the coincidences *and* the separation.
- **Four things stayed out of the theme because they are not taste**: the scrim's *alpha*
  (PRD §4.6's floor is an instrument — only its three colour channels moved), the xterm
  256-colour cube and greyscale ramp (indices 16..=255 are a standard, not a palette),
  truecolor and OSC values a running program sent, and `Color32::WHITE` at the five
  `painter.image` calls, where it is the identity multiplier rather than a colour.
- ⚠️ Green and ready to deploy, **not** seen: no window has been opened on it. See
  `SHELL_ARCHITECTURE.md` §1.4 and its honesty ledger.

### Console — one document that says what the console is, and what has not been looked at

- **`doc/console_overview.md`.** The console's argument, its shape and its status now exist in
  one place instead of being spread across an execution plan, three protocol docs, a demo
  script, a 2000-line living architecture doc and two issues. It is written for a reader who
  knows Organon thoroughly and knows nothing about the console — the engine, `Shared`, the
  editions and the CLI are assumed rather than re-explained — because that is the reader who
  keeps arriving. `SHELL_ARCHITECTURE.md` stays the authority and the overview says so.
- 🚨 **Four status words, used on every capability, and the point is that they do not
  collapse into one another.** *seen* (a person watched it work on real hardware), *unseen*
  (built, green, and nobody has looked), *declined* (deliberately not built, reason recorded)
  and *planned*. The honesty ledger already tracks exactly this distinction; a feature list
  that flattened it would be the most useful-looking and least honest thing that could be
  written about the console, since the unseen list is currently longer than the seen one.
- **Two live doc disagreements were found by reading and are named in the overview rather
  than silently picked between.** The honesty ledger still says *"no GPU has seen Tier 2's
  materials"* while the demo script records them checked material-by-material on 2026-08-11;
  and the demo script's beat 8 still says the approval card has not been seen on screen while
  the ledger records a human driving it — which is how the 60 s client deadline was found. In
  both cases the later record is right and the earlier one was never revised. The overview
  says which to trust and that the other wants fixing; neither doc is edited here, because a
  paper is not the place to quietly change what another document claims.
- **Nothing else moved.** No source file, no test, no build. `cargo test -p organon-shell
  --lib` was run to source one number the document quotes.

### Console Spike — a dispatched agent's card says what it is doing while it does it

- ✏️ **The subagent lifecycle the CLI has always sent is rendered.** A `Task` card used to
  say "running" and then nothing at all for eight to sixteen minutes — the agent's whole
  working life as a spinner. Claude Code narrates that life the entire time, on five
  undocumented `system` subtypes (`task_started`, `task_progress`, `task_updated`,
  `task_notification`, `task_summary`), and the console could not see any of them: they are
  **main**-scoped, with no `parent_tool_use_id` key at all, and §5.9.3 rule 5 correlates on
  exactly that key. All five decoded to `Notice` and drew nothing. **Rule 5b** is the second
  correlation they need. The card now carries a `task` row — the agent's own gloss
  ("Reading one.txt"), its last tool, its tool count, the harness's elapsed and its tokens,
  and a terminal status. Thirteen lines of the real capture that used to draw nothing.
- 🚨 **Two of the five do not correlate the way every doc in this tree said they did**, and
  the wrong reading fails silently. `task_updated` carries a `task_id` and a `patch` and
  **no `tool_use_id`** — so keying on that field alone, which is what the standing
  description licensed, drops **every status transition in the stream** while looking like
  it works. And `task_summary` carries neither key, only a nullable `detail`: it is a gloss
  of what the *session* is doing, belongs to no card, and stays unmapped. The key is
  `task_id`, paired against a card by any line that states both. Four docs corrected.
- 🚨 **The family reaches depth 2, where every other subagent line on this wire stops at
  depth 1** — a finding the fixture had to be read a second time to see. A nested agent
  whose `assistant` and `user` lines are never forwarded has its whole lifecycle forwarded,
  naming a call that exists only as a *step* inside its grandparent's log. Merging that
  would have made the outer card read "Reading one.txt · 1 tool · completed" — the
  grandchild's work in the parent's voice, while the parent was still going. **Declined and
  counted**, because a card holds one progress value with nowhere to record a depth. That
  counter reads non-zero on a *healthy* nesting fan-out, which is why it is not the orphan
  counter beside it. ⚠️ The capture's nested task sends **four** `task_*` lines and
  `Stats::nested_subagent_progress` reads **3** — a split, not a discrepancy: its
  `task_started` arrives one line *before* the `tool_use` block that creates its card, so
  that one is counted by `orphan_subagent_progress` instead. **3 here + 1 there**, each half
  pinned by its own test.
- 🚨 **This does not soften §5.9.1, and the row is built so it cannot drift into implying
  that it does.** Progress metadata is not token deltas — not one character of the agent's
  own prose is on these lines. No caret, no partial text, nothing that suggests live prose.
  `MapStats::subagent_stream_events` still reads **0** on the real capture and is still the
  standing canary. The elapsed shown is the **harness's** stopwatch as of its last line and
  is frozen between them: `conversation.rs` has no clock by design, and a ticking one would
  be the view's own arithmetic in the harness's voice, still counting for an agent that had
  quietly died.
- ⚠️ **One source per fact, because the two disagree.** An `Agent` `tool_use_result` carries
  its own `totalTokens` / `totalDurationMs` / `totalToolUseCount`. The durations and tool
  counts match the `task_*` figures exactly; the **token totals do not** — 62 949 against
  62 951, and 63 564 against 63 803, the result being struck later and counting output the
  notification had not seen. Both are honest, which is precisely why taking both would be
  wrong: one card's token count would jump at completion with nothing to explain it. Only
  the `task_*` stream is read — the one that exists while the card is otherwise silent.
- ⚠️ **A progress line for a card the transcript no longer holds is counted and dropped**,
  the one place this tree declines `orphan_results`' keep-it-anyway precedent. A step
  carries content that would be lost; a progress line carries nothing not either restated
  by the card's own arguments or superseded by the next line — and the card the orphan path
  would open opens `Running`, which is exactly wrong for the `task_notification` most likely
  to outlive its card. A card confidently disagreeing with its own header is worse than the
  silence this change exists to end.
- 📌 **`MapStats::unmapped` kept its name because it kept its meaning.** Its population went
  19 → 6 on the capture; "we drew nothing for this line" is still exactly what it says.
  Contrast `subagent_dropped`, which was *removed* rather than renamed when its sense
  reversed — a counter telling the truth about a smaller set is a different thing from one
  whose name has become a lie. (The 19 is also now pinned with its arithmetic: 19 `system`
  lines less the `init` that maps, **plus** a `rate_limit_event` that is not a `system` line
  at all.)
- ⚠️ **Nobody has seen this on screen.** Every claim is pinned by tests against the real
  capture and every glyph in the row is one `step_mark` already measured present in Hack —
  but the last time a subagent card changed, it took a human looking to find that its
  marker was tofu, and no replay could have. 461 tests in the compositor lib, from 443.

### Console Spike — the console serves its own verbs, and can be told to stop asking

- 🚨 **The bug was one argument, and it had been there for weeks.** The console built its
  in-process MCP server as `McpServer::new(&[], …)` — an empty spec table — so it answered
  permission requests for everything the agent did and exposed **zero** capability tools.
  The consequence was visible on screen: to open the portal, an agent ran
  `./organon.exe console portal open` through `Bash`, spawning a second process to send a
  message to the process it was already living inside, and the approval card asked *"may I
  run this shell command"* instead of naming a capability. Every piece needed already
  existed — `ToolEntry::from_spec`, `input_schema`, `set_tools`, argument checking against
  the same `CommandSpec` that generated the schema, and `console_specs()` on the other side.
  Nobody had joined the two ends. **456 tests in the compositor lib, from 443.**
- **What the joining actually needed, since it is not one line.** A dispatch has to act on a
  console verb from the *serve thread*, while the `CommandService` that validates one borrows
  the session log on the *UI thread* — the constraint the empty table was standing in for.
  The answer is neither a channel nor a second service: `SidecarDispatch` converts the tool's
  arguments through the **same** `op_from` the CLI path uses and appends the line to the
  console's own command sidecar, which the frame loop already drains through that very
  service — validation, audit record and apply, unchanged. One vocabulary, one audited apply
  path, one process. ⚠️ So the tool returns `{"accepted": "portal open"}`, not "applied": the
  op lands on the next frame, and a post-validation failure reaches stderr rather than the
  model. That is the price of reusing the audited path instead of building a second one.
- **The vocabulary is handed down, for the reason the button and slider tables are.**
  `ConversationPane::new` takes a `Capabilities { specs, dispatch }`; the compositor crate
  cannot see the substrate's material tables and must not learn to, and applying a verb needs
  the `Shell` that owns the backdrop. `Capabilities::none()` is the caller with nothing to
  offer, and it is still the safest shape — a server that answers for everything and exposes
  nothing.
- 🚨 **The security property is now re-measured by the console, every session — and the live
  check against this build is OUTSTANDING.** `doc/console_approval_protocol.md` §7 measured
  that Claude Code withholds the `--permission-prompt-tool` handler from the model's own tool
  list, and §9 point 4 says that guarantee is tied to the flag and must be re-checked **per
  server**. Serving real capability tools from the same server is exactly the change that
  could disturb it, and the session that made the change could not launch the console or
  build a release binary, so no `system/init` from this build has been read. What was built
  instead is the check: `ExposureAudit` reads the tool list the init event already carries and
  prints its verdict to stderr and to the band at every init. ⚠️ It names **three** states,
  not two — handler withheld, handler present (🚨, "do not trust this session's cards"), and
  *nothing reported*, because a pass read off an empty list is the false negative the whole
  arrangement exists to avoid. **Read that line before trusting a card on a fresh build.**
- **"Allow everything for this session" — the console's own memory, widened.** A fourth
  button on the approval card, session-scoped and dying with the tab like the per-call
  entries beside it. It is **not** an upstream permission mode and is not implemented as one:
  `bypassPermissions` is unreachable and `dontAsk` *refuses* rather than allows, both
  measured. The handler still runs, the card is still drawn, every call is still recorded —
  the console simply answers yes on the human's behalf.
- 🚨 **The band carries a standing marker for exactly as long as it is on**, derived in
  `strip_content` from the memory's own flag so it can neither stick nor be dismissed — the
  rule already set for `dontAsk`, applied to a grant the human made themselves. ⚠️ **It says
  which of the two facts it is**: *"you allowed everything — the console is not asking"* and
  *"you are not being asked — anything needing permission is refused"* are different
  conditions with different remedies, both can be true at once, and the band shows both. The
  amber is `MODE_ALERT`'s, not red, for the reason already argued — this band is read for
  hours and a permanent klaxon trains the eye past it. Clicking the marker revokes the grant,
  with no confirmation: it withdraws an authority rather than granting one.
- ⚠️ **A per-call decision outranks the blanket one**, so a call denied-and-remembered stays
  denied under an "allow everything" — the wide grant is the default for calls nobody decided,
  never an overrule of a specific refusal. The card records **who** answered
  (`AnsweredBy::{Click, ThisCall, SessionAllow}`) because the two standing sources are revoked
  in different places, and a card that could only say "from a decision you already made" would
  send a reader looking for a `forget` button that is not there.
- ⚠️ **A verb that is silently not served now says so where a human is.** Two spec names that
  sanitise to one MCP tool name leave the later one unserved, and that was reported by an
  `eprintln!` alone — invisible to a console started from a PATH shim, which is how it is
  started. This change's own argument for the exposure audit ("on stderr *as well as* in the
  band's log") applied a few dozen lines away and not here. `start_approvals` now hands the
  sentence back for `ConversationPane::new` to seed the log with, so it lands at the head of
  the scrollback — a route that only became real when the merge before this one started
  drawing that log at all. The path stays dead by construction (a test asserts the real table
  collides with nothing), so `collision_note` is pure and pinned by test: a safety net nobody
  has pulled is worth exactly as much as its test.
- **`AnsweredBy::is_standing()` is gone rather than wired in.** It collapsed `ThisCall` and
  `SessionAllow` into one boolean, which is precisely the distinction the enum exists to keep
  — every reader matches all three arms because each owes the human a different sentence and a
  different place to undo it. Nothing asked the question, and a helper whose answer no correct
  reader wants is worse than absence. The doc comment now says so, so the next person does not
  re-add it.
- **What is built and not seen.** No agent has called a capability tool yet, so the card
  naming *"organon · console.portal"* instead of a shell command exists and has not been
  looked at; and 📌 a capability tool costs the agent a `ToolSearch` first, because MCP tools
  arrive deferred — the first console verb in a session is slower than the `Bash` call it
  replaces, and only the second onwards is cheaper.

### Console Spike — a conversation tab now runs in a project, and says which one

- 🚨 **An agent in a conversation tab was standing in no project at all, silently.** The
  built-in `claude-chat` row carries no `cwd`, `AgentSession::spawn` reads `None` as *the
  app's own directory*, and a console started from a PATH shim is nowhere in particular — so
  the agent saw no repo-local `.claude/skills/`, no project `CLAUDE.md`, no
  `SHELL_ARCHITECTURE.md`. Measured: it answered `Unknown skill: organon-cli` with the skill
  correctly on disk, and separately spent several approval cards running `ls` and `--help` to
  rediscover a CLI with an 18 KB guide in the checkout. The only symptom is an agent that
  seems oddly ignorant. It also failed execution plan §5.9.26 at its first step: an agent that
  cannot see the repo cannot extend the console from inside the console.
- **Four rules in one pure place, and the product still names no project.**
  `harness::conversation_cwd` resolves, in order: the harness's own `cwd`,
  `$ORGANON_SHELL_PROJECT`, **the nearest project root at or above the launch directory**,
  then the launch directory. Rule 3 is the one that does the work — *`cd` into a checkout and
  start the console* lands in that checkout's root with no configuration whatsoever, for any
  checkout, so Organon's own repo is reachable for exactly the reason everybody else's is and
  no path ships in product data. The marker is `.claude/`, then `CLAUDE.md`, then `.git`.
- ⚠️ **Home is never discovered, only inherited.** The walk stops at the home directory,
  because a `~/.claude` is user-global configuration rather than a project — otherwise a
  console launched from `~/Documents` would quietly aim at the whole home directory. Launching
  *in* home still lands there.
- 📌 **Terminal tabs are deliberately unchanged.** A shell announces its directory and `cd` is
  one keystroke, so starting in `native/` because that is where you were is right; ascending
  would be an unasked-for correction. An agent's directory is invisible *and* decides which
  instructions exist at all.
- ⚠️ **Nothing is silent now, including success.** `harness::cwd_notes` states the directory
  and *which rule chose it* every time — not only on failure, because a resolution can be
  wrong in a way the code cannot detect — plus a warning when the directory satisfies no
  marker at all. Said to `stderr` for whoever launched from a terminal and into the pane for
  whoever is looking at the console.
- ⚠️ **`ConversationPane`'s log had never been drawn anywhere.** `pub fn log()` had no caller,
  so *"approvals are not wired — a tool that needs permission will fail instead of asking"*
  has been written to nobody for the pane's whole life. The scrollback now draws it, dimmed,
  above the first message.
- 🚨 **Half-verified, and the half that is open is named.** Against the real `claude.exe`,
  `system/init` lists `organon-cli` in `slash_commands` from inside *and* outside the repo,
  but in **neither** case in its `skills` array, while three neighbouring user-global skills
  appear in both. Duplicate copies, description length, file size and frontmatter shape are
  each ruled out by measurement. So the cwd is right and the skill is registered; whether the
  model is *offered* it is unanswered, and the honesty ledger carries the remaining hypothesis
  and the one cheap test for it. 443 tests in the compositor lib, from 433.

### Console Spike — the portal's camera, and the rule that a hand outranks an agent

- **`organon console camera` — the viewpoint an agent could never move.** Before this the CLI
  could choose everything the world *is* and nothing about where to stand to look at it: the
  catalog carries `cam_path` / `cam_speed` / `cam_kick` / `cam_damping` and **no distance, no
  zoom, no FOV anywhere**. James, watching an agent try to show him something through the
  portal: *"it's having trouble because the camera is far away and I don't think the CLI has
  commands to move it, but it's fundamentally working."* The state was already there —
  `World::apply_camera_input` writes the yaw, pitch and distance the portal's drag and wheel
  drive — with no surface on it. `--reset`, `--yaw`, `--pitch`, `--distance`, any subset, one
  move.
- ⚠️ **Two cameras, and the change is careful not to conflate them.** `cam_path` is the
  *world's* auto-orbit: part of the composition, in `Shared`, saved in a preset. This is where
  the *viewer stands*: host state on `World`, in no snapshot and no preset. They **compose** —
  the finalization adds the auto-orbit's offset to this base — so a shot framed here still spins
  if `cam_path` says to, and neither had to learn about the other.
- 🚨 **The hand always wins, and it is enforced rather than remembered.** A drag and a typed
  command write the same three fields and `apply_camera_input` cannot tell them apart, so
  without arbitration the last writer in the frame wins by accident and a command landing
  mid-drag moves the picture under a hand that is holding it. *A control that fights your hand
  is worse than no control.* The stamp is taken in `redraw` from the gesture — the last place
  the two are still distinguishable — and `organon-shell/src/camera.rs` owns the policy as a
  pure, tested predicate. This is the same rule the workshop's lighting renderer already runs
  against a hand on the lamp.
- **The hold is two seconds, and the number is bounded rather than felt.** *Longer* than any gap
  inside one interaction (a drag stamps every ≈16 ms, a wheel-notch train ≈100 ms, a hand
  releasing to re-grab a few hundred), so a pause mid-gesture is never read as the end of one.
  *Shorter* than the time it takes to **ask** for something, so a command caused by the person
  is not refused when it arrives. Both properties are tests, not prose. ⚠️ The refused command
  is **dropped, never queued**: a deferred framing arrives after the hold expires as a jump into
  a shot the person has since composed — the same failure, delayed.
- **Absolute, and that is forced rather than chosen.** The console lane is fire-and-forget with
  no return path, so a caller can never read where the camera is and therefore can never compute
  a delta. That is also why `--reset` is not a convenience: it is the one framing nameable
  *without* knowing the current one, so `--reset --distance 40` is a complete workflow with no
  read-back in it. `scene_input::DEFAULT_*` are `World::new`'s own initial values, named rather
  than copied, so reset is provably the framing the window opened with.
- **Refused, not clamped — and the asymmetry with the hand is the point.** `apply_camera_input`
  clamps, because a wheel that reaches the ceiling simply stops; a typed `--distance 9000` is
  not an overshoot but a number that means something else, and a silent clamp would let the
  mistake look like it worked. So the band gates the command lane twice (clap, then the
  schema's `ArgKind::Float`) with the world's clamp underneath as the belt — and all three read
  the **same** `scene_input` constants, since a second copy is how an agent comes to be refused
  a viewpoint the drag can reach.
- **It says so when it moves something nobody is looking at.** A substrate rig overrides the
  whole camera tuple and `off` draws nothing, so a framing with the portal closed succeeds,
  moves real state and changes not one pixel — the portal's own documented trap, met from the
  other side. The console cannot fix it (the camera really did move), so it reports on stderr
  instead of pretending.
- ⚠️ **Immersive, full screen and the animated grow were NOT built**, and the honest reason is
  recorded rather than deferred silently: the recon's "immersive is nearly free" does not
  survive contact. It is true of the *rendering* and false of the *painting* — `paint_portal`
  paints **over** the front-end, which is what floating means, while immersive needs the image
  **under** the glyphs with the scrim over it, and the scrim lives inside `term_view::draw`'s
  banded-backdrop arm. That is a new integration, not a variant added to `portal::step`, and
  half of it would have left the portal in a state with no way out. The correction is in
  `SHELL_ARCHITECTURE.md` §2 so the next scoping starts from it.
- 🚨 **An optional argument spelled `null` is absent — and until review, it was a type error.**
  `console.camera` is the first spec on this lane whose arguments are optional, and
  `validate_args` (`organon-shell/src/command.rs`, written when every schema was required-only)
  matched on key *presence*: `op_args` emits the whole slot list, so a partial framing
  serialized to `{"reset": false, "yaw": null, "pitch": null, "distance": 40.0}`, the `null`
  reached the `Some(value)` arm, `ArgKind::Float`'s `as_f64` returned `None`, and
  `CommandService::dispatch` refused the call — **`--distance 40`, this change's own flagship
  example, included** — before `op_from` was ever reached. Fixed in `validate_args` rather than
  by omitting the keys: it is a general property of optional arguments, so the next verb with
  one does not re-find the trap, and it makes the reading uniform with `args: null` meaning "no
  arguments", which this function has always done one level up. A **required** argument spelled
  `null` is still refused, and now reports "missing" instead of naming a type.
- ⚠️ **The comment asserting that behaviour predated the behaviour**, and that is the defect
  class worth naming: a contract written down, believed by its author, never implemented, and
  invisible until the first caller depended on it. Nothing failed, because nothing tested the
  boundary the comment spanned — every camera test called `op_from`/`op_args` directly, which is
  precisely why none of them saw it. Both halves are now pinned: the rule in the pure crate
  (`an_optional_arg_present_as_null_is_absent_and_a_required_one_is_missing`), the whole lane in
  `shell_main.rs` (`a_partial_framing_survives_the_real_dispatch_and_reaches_the_target`,
  wired with the real specs, the real target and a real `SessionLog`).
- ⚠️ **A second consequence of the same compile-don't-run split**, found while fixing the
  first: `a_capability_call_becomes_the_sidecar_line_the_cli_would_have_written` loops over
  every entry in `console_specs()` and panics on a name it has no arguments for — a guard that
  exists so a new verb cannot be added without proving its tool call writes a line the drain
  reads back. Adding `console.camera` to the catalog armed it, and nothing here could run it.
  It now supplies the *partial* framing, which is the case worth pinning anyway.
- 🚨 **Nobody has seen any of this move.** 480 tests in the compositor lib (from 433 before the
  camera, 479 as rebased) and `cargo check --features shell-edition --bin organon-console`
  clean — neither is evidence that a picture moved. Two thirds of the new tests (the wire round
  trip, the ranges, the schema bands, and the whole-lane dispatch test above) live in the root
  package and were **compiled, not executed**; the safety-critical third — who owns the camera,
  and now the optional-argument rule — is deliberately in the pure crate, where it runs.

### Console Spike — the canary on `tool_use_result` fired, on its first real chance

- 🚨 **A third `tool_use_result` shape exists, and the counter built to notice one noticed
  it.** Two changes landed independently: one taught the tool card to read the undocumented
  sibling object, recording in the same breath that both captured examples were `Read`
  results and that `result_detail` reads the `file` sub-object *whatever* the line claims to
  be — a bet on shape stability, with `MapStats::tool_details_declined` named as the canary.
  The other replaced the hand-written subagent fixture with a real fan-out. Their merge put
  the two together: the real capture carries two `tool_use_result` objects, both **`Agent`**
  results (`status`/`prompt`/`agentId`/`agentType`/`usage`, no `file` sub-object, no `type`
  key), so `result_detail` declined both and the counter went 0 → 2. The failure was a test
  asserting the old fixture's premise, not a bug: nothing was mis-parsed, nothing was
  attached, and the shape announced itself in a number rather than in a card quietly showing
  figures no tool sent.
- **The test was re-contracted rather than re-pointed.** Its premise — "a capture with no
  `tool_use_result` on it" — is simply false of the fixture now, and preserving it by aiming
  at a different file would have thrown away the better test the collision handed over:
  *a `tool_use_result` of a shape this card cannot use is declined and counted, never
  mis-parsed and never attached.* It asserts both halves, because a test checking only the
  decline count would pass while details were silently attaching. Renamed to say that, with
  the movement of both numbers justified in its own doc comment.
- ⚠️ **`result_detail` is deliberately not widened to parse `Agent` results.** What such a
  card should show — a token total, a duration, a nested agent's id — is a card-design
  question no observation answers yet, and inventing an answer is the move the four-field
  list exists to refuse. An `Agent` result renders no detail, and says so in a number. 420
  tests in the compositor lib.

### Console Spike — the band stopped rearranging itself at the end of the first turn

- **The dim half of the status strip is now present from the first frame.** It used to be
  empty until the first turn's `result`, at which point the session cost, the context ring
  and the last-turn figure all arrived together — so a band a hand had been looking at for a
  minute reshuffled at exactly the moment the session became interesting. The cost is now on
  the band from the moment the tab opens, reading `$0.0000`, which is not a placeholder but
  the true total of what has been spent. 421 tests in the compositor lib, from 417.
- 🚨 **This reverses a documented decision, and the reversal is decomposed rather than
  waved through.** `ContextSlot::Unknown` was deliberately built to draw **nothing at all**,
  on the stated grounds that an empty ring asserts *"0 % full"* — specific, confident and
  false. That was right about the **arc** and wrong about the **circle**: a ring with no arc
  in it is not a needle pointing at nought, it is the container the answer will appear in.
  So the **track is chrome and the fill is the measurement** — the track is drawn from the
  first frame, the arc still refuses until a `result` has stated a window and a
  `message_start` has stated a prompt. The honesty point survives intact; what it lost was
  the argument about which part of the ring it applied to.
- ⚠️ **An unmeasured ring must not be mistakable for a measured nought, and that case is
  reachable rather than theoretical** — a zero prompt against a known window builds a real
  reading whose arc is also empty. Two states drawing one picture would *be* the false claim
  the original rule existed to prevent. So the track carries the difference: `CONTEXT_TRACK_EMPTY`
  is visibly fainter than the track a measured reading sits on, and the hover says
  *"not measured yet · waiting on: a window from `result`, a prompt from `message_start`"*
  where the other says *"0 % at the last request"*. A shade alone is not an answer to
  "which is this?".
- ⚠️ **`last turn` has no honest zero, so it is omitted rather than invented.** Nought spent
  is a total and a bare ring is a container, but `last turn 0.0s` is a *duration* asserted
  about an event that did not happen. It arrives at the first `result` alongside the ring's
  first arc, and the band's **height** does not move when it does — which is the property
  that was actually asked for. `remembered decisions` stays conditional on its own grounds:
  it counts things the reader themself did.
- **`the_strip_is_one_band_and_leaves_the_scrollback_the_rest` was not loosened, and its
  "identical height with everything in it as with nothing" assertion now carries more
  weight** — the ring is a child of the band's layout on every frame rather than only on
  measured ones, so the cold case is no longer trivially empty. A second test states the
  same equality as its primary claim rather than as a corollary, because it is the whole
  point of the change. Two existing tests moved and are named in `SHELL_ARCHITECTURE.md`;
  none was deleted.
- **The band no longer counts the models.** Every `initialize` ack wrote a note —
  *"the session offers 5 models"* — onto the band's single line of diagnostic width. It is a
  number nobody can act on and the list is one click away on the model plate. ⚠️ The list
  itself is untouched: it is what the picker is built from.
- 🚨 **A subagent step marker was tofu, and `.monospace()` was never going to fix it.** James
  ran a real fan-out and the card read `□ Bash` where a returned step belonged — at a draw
  site that had asked for the mono face since the day it was written. Measured by reading the
  `cmap` tables of all four fonts egui 0.33 bundles: **`✓` U+2713 and `✗` U+2717 are in none
  of them**, and egui does no OS font fallback, so choosing a family only chooses which font
  is missing the glyph. They are now `•` U+2022 and `×` U+00D7, both present in *both* faces.
  The rule is therefore two rules — **choose a character Hack has, then ask for Hack** — and
  the same read confirms the earlier fix was right about its own case.
- ⚠️ **The guard that missed it is replaced by one that cannot.** The old test forbade a
  single range (`U+2500..=U+259F`) at a single site (a band reading); `✓` is in neither. The
  new one is an **allowlist** of every non-ASCII character the console is *measured* to be
  able to draw, applied to the band, the chips and the subagent step markers — so adding a
  symbol now means measuring it first. That is worth more than the one-character fix, since
  this defect has now reached a fourth site by growing a new one each time.
- 🚨 **Unseen, and a person has to settle it.** Whether a fainter circle reads as "waiting
  for a reading" rather than as "a ring someone forgot to finish" is a claim about legibility
  at the edge of the eye, and no test can make it. If it fails, the fix is a wider gap
  between the two track values, not a return to drawing nothing.

### Console Spike — a one-character edit stopped rendering as twenty lines

- **The `Edit` diff is aligned now.** It printed `old_string`'s lines as removals and
  `new_string`'s as additions with nothing between them, so a one-character change inside a
  ten-line block came out as **ten removals followed by ten additions** — honest about what
  arrived, and useless to read. `text_diff::line_diff` trims the common prefix and suffix, aligns
  what is left by longest common subsequence, and elides long unchanged runs to three lines of
  context. Measured on the test that is the whole point of the change: one changed character in a
  ten-line block is now **one removal and one addition**, and the same change 200 lines into a
  400-line block costs the same rows — a diff's size is the size of the *change*, not of the block
  it sits in.
- **No diff crate.** `organon-shell` is deliberately dependency-light (its `Cargo.toml` header
  requires every edge to earn its line), and after a prefix/suffix trim the changed region is
  small enough that a plain LCS is the whole algorithm — ~120 lines in a module with no egui in it,
  tested with plain strings. `crate::term::encode_key`'s shape: put the decision in a pure
  function, then test the function.
- ⚠️ **Three bounds, not one, and each says what it kept back.** `MAX_CELLS` (20 000) refuses the
  alignment for a changed region past ~141 × 141 lines and degrades to a block replacement,
  *naming the two sizes on the card*. `MAX_RUN` (8) caps each run of one kind. `MAX_ROWS` (24) caps
  the whole diff. 🚨 **`MAX_RUN` is not redundant with `MAX_ROWS` and dropping it is a silent
  regression:** a global row cap truncates the tail, and in a block replacement every removal
  precedes every addition — so a global cap alone shows a wall of red and *no green at all*, which
  is worse than the unaligned rendering it replaced.
- **Whitespace-only and no-change edits stop reading as noise.** An identical pair draws no rows
  and says `no change — old_string and new_string are identical` (it used to print the block twice,
  which is the loudest possible way to say nothing happened). A re-indent, a stripped trailing
  space or a changed line ending is named `whitespace only — no visible character differs`, because
  its rows are *visibly identical* and a reader with no note reads the card as broken. ⚠️ The
  predicate is computed on the whole strings rather than per row, which is also what catches a
  **trailing-newline** difference — `str::lines` cannot see one, so there is no row for it, and
  without this the card would have claimed the two were identical when they differ by a byte.
- **`tool_use_result` is surfaced — and only the four fields a real capture contains.** The
  undocumented sibling of `message` was decoded and dropped. For a `Read` it carries
  `filePath`/`numLines`/`startLine`/`totalLines`, and a card now reads `4 of 900 lines, from line
  40` — a thing a terminal never sees, because the CLI does not print it. 🚨 **The list stops
  there.** A byte count, an exit status, a truncation flag, the unified patch Pi's `Edit` result
  carries: all absent because **nothing has been observed sending them**. An omitted field beats an
  invented one.
- 📌 **The path is not printed twice, and `content` is dropped.** A `Read` card already shows
  `file_path` as an argument field, so the detail contributes only the counts — except on an
  **orphan** card, where there are no arguments and the detail's path is the only record of what
  the tool touched. `content` is the file's text, which the `tool_result` block already carries in
  numbered form.
- 🚨 **A detail on a line carrying two `tool_result` blocks is declined and counted, never
  attached to both.** `tool_use_result` is a sibling of `message`, not of a block inside it, so
  nothing says which call it describes. Every capture has exactly one result per line; two is
  unobserved, and guessing would put one call's line counts on another's card — wrong in precisely
  the way this front-end exists to avoid. `MapStats::tool_details` / `tool_details_declined` count
  both outcomes, and are **separate from `unmapped`**: ⚠️ the module doc claimed
  `tool_use_result` was counted there and it never was — the `user` line it rides on always mapped,
  so `unmapped` would have said a line was unrendered while its card was drawn in full.
- **Declined: thinking blocks.** The decoder reads them and **no real capture on this machine
  contains one** — the only fixture that has one is `claude_stream_edges.jsonl`, which declares
  itself hand-written. Rendering against an unobserved shape is what the subagent path's 🚨 in the
  honesty ledger already costs once; a second is not worth a dimmed paragraph. Re-scope it the
  first time a capture shows one.
- 🚨 **Nobody has seen any of this on screen.** Everything above is pinned by headless tests
  against committed captures, which is replay and not a conversation. 417 tests in the compositor
  lib, from 390.

### The skill now tells an agent how to change the console it is running in

- **`.claude/skills/organon-cli/SKILL.md` gains "Changing the console from inside it".** The
  skill has always taught an agent to *operate* Organon from outside it; a tab of an Organon
  Console is the one place that framing is wrong, because there the agent lives inside the
  thing it is being asked to change. The new section covers where the code is (routing into
  `SHELL_ARCHITECTURE.md`, which is hook-enforced current rather than a shipped snapshot), the
  exact verification bar, the traps that are not visible in the code, and the honesty rule that
  governs anything displayed — stated in the console's own terms (measured, or derived from a
  measurement by a stated rule) rather than borrowing Mind's four-way provenance markers, which
  `SHELL_ARCHITECTURE.md` does not use.
- **It is assembly, not authorship** — which is the useful part. Every fact in it was already
  written down for its own reasons: the shift-permissive modifier match and the
  `ScrollArea`-in-`bottom_up` collapse in `SHELL_ARCHITECTURE.md`, the declined readouts in
  `SessionFacts`'s doc comment, the harness registry's two traps in
  `doc/console_spike_execution_plan.md` §5.9.26. The skill's job was to point at them in the
  shape an agent reaching for them needs, not to restate them.
- 🚨 **No mechanism was designed, deliberately.** How a change takes effect — hot load, rebuild,
  or data-only — is open by James's decision, and the section says so and says not to reach for
  it. What it does record is the one seam that already works without a rebuild: `harness::load`
  merges `harnesses.json` over the built-ins by id, so a new tab type is a data change. Nothing
  else is read from disk, so everything beyond a harness row is code.
- **Nothing is enumerated.** No commands, no parameters, no catalog contents — §10's rule, and
  the only reason a file this size can describe a surface this large without rotting. The one
  table added routes eight modules to the section of the architecture doc that owns them, and
  names that doc as the authority over itself.

### Console Spike — the subagent fixture stopped being a guess

- **A real fan-out was captured and replaced the reconstruction.** One run of `claude.exe`
  2.1.228 on the console's own argv (`agent_session.rs::ARGS`), given a prompt that dispatches
  two agents in parallel, one of which dispatches an agent of its own.
  `fixtures/claude_stream_subagent.jsonl` is that stream, sanitised to the existing convention;
  the README's honesty split collapses to "captured". **The correlation held** — every
  subagent-scoped line routed onto the card that spawned it, `subagent_routed` 6,
  `subagent_unrendered` 2, `orphan_subagent_activity` 0, nothing on the orphan path. The wire
  shape did not, in three ways, and each was a test that failed rather than an expectation
  quietly moved.
- 🚨 **The dispatch tool is named `Agent`. `system`/`init` advertises it as `Task`.** Both
  spellings are in the same capture, in the same session. The only reason a fixture saying
  `Task` survived weeks of green tests is that nothing in this crate routes on the tool name —
  correlation is `parent_tool_use_id` alone. A view that special-cased the name would have
  matched nothing, for as long as it took someone to run a real fan-out. Now pinned by a test
  that asserts *both* spellings, so the day they converge is a failure and not a mystery.
- 🚨 **The wire stops at depth 1, so the flattening machinery guards a case nothing produces.**
  The second agent really did dispatch its own. That dispatch appears exactly twice — a
  `tool_use` and a `tool_result`, both scoped to *its parent* — and lands as an ordinary
  depth-1 step. The grandchild's own lines never arrive: its `tool_use.id` is never once a
  `parent_tool_use_id`, so a card sees a nested agent's existence and its answer and nothing of
  its work. `MAX_TRACKED_DEPTH`, `subagent_owner` and the depth badge are **kept** — nothing
  promises the CLI will keep withholding those lines, and the hazard they close is real the day
  it does not — but their coverage is now `conversation.rs`'s synthetic tests, which declare
  their provenance, and the claim that a capture proved it is withdrawn.
- ⚠️ **No subagent in the capture said anything.** Every subagent-scoped `assistant` line
  carried a `tool_use` block and nothing else; the answer reached the console only as the
  parent's `tool_result`. `Subagent::Said` — which the reconstruction exercised twice — is now
  backed by no observation at all. Kept, because the schema permits it and declining a text
  block would be a silent loss; but a card fills with *steps*, and a design expecting prose was
  expecting.
- 🚨 **Confirmed, and this is the one that mattered most:** §5.9.1's measurement that Claude
  Code never forwards token-level deltas from a subagent. All 41 `stream_event` lines in the
  capture are main-scoped, including the ones streaming the dispatch's own arguments, and
  `subagent_stream_events` reads 0. The rendering path was designed around a constraint that
  holds. Also answered, having been explicitly open: a subagent emits **no `result` and no
  `system` line of its own** either — `parent_tool_use_id` is absent as a *key* on every
  `system` line in the file.
- ⚠️ **`ToolOutcome::text` was joining content blocks with nothing, and it showed.** Every
  array-form tool result in every earlier fixture held exactly one block, so the separator was
  unfalsifiable. An `Agent` result carries **two** — the subagent's answer, then a trailer
  naming its `agentId` and `<usage>` — and the card read `bravoagentId: a4d5…`, the last word
  of the answer welded to the next block's first. These are separate content blocks, not
  fragments of one string (that is what `input_json_delta` is), so they are now joined the way
  separate blocks read.
- 📌 **The capture opened a door the design did not know was there.** Five undocumented `system`
  subtypes carry subagent lifecycle — `task_started`, `task_progress`, `task_updated`,
  `task_notification`, `task_summary` — with a rolling `description` ("Reading one.txt"),
  `last_tool_name`, `usage.tool_uses`, `duration_ms`, a terminal `status`, and a `tool_use_id`
  naming the card. They are **main**-scoped, carrying no `parent_tool_use_id` key at all, so
  rule 5 cannot reach them; all five decode to `Notice` and render nothing today. That is a gap
  rather than a decision — it was never weighed, because nobody knew the lines existed — and it
  is the cheapest remaining improvement to a coordinator view. It does not soften the liveness
  finding above: progress metadata is not token deltas.
- ⚠️ **One sanitisation in that fixture is not a straight replacement.** A dispatched agent's
  prompt contains the working directory, and the `input_json_delta` fragments carrying it split
  **mid-path** — the whole path is contiguous only in their concatenation, so no per-fragment
  replacement can match it, and a naive scrub silently leaves the path in the file. Those
  fragments were scrubbed as one string and re-split into the same number of pieces at the same
  proportional offsets, with "the fragments rebuild exactly the settled `input`" asserted
  either side. `fixtures/README.md` says so where someone re-capturing will read it.
- organon-shell lib: **393 passing, from 390** — two tests built on the reconstruction rewritten
  into five built on the capture. No expected value was adjusted to make a test pass.

### Console Spike — a portal into a 3D world, floating in the terminal

- **`organon console portal open` floats a live, orbitable window onto the world over the
  transcript.** The transcript scrolls past underneath it; the portal holds its place on
  **screen**. That is the new thing — every anchor the console had before this was a *scroll*
  anchor (`block_anchor` pins a rectangle to a run of lines and the picture rides them off the
  top), and this is the complement. Drag it to orbit, wheel over it to zoom, `portal close` to
  give the rows back. Pure state machine, rect arithmetic and pointer test in
  `organon-shell/src/portal.rs`; 399 tests in the compositor lib, from 390.
- ✅ **"Control Organon from the shell" turned out to be already built.** The CLI's parameter
  lane drains inside `World::frame_body`, which is what `render_to_texture` runs, and the
  console injects `ORGANON_IPC_NS` into every tab it spawns — so `organon set`, `organon
  generator` and `organon recipe`, typed at a prompt *in a console tab*, drive the world *in the
  portal*, with **no new code at all**. The only thing standing between that and being visible
  was a rectangle to look through.
- 🚨 **The portal shows the World and not the substrate, and that is correctness rather than
  preference.** `world.rs:6526` reads an installed substrate rig first and returns its whole
  six-tuple *before* `yaw`/`pitch`/`distance` — which are exactly what `World::apply_camera_input`
  writes. A substrate portal would take a drag, convert it, apply it and draw an identical
  frame: green build, no log line, and an investigation starting in `scene_input.rs`, which is
  correct code. Verified against the source before it was relied on.
- 🚨 **The portal claims the wheel, which reverses a decision this tree argued the other way.**
  A scene patch deliberately does not — *"a scene patch is something to look at, so the wheel
  over one keeps scrolling the page exactly as the wheel over a paragraph does."* The portal is
  the other thing, and the reversal is deliberate and scoped to it: **a scene patch is a
  picture, a portal is an instrument.** ⚠️ It has to be an *explicit rect test*, because
  `term_view` reads the wheel and every key from **raw input** — egui layer order, an `Area` and
  a modal are all equally invisible to it. `block_panel::pointer_inside` is the precedent and
  this copies it.
- 📌 **At most one `World` render per console frame, in every state, by construction** —
  `engine_plan`, proved over its whole input space. `SURFACE_RENDERS_PER_FRAME` rules the
  two-render case out (`frame_index` and the TAA jitter phase are shared between the targets,
  invisible on a still plane and visible-and-intermittent on a moving World), and a live portal
  beside a live backdrop is precisely it. So an open portal **takes the frame**. ⚠️ The cost,
  stated rather than discovered: while it is open the backdrop does not paint and a scene patch
  has no picture to sample. `backdrop_source` is never written, so closing restores everything.
- **A field beside `backdrop`, not a `SurfaceKey` variant.** Eviction is a policy for many things
  competing for few slots; a portal is one thing that is open or closed, requested every frame,
  so the variant would exist only to be excluded from the function the type serves. The deciding
  argument is smaller: the portal must work in a **terminal** tab, where `ElementId` means
  nothing at all. `SurfaceKey`, its tests, `SurfaceImages` and the `conversation_view` seam are
  untouched.
- 🚨 **Nobody has seen it.** Built without a GPU: every decision is a headless test and
  `cargo check --features shell-edition` is green, and neither is evidence that a pixel appears.
  Whether it reads as *floating*, whether the default world is legible at that size, whether the
  drag rate suits a hand — all of it needs James at the machine. ⚠️ Two known gaps recorded
  rather than hidden: in a **conversation** tab the wheel over the portal zooms *and* scrolls
  (that front-end's `ScrollArea` has already read the delta by the time the region registers),
  and a **window-resize drag** reallocates the portal's texture every frame with a log line each,
  exactly as an open conversation surface already does. `SHELL_ARCHITECTURE.md`'s honesty ledger
  carries both.
- **Deliberately not built:** immersive, full screen, the animated grow, and the click and
  double-click transitions. One visible beat. The seam is `portal::step` being total over
  `(state, event)`, and the render-budget invariant already survives the states that come next.
  ⚠️ Escape is not consumed either, and that is a decision: in a terminal tab the keyboard is
  the child's and `vim` needs it, so taking it must be conditional on a state — and the states
  that need an Escape are the ones where a prompt may not be reachable, which this tier does not
  build.

### Console Spike — the twelve agents a coordinator dispatched stopped being eight minutes of silence

- **A subagent's work now renders inside the tool card that spawned it.** A coordinator session
  that fans out used to show a `Task` card on "running" for eight to sixteen minutes and then a
  wall of text; the events were arriving the whole time and §5.9.3 rule 5 was dropping them,
  because rendered as ordinary events they become assistant turns belonging to nobody. They are
  folded onto the card named by their `parent_tool_use_id` instead —
  `AgentEvent::SubagentActivity` is the one transcript event that **addresses an existing element
  rather than appending one**, which is precisely what stops a subagent acquiring a place in the
  flow of its own. 373 tests in the compositor lib, from 353.
- 🚨 **This renders the activity; it does not make it live, and nothing here pretends otherwise.**
  §5.9.1 measured that Claude Code **never forwards token-level deltas from a subagent** — that is
  unchanged and cannot be worked around from this side. Activity arrives as complete bursts,
  sometimes minutes apart. So `Subagent::Said` carries a whole string with **no completeness bit**
  (there is no provisional state for it to be in), the card reports **counts and finished steps
  rather than liveness**, and nothing streams a caret the way the agent's own prose does.
  `MapStats::subagent_stream_events` is the canary on that measurement: zero forever, or the path
  needs redesigning rather than patching.
- ⚠️ **Depth is flattened to one and recorded, not nested** — a subagent can dispatch its own, and
  cards inside cards inside a scrollback have no bottom. A chain of any length collapses onto the
  one card a human can see, each step carrying the depth it happened at. 🚨 **The trap, found by a
  test that failed:** capping *attribution* at a maximum depth instead of capping the reported
  *number* makes deep steps fall through to the orphan path and open **new top-level cards** — the
  nesting hazard again wearing a flat disguise. `MAX_TRACKED_DEPTH` now bounds the badge and never
  the correlation; what bounds the ownership map is eviction.
- **Orphans follow `orphan_results`' precedent rather than inventing one.** Activity naming a call
  we never saw — a compaction boundary, a resumed session, a card the cap evicted out from under a
  subagent still working inside it — is kept on a nameless card and counted
  (`orphan_subagent_activity`). ⚠️ It opens **`Running`**, which is a claim: it is behaviour 1's
  derivation, since live activity from inside a call is the strongest available evidence the call
  has not returned, and opening it `Complete` would fabricate the result behaviour 3 refuses to
  fabricate. A `Returned` with no matching `Used` is kept the same way, one level in.
- ⚠️ **`MapStats::subagent_dropped` is removed, not renamed.** Its sense reversed, so keeping the
  name would have left a counter whose every reader was wrong in the worst direction;
  `subagent_routed` and `subagent_unrendered` replace it and must be read together. A step landing
  on a card already in the flow reports `Change::Updated`, never `Appended` — the view re-arms its
  scroll-follow on the latter, so getting that wrong would yank a reader to the bottom every time
  any subagent spoke. A per-card `max_subagent_steps` cap evicts from the front and says how many
  it dropped, because one `Task` must not be able to evict the conversation around it by working
  hard.
- 🚨 **Nobody has seen this on screen, and the fixture is a reconstruction.** No capture on this
  machine contains a `Task` call **at all**, so `fixtures/claude_stream_subagent.jsonl` is a shape
  reasoned from the schema rather than observed, and is declared as hand-written in that
  directory's README. The correlation is sound — it is the decoder's own measured field applied
  twice — but whether a real subagent emits exactly these line kinds in this order is unverified,
  as is every pixel of the card. Re-capture at the first real fan-out. ✏️ **Re-captured** — see
  the entry above; the correlation held, three shape claims did not, and the pixels are still
  unseen.

### The + menu drops because of its geometry, not because egui caught it

- **The new-tab popup is anchored under the button instead of above it.** It was placed with
  `plus.rect.left_top() - vec2(0, 8)` on a `LEFT_BOTTOM` pivot — the popup's bottom edge 8 px
  *above* the button, so the list grew upward. That was right when the tab strip ran along the
  bottom of the window. It has been a `TopBottomPanel::top("tab-strip")` for some time, which
  put the anchor at roughly **y = −8**, off the top of the screen.
- ⚠️ **It looked correct anyway, and that is the actual defect.** egui clamps an `Area` back
  inside the screen rect, so the menu landed in approximately the right place *as a fallback*,
  never as a placement — and a change to that clamping, or to the strip's 30 px height, would
  have moved it with nothing to catch the move. The anchor is now `left_bottom() + vec2(0, 8)`
  on `LEFT_TOP`, derived from the **button** rather than from the strip, so a future height
  change cannot re-open it. The module doc's word "drops" is true by construction now.
- 📌 Position is not testable here — it needs a GPU and a display, and no test covers it either
  before or after. The 353 compositor-lib tests are unchanged and still pass; what changed is
  that the correct position no longer depends on a library's error handling.

### The organon-cli skill lives where the tool reads it, as a real directory (#19)

- **`.claude/skills/organon-cli` was a git symlink (index mode `120000`) pointing at
  `../../skills/organon-cli`.** On any checkout with `core.symlinks=false` — the default on
  Windows without Developer Mode or admin — it materialises as a **24-byte plain text file
  containing that path**. ✅ Verified in this checkout before the change: 24 bytes, exactly that
  string. It is not a directory, nothing resolves it, and there is **no error and no warning** —
  the skill simply is not loaded. Every `SKILL.md` update this week was written on the assumption
  that agents were reading it; on this platform none of them could.
- **`SKILL.md` now lives at `.claude/skills/organon-cli/SKILL.md` as an ordinary tracked file**
  and the top-level `skills/` directory is gone. That is issue #19's option 1: one home, at the
  path the tool actually reads. A fresh clone on any platform gets a real directory — no
  `core.symlinks`, no Developer Mode, no per-machine junction, nothing to remember. `git ls-files`
  now reports **zero** mode-`120000` entries in the whole tree, so the class of defect is closed
  rather than this instance of it.
- **What it costs, stated rather than buried:** the skill is no longer at a tool-neutral top-level
  path, so its location is now coupled to Claude Code's convention. That is the honest trade and
  the right one — Claude Code is the only consumer, and a vendor-neutral path that silently fails
  to load is worth less than a vendor-specific one that works. The rejected alternatives:
  `core.symlinks=true` needs admin (so it is not a fix a fresh clone can apply), and duplicating
  the file in both locations is a second copy of a maintained thing, which this tree has paid for
  repeatedly.
- **Four consumers moved in the same change**, because a path fix that leaves references behind is
  the same defect wearing a different hat: `.claude/hooks/doc-rules.sh`'s trigger row (the
  `SKILL.md`-is-stale rule would otherwise have watched a file that no longer exists, silently),
  `ci.yml`'s `paths-ignore` (the `skills/**` line folded into `.claude/**`, which already covered
  it), and the repo maps in `ARCHITECTURE.md` and `CLAUDE.md`. The prose references in `doc/` moved
  too; the execution plan's §5.9 finding is **kept as written** and annotated, because it records
  what was true and how it was measured.

### Console Spike — the strip grew a context ring, on a numerator that is a prompt rather than a bill

- **A small ring at the far right of the conversation strip fills with blue as the context fills
  and turns amber at three-quarters.** It measures **context at the last request**:
  `Usage::prompt_tokens()` of the most recent `message_start` over `modelUsage.contextWindow` for
  the model that served it. Both halves **measured**, nothing derived and nothing summed. 366 tests
  in the compositor lib, from 353.
- 🚨 **`SessionFacts` had declined this readout, and the refusal was half right.** The denominator
  was never unavailable, only undecoded — `modelUsage` is now parsed (`contextWindow`,
  `canonicalModel`, `maxOutputTokens`, per-model `costUSD`), measured at **1 000 000** for
  `claude-opus-5[1m]`. What was genuinely missing was a numerator, and the obvious one is
  **wrong**: a `result`'s `usage` is summed across the turn's API round trips, which the
  `iterations` array beside it proves. On the two-request capture the requests carry **52 556** and
  **54 050** tokens while the `result` reports **106 606** — exactly their sum, **1.97×** the
  conversation actually in front of the model. A ring on it would have read 11 % where the truth
  was 5 %, filled at twice the real rate, and looked entirely plausible. Both numbers are asserted
  in one test so the ratio shows up in the failure message.
- **So the ring moves per API round trip, not per turn**, and a compaction that shrinks the prompt
  shrinks the ring at the next request. `last_prompt_tokens` is assigned, never added. The hover
  says "at the last request" and names both wire fields — that phrase is the provenance marker,
  not a turn of phrase.
- 🚨 **"We do not know yet" draws nothing.** A ring drawn empty before a window or a prompt has
  arrived asserts *0 % full*, which is specific and false. A session's first turn therefore has no
  ring; one run without `--include-partial-messages` never gets one at all, and the test for that
  case asserts the `result`'s usage is sitting right there unused — the fallback that must not
  exist.
- ⚠️ **The 75 % threshold is the console's own judgement and says so**: nothing on the wire states
  when the CLI will compact. Chosen because the cheap answers each cost a turn or two and a turn is
  not small — the capture grew ~1 500 tokens in one round trip and had spent 5 % of a million on
  its first. Integer arithmetic, pinned at exactly 75 so the boundary cannot drift with the window.
- ⚠️ **The ring is exactly one `Body` row across**, because the band reserves its height before
  laying anything out. The one-band test now builds its busiest strip with a 91 %-full ring in it
  and still asserts the same bound and the same identical-height property — **no assertion was
  loosened.** Drawn as a stroked arc rather than a pie: a wedge past 180° is not convex and egui's
  tessellation would have folded it over exactly as the reading became urgent.

### Console Spike — the two plates the strip only reported became the two controls that change them

- **The model plate and a new permission-mode plate beside it are clickable.** `set_model` and
  `set_permission_mode` go down the same stdin turns go down and are acked on the same stdout
  events come back on — no respawn, no session-continuity problem, no resume. Measured 272 ms and
  17 ms against `claude.exe` 2.1.228; `doc/console_session_control_protocol.md` is the capture and
  `agent_session.rs::control_request_line` is pinned **byte-equivalent against the sentences that
  document quotes**, because a typo in a subtype comes back `Unsupported control request subtype:
  …` and a user experiences that as "the picker does nothing". 353 tests in the compositor lib,
  from 313.
- 🚨 **Correlation is the whole hazard, so it is a type of its own.** A `control_response` carries
  the `request_id` the console invented **and nothing else** saying which verb it answers —
  `set_model`'s ack has no body at all. `ControlDesk` is the one place that knows an id means "the
  model change", split out so it is testable without a process: an id issued, an ack matched, an
  ack belonging to nobody, a request never answered. 📌 The other end of the same seam is
  `agent_map` recording **no fact** from an ack, deliberately — it never issued the request and
  cannot know, and two writers for one field where one is guessing is worse than one clean source.
- ⚠️ **Nothing is ever gated on an ack.** `CONTROL_DEADLINE` is 20 s and it *releases a marker*
  rather than unblocking a wait; the composer, transcript and strip never wait on a control. It is
  a **sweep on the pane's existing per-frame pump** — no timer, no thread, no queue — and a request
  is recorded in flight *before* the write, since a half-failed write and a delivered one are
  indistinguishable from this side. **Twenty is set by the slowest request, not the fastest:** both
  acks above are sub-second, but `initialize` goes out at spawn, where a **1.3–3.3 s** band to a
  session's first announcement was measured while MCP servers and skills warm up.
- **The model picker is built from the CLI's own `models` array and from nothing else** — asked
  once, at spawn, so no table in this crate can go stale and an empty list draws a picker that says
  the list has not arrived. 📌 Side effect worth having: `system/init` was measured to arrive only
  once input is pending, so a tab nobody typed into never announced itself — the `initialize` line
  *is* input, so the strip now learns its model at spawn instead of at the first human turn.
  ⚠️ Two rows can both be current (`default` and `opus[1m]` resolve identically in the capture), so
  current-ness matches on `resolvedModel` **and** `value`.
- 🚨 **The pending plate: `set_model`'s ack carries no body, so between the click and the repeat
  `system/init` the console knows what it ASKED FOR and not what it GOT.** Asserting the new name
  would be the plate lying about the one fact it exists to report; asserting nothing would make the
  click look dead. So the plate keeps the **confirmed** model and shows the destination beside it
  as a dim italic `→ Sonnet`. ⚠️ It clears when **the reported model moves at all** — not when it
  equals a predicted string, because `set_model` takes an alias, the session reports a resolved id,
  and the alias→id table is the CLI's; predicting it would strand the marker on every alias this
  build has not met. Selecting the row already in use is a **no-op**: `set_model` to the current
  model produces an ack and no repeat init, so the marker would have nothing to clear it.
- 🚨 **Rule 3 was amended to make that possible, and the ruling is James's.** A changed model is
  restated *only* in a repeat `system/init`, which first-init-wins dropped — so the model would
  have genuinely changed while the plate said `claude-opus-5[1m]` until the tab closed. **`model`
  and `permission_mode` are now latest-init-wins; `cwd`, `cli_version`, `tools` and `mcp_servers`
  stay first-init-wins.** ⚠️ Taking the whole later init would be the wrong repair and that is
  measured too: between the same two inits `tools` went 33 → 128 and `mcp` 0 → 4 with nothing asked
  to change, because deferred MCP tools had finished loading — **an init is a restatement, not a
  change notification**. Written into `doc/console_spike_execution_plan.md` §5.9.3 rule 3; the test
  that pinned the old behaviour was **re-scoped and renamed**
  (`a_second_init_does_not_overwrite_the_sessions_identity`) rather than deleted.
- 🚨 **The fake human turn is suppressed.** A model switch emits a user-role message wrapping
  `<local-command-stdout>Set model to sonnet (claude-sonnet-5)</local-command-stdout>`, *before* the
  ack, which would have rendered as a turn the human never typed. The predicate is narrow on
  purpose — **swallowing a real sentence is far worse than showing a spurious one**: exactly one
  text element, `strip_prefix` **and** `strip_suffix` rather than `contains` (so the tag stays safe
  to quote inside a larger message), and ⚠️ deliberately **not** keyed on `isReplay`, which is
  `true` on genuine human turns too — replay is how a human turn reaches the transcript at all — so
  requiring it would exclude nothing real while letting a future unflagged narration through. The
  one residual false positive is a human whose *entire* message is a verbatim wrapper pair, and
  `MapStats::local_commands_suppressed` counts every suppression **apart from `unmapped`** so that
  number climbing while somebody is typing is how it would be caught. `control_responses` is
  counted apart for the same reason: "the CLI answered us" and "we drew nothing" are different
  facts.
- 🚨 **The permission-mode control is designed around the mode that can silence the console.**
  `dontAsk` is **not** a bypass that lets things through — prompts never reach the console's
  handler and gated tools come back **refused** (`decision_reason_type: "mode"`), while the console
  still passes `--permission-prompt-tool` and still *looks* like the authority. On James's brief
  (*"we need to make what it does unmistakable for the don't ask policy"*): exactly three rows,
  **each labelled by what happens rather than by the mode's name**, with the consequence as the
  label and not a tooltip — a hover puts the one sentence that matters behind a gesture nobody
  makes while deciding. `bypassPermissions` is not offered (the CLI refuses it without a launch
  flag we do not pass, so the row would be a dead button); `auto` and `plan` are not offered (never
  measured, and the control governing authority is the wrong place to guess).
- ⚠️ **The marker is persistent, not a one-time confirmation**, and it is *derived* in
  `strip_content` from the reported mode every frame — so it is on the band exactly while the mode
  is non-default, and cannot get stuck on, stuck off, or dismissed. A dialog clicked through at the
  moment of choosing is the warning people stop reading; the hazard is the hours afterwards. 📌
  Amber, deliberately **not** red: this band is looked at constantly and a permanent klaxon trains
  the eye to skip it. ⚠️ A mode arriving from **outside** the picker is still reported and still
  marked — **the shortlist governs what can be chosen, never what can be shown**.
- **Three tofu sites fixed, with three different fixes because the right one depends on the
  setting.** egui's proportional face carries no box-drawing or block-element glyphs and a missing
  glyph draws as an empty box; James confirmed the first on screen. The run-end rule's `──`
  (U+2500 ×2) becomes `—` (U+2014), because a rule leading into small dim **proportional** text
  should not become monospace. The streaming caret's `▍` (U+258D) becomes `|`, because it is
  concatenated into prose that is proportional on purpose — so the glyph changes rather than the
  face. The strip's `◈`/`●` take `.monospace()` at the **draw site**, leaving the strings untouched
  so every existing test still pins them. 📌 The precedent already existed (the approval card's
  `◈ may I`) and had simply not been generalised; a new assertion forbids `U+2500..=U+259F` in any
  band reading.
- **`doc/console_session_control_protocol.md` corrected by the build, in place rather than as an
  erratum.** Four things it got wrong or left dangling: **§4b was still headed "PROPOSED, NOT
  DECIDED"** after James had ruled and the ruling had been written into the spec it amends — ⚠️ a
  doc asserting a closed question is open is worse than one that never raised it, because a reader
  either re-opens a settled decision or implements the unamended rule; **§3's double-nesting
  warning read as `initialize`-specific and is not** (`set_permission_mode` has the same
  `response.response.mode` envelope, which §8's own quoted line shows); **§2b's discriminator was
  half wrong** — the string-vs-array `content` shape is unusable, since the console's own
  `user_message_line` sends the array form and nothing guarantees the CLI will not, so **only the
  wrapper carries the decision**; and **`pending_permission_requests` /
  `pending_user_dialog_requests` had no recorded element shape** because both were empty in every
  capture — now stated as unparsed, with the staging for the probe that would settle them. One new
  open item, reported rather than glossed: **`initialize` at spawn runs slightly ahead of the
  measurement** — §1's early-answered control was `set_permission_mode`, not `initialize` — and the
  20 s deadline is what makes a wrong guess degrade to an empty picker rather than a stuck tab.

### Console Spike — the band can say the agent is thinking, and says nothing more than that

- **`Standing::Generating` — `● generating`, fourth in the status strip's priority order.**
  The band could report "3 tools running" and could not report that the model was *writing*:
  `Transcript::is_working()` is derived from unresolved tool ids alone, so prose with nothing
  in flight read as **idle** — during the stretch of a turn a person is most likely to be
  watching. Closed with a second measured signal rather than by loosening the first, so
  "N tools running" still means exactly N tool calls.
- 🚨 **The signal is the `message_start` … `message_stop` bracket, NOT `system`/`status` =
  `"requesting"`** — measured on the committed capture `claude_stream_two_tools.jsonl`. That
  run makes **two** API round trips and `"requesting"` appears **once**, on line 4, ahead of
  the first message; the second `message_start` has no status line before it. And nothing
  anywhere reports the request returning — no `"responding"`, no closing status, no
  counterpart of any kind. A state keyed off it would be shown for a session's first request
  and silently absent for every one after, with nothing to tell those two cases apart, and
  would need a clearing rule invented for it besides. The bracket is emitted once per message
  and closes itself. `"requesting"` stays read-for-facts and renders nothing, a refusal pinned
  by `a_requesting_status_alone_changes_nothing`.
- ⚠️ **`EventMapper::streaming_message` was deliberately not reused for it.** It is set on
  `message_start` and **never cleared** — `MessageStop` was a no-op arm before this change —
  so `.is_some()` means "a start was seen at some point", not "a message is open". That is on
  purpose: the id is what a late text delta keys against, and clearing it at the stop would
  trade a stuck status for a **lost sentence**. The new flag is a second field beside it,
  pinned by `closing_a_message_does_not_detach_a_late_delta_from_it`.
- **It lives beside `SessionFacts` rather than on it, and the clearing paths are the whole of
  the correctness.** Every fact is a reported value that holds until replaced; this one flips
  on and off, and a stale one would be the band claiming the agent is writing after it
  stopped. It clears on `message_stop`; on `result` (an interrupt or `error_during_execution`
  ends the turn without ever reaching a stop); on a mid-stream `system/init`, cleared
  **before** the repeat guard that otherwise drops that line; and a second `message_start` is
  *assigned* rather than counted, so two opens without a stop cannot strand a tally. ⚠️ The
  one exit with no event at all is **the process dying mid-message** — `Standing::Dead`
  outranking every other reading is what answers it, which makes those two one dependency
  rather than two features.
- 🚨 **The wart, documented rather than hidden:** between two messages of one turn the band
  falls through to "ready" for a frame or two. The bracket really has closed, and holding it
  across the gap would mean inventing a turn-open state the wire does not report — the
  version that gets stuck on when a turn ends in a way nobody predicted.
- **What it refuses.** No token count, no rate, no progress bar, no ETA, no elapsed timer:
  the wire says a message is open and says nothing about how much is left, how fast it is
  arriving or when it will stop, and there is no clock in this path by design. It shares
  `Working`'s amber, because busy-with-tools and busy-writing are the same answer to "can I
  walk away" and the colour the band must keep distinct is *blocked on you*. Generating
  outranks a finished turn's `needs_action` for the same reason running tools do: that demand
  is only cleared by the next `post_turn_summary`, so it would otherwise sit on the band
  through the entire reply that answered it. 313 tests in the compositor lib, from 302.

### Console Spike — a composer you can write in, and a band that says who you are talking to

- **The composer is multiline.** Three rows at rest, growing a row at a time to a twelve-row
  ceiling and then scrolling inside itself, on a framed plate whose edge is grey at rest,
  green while focused and red-brown when the agent is gone; the `›` glyph is gone and the
  keystroke contract lives in the hint. **Enter sends, Shift+Enter breaks the line** —
  checked by keypress on organon-one, because a green build could not have shown it.
  🚨 `egui::Modifiers::matches_logically` is **shift-permissive**: a pattern that does not
  ask for shift still matches a press *with* shift, so `TextEdit`'s default `return_key`
  cannot tell the two apart and `consume_key(NONE, Enter)` eats both. The fix is an
  inversion — **Shift+Enter is declared as the return key**, since a pattern that asks for
  shift is the one case the match is strict about, leaving a bare Enter to fall through the
  widget and be read with `matches_exact`. Ctrl+Enter and Alt+Enter are deliberately neither
  send nor newline: each is "send" in some client, and guessing wrong sends a half-written
  message. 🚨 A second trap in the same box: `Response::lost_focus()` **stops working the
  moment a `TextEdit` is multiline** (only the singleline branch surrenders focus on Enter),
  so the old submit idiom would have gone silently dead; the guard is `has_focus()`.
- **The one-line status line became a status strip, below the composer rather than above
  it.** A model plate is the headline — the reported identifier with a trailing bracketed
  suffix relocated into a badge (`claude-opus-5[1m]` → `claude-opus-5` · `1M`) and nothing
  else changed, because prettifying it to "Opus 5" would mangle the first alias, snapshot
  date or gateway id not on the nice-names table. Everything else `system/init` said —
  permission mode, CLI version, cwd, tool count, MCP roster, rate limit, session id — is on
  that plate's hover, so the band stays one line no matter what arrives. Beside it: a
  priority-ordered standing (dead ▸ waiting on a human ▸ tools running ▸ a finished turn's
  `needs_action` ▸ connecting ▸ the last turn's own summary) and at most three dim chips —
  session cost, remembered decisions, last turn's wall time.
- 🚨 **`ScrollArea::max_height` inside `Layout::bottom_up` collapses the entire column**, and
  both bands are shaped around it. The area places itself at the *top* of the space that is
  left while the bottom-up cursor sits at the bottom, so it eats everything between —
  measured at **684 pt of a 684 pt pane, for one row of text**. `ui.vertical`, `ui.scope` and
  an enclosing `Frame` inherit the failure; `ui.horizontal` places correctly and then pins
  the area to one row. So each band reserves its height first with `allocate_ui_with_layout`,
  which does go through the placer. The composer's reservation is the text's height from the
  previous frame, read from `ScrollAreaOutput::content_size` — the *unclipped* size,
  deliberately not `Response::rect`, so the measurement cannot feed back on the band that
  clips it.
- **What the strip does not show is the point of it.** No context-window percentage, no quota
  percentage, no session token total — each was investigated and none has an honest source
  (the context denominator lives only in the unmodelled `modelUsage` block, `rate_limit_event`
  carries neither numerator nor denominator, and summing per-turn usage double-counts every
  cache read). The running-tools reading says **tools, not "thinking"**, because it is derived
  from unresolved tool calls and a model writing prose with nothing in flight is not working
  by that test. Cost is labelled `session` because it accumulates on the wire while the token
  counts beside it do not. ⚠️ **The strip has been seen on screen by nobody** — it landed after
  the binary James checked was linked, and green tests are not a look; the honesty ledger and
  demo script beat 7 both say so.
- `organon-console --help` no longer advertises `/panel`, which was removed a change ago, and
  now reads "one local command". `tabs.rs`'s module doc no longer says the tab strip runs
  along the bottom; it is a top panel.

### Console Spike — the session's own facts, kept instead of discarded

- **`EventMapper` now carries `SessionFacts`, read through `facts()`.** The decoder had
  been parsing `system/init` in full and the mapper was binding the whole payload to `_`:
  the model, the cwd, the permission mode, the CLI version, the tool count and the MCP
  servers were all decoded and then dropped on the floor, and `result`, `post_turn_summary`
  and `rate_limit_event` went past untouched. Everything a status strip needs was already
  arriving; nothing was keeping it. Each field has one retention rule — **first init wins**
  for identity (a `system/init` recurs mid-stream and must not overwrite what the first
  established), **latest wins** for everything describing the most recent turn — and
  nothing is summed. 🚨 `total_cost_usd` is already cumulative across the session while its
  sibling `usage` is per turn, so adding two results double-counts everything so far; the
  field names (`cost_usd` vs `last_turn_usage`) exist so a reader cannot confuse them.
  Three numbers a strip would obviously want are **deliberately absent**, because the
  stream does not honestly carry them: a context-window percentage, a quota percentage,
  and any session token total. `num_turns` is declined too — it counts *that run's* turns
  and does not accumulate, so it read `1` on both results of the two-turn capture. Reading
  a fact off a notice does not make it mapped: `MapStats::unmapped` is unchanged and
  pinned by a test.

### Console Spike — an approval card waits for you, and closes itself when it can't

- **The approval hook was designed on the belief that the client waits indefinitely; it
  does not, and a human found out the hard way** — a card sat asking while Claude Code
  returned *"Error calling tool (Write): The operation timed out"*, so the write failed and
  the question stayed on screen offering to allow something that had already failed.
  Measured against `claude.exe` 2.1.228 with a probe server that never answered: the client
  gives up **60.0 s** after asking (60.010 s and 60.005 s, twice in one run). Also measured,
  and the reason this is a fix rather than a smaller deadline: `notifications/progress`
  against the request's own `progressToken` **resets that clock** — the same probe was
  answered at **300.1 s** after 29 beats, and the write went through. So a pending permission
  call is now answered as a `text/event-stream` and beats every 10 s while a card is open,
  and a human has as long as they need. The other half is the one that was visible: the beat
  doubles as a liveness check, so a client that hangs up ends the wait, the console **denies**
  (never allows) and the card becomes a third state — dimmed, no buttons, *"the agent stopped
  waiting."* Every open question closes the same way when the agent process ends.
- **`/panel` is removed.** It summoned a control panel wired to the console's *backdrop*,
  which a conversation has no scrollback to show — so its buttons repainted a terminal tab
  and the panel you clicked appeared to do nothing. `/surface` supersedes it completely by
  driving a rendered surface in the same view. `PanelSpec::drives` is now an `ElementId`
  rather than an `Option`, so a panel that names no target cannot be built; `ArtifactAction`,
  `ConversationOutput::actions` and the console-side loop that consumed them are gone with
  it. `/panel` typed in the composer is now an ordinary message and reaches the agent.

### Console Spike — an agent's permission request arrives as a card you can answer

- **The console now answers "may I?" for everything the agent does.** Three tools bounced on
  permission in the first real session and rendered as red errors, because nothing answered
  approvals. `--permission-prompt-tool` gates **`Bash` as well as MCP tools**, so one card
  covers all of it: the tool, its arguments as fields, and allow · allow & remember · deny.
  The console serves that tool **itself, over loopback HTTP, inside its own process** — the
  fork that matters, because a stdio server is a separate process with no access to the UI
  and every approval would have to cross a boundary and come back. The hook blocks for as
  long as a human takes, on a serve thread that is never the UI's. (This entry originally
  claimed that made a card with *no* timeout possible; the entry above corrects it — the
  client's own deadline is 60 s, and holding it open takes work.) Remembering is ours — there is no upstream persistence — so
  the console keys a decision on the **whole call** rather than the tool, still draws a card
  for one it answered from memory, and puts `forget` on that card; an authority granted once
  and thereafter invisible is worse than being asked every time. Scope is the session, and
  the honesty ledger says so. Verified against the real CLI: a `Write` outside the session's
  scratchpad reached the server as a `tools/call` and the file appeared, and `system/init`
  reported the server connected with **zero** of its 36 model-visible tools mentioning it —
  the handler is withheld from the model *because* the flag names it, which is why the
  console must never serve a second approval-shaped tool.

### Console Spike — the panel drives a rendered surface in its own view

- **`/surface` puts the engine's picture in the transcript, with the controls that drive it
  directly beneath.** The previous change wired a panel in a conversation to real engine
  state and the check on screen produced the finding: the effect appeared on a *different
  tab from the one it was clicked in*, because a conversation has no scrollback for a
  backdrop to band across. A control whose consequence you cannot see from where you are
  sitting is a bad instrument. So the panel now names an `ElementId` it drives, the material
  buttons and three light/exposure knobs change **that element's** look, and a driving
  panel's press is consumed by its surface rather than also repainting the console. One
  `World`, rendered into a target the conversation owns — `Shell::render_source`'s seam,
  so the window behind stays flat and the console still opens looking like an ordinary
  terminal. The rect comes from egui layout rather than from row arithmetic, which is the
  simplification the second front-end was built to buy, and it crosses the crate seam in
  **points** so `pixels_per_point` cancels in `pane_pixels_in` instead of being remembered.
  Bounded from the first line: only surfaces overlapping the viewport are rendered at all,
  four live textures across every tab (≈23 MB at the size this console draws one), evicted
  least-recently-requested with a `[surface]` line naming what went, and one engine render
  per frame — so an idle conversation costs nothing and a dragged slider repaints at full
  rate. The two-renders-in-one-frame hazard is documented rather than hidden: the world's
  clocks are wall-clock and advance by microseconds, but `frame_index` and the TAA/temporal
  history are shared, which is one reason the surface look is a still plane.

### Console Spike — a live control panel as an element in a conversation

- **`/panel` in a conversation tab puts a working egui panel in the transcript** — sliders
  that move and material buttons wired to the console's real look-change path, the same
  `apply_console` a typed `organon console background <name>` reaches. The terminal host
  needs a whole protocol to put a rectangle in its page (a printed gap, a claim,
  absolute-line anchoring, surviving ConPTY); here an artifact is an element in a list that
  draws itself, and that difference is the argument for the second front-end. The transcript
  stays pure: `Body::Artifact` carries a *description* — a title and control **names** —
  while every value a hand can move lives in the view, keyed by `ElementId`, which is what
  those stable ids were for. `/panel` is a temporary seam; the agent summoning a panel with
  a tool call replaces it.

### Console Spike — a claimed rectangle has a kind, and one of them is a working panel

- **`organon console patch --up N --rows M --kind <scene|panel>`.** The writer prints its own
  gap through the ordinary PTY and then says where it is; the console records the rectangle
  and paints it, and never writes into the terminal. `scene` samples the rendered substrate
  through those rows — today's behaviour, unchanged. `panel` puts a **live egui control panel**
  there instead: sliders that move when dragged and buttons wired to the console's real
  look-change path, so clicking `metal` inside the scrollback and typing
  `organon console background metal` are the same call from `apply_console` onwards. The kind
  selects the paint and nothing before it — the claim, the anchor arithmetic and the per-pane
  ledger are common to both, which is where an error would be invisible on screen.

### Console Spike — the console grows a second front-end: a conversation, rendered natively

- **A conversation tab drives an agent over pipes and draws its event stream itself** — no
  PTY, no ConPTY, no character grid. `ORGANON_SHELL_TABS=claude-chat` opens one beside the
  terminal tabs, which are unchanged and remain the universal fallback. A tool call renders
  as a **card** (name, arguments as fields, running/ok/error, clipped output) and an `Edit`
  as a real diff, because both arrive as structure rather than as the text a terminal would
  have printed. Five modules: `agent_event` decodes the NDJSON, `conversation` folds the
  transcript, `agent_map` is the seam between them, `agent_session` owns the child process,
  `conversation_view` draws it. `SHELL_ARCHITECTURE.md` §1.1 owns the shape and the honesty
  ledger says what has and has not been seen running.

### Console Spike — the console can open a hole in its own transcript

- **`organon console block <rows>` reserves a contiguous run of blank rows** in the active
  tab, just below the cursor, and the next prompt lands underneath them. Nothing is painted
  into them yet — a GPU-rendered scene or panel pinned into exactly those rows is the next
  increment. What this one buys is that the rows genuinely **exist**, so text written
  afterwards flows below them instead of over them.
- **The mechanism is the parser the console already owns**, not a second one.
  `TermSession::feed_local` advances `vte::ansi::Processor` against bytes the console
  generated itself — the same call the PTY pump makes — so a fed `\r\n` takes exactly the
  path the shell's own newline takes. A reserved row is an ordinary scrollback row: it ages,
  scrolls, evicts at the cap and reflows on a resize, because it *is* one. Three tempting
  alternatives are wrong and are named in the code: writing newlines to the pty master
  presses Enter at the child, `insert_blank_lines` discards rows off the bottom instead of
  filing them in history, and a bare `grid_mut().scroll_up()` skips damage tracking and
  never moves the cursor.
- **The feed is bracketed like a real pump, and cannot be called any other way.**
  `TermSession::feed_local` is `pub(crate)`, so it is unreachable from the console except
  through `term_view::PaneAnchor::feed_local`, and `PaneAnchor::bracketed` is now the single
  function every parser advance routes through. An unbracketed feed against a full buffer
  can evict lines the scroll anchor never learns about, which would make every absolute line
  index recorded before it permanently wrong, silently.
- **The command reports where the hole went**: `[block] opened 12 rows @ line 1187 (pane 0)`
  on stderr, in `[epochs]`' register, because an arithmetic error in that index is invisible
  until something is painted into the wrong rows.
- **The first console argument that is a number**, which the lane had not carried before. The
  row count is bounded twice on purpose: clap's range is where a person gets a good error
  before a byte is written, and the dispatch checks it again because `ArgKind::Int` has no
  bounds to state and a hand-written sidecar line never met clap. A count that does not parse
  is skipped like an unknown verb rather than clamped — a clamp opens a block nobody asked
  for.
- **Six limits are written down rather than fixed**, all accepted: a width change reflows and
  can drop the block's topmost row outright, eviction erodes a block from the top invisibly
  at the live edge, `\x1b[3J` wipes the scrollback silently, a resize under the alternate
  screen moves the primary grid, feeding under the alternate screen writes into a grid with
  no scrollback, and the once-per-frame sidecar drain is out of band with the PTY byte
  stream — so the index is right only while the child is idle. The in-band fix, a private OSC
  scanned in the pump, is a later increment.

### Console Spike — the parameter ranges are true, and pinned

- **Nine of the 45 scriptable parameters advertised a range the engine does not have.**
  `agent.rs::id_range` and `clip.rs::RANGES` are hand-written mirrors of `params.rs` that
  nothing checked, and they had drifted. `trans_amp_x/y/z` claimed a maximum of 200 against
  the engine's 20 — a factor of ten, and published that way in
  `doc/reference/parameters.md`. `exposure`, `bloom_intensity`, `sss_power`, `irid_scale`
  and `cam_damping` were wrong at one end or both, and `cam_path` offered a twelfth camera
  path that does not exist. All nine now read what `params.rs` says.
- **Nothing errored when they were wrong, which is why they stayed wrong.** An agent told a
  param runs to 200 when it stops at 20 gets no complaint — it gets a silent clamp and a
  look it did not ask for. `recipe.rs` was validating every built-in recipe against the same
  bad bounds, so the guard meant to catch an out-of-range recipe was holding the wrong ruler.
  It reads `id_range`, so it is corrected by the same change; every shipped recipe was
  already inside the true bounds.
- **A test now pins both tables to the parameters themselves.**
  `taper_round_trips_against_the_engine_range` reads every bound off
  `OrganicMathParams::default()` instead of restating it — a third copy of a number is just
  a third thing to drift — so the next disagreement fails the build. Only the id-to-field
  join is hand-written, and the compiler checks that.
- **It also pins the taper, over all 1372 host parameters.** Treating a range as two numbers
  is only honest while the mapping between them is linear, which is an accident of `flin()`
  today rather than a guarantee. The day someone reaches for a skewed range, two numbers
  stop describing the parameter — and this fails then, rather than after the tables have
  quietly gone wrong again.
- Two CC slots are exempt, each for a stated reason: slot 16 carries
  `inc_scale × 10^speed_exp`, an expression rather than a parameter, whose narrower CC span
  is a deliberate playable range; slot 26 has been reserved since the Pulse Depth knob was
  removed. Both are argued at the table and named in the test, so a slot that gains a
  parameter has to be joined rather than left alone.
- `doc/reference/parameters.md` regenerated.
### Console Spike — Tier 4: it scrolls, and it remembers

- **A look applies forward; history keeps its own.** `organon console background <name>`
  now closes the current backdrop look at the line the cursor is on and opens the next one
  below it, so the new look **scrolls in from the bottom** as output pushes the old text up,
  and every older region of scrollback keeps the look it was written under. Scroll up
  through a session with three changes in it and you scroll back through three looks.
  Nothing is ever restyled after the fact — there is no restyle-everything path, on purpose.
- **The picture of a look is taken when it stops being live.** The backdrop texture already
  *is* that look's rendering, so it is copied into a texture of its own the moment the look
  changes, before the next one renders. That is the whole mechanism: no past look is ever
  re-derived or re-rendered, which is what keeps the cost bounded and the history honest.
- **A small, honest, logged cap.** Eight epochs — 63 MiB of pane-sized textures at 1080p,
  253 MiB at 4K, stated rather than described. Past it the two oldest merge, the newer look
  surviving so what is lost is furthest from the cursor, and every eviction prints
  `[epochs] evicted <look> @ line <n> (cap 8)` to stderr. Eight is deliberately small enough
  that the eviction path actually runs in a long session instead of being untested safety
  code.
- **`background world` and `background off` collapse the history** instead of adding a look.
  A live world is not a still life and freezing a frame of it would be a lie labelled a
  look; `off` has no picture at all. The rows written while the backdrop was off keep their
  plain background afterwards, because the epoch that owns them has no picture to paint.
- **Two new pure modules, both testable without a GPU or a window**:
  `organon-shell/src/scroll_anchor.rs` (absolute line indices → viewport bands: emission
  ages a boundary for free, scrolling moves the window not the text, a row resize needs no
  bookkeeping, and the alternate screen is always exactly one band) and
  `native/src/substrate_epochs.rs` (the ledger, the cap, the merge, and the texture
  decisions as data).
- **Fixed on the beat check: the backdrop was sized in points, so history was magnified.**
  The first run on a 225 % display showed wide historical bands as blurred washes with the
  live band crisp. The bands were right — a cached epoch measured pixel-identical to a live
  render of the same look at the same pane size — and the *size* was wrong: the backdrop
  texture was built as `pane_points × remembered_scale`, and the value standing in for a
  scale egui had not reported yet multiplies exactly like a real 100 % display. So the live
  texture spent its first frames at 1100×690 where 2475×1553 was meant, and a look closing
  in that window filed a picture 2.25× too small — which the live texture then outgrew and
  the snapshot never could. The pane is now sized as its **share of the window** applied to
  the swapchain (`scene_input::pane_pixels_in`), so the scale cancels rather than being
  guessed, and a point-sized backdrop is unrepresentable rather than merely unlikely.
- Known and recorded rather than hidden: a resized pane **stretches** cached history into
  its bands (the live band stays exact), the eviction counter under-counts rather than
  over-counts once scrollback is full, and a column resize can slide a band edge by the
  number of wrapped rows above it. `SHELL_ARCHITECTURE.md`'s honesty ledger carries each
  one with its reason.

### Console Spike — Tier 2: a backdrop you can type at

- **`organon console background <name>` changes the console's backdrop live.** Four
  substrate materials (`graphite`, `paper`, `slate`, `metal`) and two lighting rigs
  (`studio`, `daylight`), plus the three sources `world` / `off` / `substrate` — typed in a
  console tab, applied on the next frame, with no window flicker and no terminal
  relayout. `organon console rig <name>` picks the light.
- **A third command transport, because it has a third destination.** Console verbs append
  to their own namespaced sidecar (`<ns>-console.txt`) drained by the **console**, not to
  `cli.txt`, which is drained by the World and cannot reach `Shell` state at all. The
  drain reuses the World's own file-length watermark and its construction-time seed
  verbatim, so a backlog from before the console started never replays while a command
  typed a moment after launch always does. Unknown verbs and unknown names are both
  skipped in silence — that is the format's whole versioning story.
- **The #472 material gate now admits the membrane sheet.** `render.rs` split one
  predicate into `cube_draw` (the bevel morph, unchanged) and `material_draw`, so a flat
  lit plane can carry a procedural map stack — the thing Tier 1 recorded as the reason its
  substrate had no surface variation. A uniform-value gate, not a pipeline one, and inert
  with no material configured.
- **The command service is live in the product for the first time.** `console.background`
  and `console.rig` are registered on `organon-shell`'s `CommandService`, with their
  argument schemas built from the material and rig tables themselves, and every drained op
  is dispatched through it — so each one leaves a `CommandRun` record in a real session
  log, success and rejection alike.
- **One table, one guard.** The `organon` CLI's clap value lists are now asserted equal to
  the tables the renderer draws from, so a name that completes is a name that can be drawn.
  (The three *source* words are pinned by a literal on each side rather than bound — the
  two ends are separate `[[bin]]`s; recorded in `SHELL_ARCHITECTURE.md`'s honesty ledger.)
- Launching with `ORGANON_SHELL_BACKDROP=substrate` still publishes Tier 1's snapshot byte
  for byte: no material is applied until one is named.

### Console Spike — the binary is `organon-console`

- **The artifact is renamed; nothing an identifier is read by changes.** The `[[bin]]`
  target `organon-shell` is now **`organon-console`** — so it is
  `cargo build --release --features shell-edition --bin organon-console`, and the old
  spelling now fails with "no bin target named `organon-shell`". Explicitly *not* renamed,
  because each of these is read by something outside this crate: the **crate**
  `native/organon-shell` (`-p organon-shell` still names it), the cargo **feature**
  `shell-edition`, every **`ORGANON_SHELL_*` environment variable**
  (a shipped flag surface), the **`organon-shell` IPC namespace** (a wire identifier the
  `organon` CLI joins on), `%APPDATA%\OrganonShell`, and `SHELL_ARCHITECTURE.md`'s filename.
  Collapsing those needs deprecation aliases and a coordinated change, not find-and-replace.
- **The console introduces itself as Organon Console.** The window title, the `--help`
  header and usage line, `--version`, the startup banner and the `organon-console:`
  diagnostic prefixes now read the public name from a local `PRODUCT_NAME`, deliberately
  shadowing `EDITION.product_name()` — which stays "Organon Shell" because `organon-core`'s
  `Edition` is shared spine. A test pins the split: the header must not say "Organon Shell",
  and `ORGANON_SHELL_BACKDROP` must still be there.

### Console Spike — Tier 1: the lit substrate

- **A second backdrop source for Organon Shell.** `ORGANON_SHELL_BACKDROP=substrate` puts
  one flat, still, lit plane behind the glyphs instead of the generative world: a pure
  `Shared`-state builder (`substrate_scene`) drawn through the existing
  `RenderPath::Membrane` — no new shader — and framed by a pure narrow-lens camera rig
  (`substrate_camera`) at a 10° vertical FOV, re-framed on every resize. `=1` still selects
  the world, unchanged, because the `organon` CLI's override lane drains inside the world's
  frame path and replacing it would kill the console's live response.
- **The world gained an absolute camera rig.** A third arm on the camera finalization
  overrides centre/yaw/pitch/distance/roll/FOV as a set and latches off the auto-follow while
  it is installed; the FOV clamp floor moved 10° → 4° at *both* of the two sites that clamp it
  (moving one alone does nothing).
- **Fixed: the backdrop was vertically squashed.** The texture was sized to the window and
  then painted at UV 0..1 into a panel 30 points shorter. It is now sized to that panel, one
  frame behind, with the same clamps the Mind editor's viewport uses — which also changes, and
  corrects, the existing `ORGANON_SHELL_BACKDROP=1` rendering.
- The legibility scrim's alpha is now a pure `term_view::scrim_alpha`, with its structural
  floor pinned by a test against hostile input.

---

## Before this repository

### The instrument

The first thing that existed was a plugin. Organon began as a faithful reimplementation
of *Organic Math*, a cube-field visualizer, and the reimplementation immediately raised
the question the whole project has been answering since: what else can this generator do?

- A **VST3/CLAP plugin plus a standalone**, built on nih-plug, with the fullscreen visual
  as a **separate process** reading a memory-mapped snapshot. Two processes was an early
  and load-bearing decision: a host's audio thread can't be blocked by a renderer.
- Host **tempo sync** via a PLL, MIDI CC routing, clip export, and audio-reactive band
  analysis — the parameters move to the music, which is the point of it being a plugin.
- The algorithm itself, isolated as pure unit-tested functions: rotate-then-translate
  composition (mirroring OpenGL's `glRotatef`/`glTranslatef` order, which is what makes
  the motion organic rather than mechanical) and a fourth accumulating strand that
  compounds transforms without reset — the source of the tentacle and helix families.

### The renderer

What began as instanced cubes became a full real-time stack, because each new generator
asked for a way to be *seen*:

- PBR materials, image-based lighting, punctual lights, HDR output with true EDR on
  macOS, tone mapping, palettes, SSAO.
- Screen-space reflections, bounced GI, spectral glass with dispersion, a hardware
  ray-traced path, voxel GI, temporal accumulation, and a post stack.
- 27 generators and a matching set of surface modes — how nodes become geometry: swept
  tubes, metaballs, membranes, voxels, neural tissue, plexus, splats.
- Later arrivals: a time-marched **field engine** (PDEs on a grid), a **kaleidoscope**
  pass, a **creature engine**, a **neural-network generator** with a gallery of
  synthesized graphs, and an HDR starfield driven by the embedded Yale Bright Star
  Catalog.
- An in-app **production recorder** for capturing HDR clips, and a **frame harness**
  (`native/verify`) that turns rendering into pass/fail against committed goldens — the
  only test in the project that can see a picture.

### Organon Mind

Then the engine was pointed at something other than music: a language model.

- A **GGUF reader** that parses a model file's header and tensor directory and draws the
  model's **true wiring** — layers, heads, experts, the residual stream — as a structure
  in the same 3-D engine.
- An **activation ring** for live inference, an **embedded llama.cpp runtime** behind an
  opt-in feature (the default build stays C++-toolchain-free), and a synthetic frame
  writer that exercises the whole live path with zero inference.
- A set of **lenses** — quantitative instrumentation, an inference-geometry atlas,
  concept views — and the commitment that governs all of them: **every displayed quantity
  is labeled with its provenance** (measured / derived / proxy / projection), with a
  standing ledger recording which is which and what is still a proxy.

### The workshop around the code

Roughly half the effort that does not show up on screen:

- The **`organon` CLI** — drive the running instrument from a terminal: read state, set
  parameters, apply recipes, take a snapshot.
- **Preset and clip machinery**, an app-support store, network galleries.
- **Documentation discipline** that is enforced rather than hoped for: architecture docs
  updated in the same change as the code, session hooks that measure doc drift, structure
  drift and the context each session costs.
- **CI** running the full edition matrix, and an automated review agent on every PR.

### Taking the engine apart

With three products sharing one codebase, the monolith was split — carefully, in tiers,
with the acceptance test written down first:

- **`organon-core`** — the host-free spine: math, IPC, params, GGUF, editions. Its
  acceptance test is a dependency check: no plugin framework, no GPU crates, no UI.
- **`organon-render`** — the renderer as its own crate: the surface modules, the shaders,
  the star catalog.
- **`organon-mind`** — Mind's own code, free of the plugin framework.
- The `World` god-struct partitioned into ownership clusters, then made
  compiler-enforced rather than conventional.
- A measured answer to "are these crates actually independent?" — cross-crate churn,
  computed by a script rather than asserted, because a number nobody can re-derive rots.

### More platforms

- A **Windows port**: bundle and deploy scripts, DLL-lock handling, HiDPI for the
  standalone, true HDR through the scRGB swapchain, CUDA for the embedded runtime, and
  Windows legs in CI.
- A **WebGPU port** of the same `math.rs` compiled to WASM — built, then deliberately
  **parked**. It is not in this repository: development is Rust-native only, and shipping
  a parked port would have meant publishing a second, staler answer to the same question.

### Organon Shell

The third product: an agent-operating workstation. Founded, built as a five-panel
workspace, falsified by actually using it, and re-founded as a **GPU-composited
terminal** — its own crate and a third `Edition` over the same engine.

### Open-sourcing

The work that produced this repository: an audit of the whole tree, a licensing decision
(permissive engine, GPL plugin — see [`LICENSING.md`](LICENSING.md)), and an export tool
that materializes this public tree from the private monorepo with a byte-identity gate
and a fatal privacy scan, so that what is published is exactly what was reviewed.
