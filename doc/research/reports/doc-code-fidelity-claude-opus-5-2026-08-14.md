---
brief: doc-code-fidelity
model: claude-opus-5
model_surface: GitHub Actions, repository checked out
run_date: 2026-08-14
commit: c2ed970
status: unreviewed
adjudicated_by:
notes: automated run in CI with file access; claims are checkable but NOT checked — adjudicate before citing
---

## Summary

Everything below was checked against the tree at `c2ed970`; every finding cites both
halves.

The drift is not random. It clusters on **one event: the crate extractions**
(`organon-core` #626 T3, `organon-render` #626 T4, `organon-scene` organon#49 T3). The
per-file discipline held — §19's rows are individually accurate about *what each module
does*. What rotted is everything that **counts or locates**: `ARCHITECTURE.md` §2's
directory tree still describes a single `native/src` holding ~85 `.rs` and 54 `.wgsl`
(actual: 45 and 4) and names two crates that no longer exist; §19's file map leaves ~48
relocated modules unprefixed, so they read as `native/src/*`; and `organon-scene` — a
landed workspace member, 6 files, ~5k lines — is absent from `CLAUDE.md`'s repository
map, `README.md`'s shape, and `LICENSING.md`'s licence table, each of which claims to be
exhaustive.

Two findings would actively mislead an agent: `ARCHITECTURE.md:1519` prescribes a bare
`cargo test`, which the same document's §19.0.1 calls a coverage loss; and
`ARCHITECTURE.md:626` says 26 generators against a code-pinned 27.

The generated `doc/reference/` guarantee holds. `doc/guide/` is clean.

## Findings

### The findings table

| `document:line` | What it claims | `source:line` | What is actually true | Severity |
|---|---|---|---|---|
| `ARCHITECTURE.md:1519` | `cargo test` is the test command (`# math + naga WGSL + layout goldens`) | `ARCHITECTURE.md:1749`, `README.md:51`, `CONTRIBUTING.md:70`, `CLAUDE.md` | A bare `cargo test` runs the root package only. The same document's §19.0.1 calls this "how a coverage loss hides in plain sight"; README, CONTRIBUTING and CLAUDE.md all say `--workspace`. §15 is the section a reader reaches first. | **breaks an agent** |
| `ARCHITECTURE.md:626` | "### The 26 generators" | `native/organon-core/src/params.rs:280` | `pub const ALL: [GeneratorMode; 27]`. The table *directly underneath the heading* runs id 0–26 = 27 rows, and `ARCHITECTURE.md:531` says "27 generators" in the same file. | **breaks an agent** |
| `ARCHITECTURE.md:98`–`:99` | `native/organon-wasm/` and `native/organon-manifest/` are directories in the repository layout | `ls native/` | Neither exists. The workspace members are `xtask`, `organon-core`, `organon-mind`, `organon-render`, `organon-scene`, `organon-shell` (`native/Cargo.toml:10`). | **breaks an agent** |
| `ARCHITECTURE.md:90` | `src/` holds "~85 .rs + 54 .wgsl" | `native/src/` | 41 `.rs` + 4 `.rs` under `bin/` = 45; 4 `.wgsl` (`capture`, `nca`, `overlay`, `rt_debug`). 54 is the *whole-tree* shader count; 50 of them are in `organon-render/src`. | **misleads a reader** |
| `ARCHITECTURE.md:89` | "native/ THE CRATE — both products" | `native/organon-core/src/edition.rs:3`, `ARCHITECTURE.md:190` | Three products. `edition.rs`: "One crate family, three products." `ARCHITECTURE.md:190` itself: "**Shell** is the third product". | misleads a reader |
| `ARCHITECTURE.md:112` | "That is the whole tree" (§2's listing) | `native/Cargo.toml:10` | §2's listing names none of `organon-core`, `organon-render`, `organon-mind`, `organon-scene`, `organon-shell` — five of the six workspace members. | **misleads a reader** |
| `ARCHITECTURE.md:1701` | "As of Tier 3 there are **two** members that matter" | `ARCHITECTURE.md:1704`–`:1710` | The table immediately below has four member rows plus the root crate — and omits `organon-shell` entirely (`native/organon-shell/Cargo.toml:24`, 30 `.rs` files). | misleads a reader |
| `ARCHITECTURE.md:1710` | `organic-math-native` depends on "all three + the world" | `ARCHITECTURE.md:1706`–`:1709` | Four crates are listed above that row (`organon-core`, `organon-mind`, `organon-render`, `organon-scene`), and a fifth member exists. | cosmetic |
| `ARCHITECTURE.md:1720` | "**When `organon-render` lands** (Tier 4's remaining half), it will depend on `organon-mind`" | `native/organon-render/Cargo.toml:37`–`:46` | It has landed — 40 modules, 50 shaders — and its dependencies are `organon-core`, `wgpu`, `glam`, `bytemuck`, `image`, `half`. No `organon-mind`. `ARCHITECTURE.md:1708` (fourteen lines above) already describes it as present. | misleads a reader |
| `ARCHITECTURE.md:171` | "### 4.1 Editions — one crate, **two** products" | `native/organon-core/src/edition.rs:3`; `ARCHITECTURE.md:186` | Three: `Edition` is `Full` \| `Mind` \| `Shell`, and §4.1's own body and table (`:203`–`:207`) have three columns. | cosmetic |
| `ARCHITECTURE.md:177`–`:178` | The algorithm, shaders, `Shared`, preset store "and — critically — the **visual binary** are byte-identical between them" | `native/organon-core/src/edition.rs:13`–`:18` | `edition.rs` explicitly retracts exactly this sentence: "This paragraph used to add 'and — critically — the separate-process visual binary are byte-identical between them', **and that stopped being true in #554 Tier 4**". `ARCHITECTURE.md` still carries the retracted clause. | **misleads a reader** |
| `ARCHITECTURE.md:1694` | "Every `.rs` in `native/src` has a row here — that is the point of the table" | `native/src/console_catalog.rs:1`, `native/src/baseview_platform.rs:1`, `native/src/shell_main.rs` | Three files have no row. `console_catalog.rs` is 745 lines and is the console's parameter bridge (its own header: the thing that stops a fourth hand-written mirror of `params.rs` being born); it appears **nowhere** in `ARCHITECTURE.md`. `baseview_platform.rs` (185 lines, the baseview arm of the `EguiPlatform` seam) likewise. `shell_main.rs` is named in §4 and §19.0.1 but has no §19 row. | **breaks an agent** |
| `ARCHITECTURE.md:1863`, `:1866`, `:1872`, `:1900`, `:1909`, `:1948`, `:1949` (≈48 rows) | Unprefixed file names in §19's map, whose stated scope (`:1694`) is `native/src` | `native/organon-core/src/{math,ipc}.rs`, `native/organon-mind/src/mind_*.rs`, `native/organon-render/src/*.rs`, `native/organon-scene/src/overlay_meta.rs` | More than 40 rows name files that left `native/src`. The table *does* prefix five rows correctly (`organon-core/src/edition.rs` etc.), which is what makes the unprefixed majority read as a location claim. `ARCHITECTURE.md:129` and `:1709` state the correct homes for `math.rs` and `overlay_meta.rs` in the same document; `MIND_ARCHITECTURE.md:81`,`:254`,`:257`–`:259` give the correct `native/organon-mind/src/` paths for the `mind_*` files. | **misleads a reader** |
| `ARCHITECTURE.md:1534` | "Five suites: **lib / visual / ctl / popup-contract / wgsl**" | `native/tests/`, `native/organon-render/tests/wgsl.rs` | `native/tests/` holds four files — `egui_popup_contract.rs`, `substrate.rs`, `vecbuild_ipc.rs`, `wgsl.rs` — and `organon-render` carries a second `wgsl.rs`, plus five member crates with their own unit-test suites. | cosmetic |
| `ARCHITECTURE.md:1537` | "**`STATUS.md` carries the current count and per-suite split** — read it there, not here" | `ls STATUS.md` | No `STATUS.md` in this repository. §2:112–114 enumerates the upstream-only paths (`web/`, `src/`, `site/`, `original_code/`, `scripts/`, `brand/`, `songs/`) and does not include it, so a reader has no way to learn the pointer is dead. | misleads a reader |
| `ARCHITECTURE.md:93` | `tests/` is "wgsl.rs (naga, offline) · egui_popup_contract.rs" | `native/tests/` | Also `substrate.rs` and `vecbuild_ipc.rs` — the latter being the file §19.0.1:1848 introduces as the home of the relocated `Shared`-layout tests. | cosmetic |
| `CLAUDE.md:281`–`:307` | The repository map, closed by "That is all of it" | `native/Cargo.toml:10`; `native/organon-scene/src/` | `native/organon-scene/` — a workspace member with 6 `.rs` files and ~5,067 lines, added 2026-08-14 — has no entry. Every other member does. | **misleads a reader** |
| `README.md:112`–`:121` | "The shape of it" listing, closed by "That is the whole repository" | `native/organon-scene/Cargo.toml:46` | Same omission: `native/organon-scene` is missing from the five-line map. | misleads a reader |
| `README.md:145`–`:147` | The MIT-OR-Apache engine is "`organon-core`, `organon-render`, `organon-mind`, `organon-shell`, **and the WASM/codegen/build tools**" | `ls native/`; `native/organon-scene/Cargo.toml:49` | `organon-scene` (`license = "MIT OR Apache-2.0"`) is omitted, and the "WASM/codegen tools" it names in its place (`organon-wasm`, `organon-manifest`) do not exist in this repository. | misleads a reader |
| `LICENSING.md:8` | The licence table's engine row: `organon-core` · `organon-render` · `organon-mind` · `organon-shell` | `native/organon-scene/Cargo.toml:49` | `organon-scene` is absent from the table entirely, so the document that `CLAUDE.md` invariant #6 tells you to read *before touching a `license` field* does not say what licence one of the six members carries. | **misleads a reader** |
| `.claude/hooks/doc-rules.sh:28` | `doc/arch/topology.md`'s triggers: `native/Cargo.toml`, `organon-core/Cargo.toml`, `organon-mind/Cargo.toml`, `organon-render/Cargo.toml`, `crate-churn.py` | `CLAUDE.md` durable-docs table ("the crate graph and what may depend on what"); `native/organon-scene/Cargo.toml`, `native/organon-shell/Cargo.toml` | Two of the six members' manifests are not triggers, so a dependency change inside `organon-scene` or `organon-shell` — which is precisely "what may depend on what" — fires neither the Stop reminder nor the staleness check. (Adding a *member* still fires, via `native/Cargo.toml`.) | misleads a reader |
| `native/organon-core/src/edition.rs:46` | Behaviour 6: "**`pub mod world`** is `#[cfg(feature = "mind-edition")]`" | `native/src/lib.rs:160` | `#[cfg(any(feature = "mind-edition", feature = "shell-edition"))]`. `ARCHITECTURE.md:1908` has the current gate ("Shell #6 T1 widened it"). This matters because `CLAUDE.md` names `edition.rs`'s module doc as **the authority** on what an edition drives. | misleads a reader |
| `ARCHITECTURE.md:1706` | `organon-core` is "`math`, `ipc`, `params`, `gguf`, `gguf_data`, `edition`, `tabs`"; `CLAUDE.md:287` gives a shorter list | `native/organon-core/src/kind.rs` | Both omit `kind.rs`, which `ARCHITECTURE.md:1869` documents with its own row eight lines later. | cosmetic |

### The shape of it

Three observations that the table does not carry on its own.

**1. The drift is one event, not accumulated neglect.** Every finding above except the
generator count and the `world` gate traces to a crate extraction. The
same-change discipline `CLAUDE.md` describes is visibly working at the level it was
designed for — a *file* changes, its *row* gets updated — and visibly not working one
level up, where a file **moves** and every count, tree and licence table that summarised
where files live goes stale at once. The hooks cannot catch this: `doc-rules.sh` maps
docs to *paths*, and a path that no longer exists triggers nothing.

**2. The documents' own defences are load-bearing and worked.** `ARCHITECTURE.md:350`
tells the reader not to trust its own `Shared` size and version and hands them two grep
commands — I ran both, and both numbers (`8512` at `native/src/param_table.rs:2637`,
`0x0285` at `native/organon-core/src/ipc.rs:2258`) are correct. `ARCHITECTURE.md:1537`
does the same for the test count. `edition.rs:13` retracts its own stale sentence rather
than deleting it. **Where a document said "measure this, don't read it", the number was
right; where it stated a number flatly, the number was usually wrong.** That is the
strongest single pattern in this audit and it is the one worth generalising from.

**3. The absent crate is a worse failure than the wrong number.**
`organon-scene` landed on 2026-08-14. It is missing from `CLAUDE.md`'s map,
`README.md`'s map and `LICENSING.md`'s table, and its four `substrate_*.rs` modules have
no row in `ARCHITECTURE.md` §19. An agent reading `CLAUDE.md` (auto-loaded) and
`ARCHITECTURE.md` (injected) has no way to learn the crate exists — where a stale count
at least points at a real thing.

### What I checked and found correct

Absence of findings in these areas is a result, not silence. Each was opened and read.

- **The generated-reference guarantee.** `native/src/cli.rs:1390`
  (`generated_reference_is_current`) reads every `(name, contents)` pair from the pure
  `docs_files()` (`cli.rs:1145`) and compares it byte-for-byte against
  `doc/reference/<name>` via `docs_match` (`cli.rs:1139`, exact equality modulo CRLF).
  It lives in the **root package**, so it runs under a bare `cargo test` as well as
  `--workspace`, and it needs no GPU. The guarantee holds. Its one gap: it checks the
  six files it emits and would not notice an *extra* hand-added file in `doc/reference/`.
- **`Shared` size and layout version** — 8512 / `0x0285`, both as documented (above).
- **The binary set.** All seven `[[bin]]` targets are in the root manifest, at the names
  and gates `ARCHITECTURE.md` §4:157–166 gives, including `required-features` on
  `organon-mind` (`mind-edition`), `organon-console` (`shell-edition`) and
  `organic-math-mind-runtime` (`embedded-llm`). "one crate → seven binaries" (`:153`) is
  right.
- **`organon-render`'s counts.** `CLAUDE.md:291`, `README.md:115`,
  `ARCHITECTURE.md:1708` and `doc/arch/render.md:19` all say "36 surface submodules …
  50 shaders": `native/organon-render/src/` holds 50 `.wgsl` and 40 `.rs` — `lib`,
  `render`, `axes`, `chamber` plus exactly 36 surface modules. Four documents agreeing
  with the tree.
- **The hook inventory.** `.claude/settings.json` registers exactly one
  `load-*-doc.sh` (`load-architecture-doc.sh`), matching `CLAUDE.md`'s "Exactly ONE doc
  is SessionStart-injected". `CLAUDE.md`'s ⏸ paragraph is accurate line by line:
  `load-web-architecture-doc.sh` is present and unregistered;
  `web-architecture-doc-check.sh` and `status-week-check.sh` are registered and inert;
  `doc-coherence.sh:52` carries `[ -f "$f" ] || continue` and
  `status-week-check.sh:34` carries `[ -f "$status_file" ] || exit 0`; neither `web/`
  nor `STATUS.md` is in the tree. `.claude/skills/organon-cli` is a real directory.
- **`CLAUDE.md`'s doc→trigger table** matches `.claude/hooks/doc-rules.sh:27`–`:32`
  row for row (the render row's enumeration is a summary of a broader glob, not a
  contradiction).
- **`SECURITY.md`.** Every claim I could check holds. Notably `:46`: "`parse_url`
  enforces only the `http://` scheme, **not** that the host is loopback" —
  `native/src/agent.rs:1374` strips `http://` and accepts any authority, and the
  function's own doc comment (`:1368`) records the same gap in the same words.
- **`CONTRIBUTING.md`.** `VST3_CLASS_ID` and `CLAP_ID` are in `native/src/lib.rs`
  (`:11038`, `:11047`). `every_actuatable_id_has_a_gloss` exists
  (`native/src/agent.rs:2767`). `cargo run --bin organon -- docs --check` is real
  (`native/src/bin/ctl.rs:262`). CI runs the matrix it describes
  (`.github/workflows/ci.yml`: `default-edition`, `mind-edition`, `shell-edition`,
  `windows-crosscheck`, `windows`).
- **`doc/guide/`.** Every command and flag I sampled exists: `bundle.sh --install`
  (`native/bundle.sh:22`), `deploy.sh --dest` (`native/deploy.sh:34`), and the whole
  CLI vocabulary in `cli.md` — `status`, `catalog --manual`, `describe`, `recipes`,
  `recipe --dry-run`, `get --all`, `watch --ms`, `release`, `snap -o`, `docs --check`,
  and the negative-value case at `cli.md:64` (`allow_negative_numbers`,
  `ctl.rs:182`). `CLAUDE.md` says `doc/guide/` "describes *mechanisms*, never counts";
  a grep for counts of generators/surfaces/materials/shaders/params across all six
  files returns nothing. **`doc/guide/` is the cleanest document set in the repo.**
- **`MIND_ARCHITECTURE.md`** carries current `native/organon-mind/src/` and
  `native/organon-core/src/` paths throughout — it is *more* current than
  `ARCHITECTURE.md` §19 on the same files.
- **`SHELL_ARCHITECTURE.md`** names the less-obvious `organon-shell` modules
  (`mcp_http.rs`, `block_anchor.rs`, `scroll_anchor.rs`, `agent_map.rs`,
  `mock_agent.rs`, `text_diff.rs`); its coverage of that crate is good.
- **`UiTab::ALL`** is 8 (`native/organon-core/src/tabs.rs:149`), matching
  `ARCHITECTURE.md:207`'s "all 8".

### How many I missed, and why

**My estimate: 15–30 further findings of this class, weighted heavily toward
`SHELL_ARCHITECTURE.md` and `doc/arch/render.md`.** Reasons, in order of size:

1. **I read two of the six durable docs closely.** `SHELL_ARCHITECTURE.md` is 3,252
   lines — larger than `ARCHITECTURE.md` — and `doc/arch/render.md` is 1,206. I
   spot-checked both and read neither. `SHELL_ARCHITECTURE.md` covers the fastest-moving
   crate in the tree (11 of the last 30 commits are Console/Shell work), which is where
   drift concentrates.
2. **I checked locations and counts, not behaviour.** A row saying `fx.rs` is
   "NPR / DoF / lens FX / grade / feedback" would need the file read to falsify. §19 has
   ~90 such rows and I verified the *paths*, not the descriptions. Behavioural drift is
   the more valuable kind and I have almost no coverage of it.
3. **I did not compile anything.** Claims of the form "this is enforced by a test" were
   checked by reading the test, not by running it. A test that exists but is `#[ignore]`d
   or whose assertion is vacuous would read as correct to me.
4. **`doc/`'s 20-odd Console/Mind design documents were out of scope** and I left them
   alone, but they are checked in and several (`doc/console_discover_schema.md`,
   `doc/how_organon_works.md`) carry counts. `doc/how_organon_works.md:905` states
   "~1,170 … captured by presets", "27 generators", "10 surface modes", "8 material
   types", "8 binaries" — the four I could cheaply check are right, the preset count I
   did not check.
5. **Recall bias toward the mechanically checkable.** Counts and paths are cheap; a
   claim like "the seam is `EventResponse` + `attach_gpu`/`on_window_event`" is not. The
   findings above are therefore skewed toward the *cheapest* class of defect, which is
   also the least harmful class. I would expect a careful human reader of
   `doc/arch/render.md` alone to find several worth more than any single row above.

## What I could not determine

- **Whether the documented behaviour is the actual behaviour.** This is a GPU
  visualizer and a plugin. I cannot render a frame, load it in a DAW, or run the
  editor. Every claim in `ARCHITECTURE.md` §9–§14 and all of `doc/arch/render.md` about
  what a pass *produces* is outside what a checkout can answer. Answering it needs
  `native/verify.sh` on a GPU host and the Mac/Ableton pass.
- **Whether the tests named as guarantees actually pass at this commit.** I read
  `generated_reference_is_current`, `every_actuatable_id_has_a_gloss`,
  `shared_layout_is_stable` and `host_func_name_mirrors_core`; I did not run
  `cargo test --workspace`. A green CI run on this commit would answer it. (The
  workflow is `pull_request` + `workflow_dispatch` only, so this branch may have no
  run.)
- **Whether the `mind-edition` / `shell-edition` builds compile.** Default-off features
  are not built by anything I could run here, and `ARCHITECTURE.md:1524`'s own warning
  says a green default suite says nothing about them. Needs the CI matrix.
- **Whether `doc/reference/` contains a hand-added file the guard would not see.**
  `generated_reference_is_current` iterates `docs_files()` and never lists the
  directory, so an extra page would be invisible to it. There are six files and
  `docs_files()` emits six, but I could not run the test to confirm the pairing —
  I matched them by reading.
- **Whether the ~48 unprefixed §19 rows are understood-as-shorthand by the
  maintainer.** I report them as a location claim because §19:1694 states the table's
  scope as `native/src` and five rows carry explicit crate prefixes, which sets the
  convention. If the intent is "unprefixed = somewhere in the workspace", the finding
  reduces to a documentation-convention gap rather than a wrong path. Only the
  maintainer can settle that.
- **The cross-crate churn number** (`doc/arch/topology.md:16`, 73.6% at `7e19bc8d`).
  The document tells you to re-run `native/tools/crate-churn.py` rather than trust the
  line, and warns the number will have risen because of organon#49. I did not run it,
  so I cannot say whether the current reading is drift or agreement — and per the
  document's own framing, a stale reading there is not a defect.
- **Whether the `organon-scene` omissions are oversight or a landing still in
  progress.** The crate landed the same day as the dispatch commit. The omission is
  real either way; whether it is *drift* depends on intent I cannot read.

## Claims

C1. [verified] (high) — `ARCHITECTURE.md:626`'s heading "The 26 generators" is wrong; there are 27, pinned in code. — `ARCHITECTURE.md:626` vs `native/organon-core/src/params.rs:280` (`pub const ALL: [GeneratorMode; 27]`)
C2. [verified] (high) — The same document says 27 twenty-eight lines earlier, so `ARCHITECTURE.md` contradicts itself about the generator count. — `ARCHITECTURE.md:531` vs `ARCHITECTURE.md:626`
C3. [verified] (high) — `ARCHITECTURE.md:1519` prescribes a bare `cargo test`, the exact command §19.0.1 of the same file calls a silent coverage loss. — `ARCHITECTURE.md:1519` vs `ARCHITECTURE.md:1749`–`:1758`
C4. [verified] (high) — `ARCHITECTURE.md:98`–`:99` lists `native/organon-wasm/` and `native/organon-manifest/`, neither of which exists in the tree. — `ARCHITECTURE.md:98`–`:99` vs `ls native/`, `native/Cargo.toml:10`
C5. [verified] (high) — `ARCHITECTURE.md:90` says `native/src` holds "~85 .rs + 54 .wgsl"; it holds 45 `.rs` (41 + 4 in `bin/`) and 4 `.wgsl`. — `ARCHITECTURE.md:90` vs `native/src/{capture,nca,overlay,rt_debug}.wgsl`
C6. [verified] (high) — `ARCHITECTURE.md:112` closes a repository layout that names none of the five engine member crates with "That is the whole tree". — `ARCHITECTURE.md:88`–`:114` vs `native/Cargo.toml:10`
C7. [verified] (high) — `ARCHITECTURE.md:177`–`:178` still asserts the visual binary is byte-identical across editions; `edition.rs` explicitly retracts that sentence as untrue since #554 T4. — `ARCHITECTURE.md:178` vs `native/organon-core/src/edition.rs:13`–`:18`
C8. [verified] (high) — `ARCHITECTURE.md:1694` promises a §19 row for every `.rs` in `native/src`; `console_catalog.rs` (745 lines) and `baseview_platform.rs` (185 lines) have none, and `console_catalog.rs` appears nowhere in the document. — `ARCHITECTURE.md:1694` vs `native/src/console_catalog.rs`, `native/src/baseview_platform.rs`
C9. [verified] (medium) — `shell_main.rs` is named in §4 and §19.0.1 but has no §19 file-map row. — `ARCHITECTURE.md:166`,`:194`,`:519` vs `ARCHITECTURE.md:1861`–`:1951`
C10. [verified] (high) — More than 40 rows in §19's file map name files unprefixed that no longer live in `native/src`, against a table whose stated scope is `native/src` and which prefixes five rows explicitly. — `ARCHITECTURE.md:1863`(`math.rs`), `:1866`(`ipc.rs`), `:1872`(`mind_ui.rs`), `:1900`(`chamber.rs`), `:1909`(`render.rs`), `:1948`(`axes.rs`), `:1949`(`overlay_meta.rs`) vs `native/organon-core/src/`, `native/organon-mind/src/`, `native/organon-render/src/`, `native/organon-scene/src/`
C11. [verified] (high) — §19's `overlay_meta.rs` row contradicts §19.0's own crate table forty lines above, which places `overlay_meta` in `organon-scene`. — `ARCHITECTURE.md:1949` vs `ARCHITECTURE.md:1709`
C12. [verified] (high) — `MIND_ARCHITECTURE.md` gives correct `native/organon-mind/src/` paths for the same `mind_*` files `ARCHITECTURE.md` §19 leaves unprefixed — a direct cross-document contradiction. — `MIND_ARCHITECTURE.md:81`,`:254`,`:257`–`:259` vs `ARCHITECTURE.md:1872`,`:1874`,`:1876`–`:1877`,`:1888`,`:1890`
C13. [verified] (high) — `CLAUDE.md`'s repository map omits `organon-scene`, a landed workspace member with 6 files and ~5,067 lines, then closes with "That is all of it." — `CLAUDE.md:281`–`:307` vs `native/Cargo.toml:10`, `native/organon-scene/src/`
C14. [verified] (high) — `README.md`'s "The shape of it" omits `organon-scene` and closes with "That is the whole repository." — `README.md:112`–`:121` vs `native/organon-scene/Cargo.toml:46`
C15. [verified] (high) — `LICENSING.md`'s licence table has no row for `native/organon-scene`, so the document `CLAUDE.md` invariant #6 points at does not state one member's licence (`MIT OR Apache-2.0`). — `LICENSING.md:8` vs `native/organon-scene/Cargo.toml:49`
C16. [verified] (medium) — `README.md:145`–`:147` attributes the permissive licence partly to "the WASM/codegen/build tools", which are not in this repository. — `README.md:146` vs `ls native/`
C17. [verified] (high) — `ARCHITECTURE.md:1701` says "two members that matter" directly above a table with four member rows, and the table omits `organon-shell` entirely. — `ARCHITECTURE.md:1701` vs `ARCHITECTURE.md:1704`–`:1710`, `native/organon-shell/Cargo.toml:24`
C18. [verified] (medium) — `ARCHITECTURE.md:1720` speaks of `organon-render` in the future tense ("when it lands") and predicts a dependency on `organon-mind` that does not exist; the crate is present with six dependencies, none of them `organon-mind`. — `ARCHITECTURE.md:1720` vs `native/organon-render/Cargo.toml:37`–`:46`
C19. [verified] (medium) — `ARCHITECTURE.md:171`'s §4.1 heading says "two products" while its body, its table and `edition.rs` all say three. — `ARCHITECTURE.md:171` vs `ARCHITECTURE.md:186`,`:203`–`:207`, `native/organon-core/src/edition.rs:3`
C20. [verified] (medium) — `ARCHITECTURE.md:89` calls `native/` "THE CRATE — both products"; there are three products and six workspace members plus the root. — `ARCHITECTURE.md:89` vs `native/Cargo.toml:10`, `native/organon-core/src/edition.rs:3`
C21. [verified] (medium) — `edition.rs`'s module doc — which `CLAUDE.md` names as the authority on edition behaviour — states behaviour 6 as `#[cfg(feature = "mind-edition")]`; the gate is `any(mind-edition, shell-edition)`. — `native/organon-core/src/edition.rs:46` vs `native/src/lib.rs:160`
C22. [verified] (medium) — `ARCHITECTURE.md:1537` sends the reader to `STATUS.md` for the test count; `STATUS.md` is not in this repository and is not in §2's list of upstream-only paths. — `ARCHITECTURE.md:1537` vs `ls STATUS.md`, `ARCHITECTURE.md:112`–`:114`
C23. [verified] (medium) — `ARCHITECTURE.md:1534`'s "Five suites" and `:93`'s two-file `tests/` listing both predate `substrate.rs` and `vecbuild_ipc.rs`, and the second `wgsl.rs` in `organon-render`. — `ARCHITECTURE.md:93`,`:1534` vs `native/tests/`, `native/organon-render/tests/wgsl.rs`
C24. [verified] (medium) — `doc/arch/topology.md`'s hook triggers omit `organon-scene/Cargo.toml` and `organon-shell/Cargo.toml`, so a dependency change in either member — the doc's own subject — fires no reminder. — `.claude/hooks/doc-rules.sh:28` vs `native/organon-scene/Cargo.toml`, `native/organon-shell/Cargo.toml`
C25. [verified] (high) — The `doc/reference/` guarantee holds: `generated_reference_is_current` compares every generated page byte-for-byte against disk, is pure, and lives in the root package so a bare `cargo test` runs it. — `native/src/cli.rs:1390`, `:1145` (`docs_files`), `:1139` (`docs_match`)
C26. [inferred] (medium) — That guard cannot detect a hand-added extra file under `doc/reference/`, because it iterates `docs_files()` and never lists the directory. — `native/src/cli.rs:1395`
C27. [verified] (high) — `ARCHITECTURE.md`'s `Shared` numbers are correct, and the two grep commands it hands the reader both return what it says. — `ARCHITECTURE.md:349` vs `native/src/param_table.rs:2637` (8512), `native/organon-core/src/ipc.rs:2258` (`0x0285`)
C28. [verified] (high) — Four documents agree with the tree on `organon-render`: 50 shaders and 36 surface submodules beyond `lib`/`render`/`axes`/`chamber`. — `CLAUDE.md:291`, `README.md:115`, `ARCHITECTURE.md:1708`, `doc/arch/render.md:19` vs `native/organon-render/src/` (50 `.wgsl`, 40 `.rs`)
C29. [verified] (high) — `SECURITY.md`'s account of the agent's HTTP client is accurate: `parse_url` checks the scheme and not the host. — `SECURITY.md:46` vs `native/src/agent.rs:1374`–`:1377`
C30. [verified] (high) — `doc/guide/`'s six files contain no counts of generators, surfaces, materials, shaders or parameters, matching the rule `CLAUDE.md` sets for them, and every CLI command and script flag I sampled there exists. — `doc/guide/cli.md:20`–`:122`, `doc/guide/getting-started.md:36`,`:43` vs `native/src/bin/ctl.rs:111`–`:265`, `native/bundle.sh:22`, `native/deploy.sh:34`
C31. [inferred] (high) — The drift found here is concentrated on the crate extractions (#626 T3/T4, organon#49 T3): per-file rows survived the moves, while every count, directory tree and licence table that summarised where files live went stale together. — findings C4–C17 taken as a set
C32. [inferred] (medium) — Where a document told the reader to measure rather than read (`ARCHITECTURE.md:350`, `:1537`, `doc/arch/topology.md:19`, `edition.rs:13`), the underlying facts were current; the flatly-stated numbers were the ones that rotted. — contrast C27 with C1, C5, C23
C33. [speculative] (medium) — I estimate 15–30 further findings of this class remain, concentrated in `SHELL_ARCHITECTURE.md` (3,252 lines, fastest-moving crate) and `doc/arch/render.md` (1,206 lines), neither of which I read end to end. — none
