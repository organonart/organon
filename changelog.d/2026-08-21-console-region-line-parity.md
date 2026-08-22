### Organon Console: a panel column's command line completes itself, and its caret keeps up (#129)

James, on a running console: *"currently these do not auto-complete terms when we have specified
them with a few characters."*

⚠️ **The narrowing was never missing, and neither was Tab — the report was right about the feel and
the obvious diagnosis was wrong.** Driven through real `egui::Event::Text` on the shipped code,
`add su` narrowed to the single candidate `surface` and Tab took it. What the region line lacked
was the composer's **self-completion**: §1.9's *"do not show me the single choice, simply complete
it because it is the only option."* A control whose entire vocabulary is two verbs and one closed
panel table was still asking for a keystroke that could carry no information. `a` now becomes
`add ` and `add su` becomes `add surface `, with no Tab pressed anywhere.

🚨 **A second defect fell out of the same probe, live on the build and missed by every existing
test: the caret did not follow a rewrite.** Tab on `add su` produced `add surface ` with egui's
cursor still at index 6, so the next two characters landed as **`add suXYrface `**. That is the
composer's `/hxelp` bug arriving on the one surface nobody had driven end to end — and it is the
reason *"the palette narrows"* and *"Tab accepts"* were not evidence that completing worked. What a
hand does *next* is a third fact neither of those contains. Every path that rewrites the line
wholesale now sets `want_caret`, drained once at the end of the frame.

⚠️ **The two composer helpers are shared, not reproduced.** `completion_held` — the insertion-only
latch that stops a completion undoing the backspace that provoked it (*"once I have typed slash
surface, I am no longer able to backspace out of it"*) — and `put_caret_at_end` are now
`pub(crate)` and called from the region line over a per-line shadow of the text. A second copy of
either would be a second answer to a question the two surfaces must never disagree about, and the
measurements behind them were taken once. Both fixes are mutation-checked rather than asserted:
remove the latch and the first backspace out of `add surface ` is put straight back on the frame it
happens; remove either `want_caret` and the next characters land inside the word just completed.

⚠️ **The cost is stated rather than hidden, and it stands in the suite as an expected value.**
Self-completion takes the verb at the *first* keystroke, so muscle memory that types `add` in full
now overshoots into `add dd` — `typing_survives_the_palette_opening` asserts exactly that string
for exactly that reason. It is the composer's cost too (`/b` completes to `/background ` and
`ackground` lands after it), and it is accepted here rather than softened because a second
completion rule would be a second vocabulary. The overshoot is visible the moment it happens (an
empty ring, and the line is not runnable) and the latch makes backspacing out of it work.

📌 **What was deliberately *not* added, with the argument rather than the omission.** **No
autorun**: `console.stack` is `Reversal::Permanent`, so every candidate this control can reach
answers `fires: false` and autorun could not fire it if it were wired. **No history**: a refused
line already stays in the box, which is the case a recall buffer mostly earns its place on;
retyping now costs two keystrokes; and the composer's rule hands Up to the history on an *empty*
box, which here is the control's resting state, where the candidate ring **is** its label. **No
Escape**: egui's own defocus remains the way out. `NarrowFn` needed nothing — the ring is the
registry's own, un-expanded and otherwise untouched, and a test pins that so a local filter cannot
quietly appear.
