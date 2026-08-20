//! GPU frame timing via wgpu timestamp queries (#277 Tier 3).
//!
//! Brackets the frame's render work with two GPU timestamps and reads the delta
//! back **a frame late** (async map, no pipeline stall), reporting a smoothed GPU
//! millisecond figure to the editor's performance status bar.
//!
//! The two timestamps are written from **separate command encoders** submitted
//! before and after `Renderer::render` (which owns its own internal encoders).
//! Queue submissions execute in order, so `ts1 − ts0` spans exactly the render
//! work in between — no change to `render.rs`. The bare-encoder `write_timestamp`
//! needs `Features::TIMESTAMP_QUERY` **and** `TIMESTAMP_QUERY_INSIDE_ENCODERS`
//! (plain `TIMESTAMP_QUERY` only permits timestamps at pass boundaries).
//!
//! Entirely opt-out: `GpuTimer::new` returns `None` when the device lacks either
//! feature (the editor then shows "n/a" and leans on `cpu_ms`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Bytes for two `u64` timestamps.
const TS_BYTES: u64 = 2 * std::mem::size_of::<u64>() as u64;

pub struct GpuTimer {
    query_set: wgpu::QuerySet,
    /// `resolve_query_set` target (QUERY_RESOLVE | COPY_SRC).
    resolve: wgpu::Buffer,
    /// CPU-visible copy (MAP_READ | COPY_DST) the delta is read back from.
    readback: wgpu::Buffer,
    /// Nanoseconds per timestamp tick (`Queue::get_timestamp_period`).
    period_ns: f32,
    /// Set by the map callback once the readback is CPU-visible.
    ready: Arc<AtomicBool>,
    /// A `map_async` is outstanding (buffer busy → skip re-arming this cycle).
    mapping: bool,
    /// This frame wrote the opening timestamp and expects a closing one.
    armed: bool,
    /// Smoothed GPU ms (EMA), the reported figure.
    ms: f32,
}

impl GpuTimer {
    /// Build a timer if the device supports timestamp queries, else `None`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<GpuTimer> {
        // `begin`/`end` call `write_timestamp` on bare encoders (outside a pass),
        // which wgpu gates behind TIMESTAMP_QUERY_INSIDE_ENCODERS *in addition to*
        // TIMESTAMP_QUERY. Require both, or a device with only the base feature
        // (e.g. some Metal configs) would panic on the first `enc.finish()`.
        let needed =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !device.features().contains(needed) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("perf-gpu-timer"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("perf-gpu-timer-resolve"),
            size: TS_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("perf-gpu-timer-readback"),
            size: TS_BYTES,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(GpuTimer {
            query_set,
            resolve,
            readback,
            period_ns: queue.get_timestamp_period(),
            ready: Arc::new(AtomicBool::new(false)),
            mapping: false,
            armed: false,
            ms: 0.0,
        })
    }

    /// Write the opening timestamp, unless a readback is still in flight (in which
    /// case this cycle is skipped and we simply don't measure — the display holds
    /// its last smoothed value). Submit BEFORE `Renderer::render`.
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.armed = !self.mapping;
        if !self.armed {
            return;
        }
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("perf-ts0") });
        enc.write_timestamp(&self.query_set, 0);
        queue.submit([enc.finish()]);
    }

    /// Write the closing timestamp, resolve + copy into the readback buffer, and
    /// kick off the async map. Submit AFTER `Renderer::render`. No-op if `begin`
    /// didn't arm this frame.
    pub fn end(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("perf-ts1") });
        enc.write_timestamp(&self.query_set, 1);
        enc.resolve_query_set(&self.query_set, 0..2, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, TS_BYTES);
        queue.submit([enc.finish()]);
        // Read it back a frame late so the CPU never waits on the GPU.
        self.mapping = true;
        let ready = self.ready.clone();
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |res| {
                // Signal completion on success OR error. On a map failure (device
                // loss/reset, buffer destroyed) wgpu still fires this callback once,
                // with `Err`; if we only signalled on `Ok`, `mapping` would stay true
                // forever and the timer would wedge for the rest of the session. poll()
                // detects the failure via `get_mapped_range` and re-arms regardless.
                let _ = res;
                ready.store(true, Ordering::Release);
            });
    }

    /// Progress the async map and fold a completed reading into the smoothed ms.
    /// Call once per frame. `device.poll` is non-blocking (`PollType::Poll`).
    pub fn poll(&mut self, device: &wgpu::Device) {
        if !self.mapping {
            return;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if !self.ready.swap(false, Ordering::AcqRel) {
            return;
        }
        // Only a successful map yields a readable range — guard the read + unmap so
        // a failed map (callback fired with `Err`) doesn't unmap an unmapped buffer.
        // Either way `mapping` is cleared below, so begin() re-arms next frame.
        if let Ok(view) = self.readback.get_mapped_range(..) {
            let ts: &[u64] = bytemuck::cast_slice(&view);
            if ts.len() >= 2 {
                let delta = ts[1].wrapping_sub(ts[0]) as f32 * self.period_ns / 1.0e6;
                // Guard against garbage on the first primed frame / a wrapped tick.
                if delta.is_finite() && delta >= 0.0 && delta < 1000.0 {
                    self.ms += (delta - self.ms) * 0.1;
                }
            }
            drop(view);
            self.readback.unmap();
        }
        self.mapping = false;
    }

    /// The smoothed GPU frame time in milliseconds.
    pub fn ms(&self) -> f32 {
        self.ms
    }
}
