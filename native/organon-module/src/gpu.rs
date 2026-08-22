//! **The two preallocated GPU halves — behind the `wgpu` feature, and only behind it.**
//!
//! The contract is wgpu-free (see the crate manifest). These two helpers are not the
//! contract; they are the code T0's measurement says must exist so that **preallocation is
//! structural rather than advisory**:
//!
//! | | Whose half | What it owns, once |
//! |---|---|---|
//! | [`FrameReadback`] | the producer's | the staging buffer `copy_texture_to_buffer` writes into |
//! | [`FrameTexture`] | the console's | the destination texture `write_texture` writes into |
//!
//! 🚨 **Those are exactly the two allocations T0 found dominate the measurement.** A fresh
//! staging buffer and a fresh destination texture per iteration cost **2.72 ms at 1440p
//! against 1.37 ms reused**, and 3.3× of the gap is `memcpy out` copying *identical bytes* —
//! first-touch page faulting on a destination that has never been written. A per-frame
//! allocation therefore measures 2.7 ms, reads as a verdict on mechanism B, and buys `unsafe`
//! per-backend interop to fix an allocator problem. `FrameTexture` and `FrameReadback` each
//! expose an `allocations()`
//! counter so a test can assert the absence rather than a comment claim it.
//!
//! 📌 **One copy the T0 harness paid and this does not.** That harness de-padded the mapped
//! rows into a tight buffer before `write_texture`, because it was measuring the byte cost a
//! ring would pay. It does not have to: `write_texture`'s `bytes_per_row` has no alignment
//! requirement — only `copy_texture_to_buffer`'s does — so the padded rows go up as they lie
//! and the de-pad pass disappears. [`crate::wire::ROW_ALIGNMENT`] being wgpu's own
//! `COPY_BYTES_PER_ROW_ALIGNMENT` is what makes both ends of that true at once.

use crate::presence::{Poll, Present};
use crate::ring::FrameWrite;
use crate::wire::{FrameCapacity, PixelFormat, SLOT_HEADER_BYTES};

/// This protocol's format, as wgpu spells it.
pub fn wgpu_format(f: PixelFormat) -> wgpu::TextureFormat {
    match f {
        PixelFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        PixelFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
    }
}

/// wgpu's format, as this protocol spells it — `None` for everything the wire cannot carry.
pub fn from_wgpu_format(f: wgpu::TextureFormat) -> Option<PixelFormat> {
    match f {
        wgpu::TextureFormat::Rgba8UnormSrgb => Some(PixelFormat::Rgba8UnormSrgb),
        wgpu::TextureFormat::Bgra8UnormSrgb => Some(PixelFormat::Bgra8UnormSrgb),
        _ => None,
    }
}

/// The same bytes, as a format that **decodes nothing on sample**.
///
/// 🚨 **This exists because the obvious thing is wrong in a way only an eye can see.** The wire
/// carries sRGB-encoded bytes and nothing else does — [`PixelFormat`] has two variants and both
/// are sRGB, because that is what a producer's swapchain-shaped texture holds. A view in the
/// texture's own format therefore converts sRGB to linear at every sample, which is correct for
/// a consumer that wants linear and **wrong for one that linearizes for itself**.
///
/// egui is the second kind: its shader converts its own samples. Organon's console already
/// carries this exact pairing and its own words for it — `console_main.rs`'s
/// `BACKDROP_SAMPLE_FORMAT`: *"render the world through the sRGB format, hand egui a non-sRGB
/// view of the same bytes — egui's shader linearizes its samples itself, and a decoded-on-sample
/// view would linearize twice and come out dark."*
///
/// ⚠️ **The failure is a picture that is merely too dark**, which is why it is a function rather
/// than a comment: nothing errors, nothing tears, the counters all read clean, and the only
/// report is a person saying the viewport looks murky. A pure function is a thing a test can
/// pin; a rule at a call site is a thing the next call site does not know about.
pub fn linear_view_format(f: PixelFormat) -> wgpu::TextureFormat {
    match f {
        PixelFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8Unorm,
        PixelFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8Unorm,
    }
}

// ---------------------------------------------------------------------------------------
// The console's half
// ---------------------------------------------------------------------------------------

/// **The texture the console samples, allocated once per size and reused.**
///
/// 🚨 [`FrameTexture::apply`] takes a [`Poll`] rather than a frame, which is what makes §4.6's
/// *never the last good frame* impossible to forget: a caller that hands over a `Poll::Lost`
/// gets its picture dropped whether or not it remembered to. There is no method that uploads
/// without a poll to justify it.
///
/// ⚠️ **`Forget` drops the PICTURE, not the ALLOCATION.** A producer that died and is
/// restarted must not pay for a new texture — that is the per-frame-allocation trap arriving
/// on the recovery path, where it would be hardest to notice because it only costs anything
/// when something has already gone wrong.
pub struct FrameTexture {
    format: PixelFormat,
    /// What [`FrameTexture::view`] hands out, which is **not** necessarily the texture's own
    /// format — see [`FrameTexture::sampled_as`].
    view_format: wgpu::TextureFormat,
    texture: Option<wgpu::Texture>,
    view: Option<wgpu::TextureView>,
    size: (u32, u32),
    has_picture: bool,
    allocations: u64,
}

impl FrameTexture {
    /// A texture sampled in its **own** format — sRGB in, linear out at the sampler.
    ///
    /// ⚠️ Right for a consumer that wants linear values; wrong for one that linearizes for
    /// itself. See [`FrameTexture::sampled_as`] and [`linear_view_format`].
    pub fn new(format: PixelFormat) -> FrameTexture {
        FrameTexture {
            format,
            view_format: wgpu_format(format),
            texture: None,
            view: None,
            size: (0, 0),
            has_picture: false,
            allocations: 0,
        }
    }

    /// Hand out a view in `view_format` instead of the texture's own.
    ///
    /// 🚨 **The one caller this exists for is a consumer whose shader linearizes its own
    /// samples**, and the only sane argument is [`linear_view_format`]'s answer — that function
    /// carries the whole reasoning and the measurement Organon's console already made.
    ///
    /// The bytes on the wire do not change and neither does the texture: this is a
    /// reinterpretation at the sampler, which is why it can be a builder rather than a second
    /// upload path. It is applied to the texture's `view_formats` as well, so the view is legal
    /// rather than merely requested.
    ///
    /// ⚠️ **Call it before the first [`FrameTexture::apply`].** Changing it afterwards would
    /// have to rebuild the texture — `view_formats` is fixed at creation — which is the
    /// per-frame allocation this whole file exists to make impossible, so it is not offered.
    /// Nothing enforces the ordering because nothing can: a later call is honoured for the next
    /// allocation and cannot retroactively legalise a view of a texture that did not declare it.
    /// Set it once, at construction, beside `new`.
    pub fn sampled_as(mut self, view_format: wgpu::TextureFormat) -> FrameTexture {
        self.view_format = view_format;
        self
    }

    /// The view to sample, or `None` when there is nothing the console may show.
    ///
    /// `None` covers all of: nothing has arrived yet, and the producer is in any state §4.6
    /// forbids painting through. A call site that draws the sentence whenever this is `None`
    /// has implemented the whole vacancy rule.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        if self.has_picture {
            self.view.as_ref()
        } else {
            None
        }
    }

    /// The size of the picture currently held.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.has_picture.then_some(self.size)
    }

    /// How many textures have been created. Grows only when the producer's frame size
    /// genuinely changes — never per frame, and never on a producer restart.
    pub fn allocations(&self) -> u64 {
        self.allocations
    }

    /// Obey one poll.
    ///
    /// ⚠️ It matches on the **poll**, and asserts that its [`Present`] agrees, rather than
    /// matching on the `Present` and having an arm for a `Frame` that cannot arrive. A
    /// `Present` match would need an unreachable arm — `region.rs`'s standing rule is that an
    /// unreachable arm is an untested branch pretending to be a design — and it would put the
    /// two answers out of the compiler's reach at the one site that has to obey both.
    pub fn apply(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, poll: &Poll<'_>) {
        match poll {
            Poll::Frame(view) => {
                debug_assert_eq!(poll.present(), Present::Upload);
                debug_assert_eq!(view.format, self.format, "the channel changed format mid-run");
                self.ensure(device, view.width, view.height);
                let Some(tex) = self.texture.as_ref() else { return };
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    view.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        // The padded stride, straight from the ring — see the module docs on
                        // the copy this does not pay.
                        bytes_per_row: Some(view.row_stride),
                        rows_per_image: Some(view.height),
                    },
                    wgpu::Extent3d {
                        width: view.width,
                        height: view.height,
                        depth_or_array_layers: 1,
                    },
                );
                self.has_picture = true;
            }
            Poll::Starting { .. } | Poll::Holding => {}
            Poll::Stalled { .. }
            | Poll::NotProducing { .. }
            | Poll::Lost { .. }
            | Poll::Gone => self.has_picture = false,
        }
        debug_assert!(
            !(self.has_picture && poll.present() == Present::Forget),
            "the texture kept a picture the verdict forbids"
        );
    }

    fn ensure(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.texture.is_some() && self.size == (w, h) {
            return;
        }
        let stored = wgpu_format(self.format);
        // The reinterpretation has to be declared at creation or the view below is illegal —
        // wgpu validates `create_view` against exactly this list. Declaring the same format
        // twice is not an error and is cheaper than branching on whether it differs.
        let view_formats = [stored, self.view_format];
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("organon-module frame"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: stored,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &view_formats,
        });
        self.view = Some(tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("organon-module frame sample"),
            format: Some(self.view_format),
            ..Default::default()
        }));
        self.texture = Some(tex);
        self.size = (w, h);
        self.allocations += 1;
    }
}

// ---------------------------------------------------------------------------------------
// The producer's half
// ---------------------------------------------------------------------------------------

/// Why a readback did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadbackFault {
    /// The source texture is not the size the frame was begun at.
    SizeMismatch { texture: (u32, u32), frame: (u32, u32) },
    /// The source texture's format is not the channel's.
    FormatMismatch,
    /// The map never completed. In practice: a lost device.
    MapFailed,
}

/// **The producer's staging buffer, allocated once at capacity and reused for every frame.**
///
/// The buffer is sized for the channel's whole capacity rather than for the current frame,
/// which is what makes a resize free on this side too: a smaller frame simply uses a prefix.
///
/// # 🚨 This IS the producer's readback. A producer that also does its own pays twice.
///
/// [`FrameReadback::capture`] takes the producer's texture and copies it **into the ring slot**
/// — the GPU→CPU half of the boundary happens here, on the producer's queue, and it is the
/// half T0 measured as the producer's added stall. So a hosted producer does **not** call
/// whatever `read_frame`-shaped method its own engine offers; that path is for its standalone
/// host.
///
/// ⚠️ **Calling both runs two GPU→CPU copies of the same texture into two staging buffers, and
/// the second is pure waste with no error anywhere.** Nothing here can detect it — the two
/// copies are independently valid — so it is written down instead. ✏️ Reported by the Ascent
/// session, which found its own `read_frame` eagerly holding **16.6 MiB at 1080p and 29 MiB at
/// 1440p** that a hosted module would never touch.
pub struct FrameReadback {
    staging: wgpu::Buffer,
    capacity: FrameCapacity,
    allocations: u64,
}

impl FrameReadback {
    pub fn new(device: &wgpu::Device, capacity: FrameCapacity) -> FrameReadback {
        let bytes = capacity.slot_stride() - SLOT_HEADER_BYTES as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("organon-module readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        FrameReadback { staging, capacity, allocations: 1 }
    }

    /// Always 1. See the module docs.
    pub fn allocations(&self) -> u64 {
        self.allocations
    }

    /// Copy `src` into the frame being written.
    ///
    /// This is the shape T0 called *fused / reused* and measured at 0.06 ms of added producer
    /// stall at 640×360 and 0.35 ms at 2560×1440: `submit`, then `map_async`, then **one**
    /// `poll(Wait)`. The split form pays for a second poll to tell GPU time from map time,
    /// which is a measurement's need and not a producer's.
    pub fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        src: &wgpu::Texture,
        frame: &mut FrameWrite<'_>,
    ) -> Result<(), ReadbackFault> {
        let (w, h) = (frame.width(), frame.height());
        if src.width() != w || src.height() != h {
            return Err(ReadbackFault::SizeMismatch {
                texture: (src.width(), src.height()),
                frame: (w, h),
            });
        }
        if from_wgpu_format(src.format()) != Some(self.capacity.format) {
            return Err(ReadbackFault::FormatMismatch);
        }
        let stride = frame.row_stride();
        let bytes = stride as usize * h as usize;

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("organon-module readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: src,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(Some(enc.finish()));

        let slice = self.staging.slice(0..bytes as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        match rx.try_recv() {
            Ok(Ok(())) => {}
            _ => return Err(ReadbackFault::MapFailed),
        }
        let Ok(mapped) = slice.get_mapped_range() else {
            self.staging.unmap();
            return Err(ReadbackFault::MapFailed);
        };
        frame.pixels_mut()[..bytes].copy_from_slice(&mapped[..bytes]);
        drop(mapped);
        self.staging.unmap();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🚨 The one duplicated constant in this crate, and the test that keeps it honest.
    /// `wire::ROW_ALIGNMENT` is `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` written out by hand
    /// because the contract must not depend on wgpu. The constant is duplicated; the
    /// *agreement* is checked, so a wgpu that ever changed it would fail here rather than in a
    /// validation error on somebody's machine.
    #[test]
    fn the_row_alignment_is_wgpus() {
        assert_eq!(crate::wire::ROW_ALIGNMENT, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    }

    /// CONTRACT: **the linear view is the same channel order and the same width, with the
    /// decode taken off.**
    ///
    /// ⚠️ **Mutation-tested, and the mutation is the one somebody would actually write**: making
    /// this the identity — the shape it would have if the pairing were forgotten — fails at
    /// *"the linear view must not decode"*. Swapping the two answers (Rgba to `Bgra8Unorm`)
    /// fails at *"the channel order is not the sRGB decode and must not change with it"*, which
    /// is worth its own assertion because a swapped answer is a picture with red and blue
    /// exchanged, and that reads as a producer bug rather than as a view format.
    #[test]
    fn the_linear_view_takes_the_decode_off_and_changes_nothing_else() {
        assert_eq!(
            linear_view_format(PixelFormat::Rgba8UnormSrgb),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        assert_eq!(
            linear_view_format(PixelFormat::Bgra8UnormSrgb),
            wgpu::TextureFormat::Bgra8Unorm,
        );
        for f in [PixelFormat::Rgba8UnormSrgb, PixelFormat::Bgra8UnormSrgb] {
            assert_ne!(linear_view_format(f), wgpu_format(f), "the linear view must not decode");
            // The channel order is a property of the wire and the decode is not, so taking one
            // off must not move the other. `from_wgpu_format` cannot answer for a non-sRGB
            // format by design, so the comparison is on the wgpu names.
            let (a, b) = (linear_view_format(f), wgpu_format(f));
            let swapped = matches!(
                (a, b),
                (wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb)
                    | (wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8UnormSrgb)
            );
            assert!(!swapped, "the channel order is not the sRGB decode and must not change with it");
        }
    }

    /// `sampled_as` is a builder and the default is the texture's own format — so a caller that
    /// does not know about the double-linearization trap gets today's behaviour unchanged, and
    /// only a caller that asks pays for the reinterpretation.
    #[test]
    fn a_frame_texture_samples_in_its_own_format_until_asked_otherwise() {
        let plain = FrameTexture::new(PixelFormat::Rgba8UnormSrgb);
        assert_eq!(plain.view_format, wgpu::TextureFormat::Rgba8UnormSrgb);
        let linear = FrameTexture::new(PixelFormat::Rgba8UnormSrgb)
            .sampled_as(linear_view_format(PixelFormat::Rgba8UnormSrgb));
        assert_eq!(linear.view_format, wgpu::TextureFormat::Rgba8Unorm);
        // Neither has allocated anything by being configured.
        assert_eq!(plain.allocations(), 0);
        assert_eq!(linear.allocations(), 0);
    }

    #[test]
    fn the_two_formats_round_trip_and_nothing_else_is_admitted() {
        for f in [PixelFormat::Rgba8UnormSrgb, PixelFormat::Bgra8UnormSrgb] {
            assert_eq!(from_wgpu_format(wgpu_format(f)), Some(f));
        }
        assert_eq!(from_wgpu_format(wgpu::TextureFormat::Rgba16Float), None);
        assert_eq!(from_wgpu_format(wgpu::TextureFormat::Depth32Float), None);
    }

    /// The GPU-free half of the console's contract: a texture with no device attached still
    /// answers the vacancy question correctly, and `Forget` never reports a picture.
    #[test]
    fn a_texture_with_nothing_in_it_shows_nothing() {
        let t = FrameTexture::new(PixelFormat::Rgba8UnormSrgb);
        assert!(t.view().is_none());
        assert!(t.size().is_none());
        assert_eq!(t.allocations(), 0);
    }

    /// **The whole boundary, both halves, against a real device.**
    ///
    /// `#[ignore]`d and skipping cleanly with no adapter, on `frame_boundary.rs`'s precedent:
    /// CI has no GPU, and a test that claims to skip on a GPU-less machine and has never been
    /// run on one is a claim rather than a behaviour.
    ///
    /// ```text
    /// cargo test -p organon-module --all-features -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs a GPU; run explicitly: cargo test -p organon-module --all-features -- --ignored --nocapture"]
    fn a_texture_goes_through_the_ring_and_comes_back_the_same_with_one_allocation() {
        use crate::sim;
        use crate::{FrameCapacity, ModuleChannel, Poll, ProducerChannel};
        use std::time::Instant;

        const W: u32 = 128;
        const H: u32 = 96;
        const FRAMES: u64 = 8;

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            },
        )) {
            Ok(a) => a,
            Err(e) => {
                println!("SKIPPED: no wgpu adapter on this machine ({e}).");
                return;
            }
        };
        println!("adapter: {:?}", adapter.get_info());
        let (device, queue) = match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("organon-module round trip"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
        )) {
            Ok(p) => p,
            Err(e) => {
                println!("SKIPPED: adapter present but no device ({e}).");
                return;
            }
        };

        let path = crate::map::tests::TempPath::new("gpu-round-trip");
        let capacity = FrameCapacity::new(W, H, PixelFormat::Rgba8UnormSrgb);
        let mut console = ModuleChannel::create(&path.0, capacity).unwrap();
        let mut producer = ProducerChannel::open(&path.0).unwrap();
        let readback = FrameReadback::new(&device, capacity);
        let mut texture = FrameTexture::new(PixelFormat::Rgba8UnormSrgb);

        let src = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("round-trip source"),
            size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let mut cpu = vec![0u8; (W * H * 4) as usize];
        for index in 1..=FRAMES {
            // Paint the source with the frame-keyed pattern, so `sim::verify` can say every
            // pixel that came back belongs to the same frame.
            for y in 0..H {
                for x in 0..W {
                    let o = (y * W + x) as usize * 4;
                    cpu[o..o + 4].copy_from_slice(&sim::pixel(x, y, index));
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &src,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &cpu,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W * 4),
                    rows_per_image: Some(H),
                },
                wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
            );

            producer.tick();
            let mut frame = producer.begin_frame(W, H).expect("a free slot");
            assert_eq!(frame.frame_index(), index);
            readback.capture(&device, &queue, &src, &mut frame).expect("readback");
            frame.commit();
            producer.published(W, H);

            let poll = console.poll(Instant::now());
            match &poll {
                Poll::Frame(v) => sim::verify(v).unwrap(),
                other => panic!("expected frame {index}, got {other:?}"),
            }
            texture.apply(&device, &queue, &poll);
            assert!(texture.view().is_some());
            assert_eq!(texture.size(), Some((W, H)));
        }

        // 🚨 T0's condition, on the GPU side: one texture, one staging buffer, eight frames.
        assert_eq!(texture.allocations(), 1);
        assert_eq!(readback.allocations(), 1);
        assert_eq!(console.staging_allocations(), 1);

        // And the picture goes when the producer does — without giving the allocation back,
        // so a restart costs nothing.
        producer.depart();
        let poll = console.poll(Instant::now());
        assert!(matches!(poll, Poll::Gone));
        texture.apply(&device, &queue, &poll);
        assert!(texture.view().is_none(), "never the last good frame");
        assert_eq!(texture.allocations(), 1, "Forget drops the picture, not the allocation");
    }
}
