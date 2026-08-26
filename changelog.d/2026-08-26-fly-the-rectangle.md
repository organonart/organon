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

⚠️ **Keys are taken from the raw event list before any widget reads them.** While a rectangle is
flown, `W` is thrust and not a character, and the composer must never see it. That is
`composer_keys`' own idiom — arbitration has to happen before a widget reads, or both act on one
keystroke.

⚠️ **Motion is coalesced into one event and ordered after every button transition.** A module
cares about the frame's displacement, not egui's sampling of it; and a click-then-drag must
arrive as *down, then moved*, or a drag begins before the button it belongs to. A still frame
sends nothing at all.

⚠️ **The delta, never the position** — `PointerMoved` is dropped rather than converted, on the
refusal table's own reason: a producer that could place the cursor could place it over a
confirmation button in some other window.

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
