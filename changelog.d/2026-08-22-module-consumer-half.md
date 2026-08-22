### A hosted module now runs, and its rectangle finally has something in it

- **The console's consumer half of the module contract.** `organon-module` had every byte of the
  protocol and nothing in the workspace depended on it; a region holding `3d ascent` painted a
  sentence because there was no process to draw a picture. Now the layout is the trigger: assigning
  that content **creates the channel, launches the module, asks it for a size, feeds it the pointer
  and paints what it publishes** — and says which of `doc/organon_module_viewport.md` §4.6's four
  sentences is true whenever it cannot. `organon-console/src/module_host.rs` is the wgpu-free half;
  `console_main.rs::service_module_frames` is the per-frame pass that owns the texture, because the
  window does.

  🚨 **The producer is handed the channel's FULL PATH, in `ORGANON_MODULE_CHANNEL`** — the decision
  a second repository is written against. Handing over `$ORGANON_IPC_NS` and letting the module
  rebuild the path was the obvious alternative and it is structurally wrong: `channel_file_name` is
  in `organon-module` and a module has it, but `ns_file` is `organon-core`'s, and
  `organon-module`'s manifest **forbids depending on that crate** — *"taking a dependency on the
  engine's spine to get one `PathBuf` would put `glam`, `half`, `bytemuck`, `serde` and
  `serde_json` into a game's build for a string."* So the namespace would oblige a module in
  another tree to re-implement a rule owned by a crate it may not link, and drift would present as
  a channel that opens nothing with no error anywhere — the module looking in the right directory
  for a file with almost the right name. The namespace is still injected, for `term.rs`'s reason
  and not as an address. ⚠️ An **environment variable rather than an argv argument**, and that is
  the deciding argument: a module keeps its own command line (T1 requires it), an appended argument
  can collide with a parser in another repository, and an environment variable cannot.

  🚨 **A launch that failed is remembered, because the frame loop is the launcher.** `HostSlot` is
  `Live` **or** `Refused`, and the second arm is why it is an enum rather than an `Option`: a
  forgotten failure is retried next frame, which is sixty process spawns a second against a binary
  that is not there. It is cleared by a person — `console module restart`, the fifth action word —
  never by a timer, because a retry on a schedule is the console guessing that something on disk
  changed.

  📌 **`RESTART_VERB` landed where `presence.rs` reserved it, and the objection beside it did not
  survive.** That note also said `restart` *"is not one of `MODULE_ACTIONS`"* — but that table is
  not the approval set; its own doc calls it *"the action words `console module` takes"* and says
  *"a fifth verb is one line here and not four"*. **A verb named in a rectangle that a person
  cannot then type is a sentence that lies.** So it is in the table, and the agreement test now
  states the distinction — four verbs change trust or the disk, and this one changes neither —
  rather than merely growing by one. `revoke` now stops a running module before removing the
  record, since withdrawing trust while frames keep arriving would leave a live picture under a
  rectangle saying *"not approved"*.

  🚨 **The launcher answers the one question the shared mapping cannot.** `presence.rs` wrote the
  limit down: *"hung and exited-without-a-farewell are not distinguishable from inside a shared
  mapping, and the thing that genuinely knows is the process handle, which is T3b's launcher's."*
  This is that launcher and it holds it — `Hosted::exited()` is a non-blocking `try_wait` asked
  **before** the poll, so a crash is named with its exit code the instant the OS knows, rather than
  as a silence that becomes `Lost` five seconds later. ⚠️ `try_wait` *failing* is not death; that
  is the OS declining to answer, and reporting *exited* for it would be inventing a fact.

  ⚠️ **The sRGB view would have been a picture that is merely too dark, and this console already
  knew.** `PixelFormat` has two variants and both are sRGB, so `FrameTexture`'s own view decodes on
  every sample — and egui's shader linearizes its samples itself. `BACKDROP_SAMPLE_FORMAT`'s comment
  has said so for months: *"a decoded-on-sample view would linearize twice and come out dark."*
  `linear_view_format` is new in `organon-module` and is a **function rather than a rule at the call
  site** for the reason the failure is nasty: nothing errors, nothing tears, every counter reads
  clean, and the only report is a person saying the viewport looks murky.

  📌 **The allocation counter is not a statistic — it is the re-registration trigger.** egui binds a
  `TextureId` to one `TextureView`, so a producer's size change makes the old registration point at
  a freed view; `FrameTexture::allocations()` *is* the question *"has the texture been remade"*.
  Which means T0's condition — a fresh staging buffer and destination per frame measuring **2.72 ms
  at 1440p against 1.37 ms reused**, 3.3× of the gap being `memcpy` of *identical* bytes — is the
  same number the console needs anyway, and it is asserted rather than intended: 600 polls and ~600
  size changes through a real channel leave the counter at 1.

  ⚠️ **What is fed is what §5.3 permits and no more**: motion and mouse buttons over the rectangle,
  no keys, no wheel, no absolute position, and no egui interaction registered on the picture. The
  two `ReleaseAll` rules are the whole of the correctness — leaving the rectangle, and losing focus
  — because without them a module holds a button for ever and presents as wedged, which is §4.6's
  fourth row reached from the one direction that is the console's fault.

  🚨 **The click latch is deliberately NOT built**, and §5.3 is the reason rather than the clock:
  *the way out must be decided before the way in is built*, and which key a module is **told** it
  will never receive is James's call. `Lifecycle` therefore never leaves `Attached` — which is
  exactly §5.1's default, and what makes hanging the launcher off the layout safe: the state a
  module arrives in is already the inert one.

  ⚠️ **No picture has been seen, by anyone, anywhere.** The producer half is `organonart/ascent`'s
  and lands separately. All of this is green, tested headlessly, and **ready to try**.
