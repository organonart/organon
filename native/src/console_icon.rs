//! Organon Console's window icon — the aperture mark, embedded as pixels.
//!
//! The Console came up with the OS default in its title bar, in Alt-Tab, and on its
//! taskbar button. This module is the whole of the fix: two PNGs compiled into the
//! binary, decoded once at window creation, handed to winit.
//!
//! # Why committed PNGs and not a build-time rasteriser
//!
//! The artwork is `assets/chrome/aperture-mark-on-dark.svg` and that file is the
//! source. winit wants raw RGBA, so something has to turn the vector into pixels, and
//! there were two honest places to do it:
//!
//! - **At build time**, with `resvg` as a `[build-dependencies]` entry. Single source,
//!   no possible drift — but the root crate has **no build script at all** today, and
//!   adding one there is not a local change: this crate builds the plugin cdylib, the
//!   standalone, the visual, the CLI and three editions, and every one of them would
//!   grow a build script (and ~20 build-dependency crates) to give one window an icon.
//! - **Ahead of time**, committing the rasters. Zero new dependencies — `image` is
//!   already a dependency of this crate for the overlay's formula PNGs — and zero build
//!   cost. The price is that the PNGs can drift from the SVG.
//!
//! The second, with the drift paid for explicitly: the SVG is committed **beside** the
//! rasters and `assets/chrome/README.md` carries the exact command that regenerates
//! them. Drawing the circles and lines in code was the third option and is the one to
//! avoid — it looks like less machinery and is actually a transcription of a design
//! asset that silently stops matching the day the asset changes.
//!
//! # Why these two sizes
//!
//! winit takes **one** bitmap per icon slot; it does not accept a set and pick. So the
//! choice is which single raster scales best into the slots Windows will ask for.
//!
//! - [`small`] is **48×48**. It is what Windows squeezes into the title bar and
//!   Alt-Tab, whose physical size is `SM_CXSMICON` — 16 px at 100 % scaling, 36 px at
//!   the 225 % this machine runs. 48 divides exactly by 3 into 16 and by 2 into 24, and
//!   *down*samples into 36, so every common slot is a reduction rather than a blur.
//! - [`big`] is **256×256**, for the taskbar and the large Alt-Tab tile.
//!
//! ⚠️ **The mark does not survive 16×16, and that is a property of the artwork.** At
//! that size the outer ring's 3 px stroke is 0.4 px and the inner ring's 1.4 px stroke
//! is 0.19 px; a 16 px render is a dark square with a grey smudge in it, with the ticks
//! and the centre dot gone entirely. 32×32 is the smallest size where it still reads.
//! Nothing here can fix that — it needs a hinted small-size variant of the drawing, and
//! that is James's call, not a code change. `assets/chrome/README.md` records the
//! measurement so nobody re-derives it.
//!
//! ⚠️ **The artwork is opaque, not transparent** — a `#0e0d0b` background rect, by
//! design ("on-dark"). Against a dark taskbar it reads as the mark; against a light one
//! it reads as a near-black tile with a gold mark inside it. Deliberate, and left alone.

use winit::window::{Icon, WindowAttributes};

/// Title bar / Alt-Tab. See the module doc for why 48.
const SMALL_PNG: &[u8] = include_bytes!("../assets/chrome/aperture-48.png");

/// Taskbar / large Alt-Tab tile.
const BIG_PNG: &[u8] = include_bytes!("../assets/chrome/aperture-256.png");

/// Decode one embedded PNG into a winit icon.
///
/// Returns `None` rather than panicking: an icon is decoration, and a window that opens
/// with the OS default is strictly better than one that does not open. The bytes are
/// `include_bytes!`d from the repo, so a failure here means the committed asset is
/// broken — which is exactly what [`embedded_icons_decode`] catches at test time.
fn decode(png: &[u8]) -> Option<Icon> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// The 48×48 mark, for the window's own icon.
pub fn small() -> Option<Icon> {
    decode(SMALL_PNG)
}

/// The 256×256 mark, for the taskbar button.
pub fn big() -> Option<Icon> {
    decode(BIG_PNG)
}

/// Hang both icons on a window's attributes.
///
/// One call site, because the two slots are set by **unrelated** APIs and only one of
/// them is portable:
///
/// - `with_window_icon` is winit's cross-platform call. On Windows it reaches
///   `ICON_SMALL` only (`platform_impl/windows/window.rs`), i.e. title bar and Alt-Tab.
///   On X11/Wayland it is the whole story. On macOS it does nothing — the icon there
///   comes from the `.app` bundle, and the Console has no bundle.
/// - `ICON_BIG`, the taskbar button, is reachable **only** through
///   `WindowAttributesExtWindows::with_taskbar_icon`, which exists on Windows alone.
///
/// So "the Console shows the OS default" was two separate defaults, and setting just the
/// portable one would have left the taskbar button — the most visible of the three —
/// untouched.
pub fn apply(attrs: WindowAttributes) -> WindowAttributes {
    let attrs = attrs.with_window_icon(small());
    #[cfg(windows)]
    let attrs = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs.with_taskbar_icon(big())
    };
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both committed rasters decode, are square, and are the sizes the module doc
    /// claims. This is the guard that makes "commit the pixels" safe: a truncated,
    /// re-exported or accidentally-resized asset fails here instead of silently
    /// shipping a window with no icon (`decode` swallows the error by design).
    #[test]
    fn embedded_icons_decode() {
        for (png, expect) in [(SMALL_PNG, 48u32), (BIG_PNG, 256u32)] {
            let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .expect("committed icon PNG decodes")
                .to_rgba8();
            assert_eq!(
                img.dimensions(),
                (expect, expect),
                "icon raster is not {expect}x{expect}"
            );
            // winit requires `rgba.len() == w * h * 4`; assert it here so a future
            // format change (a palette PNG, say) is caught as a size mismatch rather
            // than as `Icon::from_rgba` quietly returning `Err`.
            assert_eq!(img.as_raw().len(), (expect * expect * 4) as usize);
        }
    }

    /// The mark is opaque on purpose — an `#0e0d0b` background rect rather than
    /// transparency. Pinned because "make the icon transparent" is a plausible-looking
    /// edit to the SVG that would change how it reads on a light taskbar, and the
    /// decision to keep it opaque is James's.
    #[test]
    fn mark_is_opaque_by_design() {
        let img = image::load_from_memory_with_format(SMALL_PNG, image::ImageFormat::Png)
            .expect("committed icon PNG decodes")
            .to_rgba8();
        assert!(
            img.pixels().all(|p| p.0[3] == 255),
            "the aperture mark is an on-dark tile; every pixel should be opaque"
        );
    }
}
