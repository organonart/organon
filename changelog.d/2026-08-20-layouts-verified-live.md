### A layout was saved and loaded by a running console for the first time, and the ledger says what that did and did not prove

`CONSOLE_ARCHITECTURE.md` §3 opened its saved-layouts entry with *"No layout has ever been
saved, loaded or deleted by a running console. Not once, by anyone."* On 2026-08-20 James
typed the four-region arrangement by hand on the workstation — `topcenter 3d`, `left panel`,
`right panel`, `bottomcenter agent` — then `/layout save organon` and `/layout load organon`,
and both worked. The entry is **corrected in place rather than deleted**, because what one
sitting proves is narrower than "layouts work" and that difference is the entire purpose of
the ledger.

Three open questions closed on that single run. `Console::set_layout`'s assignment **does**
reach a window. The **store path is confirmed** at
`C:\Users\james\AppData\Roaming\OrganonShell\layouts.json` — 197 bytes, which retires the
separate "unconfirmed on this machine" note that stood beside it. And `layouts.json` **is**
legible: pretty-printed, one `regions` object of plain word pairs, plainly hand-editable,
which had been *"a claim about a file nobody has opened"*.

⚠️ **One question was answered in the opposite direction to the prediction.** The entry warned
that the receipt lines are `eprintln!` and that "in a GUI launched from Explorer" they go
nowhere. They did not go nowhere: the conversation front-end renders them into its own
transcript, and `ok /layout load organon — {"accepted":"layout load organon"}` is where the
load was read. The prediction was true of the terminal lane and simply did not describe the
front-end the verb is actually typed into — worth keeping as a reminder that a defect
attributed to "the whole console lane" may not reach every surface in it.

🚨 **`delete` has still never been run by a hand.** The verb has three actions and two are now
exercised; do not round that up.

📌 **The sitting also settled an open design question, by observation rather than argument.**
With `left` and `right` both holding `panel`, a single `/stack add surface` populated **both**
columns with an identical `organon · look · Surface` — one stack, two views, exactly as
`panel_stack.rs`'s header describes it. Seeing that is what decided #98 Tier C in favour of
per-region stacks. James: *"you can see why we need to have the update for the stacks because
it addressed both of them."* The same frame retires a second ledger claim — *"the panel stack
has never been looked at either"* — and produces the first evidence outside the headless test
that `param_sink`'s id namespacing survives two Surface bodies drawn at once. ⚠️ That is
evidence about **drawing**, not **writing**: §1.11's item (0) stays unchecked, because nobody
has yet turned a knob in one of those columns and watched the picture move.
