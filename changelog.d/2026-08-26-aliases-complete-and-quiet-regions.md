### A short form narrows the ring, and an empty region stops captioning itself

**Two things you see while dividing a pane, both of which were wrong in the picture.**

🚨 **`tl` has resolved to `topleft` since `ChoiceAliased` landed, and the completer never knew.**
`narrows` is prefix-only and nothing in the ring read the alias table, so `/viewport tl` ran
perfectly while the panel under it went **empty as you typed** — the worst of both: the
abbreviation exists, and using it looks like a mistake. The alias now narrows, and accepting
still writes the **long** word, so the line you end up with is the one you could have typed.

⚠️ **The alias became its own field rather than a re-read of the doc slot**, and that is not
tidiness: a `NarrowFn`'s docs are arbitrary prose (*"surface — 3 settings"*), so narrowing on them
selects a candidate by a word out of its own description. 📌 Measured rather than argued —
mutating the filter to read `doc` fails the **pre-existing** `a_lone_panel_completes_the_whole
_command`, whose ring comes from a `NarrowFn`. The mutation is what turned that from an argument
into a fact.

⚠️ **`paint_region_notice` no longer draws the region's word**, reversing a decision this repo
recorded on purpose. §1.9's argument still holds — a region that draws *nothing at all* is
indistinguishable from one that is broken — but it is satisfied by the **panel fill**, which
paints the rectangle as a surface the console owns. It never needed the name on top. James:
*"when we make viewports, do not put text in them like top, bottom left, bottom right."*

📌 **The producer sentences are untouched.** A hosted rectangle with no picture still says why
(*"ascent — not running"*) — a fact a person cannot get any other way, which is exactly what the
region's own name was not.

⚠️ The label removal has **no unit test and cannot have a useful one**: it is a paint call, and
what changed is that it paints less. It is the one part of this that needs a window.
