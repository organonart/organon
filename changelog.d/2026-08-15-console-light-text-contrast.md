### The light palette's text pays back what the surface ladder borrowed

Lowering the light page to V ≈ 0.85 — James's instruction, and a deliberate move to pale grey
card stock rather than paper — moved every text contrast ratio in the palette without touching
a single text colour. Two roles fell under WCAG AA: `secondary` on the panel at **4.12** (from
5.70) and `faint` on the page at **2.22** (from 3.06). Primary text was never at risk and is
still 13.25.

🚨 **No surface change can repay this, and that is arithmetic rather than preference.** Contrast
is a fraction, the ladder is one side of it, and the page's value is the number James named. So
the repair is on the other side: `LIGHT_SECONDARY #555b64` and `LIGHT_FAINT #737983`, each a
**uniform per-channel subtraction** — the same method the ladder itself moved by, so both roles
keep their cool tilt instead of being re-picked by eye. `faint`'s −24 lands on exactly the 3.06
it held before the move; `secondary`'s −8 is the smallest step that clears 4.5, at 4.66. Nothing
else moved: `primary`, `success`, `error` and `accent` are untouched and all clear AA.

⚠️ **One role a single value cannot rescue everywhere, and it is left failing on purpose.**
`faint` on a hairline plate (`tab_menu_missing`) reaches 2.45, not 4.5. That role labels a thing
that is *absent*; darkening it far enough to clear AA there would make "not mapped yet" heavier
on the page than live secondary text, which is the wrong sentence for a console to say. It is
recorded as a number and asserted **two-sided**, so a later drift in either direction — toward
illegible, or toward somebody quietly deciding the exception is over — fails a test.

🚨 **The nine assignment sites became two named constants**, which is the lesson the surface
ladder had already paid for. `faint` was four repeated literals and `secondary` five; a
correction spelled as nine hand-edits is a correction that lands on eight of them. Fields still
assign independently — no two roles are merged, and `roles_that_share_a_value_are_still_separate_fields`
still holds — only the *value* is stated once so one step of one ladder cannot become two
colours by accident.

📌 **And the ratio table in `CONSOLE_ARCHITECTURE.md` §1.4 is now a test.** It had been prose for
a day, and the ladder move is the proof of what prose is worth: it invalidated all seven rows
while touching none of the colours they describe, because only one side of the fraction was
being edited and nothing connected the two.
`every_light_text_role_is_measured_against_the_surface_it_is_drawn_on` computes WCAG relative
luminance and asserts each role against the surface it is really drawn on — the helper lives in
the test module rather than on `Theme`, because the console never asks a colour how bright it is
at runtime and a public method nothing calls is a worse thing to own than a duplicated formula.

⚠️ **Unverified in the way everything about this palette is unverified: nobody has looked.**
These are ratios against a published standard, not an observation, and the darkening is exactly
as unlooked-at as the page it repays. The complaint to watch for is the opposite of last time —
not "the white part is too white", but text that reads heavy against pale card stock.
