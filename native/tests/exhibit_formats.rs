//! The Console's exhibit decoder, pinned to the extension table that advertises it.
//!
//! 🚨 **The one failure in this feature that no unit test can see.**
//! `organon_core::exhibit::IMAGE_EXTENSIONS` is pure data in a crate that has no decoder, so
//! its own tests can prove `"jpg"` resolves to `Kind::Image` and nothing more. Whether this
//! build can actually *decode* a JPEG is decided a whole crate away, by the feature list on
//! `image` in `native/Cargo.toml` — which is built `default-features = false`, so a missing
//! feature is not a compile error anywhere. The two tables agreeing is what stops `/media
//! photo.jpg` being accepted by the composer, dispatched, read off the disk, and *then*
//! failing in the decoder with a message about an unsupported format.
//!
//! ⚠️ **In memory, with no fixture on disk.** #56 T4's own bar: *do not commit media fixtures
//! of any size* — "a repository that gains sample MP3s never loses them". Every image here is
//! synthesised, encoded and decoded in RAM, so the test proves the codec is present without
//! the tree gaining a single byte of sample media. The orientation tests below write one
//! *temporary* file per format so that `image::open` — the exact call the console makes — is
//! the thing under test; those files live under the system temp directory and are removed by a
//! guard's `Drop`, so the rule they honour is the one that matters: **nothing is committed.**
//!
//! # 🚨 What these tests measure, and what they leave open
//!
//! `CONSOLE_ARCHITECTURE.md` §3 records that no exhibit has ever been on a screen. That is
//! still true, and nothing here changes it. What these tests close is the *first* link of that
//! chain, which was previously asserted by reading the code rather than by running it:
//!
//! **Measured here** — bytes on disk → `image::open(path)` → `.to_rgba8()` → the RGBA buffer
//! that `ExhibitLoad::Picture` carries. For every extension in `IMAGE_EXTENSIONS`, that
//! sequence preserves *orientation* (row 0 is the picture's top row, column 0 its left column)
//! and *channel order* (the bytes come out R, G, B, A in that order). Also measured: the two
//! `console_main.rs` constants that decide the exhibit's gamma arrangement hold the pair they
//! are documented to hold, and no site in that file restates a texture format literal.
//!
//! **Not measured, and not claimed** — everything after `to_rgba8()`. `upload_exhibit`'s
//! `write_texture` row order, egui's `register_native_texture`, the `(0,0)–(1,1)` UV rect at
//! the paint site, and every GPU sampling behaviour are all downstream of the buffer these
//! tests inspect. If a flip is introduced *after* the decode, these tests pass and the picture
//! is still upside down. In particular, pinning that the sRGB-storage/UNORM-sample constants
//! agree is **not** a measurement of gamma: whether that arrangement linearizes exactly once
//! is a property of how a GPU samples the view, and a hand in front of a window is still the
//! only instrument for it.

use image::{DynamicImage, ImageFormat, RgbaImage};
use organon_core::exhibit::{Exhibit, IMAGE_EXTENSIONS, MARKDOWN_EXTENSIONS};
use organon_core::kind::Kind;

/// The `image` format an extension is decoded as, or `None` if this test does not know the
/// extension — which is itself a failure, since the point is total coverage of the table.
fn format_for(ext: &str) -> Option<ImageFormat> {
    match ext {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        _ => None,
    }
}

/// A tiny picture with four distinguishable pixels — big enough that a decode returning the
/// wrong dimensions is visible, small enough to be free.
fn synthesise() -> DynamicImage {
    let mut img = RgbaImage::new(2, 2);
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    img.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
    img.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
    img.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
    DynamicImage::ImageRgba8(img)
}

/// 🚨 CONTRACT: **every extension the console offers can actually be decoded by this build.**
///
/// A failure here means one of two edits happened without the other: an extension was added to
/// `organon_core::exhibit::IMAGE_EXTENSIONS`, or a feature was dropped from `image` in
/// `native/Cargo.toml`. Both produce the same symptom for a person — a file the console said
/// it would show, failing after it was accepted — and neither shows up in a unit test.
#[test]
fn every_offered_image_extension_has_a_decoder_in_this_build() {
    let source = synthesise();
    for ext in IMAGE_EXTENSIONS {
        let format = format_for(ext).unwrap_or_else(|| {
            panic!(
                "`{ext}` is offered by `exhibit::IMAGE_EXTENSIONS` and this test does not know \
                 which `image::ImageFormat` decodes it -- add the arm in the same change that \
                 added the extension, and the cargo feature with it"
            )
        });

        let mut encoded = std::io::Cursor::new(Vec::new());
        source.write_to(&mut encoded, format).unwrap_or_else(|err| {
            panic!("`{ext}` ({format:?}) cannot be ENCODED by this build: {err}")
        });

        let decoded = image::load_from_memory_with_format(encoded.get_ref(), format)
            .unwrap_or_else(|err| {
                panic!(
                    "`{ext}` ({format:?}) cannot be DECODED by this build: {err}. The extension \
                     is offered by `exhibit::IMAGE_EXTENSIONS`; add the matching feature to \
                     `image` in native/Cargo.toml"
                )
            });

        assert_eq!(
            (decoded.width(), decoded.height()),
            (2, 2),
            "`{ext}` decoded to the wrong size"
        );
        // The loader's own last step, exercised here because a format that decodes to an
        // unexpected colour type would fail at `to_rgba8` rather than at the decode.
        assert_eq!(decoded.to_rgba8().into_raw().len(), 2 * 2 * 4, "`{ext}` is not RGBA8");
    }
}

/// The other half of the pair: the *classifier* and this test's format table agree about which
/// extensions are pictures at all. Without this, an extension could be moved from the image
/// table to the markdown one and the test above would still pass — over an empty loop.
#[test]
fn the_extension_tables_and_this_tests_format_map_are_one_set() {
    assert!(!IMAGE_EXTENSIONS.is_empty(), "an empty table would make the decoder test vacuous");
    for ext in IMAGE_EXTENSIONS {
        assert_eq!(
            Exhibit::kind_for_extension(ext),
            Some(Kind::Image),
            "`{ext}` is in IMAGE_EXTENSIONS but does not classify as an image"
        );
    }
    // A markdown extension must NOT have a decoder arm here — that would mean the two kinds
    // had started to overlap, and a `.md` reaching `image::open` is a failure the person would
    // read as a corrupt document.
    for ext in MARKDOWN_EXTENSIONS {
        assert_eq!(format_for(ext), None, "`{ext}` is a document, not a picture");
        assert_eq!(Exhibit::kind_for_extension(ext), Some(Kind::Markdown));
    }
}

// ---------------------------------------------------------------------------------------
// Orientation and channel order
// ---------------------------------------------------------------------------------------

/// The fixture is **64 × 48**, and the two numbers differ on purpose: a transpose changes the
/// dimensions, so it is caught by the size assertion before a single pixel is read. A square
/// fixture could not tell a transpose from the truth.
const FIXTURE_W: u32 = 64;
/// See [`FIXTURE_W`] — deliberately not equal to it.
const FIXTURE_H: u32 = 48;

/// Top-left quadrant. Pure red: `G` and `B` are both zero, so a red that arrives in the blue
/// byte is a whole channel out of place rather than a shade.
const RED: [u8; 4] = [255, 0, 0, 255];
/// Top-right quadrant. Pure blue — the mirror of [`RED`], which is what makes a BGR/RGB swap
/// visible as *two* probes changing rather than one ambiguous one.
const BLUE: [u8; 4] = [0, 0, 255, 255];
/// Bottom-left quadrant. Pure green: unchanged by a red/blue swap, so it is the control that
/// says the picture arrived at all when the other two have moved.
const GREEN: [u8; 4] = [0, 255, 0, 255];
/// Bottom-right quadrant. Three unequal non-zero channels **and an alpha that is not 255** —
/// the only probe that can catch an RGBA buffer that is really RGBX, or an alpha channel that
/// has been swapped into a colour byte.
const AMBER: [u8; 4] = [255, 191, 0, 128];

/// Where each solid region is sampled, what must be there, and the corner's human name.
///
/// # Why four quadrants, and why these four
///
/// The three ways a decoder can hand back a *correctly sized* picture that is wrong are a
/// vertical flip, a horizontal flip, and a transpose. Four distinct colours, one per quadrant,
/// separate all three, and this is the whole argument:
///
/// - a **vertical** flip swaps top-left with bottom-left (red ↔ green) and top-right with
///   bottom-right (blue ↔ amber);
/// - a **horizontal** flip swaps top-left with top-right (red ↔ blue) and bottom-left with
///   bottom-right (green ↔ amber);
/// - a **transpose** swaps top-right with bottom-left (blue ↔ green) — and, because
///   [`FIXTURE_W`] ≠ [`FIXTURE_H`], also changes the reported dimensions.
///
/// No two of those act the same way on this table, so a passing run rules out all three rather
/// than ruling out "some flip".
///
/// ⚠️ **Sampled at the quadrant CENTRE, not at the picture's corner, and that is not slack.**
/// JPEG encodes 8×8 blocks with chroma subsampled across 16×16, so the pixel hard against a
/// colour boundary is exactly where ringing and averaged chroma live. The centre of a solid
/// 32×24 region is 12 pixels clear of the nearest edge in every direction, which is what makes
/// a tight tolerance honest instead of generous. The lossless formats are additionally checked
/// at the four true corners, where they must be exact — see
/// [`the_true_corners_are_exact_in_a_lossless_format`].
const PROBES: [(&str, (u32, u32), [u8; 4]); 4] = [
    ("top-left", (FIXTURE_W / 4, FIXTURE_H / 4), RED),
    ("top-right", (FIXTURE_W * 3 / 4, FIXTURE_H / 4), BLUE),
    ("bottom-left", (FIXTURE_W / 4, FIXTURE_H * 3 / 4), GREEN),
    ("bottom-right", (FIXTURE_W * 3 / 4, FIXTURE_H * 3 / 4), AMBER),
];

/// The asymmetric fixture the orientation and channel-order tests are written against.
fn asymmetric_fixture() -> DynamicImage {
    let mut img = RgbaImage::new(FIXTURE_W, FIXTURE_H);
    for y in 0..FIXTURE_H {
        for x in 0..FIXTURE_W {
            let colour = match (x < FIXTURE_W / 2, y < FIXTURE_H / 2) {
                (true, true) => RED,
                (false, true) => BLUE,
                (true, false) => GREEN,
                (false, false) => AMBER,
            };
            img.put_pixel(x, y, image::Rgba(colour));
        }
    }
    DynamicImage::ImageRgba8(img)
}

/// Whether a format's bytes survive a round trip untouched.
///
/// This is a property of the *container*, not of a quality setting, so it belongs in a table
/// rather than in a tolerance someone tunes until the test goes green.
fn is_lossless(ext: &str) -> bool {
    match ext {
        "png" => true,
        "jpg" | "jpeg" => false,
        _ => panic!("`{ext}` has no stated fidelity -- add it in the change that adds the format"),
    }
}

/// Whether a format carries an alpha channel at all.
///
/// ⚠️ **JPEG does not, and that is why the alpha assertion is per-format rather than
/// universal.** `to_rgba8()` on a decoded JPEG returns 255 everywhere because the decoder is
/// synthesising an opaque alpha, not because the fixture's 128 survived. Asserting 255 there is
/// asserting a *fact about JPEG*; asserting 128 there would be asserting a bug.
///
/// 📌 Measured while choosing this, because the answer decides whether the colour assertion can
/// run at all on the alpha-bearing quadrant: the encoder **discards** the alpha rather than
/// compositing over it. [`AMBER`] `[255, 191, 0, 128]` comes back `[255, 190, 0, 255]` — the RGB
/// intact within [`JPEG_CHANNEL_TOLERANCE`], the alpha replaced. Had it composited over black
/// the colour would have arrived near `[128, 96, 0]`, and that quadrant would have had to be
/// excluded from the colour test rather than merely from the alpha one.
fn carries_alpha(ext: &str) -> bool {
    match ext {
        "png" => true,
        "jpg" | "jpeg" => false,
        _ => panic!("`{ext}` has no stated alpha behaviour -- add it with the format"),
    }
}

/// How far a colour channel may drift through a lossy round trip, per channel, 0–255.
///
/// ⚠️ **This is what lossy compression costs on this fixture, not slack.** Measured rather than
/// chosen: encoding [`asymmetric_fixture`] with this build's `image` at its default JPEG quality
/// and decoding it back moves the probe points by **at most 1** — red `255 → 254`, blue
/// `255 → 254`, green's blue channel `0 → 1`, amber's green `191 → 190`, and nothing else moves
/// at all. The bar is set at 2 rather than at the measured 1 purely so a codec revision that
/// costs one more unit is a *review*, not a red build.
///
/// 🚨 **It is deliberately far too small to hide the failures this file exists to catch.** A
/// channel swap or a flip moves a probe by roughly 255, because every fixture colour has at
/// least one channel at 0 and one at 255. If this constant ever needs to be raised past single
/// digits, the thing to do is find out why — not to raise it.
const JPEG_CHANNEL_TOLERANCE: i32 = 2;

/// A directory under the system temp dir, unique to this process, removed when the guard drops.
///
/// 📌 Present so that `image::open` — which takes a *path*, and is the exact call
/// `console_main::load_exhibit_item` makes for `Kind::Image` — is what these tests exercise.
/// Encoding to a `Vec` and calling `load_from_memory_with_format` would test a different
/// function and would let the console's own decoder route go unmeasured. `Drop` runs on an
/// unwinding panic too, so a failing assertion still cleans up.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is before the epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "organon-exhibit-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|err| panic!("cannot create the scratch dir {}: {err}", dir.display()));
        Self(dir)
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Write the fixture out as `ext` and read it back the way the console does: `image::open` on a
/// path, then `to_rgba8()`.
///
/// ⚠️ **This is the console's two calls, not the console's function.** `load_exhibit_item`
/// lives in `src/console_main.rs`, a `[[bin]]` behind `required-features = ["console-edition"]`,
/// and no integration test can import a binary. So the sequence is reproduced here rather than
/// invoked, and the thing that keeps the two honest is that the sequence is two lines long. The
/// one step of the real function this deliberately skips is the `thumbnail` downscale, which
/// only fires above `MAX_EXHIBIT_EDGE` (2048); `the_downscale_a_large_picture_takes_does_not_
/// flip_it` covers that branch separately.
fn decode_the_way_the_console_does(ext: &str, scratch: &Scratch) -> RgbaImage {
    let format = format_for(ext)
        .unwrap_or_else(|| panic!("`{ext}` has no format arm -- see the decoder test above"));
    let mut encoded = std::io::Cursor::new(Vec::new());
    asymmetric_fixture()
        .write_to(&mut encoded, format)
        .unwrap_or_else(|err| panic!("`{ext}` ({format:?}) cannot be encoded: {err}"));

    let path = scratch.path(&format!("fixture.{ext}"));
    std::fs::write(&path, encoded.get_ref())
        .unwrap_or_else(|err| panic!("cannot write {}: {err}", path.display()));

    image::open(&path)
        .unwrap_or_else(|err| panic!("`{ext}` cannot be opened from a path: {err}"))
        .to_rgba8()
}

/// Assert one probe, spelling the comparison the way the format's fidelity allows.
fn assert_probe(ext: &str, got: [u8; 4], want: [u8; 4], corner: &str, at: (u32, u32)) {
    let expected_alpha = if carries_alpha(ext) { want[3] } else { 255 };
    let want = [want[0], want[1], want[2], expected_alpha];

    if is_lossless(ext) {
        assert_eq!(
            got, want,
            "`{ext}`: the {corner} probe at {at:?} is {got:?}, not {want:?}. Equal-but-permuted \
             bytes mean a channel-order swap; a colour belonging to a DIFFERENT corner means the \
             picture is flipped -- see PROBES for which flip each swap names"
        );
        return;
    }

    // Lossy: the three colour channels are allowed to drift by a measured amount and the alpha
    // is not. Alpha is exact even here because it is synthesised by the decoder rather than
    // carried through the compressor, so a value other than 255 is a real defect, not noise.
    assert_eq!(
        got[3], want[3],
        "`{ext}`: the {corner} probe at {at:?} has alpha {}, not {} -- alpha is not compressed \
         on this path, so this is not lossy drift",
        got[3], want[3]
    );
    for (channel, name) in [(0usize, "red"), (1, "green"), (2, "blue")] {
        let drift = got[channel] as i32 - want[channel] as i32;
        assert!(
            drift.abs() <= JPEG_CHANNEL_TOLERANCE,
            "`{ext}`: the {corner} probe at {at:?} is {got:?}, not {want:?} -- the {name} channel \
             is off by {drift}, past the measured tolerance of {JPEG_CHANNEL_TOLERANCE}. A drift \
             of a few units is what lossy compression costs; a drift of ~255 is a channel swap \
             or a flip, and the tolerance is deliberately far too small to hide one"
        );
    }
}

/// 🚨 CONTRACT: **a decoded picture is the right way up and the right colour, as far as the
/// decoder.**
///
/// `CONSOLE_ARCHITECTURE.md` §3 listed this as read-but-not-measured. The previous test in this
/// file could not have caught either failure: a 2×2 fixture symmetric under a flip cannot
/// detect a flip, and one whose bytes are read only as a *length* cannot detect a swap.
///
/// Read [`PROBES`] for the argument that these four points separate a vertical flip, a
/// horizontal flip and a transpose. Read the module header for what this does **not** cover —
/// everything after `to_rgba8()`, which is all of the GPU.
#[test]
fn a_decoded_picture_keeps_its_orientation_and_its_channel_order() {
    let scratch = Scratch::new("orientation");
    for ext in IMAGE_EXTENSIONS {
        let decoded = decode_the_way_the_console_does(ext, &scratch);

        assert_eq!(
            (decoded.width(), decoded.height()),
            (FIXTURE_W, FIXTURE_H),
            "`{ext}` decoded to {}x{} rather than {FIXTURE_W}x{FIXTURE_H} -- transposed \
             dimensions are the loudest form of the flip this test exists to catch",
            decoded.width(),
            decoded.height()
        );

        for (corner, at, want) in PROBES {
            assert_probe(ext, decoded.get_pixel(at.0, at.1).0, want, corner, at);
        }
    }
}

/// The corners themselves, for the formats that can be held to them.
///
/// 📌 Separate from the test above because the claim is different. That one says *the picture
/// is the right way round*, at points chosen so a lossy codec can meet the bar honestly. This
/// one says *the very first and very last bytes of the buffer are the picture's own corners* —
/// which pins the row order of the raw `Vec<u8>` that `ExhibitLoad::Picture` hands to
/// `upload_exhibit`, and is the strongest statement available without a GPU. It can only be
/// made where the codec is lossless, so it skips JPEG rather than weakening itself to include
/// it.
#[test]
fn the_true_corners_are_exact_in_a_lossless_format() {
    let scratch = Scratch::new("corners");
    let corners = [
        ("top-left", (0, 0), RED),
        ("top-right", (FIXTURE_W - 1, 0), BLUE),
        ("bottom-left", (0, FIXTURE_H - 1), GREEN),
        ("bottom-right", (FIXTURE_W - 1, FIXTURE_H - 1), AMBER),
    ];
    let mut checked = 0usize;
    for ext in IMAGE_EXTENSIONS {
        if !is_lossless(ext) {
            continue;
        }
        let decoded = decode_the_way_the_console_does(ext, &scratch);
        let raw = decoded.clone().into_raw();

        for (corner, at, want) in corners {
            assert_eq!(
                decoded.get_pixel(at.0, at.1).0,
                want,
                "`{ext}`: the {corner} corner at {at:?} is wrong"
            );
        }

        // The buffer itself, not the accessor: `get_pixel` and `into_raw` could in principle
        // disagree about row order, and it is `into_raw` whose bytes reach `write_texture`.
        assert_eq!(
            &raw[0..4],
            &RED[..],
            "`{ext}`: byte 0 of the raw buffer is not the top-left pixel"
        );
        assert_eq!(
            &raw[raw.len() - 4..],
            &AMBER[..],
            "`{ext}`: the last four bytes of the raw buffer are not the bottom-right pixel"
        );
        checked += 1;
    }
    assert!(checked > 0, "no lossless format in IMAGE_EXTENSIONS -- this test became vacuous");
}

/// The one step of `load_exhibit_item` the tests above skip: the `thumbnail` downscale a
/// picture wider or taller than `MAX_EXHIBIT_EDGE` (2048) is put through.
///
/// ⚠️ **A resampler is a second place a flip can be introduced**, and it is on the path that
/// every photograph off a camera takes — a 4000-pixel-wide picture is the common case, not the
/// edge one. Run at a small scale here because the property is about the transform, not the
/// size: the same call, the same aspect-preserving box filter.
#[test]
fn the_downscale_a_large_picture_takes_does_not_flip_it() {
    let scratch = Scratch::new("thumbnail");
    for ext in IMAGE_EXTENSIONS {
        if !is_lossless(ext) {
            continue;
        }
        let decoded = DynamicImage::ImageRgba8(decode_the_way_the_console_does(ext, &scratch));
        let small = decoded.thumbnail(FIXTURE_W / 2, FIXTURE_H / 2).to_rgba8();

        assert_eq!(
            (small.width(), small.height()),
            (FIXTURE_W / 2, FIXTURE_H / 2),
            "`{ext}`: the thumbnail is the wrong size"
        );
        for (corner, _, want) in PROBES {
            let at = match corner {
                "top-left" => (small.width() / 4, small.height() / 4),
                "top-right" => (small.width() * 3 / 4, small.height() / 4),
                "bottom-left" => (small.width() / 4, small.height() * 3 / 4),
                _ => (small.width() * 3 / 4, small.height() * 3 / 4),
            };
            assert_eq!(
                small.get_pixel(at.0, at.1).0,
                want,
                "`{ext}`: after the downscale the {corner} probe at {at:?} is wrong -- a colour \
                 belonging to another corner means the resampler flipped the picture"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------
// The gamma arrangement, pinned against drift
// ---------------------------------------------------------------------------------------

/// The console binary's source, read as text.
///
/// 🚨 **As TEXT, and the reason is a constraint rather than laziness.** The two constants live
/// in `src/console_main.rs`, which is a `[[bin]]` behind `required-features =
/// ["console-edition"]`; no integration test can import a binary target. The obvious
/// alternative — move them into a library — would put `wgpu` types in `organon-console`, and
/// `doc/arch/topology.md` forbids that crate `wgpu` outright. So the choice was a source-text
/// pin or nothing, and a pin that states its own reach beats a ledger line that stays open.
fn console_main_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/console_main.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "cannot read {} -- if the console binary moved, this pin moves with it: {err}",
            path.display()
        )
    })
}

/// The format name a `const` declares, e.g. `Rgba8UnormSrgb`.
fn declared_format(src: &str, konst: &str) -> String {
    let needle = format!("const {konst}:");
    let line = src
        .lines()
        .find(|l| l.trim_start().starts_with(&needle))
        .unwrap_or_else(|| panic!("`{konst}` is not declared in console_main.rs any more"));
    assert!(
        line.contains("TextureFormat::"),
        "`{konst}` no longer names a `TextureFormat` on its own line: {line}"
    );
    // `rsplit` cannot return `None`, so the `contains` above is what makes this honest: without
    // it a line with no separator yields the whole line and the take_while returns nonsense.
    let tail = line.rsplit("TextureFormat::").next().unwrap_or_default();
    tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
}

/// 🚨 CONTRACT: **an exhibit and the engine's own render share one gamma arrangement, and the
/// three places that say so cannot drift apart.**
///
/// `BACKDROP_FORMAT` is the sRGB *storage* format and `BACKDROP_SAMPLE_FORMAT` the non-sRGB
/// *view* egui samples; `make_surface_texture` and `upload_exhibit` are both documented as
/// using "the pair exactly". Until now that was prose in three places with nothing holding it
/// together — and getting it wrong is not a crash, it is a photograph that looks washed out
/// beside a surface, which only a hand can see.
///
/// The pin has three parts, and the third is the one that does the work:
///
/// 1. the two constants hold the documented formats;
/// 2. **no site in the file names a `TextureFormat::Rgba8*` literal** — every texture goes
///    through the constants, so a new one cannot quietly pick its own pair;
/// 3. the storage/view spellings are **balanced**: as many `format: BACKDROP_FORMAT` as
///    `format: Some(BACKDROP_SAMPLE_FORMAT)` as `view_formats: &[BACKDROP_SAMPLE_FORMAT]`, and
///    zero of either crossed spelling. That is what catches the actual mistake — a texture
///    created in the *sample* format, or viewed in the *storage* format, which is the double
///    linearization the comment at `BACKDROP_FORMAT` warns about.
///
/// 🚨 **This measures agreement between constants, NEVER gamma.** Whether sRGB storage sampled
/// through a UNORM view linearizes exactly once is a GPU behaviour; nothing here runs a GPU.
/// Read the module header before ticking anything off a ledger with this.
#[test]
fn the_exhibit_and_the_backdrop_cannot_drift_out_of_one_gamma_arrangement() {
    let src = console_main_source();

    assert_eq!(
        declared_format(&src, "BACKDROP_FORMAT"),
        "Rgba8UnormSrgb",
        "the backdrop's STORAGE format changed. Every comparison in this file, and the two \
         `make_surface_texture` pair comments, describe an sRGB storage format"
    );
    assert_eq!(
        declared_format(&src, "BACKDROP_SAMPLE_FORMAT"),
        "Rgba8Unorm",
        "the backdrop's SAMPLE format changed. It must be the non-sRGB view of the same bytes: \
         egui's shader linearizes its samples itself, so a decoded-on-sample view linearizes \
         twice and the picture comes out dark"
    );

    // Part 2. Both occurrences must be the two `const` lines themselves.
    let literals = src.matches("wgpu::TextureFormat::").count();
    assert_eq!(
        literals, 2,
        "console_main.rs names `wgpu::TextureFormat::` {literals} times; it must be exactly the \
         two `const` declarations, so that every texture in the file goes through them. If you \
         added a DOC COMMENT spelling the full path, that is what tripped this -- write the \
         format in backticks without the `wgpu::TextureFormat::` prefix, as the existing \
         comments do. If you added a texture with its own format, give it a named constant"
    );

    // Part 3. The pairing, counted rather than parsed, so a new texture site joins the pin for
    // free. Depends on rustfmt's one-space-after-colon, which is the house spelling everywhere
    // in this file.
    let storage = src.matches("format: BACKDROP_FORMAT").count();
    let view = src.matches("format: Some(BACKDROP_SAMPLE_FORMAT)").count();
    let declared = src.matches("view_formats: &[BACKDROP_SAMPLE_FORMAT]").count();
    assert!(storage > 0, "no texture in console_main.rs is created in BACKDROP_FORMAT any more");
    assert_eq!(
        view, storage,
        "the sRGB-storage/UNORM-view pair is unbalanced: {storage} textures created in \
         BACKDROP_FORMAT but {view} views in BACKDROP_SAMPLE_FORMAT. The two counts must match, \
         because the pair is the arrangement -- a texture without its non-sRGB view is sampled \
         through an sRGB one and linearizes twice, which is the washed-out picture the comment \
         beside `BACKDROP_FORMAT` warns about, and a view without its texture means some site \
         picked a storage format this pin cannot see"
    );
    assert_eq!(
        declared, storage,
        "{storage} textures are created in BACKDROP_FORMAT but only {declared} declare \
         `view_formats: &[BACKDROP_SAMPLE_FORMAT]`. wgpu rejects a view whose format the texture \
         did not declare, so this one fails at run time rather than looking wrong"
    );

    assert_eq!(
        src.matches("format: BACKDROP_SAMPLE_FORMAT").count(),
        0,
        "a texture is being CREATED in the sample format. Storage is sRGB; the non-sRGB spelling \
         belongs to the view alone"
    );
    assert_eq!(
        src.matches("format: Some(BACKDROP_FORMAT)").count(),
        0,
        "a view is being created in the sRGB STORAGE format -- that is the decoded-on-sample \
         view the `BACKDROP_FORMAT` comment warns about, and it linearizes twice"
    );
}
