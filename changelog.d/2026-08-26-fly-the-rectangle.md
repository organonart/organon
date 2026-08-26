### A hosted module can be flown — the console finally speaks the input protocol

`organon_module::input` has carried the input grant since T5b's contract landed: four events, a
published set of keys that can never be delivered, and a refusal table saying what a hosted
module may never be handed. **Nothing on the console side ever spoke it.** A producer could draw
a picture into a region and had no way to be driven — the grant fully specified, fully tested,
and wired to nobody.

🚨 **The way out was decided before the way in was built**, as §5.3 requires. **`Escape`
leaves.** It is safe to spend because it is already in `RESERVED` — refused at the *encode* site
so a console that forgot could not leak it, and published in the mapped header so a module that
does not link the crate is still *told*. A game that swallows Escape and a console that needs
Escape never collide, because the module never receives it.

**A click inside a hosted rectangle takes the latch**, which is one `Option<String>` and not a
set: two hosted regions cannot both be flying, and moving it hands back the displaced producer.

🚨 **Every exit emits `ReleaseAll`, structurally rather than by memory.** `Latch::latch` and
`Latch::release` are `#[must_use]` and answer *who* must be told, so an exit that forgot would
not compile. There are four, and the fourth has no gesture behind it: `Escape`, focus loss, a
click on a different hosted rectangle, and **the producer's rectangle no longer being drawn** —
`/viewport` can take the region away, or the process can die, while keys are held. They prevent
one failure: a key that went down inside a flight whose `Up` the module never sees, leaving it
thrusting forever with nobody at the keyboard.

⚠️ **Keys are taken from the frame's `RawInput` before `Context::run` sees it.** While a
rectangle is flown, `W` is thrust and not a character, and the composer must never see it.

🚨 **The first cut did this against `Context::input_mut` and it did nothing at all.**
`Context::run(raw, …)` calls `InputState::begin_pass`, which rebuilds the frame's state with
`events: new.events.clone()` and `focused: new.focused` **straight from the incoming `RawInput`**
(egui 0.33.3, `input_state/mod.rs:571`) — so the steal was applied to the previous frame's
leftovers and discarded. The composer went on receiving every key, and the module was fed events
one frame stale. **Found by the automated review, verified at egui's source rather than taken on
trust, and the fix is not a move but an extraction**: `take_from_raw` is now a function over
`&mut Vec<egui::Event>`, which can be tested against a hostile frame. Six tests came with it,
including one that fails against the exact original bug.

⚠️ **Buttons are SHARED rather than stolen**, which the fix forced and which is load-bearing:
consuming them would make clicking a *different* hosted rectangle impossible — one of the four
exits, which would have become an unreachable exit pretending to be a design — and would lock
the console's own chrome while something was flying.

⚠️ **Motion is coalesced into one event and ordered after every button transition.** A module
cares about the frame's displacement, not egui's sampling of it; and a click-then-drag must
arrive as *down, then moved*, or a drag begins before the button it belongs to. A still frame
sends nothing at all.

⚠️ **The delta, never the position**, and the fix made this stronger: motion now comes from
egui's `MouseMoved`, which is *already* a delta, so no conversion exists in which an absolute
position could leak. `PointerMoved` is never sent and is left in the list, because egui needs it
for its own hover state.

📌 **Three guards on the reserved keys, and only one is the guarantee.** `input::push` refuses
them; `translate` drops them so a flown module cannot even cost a ring slot for an undeliverable
event; and the frame loop takes `Escape` without forwarding it, which is the one that gives a
person their keyboard back. Never rely on the second or third alone.

📌 **The music needed no change.** §5.2 already answers it: the protocol has deliberately no
audio path, *"a separate process can open WASAPI itself and Ascent already does — the grant is
honoured, not prevented."* A mute control is a separate question and does not want a protocol
verb; the console owns the child process, so the OS mixer is the place for it.

⚠️ **Green and ready to try, not verified working.** Nobody has flown anything. The latch, the
translation and the key map are unit-tested and mutation-tested; what is unverified is whether
Ascent reads these as flight, whether the pointer delta is the right scale, and whether
click-to-latch is the gesture that feels right in a divided pane.
