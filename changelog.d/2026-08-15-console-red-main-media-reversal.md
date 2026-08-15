### `main` did not compile, and neither branch that broke it was wrong

🚨 **`main` @ `2018d41` failed to build at all** — not a test, not an edition, the
`organon-console` library itself:

```
error[E0063]: missing field `reversal` in initializer of `registry::Entry`
   --> organon-console/src/registry.rs:544
```

Two of the four legs of the console's verification bar were red on it
(`cargo test -p organon-console --lib` and
`cargo check --features console-edition --bin organon-console`), which between them cover
everything downstream, so **every branch cut from `main` inherited a tree that would not
build**. The `VERB_MEDIA` entry in `view_entries()` now states its reversal, and the tree
compiles again.

⚠️ **The interesting part is how it landed, because nothing that ran was wrong.** `/media`
was added to `view_entries()` on the exhibit branch (`94e26c7`); `Entry::reversal` was
added to every entry *that existed at the time* on the autorun branch (`8307e5c`). The two
hunks are a few lines apart in one `vec![]`, so git merged them cleanly, with no conflict
to review — and produced an initializer missing a field that did not exist when it was
written. Each branch was green on its own and stayed green; the defect exists only in their
combination, and it was created by the merge rather than by either author.

📌 **So the lesson is not a missing test — it is that the bar was not re-run after the
merge.** A missing struct field is `E0063`; the compiler catches it the first time anyone
builds, and no test could catch it earlier or better. What was missing was somebody
building. This repo already knew the shape ("test counts and claims that were true of one
branch are false of the merge"); this is the same sentence with the word *counts* removed.

**What a test can add here is the value, not the presence**, and one now does.
`the_view_lane_states_what_can_be_taken_back_and_an_exhibit_cannot` pins all four view-lane
verbs: `/surface`, `/media` and `/organon` are `Permanent` because each leaves an element in
the transcript and no verb in the table takes one out, while `/help` is `Recoverable`
because reading a table changes nothing. A *missing* field is loud; a *wrong* one is silent,
and the wrong one here would have let a keystroke place an exhibit unasked.

⚠️ `Permanent` is also the only answer `/media` could have had. Its argument is `Text` — a
path has no closed value space — so the command panel never has a lone candidate to
complete, and autorun is never offered the line in the first place. The field is what makes
that a stated property rather than a coincidence of the argument's kind.

## The same merge broke a second thing, and only the first one was a compile error

🚨 **Repairing the build revealed `the_real_table_says_which_verbs_may_run_without_an_enter`
had never run against a table containing `media`.** That test pins the reversal column of the
console's *whole* vocabulary as a literal list, and the list has no `media` row — for the
identical reason: `/media` joined the view lane on the branch where `Reversal` did not exist,
and the test itself arrived on the branch where `/media` did not. Two casualties, one merge,
neither side red, no conflict to review.

⚠️ **This one is worse than the first, and the difference is the lesson.** A missing struct
field is `E0063` — loud, immediate, and impossible to ship past anyone who builds. A stale
whole-vocabulary list is a *failing test*, which means it can only be caught by something that
**runs** it, and it lives in `console_main.rs`, the **root crate**. The console's four-leg bar
type-checks that crate (`cargo check --tests -p organic-math-native`) and never executes it —
the honesty ledger has said so for some time. So the local bar was green on all four legs with
this test failing, and **CI is what caught it**. That is the bar working exactly as documented
rather than a surprise, and it is worth stating plainly: on a change that adds or moves a
console verb, a green four-leg bar is not evidence about this test.

📌 The comment beside `compact_line`'s hidden `+N` count already told this story once — three
verbs have now moved that line, and both times the mechanism was a merge invalidating a
statement about the vocabulary as a whole from a hunk that touched neither end of it. The
fix for both is the same discipline: **re-derive the list from `view_entries()`; never append
to it and assume the order.**
