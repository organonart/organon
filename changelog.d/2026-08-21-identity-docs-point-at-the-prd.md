### The identity docs point at the PRD, and two of them turned out to be about the visualizer

`doc/organon_prd.md` §1.1 became the canonical description of what Organon is when the PRD landed;
this converts the surfaces that were still carrying their own wording. Six existed, which is how
the claim came to be stale in six places at once: `README.md`, `doc/how_organon_works.md` (twice —
§1 and §16), `doc/equations_into_light.md`, `doc/guide/README.md`, and `CLAUDE.md`'s naming
section.

📌 **The interesting outcome is that only four of them wanted the same fix.** `README.md` and
`how_organon_works.md` describe the product, so they are **rewritten** to lead with the layout and
the agent invariant, with the visualizer named as one of the things Organon hosts.
`equations_into_light.md` and `doc/guide/` are genuinely about the **visualizer** — an essay about
the render stack and a guide to playing generators, surfaces and materials — so they are **scoped**
instead: their titles now say which arrangement they cover, and they point at §1.1 for the whole.
A document about one arrangement is not a document about the product, and treating every mention
of "Organon is…" as a thing to overwrite would have produced six restatements of §1.1 in place of
six wordings of the old claim.

🚨 **Each rewritten doc carries two claims kept apart, and that separation is the point.** What
Organon *is* — one application whose identity is data, no arrangement valid without a live agent —
and how it *ships today* — three binaries chosen by a compile-time edition, with #111 not started.
Both are true right now. `doc/organon_is_the_product.md` §5 warns that a rename outrunning its
mechanism leaves documents describing a thing that does not exist, and the way past that warning is
not to delay the words but to refuse to conflate them: no sentence here claims a mechanism that is
not built, and every doc names `doc/organon_prd.md` §12 as the honest state of play.

⚠️ **`how_organon_works.md`'s status line was NOT advanced, and the restraint is deliberate.** Its
header promises that counts are re-measured at the stated date, and this change re-measured
nothing — so the week-33 date stands for every count in the file, and a dated note records that §1
and §16 were reframed on 2026-08-21 without touching them. Bumping the date would have asserted a
measurement that never happened, in a document whose whole value is that its numbers were taken
rather than remembered.

✏️ **`CLAUDE.md`'s naming section is the one left standing, on purpose.** It describes the
mechanism — the edition, the binaries, which identifiers are load-bearing because something else
reads them — so it moves when the mechanism moves, under #111. And ✏️ `doc/guide/README.md` was a
**sixth** copy that neither §5's move list nor #111 had named; both lists now record all of them.

✏️ **And §5's own preamble is fixed, because the ✅ rows are what broke it.** It read *"none of it
should happen before #98's tiers land"* — consistent while every row was pending, and
self-contradicting the moment word-rows above it were marked done. The split it always meant is
now stated where a reader meets the table: **a row that changes words a person reads may go as
soon as the decision is ratified; a row that changes a mechanism something else reads waits for
#98.** That is the same distinction this change rests on, and leaving it six lines below the
table is how a document comes to forbid what it is simultaneously recording as done.
