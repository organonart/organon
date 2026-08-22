# The module frame boundary, measured — GPU→CPU readback and re-upload on an RTX 5090

**2026-08-21.** `doc/organon_module_viewport.md` §4.4 names three numbers that do not exist and
refuses to choose between mechanism **A** (a shared GPU texture — zero copy, `wgpu-hal` interop,
`unsafe`, per-backend) and mechanism **B** (a shared-memory frame copy — portable, no `unsafe`,
the mechanism `ipc::ns_file` already runs on) until somebody produces them.

This document produces **number 1** — *"wall-clock cost of a readback of a region-sized texture on
the 5090, including the fence wait, on the producer's queue"* — and **number 3** — *"what that does
to the console's own frame budget"*.

🚨 **Number 2 is not here and was not attempted.** *"Frames of latency between 'the module drew it'
and 'the console painted it'"* needs a second process and a protocol that does not exist yet.
Nothing below is an estimate of it, and no number below should be read as one.

---

## The machine, and how to reproduce it

```bash
cd native
cargo test -p organon-render --release -- --ignored --nocapture
```

| | |
|---|---|
| host | `organon-one` — AMD Ryzen Threadripper PRO 9955WX (16C), 32 GB, Windows 11 Pro 10.0.26200 |
| adapter | **NVIDIA GeForce RTX 5090** (`DiscreteGpu`), driver `NVIDIA 610.88` |
| backend | **Vulkan** |
| wgpu | 30 |
| format | `Rgba8UnormSrgb`, 4 B/px — `console_main.rs`'s `BACKDROP_FORMAT`, not a stand-in |
| sampling | 64 iterations after 12 warm-up; **median and p90**, never a mean |
| code | `native/organon-render/tests/frame_boundary.rs`, `#[ignore]`d — no binary, no cargo feature |

📌 **The no-adapter path was exercised rather than assumed.** A test that claims to skip cleanly on
a GPU-less machine and has never been run on one is a claim, not a behaviour. Forcing wgpu to find
nothing reproduces it here:

```bash
WGPU_BACKEND=noop cargo test -p organon-render --release -- --ignored --nocapture
# SKIPPED: no wgpu adapter on this machine (No suitable graphics adapter found; ...)
# test result: ok. 1 passed; 0 failed
```

⚠️ **The backend is wgpu's choice, not an instruction.** §4.4 already notes that no `Backends::`
restriction is set anywhere in this tree, and that mechanism **A** needs both processes on the same
backend *and* the same adapter. On this machine, unrestricted, wgpu picks **Vulkan** — so these
numbers are Vulkan numbers, and a future D3D12 leg is a different measurement rather than a
rounding difference. This is the fact §4.4 called "a thing to pin before it is a thing to debug",
and it is now a thing that has been observed rather than assumed.

⚠️ **The test measures the frame *boundary*, not a frame.** The source texture is filled once and
copied repeatedly, so `submit→fence` is the copy alone. A real producer appends the copy after its
own render and the fence wait swallows both — but the *added* stall the boundary imposes is the
thing §4.4 asks for, and that is what is isolated here.

---

## The number §4.4 asked for: the producer's added stall

**The fused shape** — `submit`, then `map_async`, then **one** `poll(Wait)` — is what a real
producer would write. This is the stall that becomes stutter inside the game.

| size | bytes | **stall (median)** | **stall (p90)** | % of a 16.7 ms frame |
|---|---:|---:|---:|---:|
| 640 × 360 | 0.88 MiB | **0.064–0.071 ms** | 0.097–0.098 ms | 0.4 % |
| 900 × 506 *(unaligned)* | 1.74 MiB | **0.079–0.080 ms** | 0.110–0.111 ms | 0.5 % |
| 1280 × 720 | 3.52 MiB | **0.127–0.128 ms** | 0.154–0.176 ms | 0.8 % |
| 1920 × 1080 | 7.91 MiB | **0.199–0.204 ms** | 0.226–0.230 ms | 1.2 % |
| 2560 × 1440 | 14.06 MiB | **0.344–0.347 ms** | 0.376–0.386 ms | 2.1 % |

Ranges are across three separate runs of the whole sweep, because a single run of anything on this
machine has repeatedly turned out to be a number rather than a measurement.

**Split into its parts** (the split shape pays for a second `poll` to tell GPU time from map time;
buffers reused, which is the shape a real ring takes):

| size | encode | submit→fence | map wait | memcpy out | re-upload |
|---|---:|---:|---:|---:|---:|
| 640 × 360 | 0.003 | 0.066 | 0.003 | 0.037 | 0.087 |
| 900 × 506 | 0.003 | 0.077 | 0.002 | 0.081 | 0.115 |
| 1280 × 720 | 0.003 | 0.118 | 0.003 | 0.141 | 0.168 |
| 1920 × 1080 | 0.005 | 0.195 | 0.004 | 0.268 | 0.327 |
| 2560 × 1440 | 0.009 | 0.329 | 0.005 | 0.468 | 0.563 |

*(medians, ms)*

📌 **`map wait` is ~3 µs, and that is not the map being free.** Once the copy's fence has already
signalled, the buffer is immediately mappable and the second `poll` only runs the callback. The
wait is real and it lives in `submit→fence`; the split just puts it where it belongs. Anyone
reading a 3 µs map as "mapping costs nothing" has read the split shape as though it were the fused
one — the fused `stall` column above is the honest single number, and it is very close to
`submit→fence` + `map wait` in every row.

---

## Number 3: what a full round trip costs against 16.7 ms

The whole boundary — the producer's readback **plus** the console's `write_texture` back onto the
GPU — with buffers reused:

| size | producer half (stall + memcpy) | console half (re-upload) | **total** | % of 16.7 ms | % of 8.3 ms (120 Hz) |
|---|---:|---:|---:|---:|---:|
| 640 × 360 | 0.10–0.11 ms | 0.08 ms | **0.19 ms** | 1.1 % | 2.3 % |
| 900 × 506 | 0.16 ms | 0.11 ms | **0.26–0.27 ms** | 1.6 % | 3.2 % |
| 1280 × 720 | 0.27 ms | 0.17 ms | **0.44 ms** | 2.6 % | 5.3 % |
| 1920 × 1080 | 0.47–0.51 ms | 0.33–0.36 ms | **0.80–0.88 ms** | 4.8–5.3 % | 9.6–10.6 % |
| 2560 × 1440 | 0.83–0.90 ms | 0.58–0.65 ms | **1.41–1.55 ms** | 8.4–9.3 % | 17–19 % |

⚠️ **The two halves land in two different budgets and must not be added as though they were one.**
The producer pays `encode + stall + memcpy`; the console pays `re-upload`, on its own frame, beside
the `World` render it is already doing. The "total" column is the round trip, not a number either
process experiences.

---

## Three findings that were not the point and matter anyway

### 1. Allocation strategy is worth more than the copy, and gets it backwards if you skip it

Fresh staging buffer and fresh destination texture every iteration, against both allocated once:

| size | fresh, total | reused, total | ratio |
|---|---:|---:|---:|
| 640 × 360 | 0.288 ms | 0.196 ms | 1.5× |
| 1280 × 720 | 0.812 ms | 0.432 ms | 1.9× |
| 1920 × 1080 | 1.560 ms | 0.798 ms | 2.0× |
| 2560 × 1440 | 2.719 ms | 1.374 ms | 2.0× |

🚨 **And almost all of the difference is in `memcpy out`, not in the GPU work** — 1.561 ms fresh
against 0.468 ms reused at 1440p, a 3.3× gap on a phase that copies identical bytes both times.
That is first-touch page-faulting on a freshly allocated destination, not memory bandwidth. It is
worth naming loudly because it is exactly the shape that would produce a *confident wrong
conclusion*: a naive per-frame implementation measures 2.7 ms at 1440p, reads that as "the copy is
too expensive, we need mechanism A", and buys `unsafe` per-backend interop to fix an allocator
problem. **A ring is not an optimisation here; it is the difference between measuring the boundary
and measuring `malloc`.**

### 2. `memcpy out` is the phase that does not scale, and it is the console's real ceiling

`submit→fence` runs at 13 GB/s at 640×360 and 43 GB/s at 1440p — it gets *more* efficient with
size, as a PCIe transfer should. `memcpy out` is flat at 24–30 GB/s reused (and 8–9 GB/s when it is
also faulting pages in), because it is a single-threaded CPU row copy and nothing about a 5090
helps it. At 1440p it is the largest single phase of the producer's half. If this boundary is ever
the problem, the CPU copy is where to look first, not the GPU.

### 3. Row padding — what was applied, and where it would bite

`copy_texture_to_buffer` requires `bytes_per_row` be a multiple of `COPY_BYTES_PER_ROW_ALIGNMENT`
(256). At RGBA8 the four canonical sizes are **already aligned and pad by zero bytes**:

| width | unpadded row | padded row | waste |
|---|---:|---:|---:|
| 640 | 2560 B | 2560 B | 0 % |
| **900** | **3600 B** | **3840 B** | **6.25 %** |
| 1280 | 5120 B | 5120 B | 0 % |
| 1920 | 7680 B | 7680 B | 0 % |
| 2560 | 10240 B | 10240 B | 0 % |

⚠️ So a sweep of only "nice" widths would have exercised the padding path **zero times** while
looking complete, and whoever later picks a region 900 or 1100 px wide would meet it fresh. The
sweep carries 900 × 506 deliberately: it pays 6.25 % more staging bytes, and its `memcpy out` is a
genuine per-row loop rather than one contiguous copy. The cost is visible but small — 0.079 ms
against 0.037 ms at roughly twice the pixels, i.e. in line with the byte count rather than
penalised by the padding. **A region viewport is sized by a layout, not by a power of two, so the
padded path is the normal case and the aligned one is the accident.**

---

## What this means for the A-versus-B choice in §4.4

**Stated plainly: on this machine, mechanism B's copy is affordable at every size measured,
including the full pane.**

- The producer's added stall — §4.4's number 1, the one that becomes stutter in the game — is
  **0.06 ms at a small region and 0.35 ms at 2560 × 1440**. At 60 Hz that is 0.4 % to 2.1 % of a
  frame. A game that cannot absorb 0.35 ms has a problem the frame boundary did not cause.
- The full round trip at a **typical region viewport (1280 × 720) is 0.44 ms, 2.6 % of a 16.7 ms
  frame**, split across two processes' budgets.
- At the **full pane at 1440p it is 1.4–1.55 ms, 8–9 %** — still affordable at 60 Hz, and the first
  number in this document that would want a second look at 120 Hz, where it is 17–19 %.

That is the answer §4.4 was waiting for, and it supports the ordering the design already proposed
— *"B first, behind the same seam, and A when the measurement says the copy is what hurts"*. The
measurement says the copy does not hurt yet. It does not say A is unnecessary; it says A is not yet
*justified*, which is the standard §4.4 itself set for buying `unsafe` per-backend interop.

**Two conditions on that reading, both of which would change it:**

1. **A ring, not a per-frame allocation.** The fresh-allocation column is 2× the reused one and 2.7
   ms at 1440p. B is affordable *given* a preallocated ring; B implemented naively is not, and it
   fails in a way that reads like a verdict on the mechanism.
2. **60 Hz.** At 120 Hz the full-pane round trip is a fifth of the budget and the recommendation
   above is no longer obvious for the pane. It stays obvious for a region.

---

## What was NOT measured — read this before citing anything above

- 🚨 **Frames of latency across two processes** (§4.4's number 2). Not attempted. It needs a second
  process and a protocol that does not exist. Nothing here is a proxy for it, and the numbers above
  are *throughput and stall*, which is a different question from *how stale the painted frame is*.
- 🚨 **Mechanism A was not measured at all.** Nothing in this document says a shared GPU texture is
  faster, slower, or works. It says the copy is cheap enough that A does not have to be tried yet.
- **The shared-memory ring itself.** `memcpy out` here lands in process-local memory. A copy into a
  memory-mapped file is the same order of magnitude but not the same number, and the ring's
  synchronisation, double-buffering and tearing behaviour are untouched.
- **Contention.** The GPU is otherwise idle during this test. A real producer's copy competes with
  its own render; a real console's re-upload competes with `World`. Both numbers would rise, and by
  how much is not known from this.
- **Any machine that is not this one.** This is the fastest consumer GPU available, on PCIe 5, with
  a 16-core CPU. `memcpy out` in particular is a CPU number and will not travel. Nothing above
  should be quoted as "the cost of a frame copy" without the adapter line beside it.
- **The `World` render, the region path, `engine_plan`, or anything in the console.** No console
  file was touched and no console behaviour was exercised. This is a standalone harness with its
  own device and queue.
