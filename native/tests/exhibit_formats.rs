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
//! the tree gaining a single byte of sample media.

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
