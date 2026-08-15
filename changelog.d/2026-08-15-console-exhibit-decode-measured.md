### The exhibit decoder is measured now, not read

`CONSOLE_ARCHITECTURE.md` §3 listed one line of the exhibit's ledger as specifically
unverified: *that a decoded picture is the right way up and the right colour*. It said so
honestly — the pair was "matched to `make_surface_texture` by reading, not by looking" — and
the part of it that needs a GPU and a hand still is. But a real part of that claim never
needed either, and this change turns that part from reading into running.

🚨 **The test that was already there could not have caught either failure, and the reason is
worth stating because it is the general shape.** `exhibit_formats.rs` round-tripped a 2×2
image through every offered extension and asserted the decode was RGBA8. **A fixture
symmetric under a flip cannot detect a flip**, and one whose bytes are only ever read as a
*length* cannot detect a channel swap — so the test would have passed just as green with a
decoder that handed back the picture upside down in BGRA. Total coverage of the extension
table was never the missing thing; a fixture that could disagree was.

The fixture is now **64 × 48** — the two numbers differ on purpose, so a transpose changes the
reported dimensions and is caught before a pixel is read — carrying four solid quadrants of
four distinct colours: red top-left, blue top-right, green bottom-left, and an amber with
**alpha 128** bottom-right. That arrangement separates the three ways a decoder can return a
correctly-sized picture that is wrong: a vertical flip swaps red with green, a horizontal flip
swaps red with blue, and a transpose swaps blue with green *and* changes the dimensions. No two
act the same way, so a green run rules out all three rather than ruling out "some flip". The
amber is the only probe that can catch an RGBA buffer that is really RGBX.

⚠️ **The probes sit at the quadrant centres rather than at the picture's corners, and that is
not slack.** JPEG codes 8×8 blocks with chroma subsampled across 16×16, so the pixel hard
against a colour boundary is exactly where ringing and averaged chroma live; the centre of a
solid 32×24 region is twelve pixels clear of the nearest edge in every direction, which is what
lets the tolerance be tight enough to be honest. The lossless formats are held to the true
corners *as well*, exactly — and to the raw `Vec<u8>` itself, whose first four bytes must be the
top-left pixel and whose last four must be the bottom-right, because it is `into_raw`'s bytes
that reach `write_texture` and `get_pixel` could in principle disagree with it about row order.

📌 **JPEG's tolerance is measured, not chosen.** Encoding the fixture at this build's default
JPEG quality and decoding it back moves the colour channels at the probe points by **at most 1**
— red `255 → 254`, blue `255 → 254`, green's blue channel `0 → 1`, amber's green `191 → 190`,
and nothing else moves at all. The bar is set at **2**, one unit above the measurement, purely so
that a codec revision costing one more unit is a review rather than a red build. That is what
lossy compression costs on this fixture, not slack: every fixture colour has a channel at 0 and a
channel at 255, so a swap or a flip moves a probe by roughly 255 — which a bar of 2 is
deliberately far too small to hide. PNG, for its part, is byte-exact everywhere, alpha included.

⚠️ **Whether JPEG *discards* alpha or *composites* over it had to be measured too, because the
answer decides whether the amber quadrant can be colour-tested at all.** It discards:
`[255, 191, 0, 128]` returns as `[255, 190, 0, 255]`, the colour intact and the alpha replaced.
Had it composited over black the colour would have arrived near `[128, 96, 0]` and that quadrant
would have had to drop out of the colour assertion rather than only out of the alpha one. So
alpha is asserted *exactly* even for JPEG, and to **255** rather than to the fixture's 128:
`to_rgba8()` synthesises an opaque alpha because the container has none, which makes 255 a fact
about JPEG and 128 an assertion of a bug.

⚠️ **The decode runs through a temporary file, deliberately.** `console_main::load_exhibit_item`
calls `image::open` on a *path*; encoding to a `Vec` and calling `load_from_memory_with_format`
would test a different function. The rule these tests honour is the one that matters — #56 T4's
*do not commit media fixtures of any size* — and nothing is committed: a guard makes a unique
directory under the system temp dir and its `Drop` removes it, on an unwinding panic too. What
they cannot do is call `load_exhibit_item` itself, which lives in a `[[bin]]` behind
`required-features = ["console-edition"]` and is therefore unimportable from an integration
test; the sequence is reproduced rather than invoked, and what keeps the two honest is that the
sequence is two calls long. The one branch of the real function that would otherwise go
unmeasured is the `thumbnail` downscale above `MAX_EXHIBIT_EDGE`, which every photograph off a
camera takes — **a resampler is a second place a flip can be introduced**, so it has its own
test.

### The gamma pair is pinned, and pinning it is not measuring it

`BACKDROP_FORMAT` is `Rgba8UnormSrgb` storage, `BACKDROP_SAMPLE_FORMAT` the `Rgba8Unorm` view
egui samples, and both `make_surface_texture` and `upload_exhibit` are documented as using "the
pair exactly". That was prose in three places with nothing holding it together. The pin has
three parts, and the third does the work: the two constants hold the documented formats; the
string `wgpu::TextureFormat::` occurs in `console_main.rs` **exactly twice**, on the two `const`
lines, so no texture anywhere in the file can quietly pick its own pair; and the storage/view
spellings are **balanced** — as many `format: BACKDROP_FORMAT` as `format:
Some(BACKDROP_SAMPLE_FORMAT)` as `view_formats: &[BACKDROP_SAMPLE_FORMAT]`, and **zero** of
either crossed spelling. That last count is what catches the actual mistake: a texture created
in the sample format, or viewed in the storage format, which is the double linearization the
comment beside `BACKDROP_FORMAT` warns about and which shows up not as a crash but as a
photograph that looks washed out beside a surface.

🚨 **It is a source-text pin, and it says so.** The constants live in a `[[bin]]`, so no test
can import them; moving them into a library would put `wgpu` types in `organon-console`, and
`doc/arch/topology.md` forbids that crate `wgpu` outright. The choice was a text pin or nothing,
and a pin that states its own reach beats a ledger line that stays open. It also counts rather
than parses, so a texture site added later joins the pin for free — but it depends on rustfmt's
one-space-after-colon, and a doc comment spelling the full `wgpu::TextureFormat::` path would
trip the count, which is why the failure message says so.

### 🚨 What this does not verify

**No exhibit has been on a screen, and this changes nothing about that.** What is measured is
one link: bytes on disk → `image::open` → `to_rgba8()` → the RGBA buffer `ExhibitLoad::Picture`
carries. Everything after that buffer is untouched — `upload_exhibit`'s `write_texture` row
order, egui's `register_native_texture`, the `(0,0)–(1,1)` UV rect at the paint site, and every
GPU sampling behaviour. **If a flip is introduced after `to_rgba8()`, these tests pass and the
picture is still upside down.** And pinning that the two format constants agree is emphatically
not a measurement of gamma: whether sRGB storage sampled through a UNORM view linearizes exactly
once is a property of how a GPU samples, and nothing here runs one. §3's ledger moves only the
decoder out of its unverified list, and the sentence about the screen stays exactly where it is.

📌 Found on the way and worth recording even though the fix is somebody else's:
`changelog.d/2026-08-15-console-red-main-media-reversal.md` opened its second section with `## `,
which `native/tools/changelog.py check` rejects — a fragment may use `### ` and deeper only,
because the release step owns the version heading. That was `main`'s state from the moment that
PR merged, so **the validator was red for every branch cut from it**, and anyone checking their
own fragment had to read past somebody else's failure to find out their own was fine. It was
demoted to `### ` on this branch and, within the same few hours and identically, on
`console/light-text-contrast` — which is the one that reached `main` first, so this branch's copy
of the fix vanished at the rebase. ⚠️ **Two branches independently finding and fixing the same
one-character defect is the cost of a shared validator that fails on somebody else's file**: the
check names the offending fragment, but nothing tells you it is already fixed on a branch you
cannot see.
