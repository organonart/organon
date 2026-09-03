### PBR text — the editor mirror is pinned: every actuatable id moves its slider, by test

#235 counted seven sites a new actuatable id costs and said the seventh — the editor's
apply-channel mirror, `apply_agent_change` in `lib.rs` — is pinned by nothing: leave an id
out of its `match` and `organon set` still moves the picture, because the visual's override
lane is what draws; only the editor's slider stops following, and `Shared` — which the
editor publishes from `params.to_shared()` and `organon describe` reads — stays put. A GPU
session measured exactly that shape after #246 (`describe atmos_enabled` still 1 after
`set atmos_enabled 0`, `glyph_gain 0.7` landing fine), so this is the pin, written to name
that failure: `the_editor_mirror_writes_every_actuatable_param` drives one `ApplyOp::Set`
per `ACTUATABLE_IDS` entry through the real function with a recording `GuiContext` and
checks the two things the mirror is responsible for — **which** param it wrote (the
`ParamPtr`, resolved through `param_map()` to the host id a DAW sees) and **what** plain
value the wrapper would apply (`preview_plain` of the normalized it was handed) — and
`the_editor_mirror_lands_the_dark_room_on_the_right_params` is that GPU measurement,
editor-side: `glyph_gain 0.7` then the nine dark-room ids at `faceplate`'s values, each on
its own host id at the value asked for, a flag thresholded at 0.5 and the count truncated.

⚠️ **A test cannot move a nih_plug parameter, and the pin is shaped around that.** The
setters (`set_normalized_value`, `set_plain_value`) live in nih_plug's crate-private
`ParamMut`; only a wrapper reaches them, and `ParamPtr`'s copy is `pub(crate)`. So the
mirror's contribution is pinned at the wrapper boundary — the standalone wrapper queues
`(ParamPtr, normalized)` from the GUI thread and applies it on the audio thread, identically
for every param — and the params → `Shared` → `current` leg is pinned separately by slot
default: the slot `current` names for an id must carry that id's own default, which a
neighbour's would not (`atmosphere[1]` is turbidity 2.0 against `atmos_enabled`'s 1.0;
`hal_width` defaults to 1 against `hal_amount`'s 0). Mutation-tested four ways, each caught
by name: an arm removed (`has no arm for ["hal_amount"]`), a flag thresholded at 1.5
(`ml_enabled (mlen): asked 1, the wrapper would apply 0`), an arm writing the wrong param
(`hal_amount wrote a different param — left: "halwid", right: "halamt"`), and the T3
control removed (`["glyph_gain"]`).

📌 **All seventy-four ids pass on the tree as merged — the mirror has a live, correct arm
for every one of the nine.** So the GPU measurement is not a defect in this code path, and
nothing here is a fix. What the tree says instead: the only id-specific gate between
`organon set` and the slider is `organon-agent`'s `id_range` — the CLI validates with it,
the visual's `apply_ops` filters the apply line with it, and its `dispatch` rejects with it
— and a **visual built before #246 has the nine off that list**: it prints nothing to the
apply channel, refuses the lane with `no Tier-1 actuation route for 'atmos_enabled'` in
`organon-mind.jsonl`, and leaves the room lit, while a #246-era CLI still says `queued`
and a #246-era editor never receives a line to mirror. The pre-#246 ids (`glyph_gain`,
`env_intensity`) work through the same stale visual exactly as measured. The other split —
a #246 visual under a pre-#246 standalone — darkens the room and leaves `describe`
unchanged. Which binary is which is a question for the machine, not the tree: the
standalone spawns the visual from `ORGANIC_MATH_VISUAL` first, then beside its own dylib,
then beside its own exe, then off `PATH` (`spawn_visual`), so a rebuilt `target/release`
is not necessarily what ran.
