# What an `Edit` card cost every frame, and what it costs now

> **Status:** measured on `ORGANON-ONE`, 2026-08-13. The instrument is
> `native/organon-shell/src/conversation_view/edit_diff_bench.rs` and is kept in the tree so
> every figure here can be re-taken rather than believed:
>
> ```text
> cargo test --release -p organon-shell --lib -- --ignored --nocapture --test-threads=1 edit_diff_bench
> ```
>
> This document is a *sibling* of `doc/console_rewrap_measurement.md`, which found this cost
> (§6, second bullet), named it, and deliberately excluded it — that benchmark's corpus is
> `Read` cards. This is the measurement it deferred.

---

## 0. The answer, in one line

**An `Edit` card re-parsed its arguments and re-aligned its diff on every frame — 1.5 µs for
an ordinary one-line edit, 78 µs for a large one — and with the scrollback unvirtualised a
long session paid all of them at 60 Hz; caching the result per card removed between 0.75 ms
and 35 ms per frame depending on what the edits were.**

The common case was never the problem. The tail was, and nothing bounded it.

---

## 1. The question, and why it was not already answered

`tool_card` drew an `Edit`'s `old_string`/`new_string` as a real aligned diff, which is one
of the clearest things the conversation front-end does that a terminal cannot. It did it like
this:

```rust
match edit_diff(card.name.as_deref(), &card.arguments) {
    Some(diff) => diff_body(ui, &diff, theme),
    None => arguments_body(ui, &card.arguments, theme),
}
```

— inside the draw call. So every frame, for every `Edit` card in the transcript,
`serde_json::from_str` walked the whole arguments blob and `text_diff::line_diff` re-ran the
alignment, and the answer was kept nowhere.

Three facts turn that from a warm inner loop into a cost linear in the whole session:

1. **The scrollback is not virtualised.** `egui::ScrollArea::show` lays out every element,
   and `egui::Label` builds its galley *before* the visibility check. An `Edit` card two
   thousand lines above the viewport paid in full. `console_rewrap_measurement.md` §5 pinned
   this with a test (`the_whole_scrollback_is_laid_out_not_just_the_visible_slice`); this
   module pins the same property for cards.
2. **`Limits::max_elements` is 10 000**, so "how many cards" has a large ceiling.
3. **The result is a pure function of the arguments**, which do not change once a card has
   settled. Every recomputation after the first produced a bit-identical answer.

`text_diff`'s own module doc already said the recomputation was deliberate and that
`MAX_CELLS` was sized against it. That was a defensible statement about **one** card. The
question nobody had asked was the **multiplier**.

---

## 2. Method

Two independent instruments, deliberately, because either alone is arguable.

**Direct.** `edit_diff` called on the exact `Arguments` a `ToolCall` fold produces, 200
distinct blobs per shape, median reported, `std::hint::black_box` around the result and a
fresh blob per sample so neither LLVM nor L1 flatters it. The `serde_json` parse is timed
separately so the two halves of the cost can be told apart. No egui at all.

**Whole-frame.** `scrollback` — the same function `conversation_view::draw` calls — driven
over a real `Transcript` inside a real `egui::ScrollArea` on a real `egui::Context`, 6 warm-up
frames discarded, median of the rest. Each corpus is run twice: once with the cache and once
with `Cache::Off`, which **clears `pane.diffs` at the top of each frame and is therefore the
old code exactly** — the same call, at the same point in the walk, with the same arguments.
The clear is inside the timed region on purpose, because the old code built *and dropped* an
`EditDiff` within one frame and excluding the drop would flatter the "before" column.

The corpora differ **only** in the tool card, so a `Read` corpus is the control.

### 2.1 The five shapes

Chosen to span what the bounds do, not to average a session:

| Shape | What it is | Where it sits |
|---|---|---|
| `one-line` | one line changed inside a six-line block | the ordinary edit, by a long way the most common |
| `hunk` | 30 lines, three changed regions | one function refactored |
| `at-budget` | 140 × 140 changed lines = 19 600 cells | the **largest** diff `MAX_CELLS` permits rather than declines |
| `declined` | 200 × 200 = 40 000 cells | past the budget; falls back to a block replacement |
| `ctx-heavy` | a 400-line common prefix, one line changed after it | 🚨 **past every bound, and no bound sees it** — see §4 |

Plus one stated mix: `1-in-10 big`, where one card in ten is `at-budget` and the rest are
`one-line`.

⚠️ **`1-in-10` is a stated distribution, not a measured one.** No long session has been
captured on this machine, so a "realistic mix" here would be invented precision wearing a
measurement's clothes. The claim it makes is weaker and checkable: *if* one edit in ten is
large, this is the cost. Supply your own `n`.

---

## 3. What one call cost

**Machine:** `ORGANON-ONE` — AMD Ryzen Threadripper PRO 9955WX (16C), 32 GB, Windows 11 Pro
10.0.26200. **Toolchain:** `x86_64-pc-windows-msvc`, `--release`. **egui/epaint 0.33.3.**

📌 **Re-taken after the rebase onto `main`'s posture work**, which put a `Form` through
`scrollback` and therefore through this bench's draw path. Every figure reproduced inside the
variance §3 and §5.4 already state. The bench passes `Form::TERMINAL` for the reason
`rewrap_bench` does — a desktop `Form` insets the column through `content_margin`, and posture
is not what either bench varies.

| shape | args bytes | rows drawn | total µs | of which parse | of which diff | fit in one 60 Hz frame |
|---|---:|---:|---:|---:|---:|---:|
| one-line | 410 | 6 | **1.5** | 0.5 | 1.0 | 11 111 |
| hunk | 2 362 | 25 | **5.6** | 1.1 | 4.5 | 2 976 |
| at-budget | 19 809 | 18 | **43.9** | 4.0 | 39.9 | 380 |
| declined | 28 449 | 18 | **20.9** | 5.3 | 15.6 | 797 |
| ctx-heavy | 58 103 | 6 | **78.2** | 10.8 | 67.4 | 213 |

Three runs of the same binary, to show what is signal:

| shape | run 1 | run 2 | run 3 |
|---|---:|---:|---:|
| one-line | 1.40 | 1.40 | 1.50 |
| hunk | 5.50 | 7.40 | 5.60 |
| at-budget | 45.30 | 41.90 | 43.90 |
| declined | 21.00 | 20.00 | 20.90 |
| ctx-heavy | 66.50 | 64.80 | 78.20 |

⚠️ `one-line`, `at-budget` and `declined` repeat to a few percent; **`hunk` varied by 25 % and
`ctx-heavy` by 19 %.** Read those two as an order of magnitude. Nothing below turns on their
second significant digit.

**The alignment dominates the parse everywhere** — 2× on the smallest shape and 6× on the
largest — which matters because it rules out the cheap half-fix. Caching only the parsed
`serde_json::Value` would have left 65–90 % of the cost in place.

### 3.1 Without `--release`

| shape | dev | release | inflation |
|---|---:|---:|---:|
| one-line | 2.20 | 1.50 | +47 % |
| hunk | 7.20 | 5.60 | +29 % |
| at-budget | 56.00 | 43.90 | +28 % |
| declined | 25.60 | 20.90 | +22 % |
| ctx-heavy | 102.50 | 78.20 | +31 % |

The workspace's `[profile.dev]` is `opt-level = 1` and the test profile inherits it. The
console ships release. **Do not quote the left column** — it is here only so that someone who
runs the bench without `--release` recognises their own numbers.

---

## 4. 🚨 The shape no bound sees

`text_diff` has three budgets and the module doc explains each. **None of them bounds
`ctx-heavy`, and that is not an oversight in any of the three — it is a gap between them.**

`MAX_CELLS` is checked **after** the common prefix and suffix are trimmed. A 400-line prefix
trims to zero alignment cells, so the largest input in the corpus passes the budget meant to
stop large inputs, and then `line_diff` builds a `DiffRow::Context` carrying an **owned
`String` per prefix line** before `elide` discards nearly all of them. `MAX_ROWS` then caps
what is *drawn* to six rows.

So the most expensive card in the corpus is also the one that draws the least, and every
budget reports it as small. That is why it is the worst shape here at 78 µs — nearly twice
`at-budget`, which is what the bounds think the worst case is.

⚠️ **This is still true after the cache.** The cache makes it happen once instead of sixty
times a second; it does not make it cheap, and a first frame after a large edit still pays it.
Bounding the trim would be a change to `text_diff` and is **not** made here — it is recorded
in §7 as the next thing, with a test
(`a_long_common_prefix_is_bounded_by_nothing_until_after_it_is_built`) that states the gap so
it cannot be closed by accident and go unnoticed.

---

## 5. What a frame cost, before and after

Medians in **milliseconds**. `uncached` is the old code (`Cache::Off`), `cached` is what ships.
The `Read` row is the control: same corpus in both columns, so it moves only by noise.

| elements | cards | corpus | uncached | cached | saved | saved µs/card |
|---:|---:|---|---:|---:|---:|---:|
| 100 | 20 | Read | 0.286 | 0.297 | — | — |
| 100 | 20 | one-line | 0.321 | 0.282 | 0.039 | 1.9 |
| 100 | 20 | hunk | 0.624 | 0.502 | 0.122 | 6.1 |
| 100 | 20 | at-budget | 1.593 | 0.702 | 0.892 | 44.6 |
| 100 | 20 | declined | 1.249 | 0.837 | 0.411 | 20.6 |
| 100 | 20 | ctx-heavy | 2.693 | 1.166 | **1.527** | 76.4 |
| 100 | 20 | 1-in-10 big | 0.459 | 0.330 | 0.128 | 6.4 |
| 400 | 80 | Read | 1.173 | 1.171 | — | — |
| 400 | 80 | one-line | 1.300 | 1.176 | 0.124 | 1.5 |
| 400 | 80 | hunk | 2.555 | 2.039 | 0.515 | 6.4 |
| 400 | 80 | at-budget | 6.724 | 2.845 | **3.879** | 48.5 |
| 400 | 80 | declined | 5.139 | 3.316 | 1.823 | 22.8 |
| 400 | 80 | ctx-heavy | 10.717 | 4.545 | **6.171** | 77.1 |
| 400 | 80 | 1-in-10 big | 1.728 | 1.262 | 0.465 | 5.8 |
| 2 000 | 400 | Read | 8.392 | 7.498 | — | — |
| 2 000 | 400 | one-line | 7.717 | 6.965 | 0.752 | 1.9 |
| 2 000 | 400 | hunk | 14.676 | 10.788 | 3.888 | 9.7 |
| 2 000 | 400 | at-budget | 35.309 | 17.606 | **17.703** | 44.3 |
| 2 000 | 400 | declined | 29.460 | 20.896 | 8.564 | 21.4 |
| 2 000 | 400 | ctx-heavy | 61.460 | 26.670 | **34.791** | 87.0 |
| 2 000 | 400 | 1-in-10 big | 9.767 | 7.319 | 2.449 | 6.1 |

### 5.1 The two instruments agree

This is the strongest thing in the document, and it is the reason to trust the rest:

| shape | saved µs/card, from frames | total µs/call, measured directly |
|---|---:|---:|
| one-line | 1.9 | 1.5 |
| hunk | 6.4 | 5.6 |
| at-budget | 48.5 | 43.9 |
| declined | 22.8 | 20.9 |
| ctx-heavy | 77.1 | 78.2 |

Two instruments that share no code — one with no egui in it at all, one differencing whole
frames of the real draw path — land within a few percent on all five shapes. Neither was
tuned to the other.

### 5.2 ⚠️ Read the *saved* column, not the *cached* one, and never across shapes

**The cached column is not comparable between rows, and the `ctx-heavy` row is the trap.**
Cached `ctx-heavy` at 2 000 elements is 26.7 ms against `Read`'s 7.5 ms, which invites the
reading "the cache did not fix it". It did — the saving is 34.8 ms. The residual is not
per-frame work at all: that corpus holds 400 × 58 KB ≈ **23 MB of argument text** in the
transcript against `Read`'s ~50 KB, and a working set three orders of magnitude larger is
slower at everything. A corpus of large edits is expensive to *hold*, which is a real cost and
a different one.

Within a shape the comparison is exact, because the two columns are the same corpus.

### 5.3 What this does and does not buy

- **The common case was never the problem.** `one-line` at 400 cards is 0.12 ms — below the
  run-to-run noise at that size, and at 2 000 elements the uncached column measured *faster*
  than the control, which is how the reader can see it is noise. Had the corpus been only
  ordinary edits, the honest answer would have been "leave it alone".
- **The mix is the argument.** At a stated one large edit in ten, 400 cards: **2.4 ms per
  frame**, which is 15 % of a 60 Hz budget on a frame that
  `console_rewrap_measurement.md` §6 already shows spending 7.9 ms on layout alone. After the
  cache that corpus is 7.3 ms against the 7.5 ms control — the `Edit` cost is gone, not
  reduced.
- **The tail is why it is worth a field.** A session of large edits went from 61 ms per frame
  (16 fps, sitting still) to 27 ms.
- **It does not make the console fast.** §6 of the re-wrap document stands unchanged: the
  transcript's *layout* is still O(scrollback) in every condition, and at 2 000 elements that
  alone is half a frame. This removed a second, independent O(scrollback) cost sitting on top
  of it. Virtualising the scrollback is still the only thing that fixes the first, and is
  still a tier rather than a patch.

---

## 6. What was built

A side map on the pane, in the idiom `ConversationPane` already had for `artifacts`:

```rust
diffs: HashMap<ElementId, Option<EditDiff>>,
```

Computed in `scrollback`'s walk and *read* by `tool_card`, which now takes the diff rather
than deriving it — so the card stays a function of what it is given, and the walk is the one
place that decides how long an answer lives. `Body::Tool` moved out of `draw_element` and into
`scrollback`'s match for exactly the reason `Body::Artifact` is already there, in a comment
that was already written: it needs state that survives between frames.

**`edit_diff` itself is unchanged and still uncached.** The cache is at the call site,
deliberately, so that the pure function stays pure and so that `Cache::Off` keeps reproducing
the old code. A test pins that split and fails if anyone memoises the function as well.

### 6.1 🚨 Invalidation is by eviction, and it had to be

The pane drops a card's entry when the fold reports `Change::Updated(id)` for it. That is the
only correct option available, and the reason is a fact worth stating on its own:

**`Arguments::complete` is not a promise of immutability.** A second `ToolCall` for an id that
is not yet *resolved* replaces the arguments text wholesale (`conversation.rs`'s `ToolCall`
arm). So a cache keyed on "this card's arguments are complete" would have shown the first
arguments' diff forever, under a card displaying the second arguments' path — silently, and
only on a card the harness happened to re-emit.

The alternatives were worse in both directions. A fingerprint cheap enough to take every frame
must be shorter than the text and can therefore collide; hashing the whole blob costs a large
fraction of what the cache saves (58 KB per card per frame, against a 78 µs saving). The fold
already names the element it changed, so here the exact answer is also the cheap one.

⚠️ **Every update evicts, not only an argument one.** A `ToolResult` lands on the same arm and
drops a diff that was still good — one recomputation per card per result, accepted
deliberately. Narrowing it would mean the pane reasoning about *which field* the fold touched,
which is the fold's knowledge and would rot silently the day a new event arm is added.

The map is pruned against the transcript at the end of `scrollback`, beside the `artifacts`
retain. An entry holds at most `MAX_ROWS` rows of text however large the edit it came from,
so the cache is bounded by the transcript's cap rather than by anything about the edits.

### 6.2 What holds it up

Five tests, four of them contracts, in `conversation_view`'s own test module rather than the
bench's — they are correctness, not measurement. They share the bench's pane driver rather
than copying it.

| Test | What it would catch |
|---|---|
| `a_cached_diff_is_what_edit_diff_would_have_returned` | the cache changing *what* is drawn, not just when |
| `replacing_complete_arguments_replaces_the_cached_diff` | 🚨 §6.1's stale-diff-under-a-new-path failure |
| `a_streaming_card_caches_no_diff_and_gains_one_when_its_arguments_settle` | a cached `None` outliving the arguments arriving |
| `a_card_the_cap_evicted_takes_its_cached_diff_with_it` | the side map leaking on an all-day session |
| `the_pure_function_is_still_uncached_and_the_cache_is_at_the_call_site` | a second cache appearing inside `edit_diff`, which would silently stop `Cache::Off` reproducing the old code |

📌 **Mutation-checked rather than assumed.** Deleting the `self.diffs.remove(&id)` line fails
the two invalidation tests and nothing else; commenting out the `diffs.retain` fails the
eviction test and nothing else. Both were run. A test that passes on broken code is not a
test, and the only way to know which one you have is to break it on purpose.

⚠️ The eviction test drives the **real** cap — a two-element `Limits` and enough cards to
overflow it — rather than assigning a fresh `Transcript` over the pane's. The short version
proves the `retain` fires and nothing about the mechanism that removes elements: eviction is
from the front, one at a time, while the pane keeps running and `next_element` keeps climbing.
Replacing the transcript wholesale is something no code in this crate does, *and* it restarts
the id counter — so it would have tested a pruning story that cannot occur, and would have
been the only place in the tree where a recycled `ElementId` was reachable at all.

---

## 7. What was NOT done, and what was NOT measured

Stated plainly, per house discipline.

- **The `ctx-heavy` gap is not closed** (§4). `text_diff` still builds an owned `String` per
  common-prefix line before eliding it, and no budget sees that. The cache makes it happen
  once per card instead of once per frame, which is why this is recorded rather than fixed
  here — a `text_diff` change belongs with `text_diff`'s own tests and would land on its own.
- **`MAX_ROWS` is off by one from its own doc.** It says "elisions and held-back markers
  included", and `cap_total` truncates to `max` and *then* pushes the `Held` marker, so a
  capped diff is 25 rows. One row of card; found while asserting the documented bound, noted
  and not changed.
- **Anything on a GPU.** `Context::tessellate` is never called, no swapchain, no paint.
- **A captured session.** The corpus's shapes are real (`Transcript::apply`, the real bodies,
  the real card); the words are written in the module. The fixtures in
  `native/organon-shell/fixtures/` are 11–77 lines.
- **How many `Edit` cards a real session holds, and how large they are.** This is the largest
  gap and it is why §2.1's mix is a stated ratio rather than a measurement. Every conclusion
  about the *mix* is conditional on it; the per-shape and per-card figures are not.
- **Any machine but this one.** The per-call figures in §3 are the portable part; the
  per-frame tables are not.
- **Whether the saving is perceptible.** 2.4 ms is a number, not a judgement about how the
  console feels at 225 % scaling. Nobody has watched it. There is no GPU in this session, so
  this is **green and ready to deploy**, not verified working.

---

## 8. What would change this document

- **Virtualising the scrollback.** The whole cost this measures is O(transcript) *because*
  every card is laid out; `show_viewport` would make both this and §6 of the re-wrap document
  moot. The bench's `the_whole_transcript_is_drawn_not_just_the_visible_slice` fails the day
  that lands, which is the signal to re-take everything here.
- **Bounding the common-prefix trim in `text_diff`.** §4's shape stops being the worst one and
  `a_long_common_prefix_is_bounded_by_nothing_until_after_it_is_built` fails.
- **A change to `MAX_CELLS`.** `at-budget` is that constant; it is 140 × 140 because the
  constant is 20 000.
- **A captured long session.** It would replace §2.1's stated ratio with a measured one, which
  is the one piece of this that is currently an assumption rather than a number.
