### The visual no longer dies when the display stops offering `Rgba16Float` (#237)

`organic-math-visual` panicked twice on the workstation — a *running* instance, not a fresh
launch — in `Surface::configure`: *"Requested format Rgba16Float is not in list of supported
formats: [Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba8UnormSrgb, Rgb10a2Unorm]"*, leaving a
"not responding" ghost window. The format list had been read **once**, at first light, and
fp16 was in it then. On the Vulkan backend (which this box selects by default, as the
`GPU:` line reports) that list is `vkGetPhysicalDeviceSurfaceFormatsKHR` — a live answer about
the display the window is on. NVIDIA advertises fp16 + extended-linear only while that output
is in HDR mode, so a monitor waking in SDR, the Windows HDR toggle, or the window landing on
the other display makes the swapchain `Outdated`, and the reconfigure re-issued a format the
surface no longer had. A validation error in `configure` is routed to the device's
uncaptured-error handler, whose default is a panic. The panic file's list identifies the
backend on its own: DX12's list is a fixed six with fp16 always present, in a different order.

**Every `Surface::configure` in the visual now goes through one function**, and the format is
chosen from the capabilities read *at that call*: `Rgba16Float` when HDR is wanted, offered,
and presentable extended-linear; else the first sRGB format — the SDR path's own choice, so
HDR-off behaviour is byte-identical; else the first offered. `hdr_active` records the
**grant**, and it — not the request — gates the headroom read, the layer tag and the
`HDR output: ON — EDR headroom …` line, so the fallback cannot claim headroom the surface
clamps at 1.0. When fp16 was wanted and not granted, stderr says which refusal it was:
`HDR output: surface offers no Rgba16Float — falling back to Bgra8UnormSrgb; EDR is off
(SDR / ACES). Offered: […]`. The configure runs inside error scopes: a failure is logged once,
the frame loop draws nothing until a configure succeeds, and the process stays alive —
`doc/pbr_text_engine.md` §13 names a GPU app dying on a lock screen as the worst failure the
screensaver case can have, and this was that failure.

📌 **A mid-run loss of fp16 falls back rather than rebuilding anything by hand.** The target
format reaches the frame from `config.format`, and the frame already rebuilds the
composite/FX/temporal pipelines whenever that differs from the last frame's — the same
edge-detect the **H** toggle has always ridden. The next reconfigure re-picks fp16 when it is
offered again.

⚠️ **`Rgb10a2Unorm` is deliberately not the fallback, although it was the obvious one and the
brief asked for it.** `composite.wgsl`'s SDR arm writes linear [0,1] and relies on the surface
being an sRGB format so the hardware applies the OETF; there is no `Rgb10a2UnormSrgb`, so a
10-bit swapchain would display the linear picture as if it were already encoded — crushed and
wrong, not slightly banded. Reaching 10-bit properly needs an sRGB encode inside the composite
for non-sRGB targets, a change to the shared shader that is out of scope here. The decision is
pure (`surface_format::pick_surface_format`) and pinned by tests, including the panic's exact
list and a mutation guard that puts 10-bit first in the offered list.

`doc/arch/render.md`'s HDR seam section carries the same account beside the platform table.
