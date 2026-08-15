# chrome — the aperture mark

Organon Console's window icon. Unlike everything else under `assets/`, these files are
**not** copied to a store directory at deploy time — they are `include_bytes!`d straight
into the binary by `native/src/console_icon.rs`.

| File | What it is |
|---|---|
| `aperture-mark-on-dark.svg` | **the source.** Two concentric rings and a centre dot in warm gold (`#d9c7a0`, inner ring `#8a7a52`) on near-black `#0e0d0b`, with ticks at N/E/S/W. |
| `aperture-48.png` | the window icon — title bar, Alt-Tab. Embedded. |
| `aperture-256.png` | the taskbar icon (Windows `ICON_BIG`). Embedded. |
| `aperture-32.png` | the smallest size the mark still reads at. **Not embedded** — kept as the small-slot source if a `.ico` is ever added (see below). |

## Regenerating the rasters

The PNGs are committed rather than rasterised at build time, so that the root crate —
which builds the plugin cdylib, the standalone, the visual, the CLI and three
editions — does not grow a build script and ~20 build-dependency crates in order to give
one window an icon. `console_icon.rs`'s module doc has the full reasoning.

The cost of that choice is that **the PNGs can drift from the SVG**. If you edit the
SVG, regenerate them in the same commit. There is no SVG rasteriser on the Organon
workstation's command line (no ImageMagick, no Inkscape; `python` is the Store stub), so
do it in Rust — a throwaway crate outside the repo, never a dependency inside it:

```bash
cargo new /tmp/svgraster --bin
cd /tmp/svgraster
cargo add resvg@0.45 --no-default-features
```

```rust
// src/main.rs — usage: svgraster <in.svg> <out-dir> <stem> <size>...
use resvg::{tiny_skia, usvg};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let tree = usvg::Tree::from_data(
        &std::fs::read(&a[0]).unwrap(),
        &usvg::Options::default(),
    ).unwrap();
    let src = tree.size();
    for size in a[3..].iter().map(|s| s.parse::<u32>().unwrap()) {
        let mut pm = tiny_skia::Pixmap::new(size, size).unwrap();
        let t = tiny_skia::Transform::from_scale(
            size as f32 / src.width(),
            size as f32 / src.height(),
        );
        resvg::render(&tree, t, &mut pm.as_mut());
        pm.save_png(format!("{}/{}-{size}.png", a[1], a[2])).unwrap();
    }
}
```

```bash
cargo run --release -- \
  native/assets/chrome/aperture-mark-on-dark.svg \
  native/assets/chrome aperture 32 48 256
```

`cargo test -p organic-math-native --features console-edition console_icon` then checks
the results are square, the expected sizes, and fully opaque.

## ⚠️ The mark does not survive 16×16

Measured, not assumed — a 16 px render was produced and looked at, magnified 16×. At
that size the outer ring's 3 px stroke lands on 0.4 px and the inner ring's 1.4 px
stroke on 0.19 px; the ticks and the centre dot disappear entirely and what is left is a
dark square with a grey smudge in it. It is not recognisable as the aperture mark.

**32×32 is the floor** — there the outer ring, the ticks and the centre dot all read,
and the inner ring is faint but present. 48×48 is comfortable.

No 16 px raster is committed, deliberately: shipping one would only give Windows an
illegible bitmap to prefer over a downscale of a good one. Fixing it properly means a
*hinted* small-size variant of the drawing — thicker strokes, dropped inner ring — which
is an artwork decision, not a code change.

## Windows has a second icon this does not set

`console_icon.rs` sets the **window** icon: title bar, Alt-Tab, and the taskbar button
while the Console is running. The **executable** icon — what Explorer draws on
`organon-console.exe`, and what a pinned shortcut shows — is a different mechanism
entirely: a Win32 `RT_GROUP_ICON` resource linked into the exe, which needs a multi-size
`.ico` and a build script (`embed-resource`'s `compile_for`, scoped with
`cargo:rustc-link-arg-bin=organon-console` so the plugin cdylib is untouched).

Not done. It would put the root crate's first-ever build script on every build of every
binary and edition, to change an icon that is only seen when launching from Explorer —
and the Console is launched from a PATH shim. `aperture-32.png` and `aperture-256.png`
are the entries that `.ico` would need if it is ever wanted.
