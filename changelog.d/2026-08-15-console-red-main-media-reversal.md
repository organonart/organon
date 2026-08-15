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
