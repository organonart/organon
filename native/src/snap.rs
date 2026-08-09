//! #452 Tier 3 ("the eyes") — a single-frame PNG snapshot of the production texture.
//!
//! Closes the see→act→see loop for an external agent (Bianca): `set` → `snap` → judge
//! → `set` again. Reads the same **production texture** the #430 recorder reads (the
//! fixed-resolution, aspect-correct offscreen target `capture.rs` renders the
//! scene+composite into), but as a **one-shot blocking readback** — no pipelined ring,
//! since a snapshot is occasional and on-demand (a single stall of one frame is fine,
//! unlike the per-frame video path).
//!
//! Visual-only, `#[path]`-included like `render.rs`/`recorder.rs` (never compiled into
//! the plugin cdylib). The pixel conversion is a **pure function** (`snap_to_rgba8`),
//! unit-tested below without a GPU; only `capture_png` touches wgpu.
//!
//! ## Fidelity
//! - **SDR** (production texture is an sRGB 8-bit surface): the bytes are already the
//!   finished, gamma-encoded image — strip wgpu's row padding (and swizzle BGRA→RGBA)
//!   and write them verbatim, so the PNG matches Organon's on-screen SDR look exactly.
//! - **HDR** (production texture is `Rgba16Float`, extended-linear EDR radiance — the
//!   true-HDR composite writes no SDR tonemap): there is no single "correct" SDR image,
//!   so we apply a documented **proxy** — Reinhard tonemap per channel then the sRGB
//!   OETF. Good enough for vision inference; it is NOT a readout of the display's EDR
//!   mapping (that lives in the compositor). A future refinement could reuse the SDR
//!   tone-map operator the composite already implements.

use std::io::Write;
use std::path::Path;

/// Convert one mapped production-texture readback (with wgpu's per-row padding) into
/// tight, top-to-bottom 8-bit RGBA suitable for a PNG. Pure so it unit-tests without a
/// GPU. `padded_row` is the row stride in bytes (`>= width * bytes_per_pixel`, rounded
/// up to 256). Alpha is forced opaque — the composite's alpha is not meaningful here.
pub fn snap_to_rgba8(
    src: &[u8],
    width: u32,
    height: u32,
    padded_row: u32,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let pr = padded_row as usize;
    let mut out = vec![0u8; w * h * 4];
    match format {
        wgpu::TextureFormat::Rgba16Float => {
            for y in 0..h {
                let row = y * pr;
                for x in 0..w {
                    let p = row + x * 8; // 4 channels × f16 (2 bytes)
                    let r = half::f16::from_le_bytes([src[p], src[p + 1]]).to_f32();
                    let g = half::f16::from_le_bytes([src[p + 2], src[p + 3]]).to_f32();
                    let b = half::f16::from_le_bytes([src[p + 4], src[p + 5]]).to_f32();
                    let o = (y * w + x) * 4;
                    out[o] = to_srgb8(reinhard(r));
                    out[o + 1] = to_srgb8(reinhard(g));
                    out[o + 2] = to_srgb8(reinhard(b));
                    out[o + 3] = 255;
                }
            }
        }
        f => {
            // Any 4-byte SDR surface: already display-encoded bytes. Swizzle BGRA→RGBA.
            let bgra = matches!(
                f,
                wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
            );
            for y in 0..h {
                let row = y * pr;
                for x in 0..w {
                    let p = row + x * 4;
                    let o = (y * w + x) * 4;
                    if bgra {
                        out[o] = src[p + 2];
                        out[o + 1] = src[p + 1];
                        out[o + 2] = src[p];
                    } else {
                        out[o] = src[p];
                        out[o + 1] = src[p + 1];
                        out[o + 2] = src[p + 2];
                    }
                    out[o + 3] = 255;
                }
            }
        }
    }
    out
}

/// Reinhard tonemap (`x / (x + 1)`), the SDR proxy for the EDR HDR path.
fn reinhard(x: f32) -> f32 {
    let x = x.max(0.0);
    x / (x + 1.0)
}

/// Linear `[0,1]` → sRGB-encoded 8-bit code (the standard IEC 61966-2-1 OETF).
fn to_srgb8(lin: f32) -> u8 {
    let c = lin.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

/// Row stride wgpu requires for `copy_texture_to_buffer` — the unpadded row rounded up
/// to `COPY_BYTES_PER_ROW_ALIGNMENT` (256). (A local copy of the recorder's helper so
/// this module stays self-contained.)
fn padded_bytes_per_row(width: u32, bpp: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256
    (width * bpp).div_ceil(align) * align
}

/// Read `tex` back to the CPU (a single **blocking** map) and write it as a PNG at
/// `path`. Returns the path (as a string) on success. Blocks on the GPU — fine for an
/// on-demand snapshot; never call this in the hot per-frame path (that's the recorder).
pub fn capture_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    path: &Path,
) -> std::io::Result<String> {
    let w = tex.width();
    let h = tex.height();
    let format = tex.format();
    let bpp = match format {
        wgpu::TextureFormat::Rgba16Float => 8,
        _ => 4,
    };
    let padded = padded_bytes_per_row(w, bpp);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snap-readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("snap-copy") });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);

    // One blocking map: submit, wait for the GPU, read. `map_async` fires its callback
    // during the `Wait` poll; the channel then hands us the result.
    let (tx, rx) = std::sync::mpsc::channel();
    buf.map_async(wgpu::MapMode::Read, .., move |res| {
        let _ = tx.send(res);
    });
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    match rx.recv() {
        Ok(Ok(())) => {}
        _ => {
            return Err(std::io::Error::other("snap: GPU readback map failed"));
        }
    }

    let rgba = {
        let data = buf
            .get_mapped_range(..)
            .map_err(|e| std::io::Error::other(format!("snap: mapped range failed: {e}")))?;
        snap_to_rgba8(&data, w, h, padded, format)
    };
    buf.unmap();

    write_png(path, &rgba, w, h)?;
    Ok(path.display().to_string())
}

/// Encode tight RGBA8 as a PNG file (the `image` crate's PNG codec, already a dep).
fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    use image::ImageEncoder;
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let file = std::fs::File::create(path)?;
    let mut bw = std::io::BufWriter::new(file);
    image::codecs::png::PngEncoder::new(&mut bw)
        .write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| std::io::Error::other(format!("snap: PNG encode failed: {e}")))?;
    bw.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_oetf_endpoints_and_monotonic() {
        assert_eq!(to_srgb8(0.0), 0);
        assert_eq!(to_srgb8(1.0), 255);
        assert!(to_srgb8(2.0) == 255); // clamps
        // 18% linear grey → sRGB code ~118 (1.055·0.18^(1/2.4) − 0.055 ≈ 0.461).
        assert!((to_srgb8(0.18) as i32 - 118).abs() <= 2);
        assert!(to_srgb8(0.5) > to_srgb8(0.25));
        // Reinhard is a soft roll-off in [0,1) — a bright highlight approaches but
        // never reaches 1 (avoid an f32-degenerate input like 1e9, where 1e9+1==1e9).
        assert!(reinhard(0.0) == 0.0);
        assert!(reinhard(1.0) == 0.5);
        assert!(reinhard(100.0) < 1.0 && reinhard(100.0) > 0.98);
    }

    #[test]
    fn sdr_passthrough_strips_padding_and_swizzles() {
        // 2×1 image, 4 bpp → unpadded row 8, padded to 256.
        let padded = padded_bytes_per_row(2, 4);
        assert_eq!(padded, 256);
        let mut src = vec![0u8; (padded * 1) as usize];
        // pixel 0 = (10,20,30), pixel 1 = (40,50,60)
        src[0..4].copy_from_slice(&[10, 20, 30, 99]);
        src[4..8].copy_from_slice(&[40, 50, 60, 99]);

        // Rgba8 → verbatim RGB, forced-opaque alpha, padding gone.
        let out = snap_to_rgba8(&src, 2, 1, padded, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(out, vec![10, 20, 30, 255, 40, 50, 60, 255]);

        // Bgra8 → swizzle to RGBA.
        let out = snap_to_rgba8(&src, 2, 1, padded, wgpu::TextureFormat::Bgra8UnormSrgb);
        assert_eq!(out, vec![30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn hdr_f16_is_tonemapped_to_srgb8() {
        // 1×1 Rgba16Float, 8 bpp → padded to 256.
        let padded = padded_bytes_per_row(1, 8);
        let mut src = vec![0u8; padded as usize];
        let put = |src: &mut [u8], off: usize, v: f32| {
            src[off..off + 2].copy_from_slice(&half::f16::from_f32(v).to_le_bytes());
        };
        // r = 1.0 (SDR white), g = huge highlight, b = 0.
        put(&mut src, 0, 1.0);
        put(&mut src, 2, 1000.0);
        put(&mut src, 4, 0.0);
        put(&mut src, 6, 1.0); // alpha ignored

        let out = snap_to_rgba8(&src, 1, 1, padded, wgpu::TextureFormat::Rgba16Float);
        // r: reinhard(1)=0.5 → sRGB(0.5) ≈ 188.
        assert!((out[0] as i32 - to_srgb8(0.5) as i32).abs() <= 1);
        // g: a bright highlight rolls off toward but below full white.
        assert!(out[1] > out[0] && out[1] <= 255);
        assert_eq!(out[2], 0);
        assert_eq!(out[3], 255);
    }
}
