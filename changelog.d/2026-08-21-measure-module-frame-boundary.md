### The module frame boundary has a number now: 0.35 ms at 1440p, and the allocator was the trap

`doc/organon_module_viewport.md` §4.4 sets out two ways a hosted module's frame can reach the
console — **A**, a shared GPU texture (zero copy, `wgpu-hal` interop, `unsafe`, per-backend), and
**B**, a shared-memory frame copy — and then declines to choose, because *"nobody has measured
either"*. `native/organon-render/tests/frame_boundary.rs` measures B. It is an `#[ignore]`d test:
no new binary, no new cargo feature, no default build cost, and on a machine with no adapter it
prints why it skipped and passes, because CI has no GPU and this is a report rather than a gate.

```bash
cd native
cargo test -p organon-render --release -- --ignored --nocapture
```

**The number §4.4 asked for — the producer's added stall, on its own queue — is 0.064 ms at
640 × 360 and 0.347 ms at 2560 × 1440** on an RTX 5090 over Vulkan, i.e. 0.4 % to 2.1 % of a
16.7 ms frame. The full round trip including the console's `write_texture` back is 0.44 ms at a
typical 1280 × 720 region and 1.4–1.55 ms at the full pane at 1440p. Every number, the phase
breakdown, and what was deliberately *not* measured is in
`doc/measurements/module-frame-boundary-2026-08-21.md`.

🚨 **The finding that would have inverted the conclusion is the allocation strategy, not the copy.**
A staging buffer and destination texture allocated fresh each iteration costs 2.7 ms at 1440p
against 1.37 ms when both are reused — and 3.3× of that gap sits in `memcpy out` (1.561 ms against
0.468 ms) copying identical bytes, which is first-touch page faulting rather than memory bandwidth.
A naive per-frame implementation therefore measures 2.7 ms, reads it as *"the copy is what hurts"*,
and buys `unsafe` per-backend interop to fix an allocator problem. A ring is not an optimisation
here; it is the difference between measuring the boundary and measuring `malloc`. The test reports
both conditions side by side for exactly that reason.

⚠️ **The backend is wgpu's choice and it is now observed rather than assumed.** §4.4 notes that
mechanism A needs both processes on the same backend and the same adapter, and that no `Backends::`
restriction is set anywhere in this tree. Unrestricted, on this machine, wgpu picks **Vulkan** — so
these are Vulkan numbers and a D3D12 leg would be a different measurement. The test prints the
adapter, driver and backend on every run for that reason.

⚠️ **Row padding is exercised on purpose rather than by luck.** `copy_texture_to_buffer` wants
`bytes_per_row` aligned to 256, and at RGBA8 every one of 640, 1280, 1920 and 2560 is *already*
aligned — a sweep of only those widths pads by zero bytes while looking complete, and leaves the
padding path unexercised for whoever sizes a region 900 px wide from a layout. The sweep carries
900 × 506 for that, and every row of output prints the padding it applied.

🚨 **What is not here: §4.4's number 2, frames of latency across two processes.** It needs a second
process and a protocol that does not exist yet, it was not attempted, and nothing in the
measurement is a proxy for it. Mechanism A was not measured at all either — the document says the
copy is cheap enough that A is not yet *justified*, which is not the same claim as A being
unnecessary.
