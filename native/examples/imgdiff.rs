//! `imgdiff` — measure a PNG frame, or the difference between two of them.
//!
//! The comparator half of the `verify.sh` frame harness (see `native/verify/README.md`).
//! Deliberately **pure measurement, no policy**: it prints one line of JSON and exits 0
//! whenever it could read its inputs. `verify.sh` owns every threshold, so a scene can
//! tighten or loosen its own gate without touching this file.
//!
//! ```text
//! cargo run --release --example imgdiff -- <a.png>                      # stats mode
//! cargo run --release --example imgdiff -- <a.png> <b.png> [options]    # diff mode
//!     --diff-out <p.png>   write an amplified difference image (triage / PR evidence)
//!     --epsilon <f>        per-channel "this pixel changed" threshold (default 0.02)
//!     --amplify <f>        difference-image gain (default 4.0)
//! ```
//!
//! **Stats mode** (one input) answers "is this frame alive?" without a reference:
//! `{"mode":"stats","w":1100,"h":760,"mean":0.21,"stddev":0.18,"max":0.98}`.
//! A black frame is `mean≈0`; a frame that rendered a flat clear colour and nothing
//! else is `stddev≈0`. That pair catches "it launched but drew nothing", which is the
//! failure a golden can't distinguish from "no golden yet".
//!
//! **Diff mode** (two inputs) reports three numbers because they fail differently:
//! - `mean_abs`  mean |a-b| across every channel, 0..1 — a global "how different"
//!               number. Robust to a few stray pixels, but nearly blind to a small
//!               local shift.
//! - `diff_frac` fraction of *pixels* where any channel differs by more than
//!               `epsilon` — the sensitive one, and the reason this tool exists. An
//!               egui row that under-fills by 64pt moves a large fraction of pixels
//!               while barely moving `mean_abs`; that is exactly the #545 class of
//!               defect that shipped three times against a green test suite.
//! - `max_abs`   the single worst channel difference, for triage.
//!
//! Mismatched dimensions report `dims_match:false` with the metrics pinned to 1.0, so a
//! policy gate fails naturally instead of needing a special case.
//!
//! Exit: 0 measured · 2 bad usage, or an input that could not be read/decoded.

use image::RgbImage;

fn usage() -> ! {
    eprintln!(
        "usage: imgdiff <a.png> [b.png] [--diff-out <p.png>] [--epsilon <f>] [--amplify <f>]\n\
         \n  one input  → stats mode (is this frame alive?)\
         \n  two inputs → diff mode  (how far has it moved?)"
    );
    std::process::exit(2)
}

fn load(path: &str) -> RgbImage {
    match image::open(path) {
        Ok(img) => img.to_rgb8(),
        Err(e) => {
            eprintln!("imgdiff: cannot read '{path}': {e}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut inputs: Vec<String> = Vec::new();
    let mut diff_out: Option<String> = None;
    let mut epsilon: f32 = 0.02;
    let mut amplify: f32 = 4.0;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--diff-out" => {
                i += 1;
                diff_out = Some(argv.get(i).unwrap_or_else(|| usage()).clone());
            }
            "--epsilon" => {
                i += 1;
                epsilon = argv.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
            }
            "--amplify" => {
                i += 1;
                amplify = argv.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
            }
            "-h" | "--help" => usage(),
            other => inputs.push(other.to_string()),
        }
        i += 1;
    }

    match inputs.len() {
        1 => stats(&load(&inputs[0])),
        2 => diff(&load(&inputs[0]), &load(&inputs[1]), epsilon, amplify, diff_out.as_deref()),
        _ => usage(),
    }
}

/// One-image liveness: mean / stddev / max over every channel, normalized 0..1.
fn stats(img: &RgbImage) {
    let (w, h) = img.dimensions();
    let n = (w as f64) * (h as f64) * 3.0;
    let (mut sum, mut sum_sq, mut max) = (0.0f64, 0.0f64, 0.0f32);
    for px in img.pixels() {
        for &c in &px.0 {
            let v = c as f64 / 255.0;
            sum += v;
            sum_sq += v * v;
            max = max.max(v as f32);
        }
    }
    let mean = sum / n;
    // Population variance, clamped: float error can push a flat image fractionally
    // below zero, and a NaN stddev would silently pass every downstream gate.
    let var = (sum_sq / n - mean * mean).max(0.0);
    println!(
        "{{\"mode\":\"stats\",\"w\":{w},\"h\":{h},\"mean\":{:.6},\"stddev\":{:.6},\"max\":{:.6}}}",
        mean,
        var.sqrt(),
        max
    );
}

/// Two-image difference: the three metrics, plus an optional amplified diff image.
fn diff(a: &RgbImage, b: &RgbImage, epsilon: f32, amplify: f32, diff_out: Option<&str>) {
    let (w, h) = a.dimensions();
    if a.dimensions() != b.dimensions() {
        let (bw, bh) = b.dimensions();
        eprintln!("imgdiff: dimensions differ — {w}x{h} vs {bw}x{bh}");
        println!(
            "{{\"mode\":\"diff\",\"w\":{w},\"h\":{h},\"dims_match\":false,\
             \"mean_abs\":1.0,\"max_abs\":1.0,\"diff_frac\":1.0,\"epsilon\":{epsilon:.6}}}"
        );
        return;
    }

    let mut out = diff_out.map(|_| RgbImage::new(w, h));
    let (mut sum, mut max_abs, mut changed) = (0.0f64, 0.0f32, 0u64);

    for y in 0..h {
        for x in 0..w {
            let (pa, pb) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
            let mut pixel_changed = false;
            let mut vis = [0u8; 3];
            for c in 0..3 {
                let d = (pa[c] as f32 - pb[c] as f32).abs() / 255.0;
                sum += d as f64;
                max_abs = max_abs.max(d);
                pixel_changed |= d > epsilon;
                vis[c] = ((d * amplify).clamp(0.0, 1.0) * 255.0) as u8;
            }
            if pixel_changed {
                changed += 1;
            }
            if let Some(img) = out.as_mut() {
                img.put_pixel(x, y, image::Rgb(vis));
            }
        }
    }

    let pixels = (w as f64) * (h as f64);
    let mean_abs = sum / (pixels * 3.0);
    let diff_frac = changed as f64 / pixels;

    if let (Some(img), Some(path)) = (out.as_ref(), diff_out) {
        if let Err(e) = img.save(path) {
            // Not fatal: the measurement is the deliverable, the picture is a nicety.
            eprintln!("imgdiff: could not write '{path}': {e}");
        }
    }

    println!(
        "{{\"mode\":\"diff\",\"w\":{w},\"h\":{h},\"dims_match\":true,\
         \"mean_abs\":{mean_abs:.6},\"max_abs\":{max_abs:.6},\
         \"diff_frac\":{diff_frac:.6},\"epsilon\":{epsilon:.6}}}"
    );
}
