### Added

- **`organon-module` — the hosted-module contract, and Organon's consumer half of the frame
  ring.** `doc/organon_module_viewport.md` §9's **T2**, and the tenth workspace member. One
  sentence to keep — §1.14's *"a producer yields a texture the console can sample, at a size
  the console asks for"* — and both trees depend on it: Organon's console to host a module,
  `organonart/ascent` to be one.

  **Mechanism B, per T0**: a memory-mapped frame ring, **three slots, preallocated**. The slot
  count is the argument — a producer must never write the slot the consumer is reading nor the
  one it has just published, and two leaves it a choice of zero in the worst case, so it would
  either stall on the consumer's upload or overwrite it. 🚨 **Preallocation is the
  measurement's condition, not a preference**: T0 found a fresh staging buffer and destination
  texture per iteration cost **2.72 ms at 1440p against 1.37 ms reused**, with 3.3× of the gap
  in a `memcpy` of *identical bytes* — first-touch page faulting. A naive path therefore
  measures 2.7 ms, reads that as a verdict on the mechanism, and buys `unsafe` per-backend GPU
  interop to fix an allocator problem. So the absence of an allocation is **asserted rather
  than commented**: `staging_allocations()`, `FrameTexture::allocations()` and
  `FrameReadback::allocations()` are all pinned at 1 across sixty frames and an oscillating
  size.

  **How a consumer knows a frame is whole**, which is the failure the ring exists to prevent
  and is invisible when it works. Three things, and it takes all three: a **per-slot seqlock**
  (odd while writing, `organon-core::ipc`'s `Shared` discipline verbatim, including that the
  counter must never repeat a value); `latest_slot` stamped **only after** that seqlock closes;
  and a **reader's hold** the producer never writes over. ⚠️ The third is a rule about a
  well-behaved producer and the first is what survives one that is not — so the consumer copies
  into its own buffer, **re-checks the counter afterwards**, and discards a slot that moved.
  A torn read is a *skipped frame*, never a painted one, and it is counted rather than given a
  `Poll` variant, because "skip and keep the picture" is a behaviour the caller already has.
  📌 A detector that is silent whenever it works cannot be tested by inspection, so
  `poll_interfered` stages the exact interleaving deterministically, in one thread — a threaded
  version would prove nothing on a good day and flake on a bad one.

  **A producer that dies mid-frame leaves an odd counter and half a picture in a slot
  `latest_slot` does not name**, so the frame path never sees the wreckage and goes on serving
  the last whole frame. The death shows on the *liveness* path instead, which is where a fact
  about the producer belongs. 🚨 **Dead versus slow, stated with its limit**: the counter is
  bumped once per producer **loop**, not once per frame — which is what tells *paused* from
  *hung*, and matters because paused is the state a module arrives in — and `Gone` is a
  farewell, so a deliberate exit is not waited out. ⚠️ **Hung and exited-without-a-farewell are
  not distinguishable from inside a shared mapping**, and nothing here pretends otherwise: the
  thing that knows is the process handle, which is T3b's launcher's. §4.6's *never the last
  good frame* is made structural rather than a rule — `Poll::Frame` is unreachable once the
  producer is judged stalled, lost or gone, and `Present` turns the verdict into the one
  instruction a caller's texture obeys, so a call site that forgets drops its picture anyway.

  🚨 **And one row of §4.6 nearly got away — a producer that refuses every frame is alive and
  silent.** Raised by the Ascent session against the first cut of the contract. It ticks, so
  the liveness counter moves, so every liveness rule calls it healthy; the state it most
  resembles is `Paused`, which is the **arrival state**, i.e. the least alarming conclusion
  available about the case §4.6 most needs named. Closed by two things, and it takes both: the
  producer may declare `Refusing` with a `RefusalReason`, and — the load-bearing half — the
  console **times frame silence on its own clock regardless**, because the party least able to
  notice it has stopped producing is the producer. ⚠️ The reason is a **name, never free
  text**: a string a module wrote, rendered in the console's chrome, is the module speaking in
  the console's voice. ⚠️ And the clock rule applies only while the console has asked for
  `Running`, restarting when it asks — without that condition it would accuse **every module
  on arrival**, since `Attached` is where they all start and an attached producer draws once
  and then legitimately nothing for ever. All three guards are mutation-tested.

  **Which side owns the size**, in three sentences: the console owns the **capacity**, once; the
  console **asks**, with no deadline; the producer **answers per frame**, and the frame is the
  truth. 📌 The consequence is what makes a resize cheap — frames already in flight carry the
  old size and are perfectly good pictures, so nothing has to be flushed. A `size_epoch` rides
  every frame, because a 640×360 request answered by a 640×360 frame is ambiguous between "it
  caught up" and "it has not moved", and the epoch is not.

  🚨 **Input carries four verbs and refuses everything else, and the refusals are the
  load-bearing half.** Four because Ascent's own `InputEvent` is four; the modules plan §10's
  rule is that every verb added to the protocol is a grant. No text or IME (keylogger-shaped),
  no absolute pointer or warp (the console owns the cursor), no clipboard, no file paths, no
  raw OS handles (that is mechanism A arriving through the input channel), no audio — and **no
  generic message or opaque payload**, which is the one addition that would make every future
  verb free, which is to say ungranted for ever. §5.3's way out is `input::RESERVED`, refused
  at the **encode** site so a console cannot leak it by forgetting; `F11` is deliberately left
  off it with the argument on both sides recorded, because that balance wants T5's interaction
  latch in front of it.

  **Versioning**: the magic at byte 0 and the wire version at byte 8 are the only permanent
  positional commitments, so a mismatch is diagnosed **before** any field whose position that
  version decides — a legible refusal naming both numbers, never a garbled picture.

  ⚠️ **The rows are padded, not tightly packed, and the argument is worth more than the rule
  because the two conventions agree by accident at every width anyone would test.** A tight
  `width * bpp` row costs strictly more work on both sides — the producer repacks to strip
  padding it already has off the GPU, and the console re-pads to upload — while matching
  `COPY_BYTES_PER_ROW_ALIGNMENT` makes the producer's staging layout *be* the ring's layout.
  At 640, 1280 and 1920, `width * 4` is already 256-aligned, so a tight producer and a padded
  consumer produce identical bytes and every natural test passes. It breaks at **900 wide —
  3600 tight against 3840 padded — and the symptom is a sheared picture, not an error**,
  because every byte is a valid pixel and only the row boundaries moved. The suite now carries
  a 900-wide round trip and a 437-wide one. ✏️ This was a real disagreement with Ascent's first
  producer, which had stripped the padding.

  ⚠️ **`MIT OR Apache-2.0`, one dependency (`memmap2`), and `cargo tree` is the acceptance test
  in both trees.** Ascent's invariant 3 forbids an edge to `organon-visual` or
  `organic-math-native`, so a transitive arrow from here would break a *second repository's*
  licence posture. The contract is also **wgpu-free**, deliberately: a wgpu type on the wire
  ties two independently built binaries to one wgpu version, `TextureFormat` has no stable
  numeric representation to put on a wire, and requiring it would say a producer must be a wgpu
  program. The two preallocated GPU halves live behind an optional `wgpu` feature instead.

  ⚠️ **§4.4's number 2 is still not measured and nothing here estimates it.** Both halves of
  the suite live in one process, which proves correctness and says nothing about staleness.
  ✏️ What changed is that it is now a **subtraction rather than a research project**: every
  frame carries the producer's `SystemTime` at publish and `FrameView::age` is the difference,
  so the number can be taken the moment two processes exist — which is the thing the design
  said had to wait for T1 and T2.

  Nothing is wired into a region yet: T4 owns the producer qualifier and the call site, and
  this tier deliberately names no viewport, no content word and no producer.
