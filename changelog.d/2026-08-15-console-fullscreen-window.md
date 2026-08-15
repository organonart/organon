### Organon Console — full screen, on a third axis rather than as a third posture

`organon console screen <full|windowed|toggle>` puts the console's window into borderless
full screen and back, and **F11 flips it from inside**. New module
`organon-console/src/screen.rs`; `CONSOLE_ARCHITECTURE.md` §1.12 owns the reasoning.

🚨 **It was asked for as "a new posture, which is full screen", and it could not be one.**
`Posture` is a **scalar**, not an enum — `Form::at(t)` lerps componentwise between two ends
and `organon console posture 0.5` is a real drawable console — so there is no third slot to
add; there is an axis. And full screen is not a point on it: every one of `Form`'s fourteen
tokens is a margin, a corner, a padding, a gap or the presence of a border, and full screen
moves none of them. It also passes `posture.rs`'s own orthogonality test verbatim — a
full-screen terminal and a full-screen desktop document are both real, and neither is a
variant of the other — so it is a third orthogonal state, and all four (posture × screen)
combinations are consoles somebody would want.

⚠️ **The form is deliberately NOT nudged when the window fills the display**, and the
argument for nudging it is a good one, which is why it is answered rather than ignored: a
2560-wide *maximized* window and a 2560-wide *full-screen* one want identical margins, and a
full-screen window on a 1280-wide laptop wants today's desktop margins unchanged. "Is it full
screen" is not the question the margin wanted asked — "how wide is it" is. Coupling them would
also mean `organon console posture terminal`, typed while full screen, not giving the person
the posture they typed.

⚠️ **No state is held.** `Window::fullscreen()` *is* the answer, so `Screen` is derived at the
moment a command arrives rather than remembered beside it — which forecloses a concrete
failure: after a window is put full screen by some other route (a platform's own control, a
tiling manager), a remembered `Windowed` would make `toggle` send it *into* full screen, so
the one word whose entire meaning is "the other one" would do nothing and report nothing.

🚨 **F11 is the way out, and the choice is measured rather than assumed.** A borderless window
has no close button. Escape is unavailable — in a terminal tab the keyboard is the child's and
`vim` needs it, so claiming it requires state-conditional ownership the console has not built.
F11 is free: `term::encode_key` returns `None` for every function key, and both conversation-side
key tables answer `Ignore` for it. All three facts are pinned by tests, so the day one stops
being true it fails there rather than fighting silently. The chord and the verb funnel into one
call, so they cannot drift.

🚨 **A key-repeat is not a press.** Holding a key streams `pressed: true` events, so without a
filter a resting finger would flip the window once per repeat and the state on release would be
decided by parity — worst on the one chord that is the way *out* of a window with no title bar.
Filtered in `screen_key` and pinned by test rather than left to be noticed on a display, because
a repeat stream is exactly what a screenshot cannot show. ⚠️ The `⌘` chords read the same event
and were deliberately **not** fixed here — existing behaviour on a different key table, and
"ignore the repeat" may not even be their right answer. ✏️ That reservation was the correct
call and PR #83 has since settled it, differently from this chord: `command_key_action` now
streams `Switch` on repeat (holding `⌘⇧]` should keep cycling, and repeating `⌘1` means
nothing) while refusing it for `New` and `Close`. Folding that into this change would have
imposed one blanket rule on two key tables that turned out to want opposite ones.

⚠️ **Named `screen`, not `fullscreen`, because two different rectangles can go full screen.**
§2's ledger reserves that phrase for a still-unbuilt *portal* state — the portal taking the whole
window. This verb says which rectangle it moves.

📌 Not remembered across launches, on posture's rule. 🚨 **Nothing has been seen full screen** —
the whole claim is that it compiles and the tests pass: 655 console lib tests (646 before) and
567 in core, four of the latter being the three screen words and the `screen full` byte-pin
riding the wire-format round trip, which this verb reaches only because Tier 5a moved that test
into a crate the console's own bar executes. §3's ledger states what that does and does not
establish, F11 actually arriving first among them.
