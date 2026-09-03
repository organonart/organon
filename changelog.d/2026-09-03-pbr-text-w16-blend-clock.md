### PBR text W16 — the blend clock runs on producer time, with the frame as its lead

T12 read `world.rs`'s inter-tick blend and reported that a 120 Hz producer on a 60 Hz
display was drawn two ticks behind and never between. **Confirmed by reading the code, and
the diagnosis was right about the symptom and half-right about the cause.** The clock was
`(now − seen_at) × tick_hz`, with `seen_at` the instant the world *read* the grid, evaluated
at build time: every 60 Hz frame reads a fresh 120 Hz grid at `since ≈ 0`, so `blend ≈ 0`
and `lerp(prev, cur, 0)` is the grid read one frame *earlier* — `Exact`, one read late, two
ticks behind. At 60 over 60 the same arithmetic drew one tick behind. A path only ever slid
when the display outran the producer. T12 attributed it to the ring carrying no publish time.
⚠️ **A publish stamp alone would not have closed it.** Two things were wrong: the period was
`1 / tick_hz` whatever the pair actually spanned (two ticks at 120/60), and the blend was
computed for the build instant of a frame that is *shown* a display period later. Measure
from the publish instead of the read and 120/60 still lands at `blend ∈ 0..0.5` — the newest
grid is 0–8 ms old at the read, over a 16.7 ms pair — drawn between two and one ticks behind.
What closes it is the **lead**: the world knows its own frame interval, and a frame built
now is the picture at `now + one interval`.

    blend = (now − tick_at + lead) / period

`period` is the pair's producer-time span, `Δtick / tick_hz`, from what is already on the
wire; `tick_at` the world instant the pair started; `lead` the world's last frame interval.
120/60: period 16.7 ms, lead 16.7 ms → 1 on every frame, the newest sample, two ticks a
frame, uniform. 30/120: period 33 ms, lead 8.3 → 0.25 / 0.5 / 0.75 / 1.0, four interpolated
frames, reaching each tick exactly as the next lands. 60/60 → 1: the sample itself. 90/60 —
`Δtick` alternating 1, 2 — an even 1.5 ticks a frame throughout. A stalled producer clamps
at 1 and holds. So what `Slide` costs is now **up to one producer period, and only while the
display outruns the producer**; `lower_grid`'s and `Motion`'s docs said "one tick" and are
corrected. All of it is `glyph_ring::BlendClock` (`Arrival`, `classify_arrival`,
`blend_for`), pure over synthetic seconds, so the schedules are pinned without a display.

📌 **Nothing new travels on the wire, and `layout_version` does not move.** The brief left
it open whether `tick / tick_hz` already *is* the producer's clock. It is, within an epoch:
the producer paces by a drift-free deadline, `tick` advances once per publish, and T11 keeps
its phosphor time nominal for the same reason (a seed reproduces a run). The world needs the
*span* between the two grids it holds, never the producer's absolute time, and a nominal span
is the better predictor of when the next tick lands than a measured one — a late publish
would otherwise stretch the *next* slide. A wall stamp buys nothing here except a phase
lock, which this deliberately is not (below). `GlyphFrame.tick`'s doc now says it is the clock.

⚠️ **A heartbeat does not restart the clock — and the settle publish is a heartbeat.** The
producer republishes at the *same* `tick` for the settle frame and every dwell beat
(`organon-glyphs/src/main.rs`), which is how the world tells one from a tick without a wall
stamp: `classify_arrival` says `Heartbeat`, the world replaces its current grid **without
rotating the previous one** (a new frame is read into a scratch grid first, so the clock can
classify it before anything moves) and `tick_at` / `period` are untouched. Two consequences.
The last tick of an effect now slides to completion under the settle frame instead of
snapping to it — the old code reset the clock on every `seq`, so the final tick's motion
was never shown. And T11's trails decaying through the dwell — a payload change at the same
tick, `generation` moving — move nothing, which T5 and T11 depend on. The silence detector
(`glyph_seen_at`, 3 s) still resets on every publish; it and the blend clock were one field
and are now two, because they answer different questions.

**Every claim mutation-tested.** A heartbeat that restarts the clock fails
`heartbeat_does_not_restart_the_clock` with *"a heartbeat must not restart the blend clock:
the slide from tick 1 to tick 2 is half-way at +16.7 ms whether or not a settle frame arrived
at +18 ms — left: 0.25, right: 0.5"*. Removing the lead (the old clock, measured from the
read) fails six, the 30/120 pin printing the series the old clock produced — *"left: [0.0,
0.25, 0.5, 0.75, …], right: [0.25, 0.5, 0.75, 1.0, …]"* — and the 120/60 pin printing T12's
number, *"frame 1: blend 0 — drawn behind the newest sample"*. A period that ignores the tick span (`1 / tick_hz` always)
survives 120/60 and 30/120 — the lead clamps it to 1 either way — and dies on 90/60 with
*"step 2: 2 ticks a frame, not 1.5"* and on the classifier's *"two ticks apart is a two-tick
pair"*. Dropping the clamp fails the stall pin (*"at +0.05 s: 2.3999999"*) and the dwell
heartbeats (*"held at the settled grid — left: 15.749998"*). Removing the epoch check
survived the original arrivals pin (a new effect's first tick is *lower*, so the tick
comparison called it a cut anyway) and the pin was strengthened until it did not: *"a new
effect is a cut whatever its tick says — left: Tick(0.008333334), right: Cut"*.

📌 **Not a phase lock, and where the brief and the code disagree.** The brief expected 120/60
to land at `blend ≈ 0.5`; it lands at 1, on purpose. When the producer outruns the display
there is nothing to bridge — every frame has a fresh sample — and 0.5 would draw exactly the
*previous* tick's position: `Exact`, one tick late, which is the shape of the defect with the
number halved. The lead is the world's frame interval, not an estimate of the producer's
phase, so at ratios that are neither `1:n` nor `n:1` (100 over 60) the step varies by up to a
fraction of a tick per frame. A locked render clock — a wall stamp on the frame, an offset
estimated as a running minimum, a constant latency — would make that uniform too, at the
cost of state that re-estimates on every epoch; it is the next step if the GPU look shows
judder at an odd ratio, and not before. Green and ready to try: `organon-glyphs --effect
slide --tick-hz 30` on a 60 Hz display is the case that interpolated nowhere before and
should show two in-between positions per tick now; the default 120 Hz producer should look
like `Exact` with no lag where it looked like `Exact` two ticks late.
