//! **What a hosted module's frame costs to move across a process boundary by copy.**
//!
//! `doc/organon_module_viewport.md` §4.4 puts two mechanisms side by side — **A**, a shared
//! GPU texture (zero copy, `wgpu-hal` interop, `unsafe`, per-backend), and **B**, a
//! shared-memory frame copy (portable, no `unsafe`, the mechanism `ipc::ns_file` already
//! runs on) — and then refuses to choose, because nobody has measured either. This file
//! produces the number that decides it: the wall-clock cost, **on the producer's own
//! thread**, of getting one region-sized frame off the GPU and back onto it.
//!
//! It is a **report, not a gate**. It is `#[ignore]`d, it builds no binary, it adds no cargo
//! feature, and on a machine with no adapter it prints why and passes. CI has no GPU.
//!
//! ```text
//! cargo test -p organon-render --release -- --ignored --nocapture
//! ```
//!
//! # What is measured, and what each phase is not
//!
//! Five phases, reported separately because conflating them is how this measurement gets
//! the wrong answer:
//!
//! | phase | what it is | the trap if you skip it |
//! |---|---|---|
//! | `encode` | building the `copy_texture_to_buffer` command | cheap, and it is *not* the cost |
//! | `submit->fence` | `submit` then `poll(Wait)` on **that submission index** | timing `submit` alone measures queue submission, not GPU work |
//! | `map wait` | `map_async` + the poll that fires its callback | `Instant::now()` around an async map with no poll measures nothing at all |
//! | `memcpy out` | de-padding the mapped rows into a tight buffer | this is the byte cost the shm ring would pay |
//! | `re-upload` | `write_texture` + a submit + its fence — the console's half | `write_texture` alone only stages; the copy happens at the next submit |
//!
//! Three conditions per size, because the allocation strategy moves the number:
//!
//! - **split / fresh** — a new staging buffer and a new destination texture every iteration,
//!   which is what a naive per-frame path pays.
//! - **split / reused** — both allocated once, which is what a real ring buffer would do.
//! - **fused / reused** — the shape a real path actually takes: `submit`, then `map_async`,
//!   then **one** `poll(Wait)`. The split form pays for an extra poll to separate GPU time
//!   from map time; the fused `stall` is the honest single number.
//!
//! ⚠️ **This measures the frame boundary, not a frame.** The source texture is filled once
//! and then copied repeatedly, so `submit->fence` is the copy alone. In a real producer the
//! copy is appended after the render and the fence wait swallows both — but the *added*
//! stall the boundary imposes is what this is for, and that is what is measured.
//!
//! ⚠️ **Row alignment is reported rather than assumed.** `copy_texture_to_buffer` requires
//! `bytes_per_row` be a multiple of [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`] (256), and at
//! RGBA8 every one of the four "real" widths here is *already* aligned — 640, 1280, 1920 and
//! 2560 all give a row that is a whole number of 256-byte units. A measurement that padded
//! by accident and never exercised the path would be a trap for whoever picks 900 or 1100
//! next, so the sweep carries one deliberately unaligned width and every row of output
//! prints the padding it applied.

use std::time::{Duration, Instant};

/// Byte-for-byte `console_main.rs`'s `BACKDROP_FORMAT`. The bytes-per-pixel is what the
/// arithmetic here turns on, so the format has to be the real one rather than a plausible
/// one — an `Rgba16Float` guess would double every number below.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BPP: u32 = 4;

/// Sizes bracketing a region viewport and the whole pane, plus one width chosen *because*
/// it is not 256-aligned.
const SIZES: &[(u32, u32, &str)] = &[
    (640, 360, "small region"),
    (900, 506, "UNALIGNED width — exercises the padding path"),
    (1280, 720, "typical region"),
    (1920, 1080, "full pane"),
    (2560, 1440, "full pane, 1440p"),
];

const WARMUP: usize = 12;
const ITERS: usize = 64;

/// The row stride `copy_texture_to_buffer` demands: the unpadded row rounded up to 256.
/// (`organon-world`'s `snap.rs` and `recorder.rs` each keep a local copy of this for the
/// same self-containment reason.)
fn padded_bytes_per_row(width: u32, bpp: u32) -> u32 {
    (width * bpp).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Nearest-rank percentile over an already-sorted slice. Median and p90 rather than a mean,
/// because the thing being characterised is a stall and a mean hides the stall.
fn pct(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// One phase's samples, reduced.
struct Phase {
    name: &'static str,
    med: Duration,
    p90: Duration,
}

fn reduce(name: &'static str, mut v: Vec<Duration>) -> Phase {
    v.sort_unstable();
    Phase { name, med: pct(&v, 50.0), p90: pct(&v, 90.0) }
}

fn print_phases(label: &str, phases: &[Phase], bytes: u64) {
    println!("    {label}");
    for p in phases {
        // MB/s is only meaningful for the phases that actually move the bytes; printing it
        // for `encode` would invite reading a command-buffer build as bandwidth.
        let moves_bytes = matches!(
            p.name,
            "submit->fence" | "map wait" | "memcpy out" | "re-upload" | "stall (fused)"
        );
        let rate = if moves_bytes && p.med > Duration::ZERO {
            let mbs = bytes as f64 / (1024.0 * 1024.0) / p.med.as_secs_f64();
            format!("{mbs:9.0} MB/s")
        } else {
            String::new()
        };
        println!(
            "      {:<16} median {:8.3} ms   p90 {:8.3} ms  {}",
            p.name,
            ms(p.med),
            ms(p.p90),
            rate
        );
    }
    // The medians do not sum to the median of the sum, and this is not pretending they do:
    // it is the sum of the per-phase medians, printed so the phases can be weighed against a
    // frame budget without adding them up by hand.
    let total_med: Duration = phases.iter().map(|p| p.med).sum();
    println!(
        "      {:<16} median {:8.3} ms   = {:5.1} % of a 16.7 ms frame",
        "TOTAL",
        ms(total_med),
        ms(total_med) / 16.7 * 100.0
    );
}

/// The whole measurement. See the module docs for what each number is and is not.
#[test]
#[ignore = "needs a GPU; run explicitly: cargo test -p organon-render --release -- --ignored --nocapture"]
fn module_frame_boundary_readback_cost() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    })) {
        Ok(a) => a,
        Err(e) => {
            println!(
                "SKIPPED: no wgpu adapter on this machine ({e}). This is a report, not a gate."
            );
            return;
        }
    };
    let info = adapter.get_info();

    let (device, queue) = match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("frame-boundary-measure"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    })) {
        Ok(p) => p,
        Err(e) => {
            println!("SKIPPED: adapter present but no device ({e}). This is a report, not a gate.");
            return;
        }
    };

    println!();
    println!("=== module frame boundary — GPU->CPU readback + re-upload ===");
    // §4.4's mechanism A needs both processes on the same backend and the same adapter, and
    // notes that no `Backends::` restriction is set anywhere in this tree. So the backend
    // below is whatever wgpu picked, not something asked for — print it, because every
    // number here belongs to it.
    println!("adapter   : {} ({:?}, {:?})", info.name, info.backend, info.device_type);
    println!("driver    : {} {}", info.driver, info.driver_info);
    println!(
        "backend   : {:?}  <- wgpu's choice; nothing in this tree restricts Backends",
        info.backend
    );
    println!("format    : {FORMAT:?} ({BPP} bytes/px) — console_main.rs BACKDROP_FORMAT");
    println!("iterations: {ITERS} measured after {WARMUP} warm-up");
    println!("alignment : COPY_BYTES_PER_ROW_ALIGNMENT = {}", wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    println!();

    for &(w, h, note) in SIZES {
        let padded = padded_bytes_per_row(w, BPP);
        let unpadded = w * BPP;
        let tight_bytes = (unpadded as u64) * h as u64;
        let buffer_bytes = (padded as u64) * h as u64;

        println!("--- {w}x{h}  ({note})");
        println!(
            "    rows: unpadded {unpadded} B -> padded {padded} B  (+{} B/row, {:.2} % waste)",
            padded - unpadded,
            (padded - unpadded) as f64 / padded as f64 * 100.0
        );
        println!(
            "    bytes: {tight_bytes} tight / {buffer_bytes} in the staging buffer  ({:.2} MiB / {:.2} MiB)",
            tight_bytes as f64 / (1024.0 * 1024.0),
            buffer_bytes as f64 / (1024.0 * 1024.0)
        );

        let src = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("boundary-src"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // Give it content once. An untouched texture is uninitialised, and wgpu would
        // zero-init it on first use *inside* the first measured iteration.
        {
            let view = src.create_view(&wgpu::TextureViewDescriptor::default());
            let mut enc = device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fill") });
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fill"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.2, g: 0.5, b: 0.8, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let idx = queue.submit([enc.finish()]);
            let _ =
                device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
        }

        let make_staging = |device: &wgpu::Device| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("boundary-staging"),
                size: buffer_bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let make_dst = |device: &wgpu::Device| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("boundary-dst"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };

        let encode_copy =
            |buf: &wgpu::Buffer, tex: &wgpu::Texture, enc: &mut wgpu::CommandEncoder| {
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: buf,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded),
                            rows_per_image: Some(h),
                        },
                    },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            };

        // De-pad the mapped rows into `out`. This is the byte cost a shared-memory ring
        // would pay; it is a per-row memcpy rather than one big one precisely *because* of
        // the 256-byte padding, which is why the unaligned row above is in the sweep.
        let depad = |data: &[u8], out: &mut Vec<u8>| {
            out.clear();
            for row in 0..h as usize {
                let start = row * padded as usize;
                out.extend_from_slice(&data[start..start + unpadded as usize]);
            }
        };

        let upload = |dst: &wgpu::Texture, bytes: &[u8]| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: dst,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(unpadded),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            // `write_texture` only *stages*. The copy happens at the next submit, and the
            // fence is what says it landed — timing the call alone would report ~0.
            let idx = queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
            let _ =
                device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
        };

        // ---- conditions 1 & 2: split phases, fresh vs reused allocations ---------------
        for reuse in [false, true] {
            let kept_staging = reuse.then(|| make_staging(&device));
            let kept_dst = reuse.then(|| make_dst(&device));
            let mut kept_out: Vec<u8> = Vec::with_capacity(tight_bytes as usize);

            let (mut t_enc, mut t_gpu, mut t_map, mut t_cpy, mut t_up) =
                (vec![], vec![], vec![], vec![], vec![]);

            for i in 0..(WARMUP + ITERS) {
                let measured = i >= WARMUP;
                let fresh_staging = (!reuse).then(|| make_staging(&device));
                let fresh_dst = (!reuse).then(|| make_dst(&device));
                let staging = kept_staging.as_ref().or(fresh_staging.as_ref()).unwrap();
                let dst = kept_dst.as_ref().or(fresh_dst.as_ref()).unwrap();

                let t0 = Instant::now();
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("boundary-copy"),
                });
                encode_copy(staging, &src, &mut enc);
                let cb = enc.finish();
                let t1 = Instant::now();

                let idx = queue.submit([cb]);
                let _ = device
                    .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None });
                let t2 = Instant::now();

                let (tx, rx) = std::sync::mpsc::channel();
                staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
                let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                rx.recv().expect("map callback").expect("map ok");
                let t3 = Instant::now();

                let mut fresh_out = (!reuse).then(|| Vec::with_capacity(tight_bytes as usize));
                {
                    let data = staging.get_mapped_range(..).expect("mapped range");
                    let out = fresh_out.as_mut().unwrap_or(&mut kept_out);
                    depad(&data, out);
                }
                let t4 = Instant::now();
                staging.unmap();

                let bytes = fresh_out.as_ref().unwrap_or(&kept_out);
                let t5 = Instant::now();
                upload(dst, bytes);
                let t6 = Instant::now();

                if measured {
                    t_enc.push(t1 - t0);
                    t_gpu.push(t2 - t1);
                    t_map.push(t3 - t2);
                    t_cpy.push(t4 - t3);
                    t_up.push(t6 - t5);
                }
            }

            let label = if reuse {
                "split phases, staging buffer + destination texture REUSED (a real ring)"
            } else {
                "split phases, staging buffer + destination texture FRESH each iteration"
            };
            print_phases(
                label,
                &[
                    reduce("encode", t_enc),
                    reduce("submit->fence", t_gpu),
                    reduce("map wait", t_map),
                    reduce("memcpy out", t_cpy),
                    reduce("re-upload", t_up),
                ],
                tight_bytes,
            );
        }

        // ---- condition 3: the shape a real path takes ----------------------------------
        // One `poll(Wait)` covering both the GPU copy and the map. The split form above
        // pays for a second poll to tell them apart; this is the honest single stall.
        {
            let staging = make_staging(&device);
            let dst = make_dst(&device);
            let mut out: Vec<u8> = Vec::with_capacity(tight_bytes as usize);
            let (mut t_stall, mut t_cpy, mut t_up) = (vec![], vec![], vec![]);

            for i in 0..(WARMUP + ITERS) {
                let measured = i >= WARMUP;
                let t0 = Instant::now();
                let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("boundary-copy-fused"),
                });
                encode_copy(&staging, &src, &mut enc);
                queue.submit([enc.finish()]);
                let (tx, rx) = std::sync::mpsc::channel();
                staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
                let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                rx.recv().expect("map callback").expect("map ok");
                let t1 = Instant::now();
                {
                    let data = staging.get_mapped_range(..).expect("mapped range");
                    depad(&data, &mut out);
                }
                let t2 = Instant::now();
                staging.unmap();
                let t3 = Instant::now();
                upload(&dst, &out);
                let t4 = Instant::now();
                if measured {
                    t_stall.push(t1 - t0);
                    t_cpy.push(t2 - t1);
                    t_up.push(t4 - t3);
                }
            }

            print_phases(
                "FUSED — encode+submit+map in one poll, buffers reused (what a real path does)",
                &[
                    reduce("stall (fused)", t_stall),
                    reduce("memcpy out", t_cpy),
                    reduce("re-upload", t_up),
                ],
                tight_bytes,
            );
        }
        println!();
    }

    println!("=== end ===");
}
