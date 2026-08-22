# How stale is a hosted module's frame — §4.4's number 2, across two processes

**2026-08-22.** `doc/organon_module_viewport.md` §4.4 named three numbers. PR #139 measured two of
them (`module-frame-boundary-2026-08-21.md`) and was explicit that it had not touched the third:

> 🚨 **The third number is still missing and nothing above is a proxy for it.** Frames of latency
> between *"the module drew it"* and *"the console painted it"* was not attempted, because it needs
> a second process and a protocol that does not exist. The numbers above are **throughput and
> stall**, which is a different question from **how stale the painted frame is** — and staleness is
> what §6 says stops being affordable at full screen.

This document produces it. T2 built the protocol, T5 built the launcher, and `organon-module-sim`
is a producer in its own process — so the number is now a subtraction over `FrameView::age`, which
is what T2 put on the wire for exactly this.

---

## The short answer

**Staleness is set by the two loops' cadences, not by the frame size.** At a 60 Hz consumer against
a ~16 ms producer, the median frame the console takes is **8–11 ms old**, p90 **15–16 ms**, worst
**17–18 ms** — that is **half a frame at 60 Hz**, and it barely moves between 640×360 and 2560×1440
despite nine times the pixels.

🚨 **So §6 stays open.** The reading that would have closed it — staleness dominated by the copy,
therefore fix it with mechanism A's `unsafe` per-backend interop — is not what the measurement
says. The transport's contribution is inside the noise of two free-running loops sampling each
other.

⚠️ **And the number is not a licence to fly at full screen.** It says the *transport* is not the
thing in the way; it does not say that a 60 Hz poll of a 60 Hz producer feels good under the hand,
which is a question about input-to-photon and about phase, and is not answered here.

---

## The machine, and how to reproduce it

```bash
cd native
cargo test -p organon-module --features sim --release --test staleness \
    -- --ignored --nocapture --test-threads=1
```

| | |
|---|---|
| host | `organon-one` — AMD Ryzen Threadripper PRO 9955WX (16C), 32 GB, Windows 11 Pro 10.0.26200 |
| producer | `organon-module-sim`, release, its own process |
| consumer | `ModuleChannel::poll` at **60 Hz** (16.667 ms), release |
| format | `Rgba8UnormSrgb`, three slots, padded rows |
| samples | 240 frames per condition after 60 discarded |
| commit | branch `module/t5-the-picture-arrives`, off `main` @ `25dfd5b` |

🚨 **`--test-threads=1` is part of the command.** See *the trap* below.

---

## 🚨 What is measured, and the two things that are not

Measured: **publish → the consumer holds the pixels.** The producer's `commit`, the console's next
poll, the seqlock read, and the memcpy into the console's staging buffer. That is the part the
*protocol* owns, and the part that would have to change if the answer were bad.

Not measured, and no number here should be read as including them:

1. **The console's own frame** — egui layout and paint, `write_texture`, the render pass, the
   swapchain present. These happen *after* the poll and are the same cost the console already pays
   for every other picture in the window.
2. **The producer's render.** That is the module's own `render`/`read_frame`; Ascent measured its
   side at a 0.12 ms median render and a 0.61 ms median readback at 1080p (`organonart/ascent` #84).

⚠️ So the honest headline is *"the transport adds this much age to a frame"*. A number for *"what is
on the glass is N ms old"* is this **plus a console frame**, and quoting the first as the second
would be citing a measurement for something it did not measure.

---

## Staleness against frame size — consumer 60 Hz, producer ~16 ms

| size | median | p90 | worst | median, in 60 Hz frames | torn |
|---|---:|---:|---:|---:|---:|
| 640×360 | 8.39 ms | 14.93 ms | 16.89 ms | 0.50 | 0 |
| 1280×720 | 11.32 ms | 15.74 ms | 16.99 ms | 0.68 | 0 |
| 1920×1080 | 7.77 ms | 15.19 ms | 17.49 ms | 0.47 | 0 |
| 2560×1440 | 9.30 ms | 16.27 ms | 17.94 ms | 0.56 | 0 |

📌 **Read the column, not the rows.** 1440p has **nine times** the pixels of 640×360 and is not
reliably worse — 1080p came in *below* 720p on this run. There is no trend here; there is scatter
around half a frame. That is the finding.

⚠️ **`torn` is zero everywhere**, which is the ring's reader-hold working against a real second
process rather than against a staged interleaving in one. It is reported because a non-zero value
would mean these timings were taken over frames that were partly rewritten under the copy.

---

## The control: staleness against the producer's *cadence*, at a fixed 1280×720

The table above is not what a transport cost looks like, so the hypothesis has to be tested rather
than asserted: the age of a frame at poll time is dominated by **the phase between two free-running
loops**. Hold the size fixed and move the producer's period.

| producer draws every | median | p90 | faster loop's period | median ÷ that |
|---|---:|---:|---:|---:|
| 4 ms | 2.61 ms | 4.43 ms | 4.0 ms | **0.65** |
| 8 ms | 4.49 ms | 8.08 ms | 8.0 ms | **0.56** |
| 16 ms | 9.15 ms | 13.70 ms | 16.0 ms | **0.57** |
| 33 ms | 10.08 ms | 16.13 ms | 16.7 ms | **0.60** |

**The last column is flat at ≈0.6 across an eight-fold change in cadence.** Staleness is
`≈0.6 × min(producer period, poll interval)` and the frame size is not in the expression.

🚨 **Two readings, two completely different consequences, which is why this control exists.** If
staleness were the transport, the fix is mechanism A — the shared GPU texture, `unsafe`,
per-backend — and §6's *"flying inside a small pane may not be the thing"* is settled against. It
is phase, so the transport contributes almost nothing, the levers are a faster producer or a
synchronised one, and mechanism A remains *not yet justified* on this evidence as well as on T0's.

### ✏️ The model in the first draft was wrong, and the measurement is what said so

The control was first written predicting **median ≈ P/2**, half the *producer's* period. It passed
at 4, 8 and 16 ms and **failed at 33 ms**, reporting 6.43 ms where P/2 is 16.5.

The failure was the useful part. Once the producer is **slower than the consumer**, the consumer is
no longer the thing doing the sampling — every frame is seen at the first poll after it appears, so
the age spreads over the *poll* interval instead. The correct model is half of whichever loop is
**faster**, because that is the one setting how long a frame sits unlooked-at:

> **median ≈ 0.6 × `min(producer period, poll interval)`**

which is why the table's fourth column says `min(P, 16.7)` rather than `P`. The conclusion is
strengthened rather than weakened: staleness is bounded by the cadences at *both* ends of the sweep
and by the frame size at neither.

---

## ⚠️ The trap this rig set for itself, recorded because it produced a credible wrong table

The two tests here run on different threads of one process and both measure 1280×720. The channel
file was named from `(pid, width, height)` — so they got **one file**: two consoles and two
producers on one mapping.

**It did not present as an error.** One test reported `only 0 sample(s) in 30 s` and skipped that
row; the other reported a 1280×720 median of **4.37 ms**, roughly 30 % better than its own solo run
of the same condition, which reads as a good measurement rather than as a collision. The full table
it printed looked complete.

Two separate fixes, because they address different halves:

- **The filename now carries a counter**, which removes the corruption.
- **`--test-threads=1` is part of the documented command**, which removes the *interference* — two
  producers and two consumers sharing a CPU means each rig reports the other's load as its own
  latency, and a unique filename does nothing about that.

📌 The general shape, for the next timing rig in this tree: *a measurement harness that is quietly
measuring a second copy of itself produces numbers that are wrong in the flattering direction and
entirely reasonable on the page.*

---

## What this does not settle

- **Mechanism A is still unmeasured**, exactly as after T0. Nothing here says a shared GPU texture
  is faster, slower, or works.
- **Nothing here is about input.** §5.3's click latch and `Lifecycle::Running` are the next tier;
  input-to-photon is a different measurement with a different rig, and it is the one §6 actually
  turns on for flying.
- **The producer is a simulator**, not a game. It draws a gradient, so it competes with nothing for
  the GPU. A real module's copy contends with its own render and the console's re-upload contends
  with `World` — T0 made the same caveat about the same machine and it is unchanged here.
- **One machine, one consumer cadence.** At 120 Hz the poll interval halves, and by the model above
  so does the staleness floor once the producer keeps up — untested.
