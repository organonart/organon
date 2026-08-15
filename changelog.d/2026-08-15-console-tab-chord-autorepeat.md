### A held ⌘ chord no longer opens — or closes — a run of tabs

`tabs::command_key_action` takes the key event's `repeat` flag and answers it per
action. Holding a key streams `pressed: true` events; egui marks each one after the
first and **leaves it in the stream**, so the console's frame loop read a resting
finger as an unbroken run of presses. The host applies one action per frame, which
bounds the rate and not the total — autorepeat is slower than the frame rate, so a
held ⌘T spawned a PTY per repeat and a held ⌘W closed tab after tab until the last
one took the console with it.

⚠️ **"Ignore repeats" would be the wrong fix for half the table, so the policy is a
property of the action rather than of the key.** `Switch` keeps its repeats — ⌘⇧[/]
cycling while held is what a cycle chord is for, and ⌘1-9 is idempotent (the host
answers with one index write, so the thirtieth repeat writes what the first did).
`New` and `Close` refuse them: both are unbounded and neither is recoverable.
Deciding on the action rather than the key means a chord added later inherits the
answer, and the match is exhaustive over `TabAction` so a new *variant* fails the
build instead of defaulting quietly.

Reproduced through a real `egui::Context` — two frames, one held ⌘T, no release
between — rather than by setting the flag by hand, because the claim under test is
egui's own behaviour. Noticed during #77's review, which fixed only its own chord
rather than fold an unrelated behaviour change into that PR.
