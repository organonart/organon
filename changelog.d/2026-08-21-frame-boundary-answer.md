### Design

- **The module-viewport design stops saying nobody has measured the frame boundary.** T0 answered
  two of §4.4's three numbers on the 5090 the same day, and a design document asserting a gap the
  tree has closed is the drift this repo spends its refusals preventing. §4.4 now carries the
  table, §9's T0 row is marked done, T2's row records what the measurement changed about its
  brief, and §10's closing line answers the question it originally declined.

  The reading: **mechanism B — the shared-memory copy — is affordable at region size (2.6 % of a
  16.7 ms frame at 1280×720) and still affordable at the full pane (8–9 % at 1440p)**, so the
  zero-copy shared-texture path is not yet *justified*, which is the standard §4.4 itself set for
  buying `unsafe` per-backend interop.

  ⚠️ Two conditions travel with it and both are the measurement's rather than a preference: the
  ring must be **preallocated** — a fresh buffer and texture per frame costs 2.72 ms against
  1.37 ms at 1440p, and 3.3× of the gap is `memcpy out` copying identical bytes, i.e. first-touch
  page faulting rather than bandwidth — and the figures are **60 Hz**; at 120 Hz the full pane
  wants another look.

  🚨 **The third number is still missing and the doc now says so twice.** Frames of latency across
  two processes was not attempted and nothing measured is a proxy for it: throughput and stall are
  a different question from how *stale* the painted frame is, and staleness is exactly what §6
  says stops being affordable at full screen.

  📌 The T0-before-T2 ordering paid: the measurement changed T2's brief in two ways a wire format
  would have had to be rebuilt for.
