# PBR text — coordinator handoff

> **What this is.** The state of the PBR text project (organon#217) as left by the coordinator
> session titled **"PBR text coordinator"** on 2026-09-03, written so a fresh session can take the
> coordinator's chair with no other context. It is *transient* — the durable state is
> `doc/pbr_text_engine.md` (§15 is the plan), `organon#217` (the running log, one comment per
> wave), and the changelog fragments. When the project ships, delete this file.
>
> **Reading order for the new coordinator:** this file → `git show origin/main:doc/pbr_text_engine.md`
> (§15 first) → `gh issue view 217 --comments` (the last five comments) →
> `.claude/skills/coordinate-sessions/SKILL.md`.

---

## 1. Standing rules from James

These are his, not the project's; they override anything below.

1. **No cards.** Workers are spawned with the `Agent` tool (`isolation: "worktree"`,
   `run_in_background: true`), never `spawn_task` chips — a chip needs a click and blocked the
   work for hours once. Every worker so far has been a subagent since that ruling.
2. **The demo says ORGANON, never OMARCHY.** The text is `native/assets/text/organon.txt`
   (landing in the W19 PR; until then the coordinator's scratchpad copy — regenerate from the
   5×7 pixel font in that PR's README if lost).
3. **Show it in the context it will run in.** A near-black room with the words on it, like the
   screensaver. Never glyphs over the generated landscape or the atmosphere sky. The nine
   `faceplate` fields that make the room dark (`atmos_enabled`, `bg_visible`, `fx_enabled`,
   `hal_amount`, `ml_enabled`, `ml_intensity`, `ml_radius`, `ml_count`, `ml_restir`) are being
   put on the CLI vocabulary by W19; until that lands, do not send a frame taken without them.
4. **The two claims to keep honest:** "green and ready to try" without a GPU look, and the
   plates (`doc/images/`) are the claim being measured against — §15's table is the gap.
5. He wants to see it. Send frames with `SendUserFile` as they happen, not at the end.

## 2. Where `main` is

`origin/main @ 90ca793` at the time of writing (check `git log origin/main -1`). Everything from
the spec (#218) through #243 is merged and was gated on `main` after each merge:

| Tier | PR(s) | State |
|---|---|---|
| Spec, ttfx correction, §15 gap-to-plates | #218 #221 #228 | done |
| T1 ring + tiles + producer crate | #224 | done |
| T2 legibility harness (+ CRLF fix) | #223 #225 | done |
| T3 look controls, held camera, `faceplate` | #230 | done |
| T5 converge on hold | #227 | done |
| T6 coaxial glass capsule | #222 | done |
| §7 sub-cell (ttfx#1 + producer) | organonart/ttfx#1, #229 | done |
| T8 tracer sees emission | #232 | done |
| T9 tile: profile, dark tiles, lanes wired | #233 #236 #239 | done |
| T10 glyphs as lights + rig | #234 | done |
| T11 persistence | #231 | done |
| T12 sub-cell rendering (slide/cut/exact) | #238 | done |
| T13 legibility gate on a real render | #240 | done |
| T14 preset ladder (five rungs) | #242 | done |
| CLI vocabulary for the text look | #235 | done |
| Blend clock on producer time | #241 | done |
| Plexus keeps the emits | #243 | done |
| Registry tests read the machine store (unrelated red CI, fixed) | #226 | done |

**In flight when this was written** — check `gh pr list` first; they may have landed:

- **W18 — #237, the EDR surface panic.** `Surface::configure` asks for `Rgba16Float` on a
  surface that offers only 8/10-bit; happened twice on a *running* visual (22:19 on 2 Sep, 10:56
  on 3 Sep), most likely on a display event. Owns `organon-visual/src/`. Merge, rebuild, then try
  to provoke it: move the window between the two displays, toggle HDR.
- **W19 — the dark-room fields on the CLI vocabulary + `native/assets/text/organon.txt`** (+ a
  gate fixture for it, `verify.sh --legibility --text`). Owns the vocabulary files
  (`organon-agent/src/lib.rs`, `src/agent.rs`, `src/cli.rs`, `src/console_catalog.rs`,
  `src/lib.rs`), `verify/legibility/faceplate.scene`, `assets/text/`. May also add
  `organon preset <name>` if cheap.

## 3. What is open, in order of value

1. **The legibility thresholds vs the plate's pools** — James's call. First real gate number
   (`main @ 4569855`, faceplate via the vocabulary, atmosphere still on): corr 0.8925 (≥0.90
   fails), bleed 0.497 at (59,4) (≤0.25 fails), stray 0.1387 (≤0.10 fails), spread 0.003. The
   leak is the bloom halo, T10's light pool on the slab, and the sky. Re-run after W19 (dark
   room) before deciding anything: `cd native && ./verify.sh --legibility-only` (stop the demo
   first — it holds the exe lock).
2. **T7 letterforms** — `ab_glyph` outlines → tessellate → extrude + bevel → cached mesh atlas;
   its customer is the Console's terminal. Not started; designed in §3/§14.
3. **T15 the scatter streaks** — velocity-keyed motion streaks with an RGB split in post
   (`fx.wgsl`, `post.rs`). Not started; the one row of §15 that is new rendering work.
4. **T4 Omarchy screensaver mode** — per-monitor fullscreen, self-contained preset file (§13),
   exit on input; lands in `organonart/omarchy` as an optional extension. Needs T3's preset
   exported standalone. Cannot be run here (no Hyprland).
5. **T10's leftovers**: a second instanced draw in `render.rs` for the anisotropic brushed
   backplane (the aniso lobe is per-draw), and a key-colour lane for the warm rim (the key is
   white). Spec is in #234's report and `doc/arch/render.md`.
6. **`cathode` grouping** — `character_id` as a group key in `plexus_graph` so edges wire a
   glyph's own cells (T14's report).
7. **`rt_caustic` emitters as photon sources** — T8 left the binding slot and a comment.
8. **`glyph_faceplate` is inert on the web rungs** after #243 — drop it from `bottled`/`cathode`
   in `preset.rs` some day.

## 4. How the loop runs (the mechanics that cost time to learn)

- **Dispatch.** `Agent` with `isolation: "worktree"`, `run_in_background: true`; the brief
  template is in `.claude/skills/coordinate-sessions/SKILL.md` and every brief cites
  `MSYS_NO_PATHCONV=1 git show origin/main:.claude/skills/coordinate-sessions/BRIEF.md` by
  command. Workers report by ending their turn; the notification carries the report. Resume a
  worker for a review fix with `SendMessage` to its agent id (the id is in the spawn result, not
  the report). Never two workers on one file — sequence by file ownership; §15.1 has the map.
- **Verify the artifact, not the report.** `gh pr diff N --name-only` against the brief's
  ownership list; `git diff --stat origin/main...origin/<branch>`; read the one function the
  design rests on.
- **Watch CI without polling by hand.** `jq` is not installed; use `gh --jq`. The pattern that
  works, as a `run_in_background` Bash:
  ```bash
  sleep 30; until [ "$(gh pr checks N --json bucket --jq '[.[] | select(.bucket=="pending")] | length' 2>/dev/null || echo 1)" = "0" ]; do sleep 60; done; gh pr checks N --json name,bucket --jq '.[] | select(.name|test("Vercel")|not) | "\(.name): \(.bucket)"'; gh pr view N --json comments --jq '[.comments[] | select(.author.login=="github-actions")] | last | .body' | grep -iE 'no blocking|should-fix|blocking' | head -3
  ```
  Do not `disown` a `&` watch — it dies with the shell.
- **One review round.** Read the last `github-actions` comment and the unresolved
  `reviewThreads` (GraphQL). A should-fix goes back to the worker by `SendMessage`; a nit you can
  fix yourself on the branch (commit from a file via `-F`, reply on the thread, resolve it). Then
  `gh pr merge N --squash --delete-branch` (the local-branch delete warning is noise). ⚠️ A guard
  that greps the review body for `**blocker` will trip on "**Blocker (fixed):**" — read it.
- **Gate `main` after every merge** with the legs the PR touched (`CARGO_PROFILE_TEST_OPT_LEVEL=0`):
  `cargo test -p organon-core` · `-p organon-world --features world` · `-p organon-render` ·
  `-p organic-math-native --lib --features console-edition` · `-p organon-console --lib` ·
  `-p organon-agent` · `-p organon-glyphs` · `cargo check --workspace --all-targets --features mind-edition`.
  Last full gate at `90ca793`: world 189, core 791, render 81+18+10+50, root lib 351, agent 43,
  console 988, glyphs 15+1.
- **Conflicts after a merge** are almost always the §15 table or the `ARCHITECTURE.md` glyph-ring
  row (every worker edits both). Rebase the branch yourself (`git checkout -B rebase-N origin/<branch>
  && git rebase origin/main`), resolve by keeping both sides' sentences, `--force-with-lease`.
- **Record each wave on #217** with `gh issue comment 217 --body-file` — never `--body "…"` with
  backticks, bash command-substitutes them (it happened; a comment had to be PATCHed).

## 5. Running the demo on organon-one (the GPU is here)

Binaries live in `native/target/release/` of the coordinator's worktree; rebuild after every merge
that touches what they link (the visual after `world.rs`/render; the producer after `glyph_ring`
or `organon-glyphs`; `organon` and `organon-standalone` after the vocabulary or presets):

```bash
cd native
cargo build --release -p organon-visual --bin organic-math-visual
cargo build --release -p organon-glyphs
cargo build --release -p organic-math-native --bin organon
cargo build --release -p organic-math-native --bin organon-standalone
cargo build --release -p organon-render --bin legibility-gate
```

⚠️ **Stop the demo before any rebuild or `verify.sh`** — a running exe holds the file lock and the
build reports `Access is denied`: `taskkill //F //IM organic-math-visual.exe //IM organon-standalone.exe //IM organon-glyphs.exe`.

Launch order (PowerShell `Start-Process` for the producer — `cmd start` hung on its arguments):

1. Delete `%TEMP%\organic-math-ipc.bin` if the layout changed (a stale 8512-byte file after the
   0x0286 bump rendered a flat grey frame that looked exactly like a regression).
2. `organon-standalone.exe` (the writer of `Shared`; it seeds the text presets via
   `seeded_text_vN`), then `organic-math-visual.exe` if it did not spawn one.
3. The look through the CLI — field names, never wire ids:
   `organon material clearcoat` then `organon set glyph_bevel 0.12 glyph_crown 0.35 metallic 0 roughness 0.22 glyph_cam_hold 1 glyph_cam_tilt 6 glyph_cam_zoom 1 cam_path 0 env_intensity 0.15 bloom_intensity 0.25 glyph_gain 1.2 glyph_profile 0.5 glyph_dark_tiles 1`
   plus, once W19 lands, `atmos_enabled 0 bg_visible 0 fx_enabled 1 hal_amount 0.35 ml_enabled 1 …`.
   `organon status` needs the standalone; `organon snap` needs the visual unoccluded.
4. `organon-glyphs.exe --input native/assets/text/organon.txt --effect rain --persist-ms 300 --dwell 25 --seed 3`
   (`--list` for effects; trails need `rain`/`pour`/`print`/`beams`/`swarm`; `decrypt` never
   lets a cell go dark; `--effect slide --tick-hz 30` is the blend-clock case).
5. `organon snap -o <png>` and look at it before sending it.

The T3 held camera (`glyph_cam_hold 1`) fits the grid; the mouse wheel is no longer needed. The
`seeded_text_vN` marker in `%APPDATA%\OrganicMath\` decides whether the standalone re-seeds the
factory presets; a user's own `faceplate` is never replaced.

## 6. Traps this machine and this harness have already charged for

- **Worktree subagents' Bash refuses** variables in paths, `wsl.exe`, `for` loops, `-C`, and even
  the substring `origin` inside a test name. Tell every worker: non-trivial commands go in a
  script under a **uniquely prefixed** scratchpad name, invoked as `bash "<absolute path>"`. The
  scratchpad is **shared across workers**; an unprefixed `bar.sh` was overwritten mid-run twice.
- `python` (3.13) is real on this box; `python native/tools/changelog.py …` works without WSL.
  `changelog.py new` needs `--slug` (Linux git cannot read a Windows worktree's branch).
- `git show ref:path` needs `MSYS_NO_PATHCONV=1` in Git Bash (the colon triggers path conversion).
- `git commit -F` with a heredoc; an apostrophe inside a heredoc breaks the harness's Bash;
  heredocs collapse `\r`/`\n` escapes inside Python — write files with the Write tool.
- Files are CRLF on disk; `sed -i` rewrites them LF (git normalises, but a shader-text test must
  strip `\r`); multi-line python replacements with `\n` silently no-op — use the Edit tool.
- `cargo test a b` with two filters runs nothing useful; one filter per invocation.
- `cargo run --bin organon -- docs` after a `params.rs` change took 32 minutes at the workspace's
  dev `opt-level = 1`; `CARGO_PROFILE_DEV_OPT_LEVEL=0` is the lever.
- `TaskStop` on a background Bash does not kill its cargo children.
- The CLI's vocabulary is the parameter **field name** (`glyph_bevel`), never the four-character
  wire id (`gtbv`). Adding an id costs seven sites in five files; the editor apply-channel mirror
  in `src/lib.rs` is the one no test pins.
- `verify.sh` on Windows: `MINGW64_NT` branch added by #240; runs its own visual in namespace
  `organon-verify`; exit 2 = could not measure, 1 = failed thresholds.
- Two CCD sessions from the first morning (W1–W5) are done; every later worker was a subagent
  whose id is meaningless in a new session — address work by PR number.

## 7. The prompt to start the next coordinator

Paste this into a new session rooted in the organon repo (a worktree is fine):

> You are the coordinator for organon#217, PBR text. Read, by command, in this order:
> `git fetch origin && MSYS_NO_PATHCONV=1 git show origin/main:doc/pbr_text_handoff.md`, then
> `MSYS_NO_PATHCONV=1 git show origin/main:doc/pbr_text_engine.md` (§15 first), then
> `gh issue view 217 --comments` (the last five comments), then
> `.claude/skills/coordinate-sessions/SKILL.md`. Set your session title to "PBR text coordinator"
> with `set_session_title`. James's standing rules are §1 of the handoff and override everything:
> no chips, subagents only; the demo says ORGANON; frames show a black room like the screensaver.
> Check `gh pr list` for anything still open from §2, merge what is green after one review round,
> gate `main`, rebuild, and take a GPU look at ORGANON in a dark room before anything else. Then
> work §3 in order, one PR per worker, disjoint files, and report to James with frames as they
> happen and with what you found that nobody anticipated.
