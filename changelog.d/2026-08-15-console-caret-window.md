### Organon Console — the character after a completion lands at the end of it

Typing `/`, then `h`, completes the composer to `/help`. The next character produced
**`/hxelp`**. The line was right and the caret was not, and the console had been recording that
as a known price: `CONSOLE_ARCHITECTURE.md` §1.9 called it "one frame of exposure, stated
rather than papered over", and the test that pinned it asserted `/hxelp` in as many words.

🚨 **It is an ordering bug, and the fix is the ordering.** `ConversationPane::want_caret` is set
by every site that replaces the line wholesale — a history recall, a Tab accept, the panel's
self-completion, autorun's accept. It was drained by `composer_box`, which draws the `TextEdit`
and therefore runs *before* the completion that rewrites the line. So the box could only ever
honour the **previous** frame's request, and by the time it ran, this frame's keystroke had
already been placed at the stale index after `/h`. The request was always exactly one frame
late; the caret arrived immediately after the character that needed it.

`want_caret` is now drained at the **end** of `conversation_view::composer`, after
`palette_complete` and `palette_autorun` have both had their say, and a new `put_caret_at_end`
writes egui's cursor state there. `composer_box` loses its `take_caret` parameter and returns
its `egui::Id` alongside the send flag — the one fact about the widget its caller cannot
derive. It still knows nothing about completions.

⚠️ **The doc's own reason for not fixing this was wrong in both halves, which is worth
recording.** §1.9 said closing the window "would mean setting egui's cursor state *before* the
widget runs, which needs the `TextEdit`'s id outside `composer_box` and entangles the box with
the registry it was deliberately split from." Before the widget is the one place the state
cannot go — the widget loads its cursor at the top and stores it at the bottom, so an early
write is overwritten by the widget itself. And handing out an id entangles nothing: `composer`
already knows both the box and the completion, so it is the only place that can see all four
rewrite sites at once. That is also why one flag still serves sites on **both** sides of the
box: the drain is last, so `want_caret` never survives a frame — and a request that outlived
its frame is precisely what `/hxelp` was.

🚨 **Two contracts that had to survive it, both pinned.** Autorun still waits for a settled
frame — a command does not run on the frame its last character landed, and a keystroke arriving
inside that wait still *cancels* the fire rather than racing it. That wait and this drain are
separate mechanisms answering separate questions (*when does a command run* against *where does
the next character go*), and §1.9 now says so rather than tying one to the other. The
insertion-only completion latch is untouched: `completion_held` is computed in the same place
from the same two strings, so backspacing out of `/surface` still walks one character at a time
and a deletion still never runs anything.

`a_command_waits_for_one_frame_in_which_nothing_was_typed` now reads `/helpx` and its comment
says what is true; the property itself is pinned directly by
`a_character_typed_after_a_completion_lands_at_the_end_of_it`, and the two pre-box rewrite
sites by `a_recalled_line_and_a_tab_accept_leave_the_caret_at_the_end_too`. All three drive
egui's own `TextEdit` through real frames, because the caret is egui's index and nothing short
of the real widget can show where it is.

⚠️ **Green, not seen.** `cargo test -p organon-console --lib` is 683 green (3 ignored),
`cargo test -p organon-core` is 593 green, and both `cargo check` legs for the console edition
are clean. Nobody has typed at a running window since the change: the defect was reported by a
hand and the cure has only been measured by a test.
