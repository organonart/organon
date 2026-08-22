//! **A test producer that uses the real producer API, including the parts that hurt.**
//!
//! 🚨 **It is not a fake and it must not become one.** Everything here goes through
//! [`crate::ProducerChannel`] — the same `begin_frame`/`commit`, the same status block, the
//! same input drain a real module uses. What it adds is a *deterministic picture* and a set
//! of levers for the three things a consumer has to survive and cannot be asked to survive by
//! inspection:
//!
//! | Lever | The failure it stages |
//! |---|---|
//! | [`SimProducer::abandon_frame`] | a producer that **dies mid-frame** — the seqlock left odd, half a picture in a slot |
//! | [`SimProducer::stop`] / [`SimProducer::depart`] | a producer that **stops**, with and without a farewell |
//! | [`SimProducer::refuse`] | 🚨 a producer that is **alive and silent** — still ticking, drawing nothing, with and without saying why |
//! | [`SimProducer::draw`] at a new size | a **size change**, including one the console did not ask for |
//! | [`crate::ProducerChannel::scribble_over`] | a producer that **ignores the reader's hold** — the tearing the ring exists to prevent |
//!
//! ⚠️ **The picture is deterministic and frame-keyed on purpose.** Every pixel is a function
//! of `(x, y, frame_index)`, so [`verify`] can say not merely *"these bytes look plausible"*
//! but *"every pixel in this frame came from the same frame"* — which is the only way to tell
//! a torn read from a whole one, since a torn frame is made entirely of valid pixels.

use crate::wire::{PixelFormat, ProducerState};
use crate::{FrameView, ProducerChannel};

/// The value a pixel of frame `f` at `(x, y)` must hold.
///
/// The frame index lands in the blue channel, which is what makes a tear visible: two frames
/// blended row-wise differ in exactly that byte, and nothing else about the image would say
/// so.
pub fn pixel(x: u32, y: u32, frame_index: u64) -> [u8; 4] {
    [(x & 0xFF) as u8, (y & 0xFF) as u8, (frame_index & 0xFF) as u8, 0xFF]
}

/// An obedient producer with the levers a consumer test needs.
pub struct SimProducer {
    ch: ProducerChannel,
    /// The size the next [`SimProducer::draw`] uses when no size is given.
    size: (u32, u32),
    stopped: bool,
    refusing: bool,
}

impl SimProducer {
    pub fn new(ch: ProducerChannel) -> SimProducer {
        let size = (ch.capacity().max_width, ch.capacity().max_height);
        SimProducer { ch, size, stopped: false, refusing: false }
    }

    pub fn channel(&mut self) -> &mut ProducerChannel {
        &mut self.ch
    }

    /// One loop iteration: bump the liveness counter and re-read the control block. A real
    /// producer does this whether or not it draws, and so does this one — that is what makes
    /// [`SimProducer::stop`] mean something.
    pub fn tick(&mut self) {
        if !self.stopped {
            self.ch.tick();
        }
    }

    /// Adopt whatever size the console is asking for, as a real producer would when it is
    /// ready to reallocate.
    pub fn adopt_requested_size(&mut self) {
        let (w, h) = self.ch.control().requested;
        self.size = (w, h);
        self.ch.adopt_size(w, h);
    }

    /// Draw and publish one frame at the current size.
    pub fn draw(&mut self) -> Option<u64> {
        let (w, h) = self.size;
        self.draw_at(w, h)
    }

    /// Draw and publish one frame at an explicit size — including one the console never
    /// asked for, which is a producer being independently sized and is legal.
    pub fn draw_at(&mut self, w: u32, h: u32) -> Option<u64> {
        if self.stopped || self.refusing {
            return None;
        }
        self.size = (w, h);
        let index = {
            let mut write = self.ch.begin_frame(w, h)?;
            let index = write.frame_index();
            let stride = write.row_stride() as usize;
            let px = write.pixels_mut();
            for y in 0..h {
                let row = &mut px[y as usize * stride..y as usize * stride + w as usize * 4];
                for x in 0..w {
                    row[x as usize * 4..x as usize * 4 + 4]
                        .copy_from_slice(&pixel(x, y, index));
                }
            }
            write.commit();
            index
        };
        self.ch.published(w, h);
        Some(index)
    }

    /// 🚨 Open a frame and **never commit it** — a producer that died with the pen in its
    /// hand. The slot's seqlock is left odd and `latest_slot` still names the previous whole
    /// frame, so the consumer must go on serving that one and never see the wreckage.
    pub fn abandon_frame(&mut self, w: u32, h: u32) {
        if let Some(mut write) = self.ch.begin_frame(w, h) {
            // Write real bytes so the slot genuinely holds half a picture rather than zeros —
            // a detector that only ever met zeroed wreckage would be a detector nobody had
            // pointed at anything.
            let stride = write.row_stride() as usize;
            let px = write.pixels_mut();
            for y in 0..h / 2 {
                for x in 0..w {
                    let o = y as usize * stride + x as usize * 4;
                    px[o..o + 4].copy_from_slice(&[0xFF, 0x00, 0xFF, 0xFF]);
                }
            }
            // Falling out of scope publishes nothing: [`crate::FrameWrite`] has **no `Drop`
            // impl at all**, deliberately. There is nothing to tidy — the slot's counter is
            // left odd and `latest_slot` still names the previous whole frame, which is
            // exactly the state a killed process leaves behind, and a `Drop` that "cleaned
            // up" would be simulating a courtesy a dead process cannot perform.
        }
    }

    /// 🚨 Keep ticking and draw nothing — **alive and silent**, the state that would otherwise
    /// read as healthy. `say_so` chooses whether the producer is well-behaved enough to name
    /// its refusal, because the console has to reach the same verdict either way.
    pub fn refuse(&mut self, say_so: Option<crate::wire::RefusalReason>) {
        self.refusing = true;
        if let Some(reason) = say_so {
            self.ch.refuse(reason);
        }
    }

    /// Draw again after a refusal.
    pub fn resume(&mut self) {
        self.refusing = false;
    }

    /// Stop looping. No farewell — the shape of a crash, a hang, or a process killed from
    /// outside.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// Stop, having said goodbye.
    pub fn depart(&mut self) {
        self.ch.depart();
        self.stopped = true;
    }

    pub fn set_state(&mut self, state: ProducerState) {
        self.ch.set_state(state);
    }
}

/// Is every pixel of `view` from the same frame, at the size the header claims?
///
/// ⚠️ **All of this function's power comes from [`pixel`] depending on `frame_index`**, and
/// nothing in the comparison can see that: `draw` writes `pixel(…)` and this checks against
/// `pixel(…)`, so both sides move together. If the frame index ever stopped reaching the
/// output, `verify` would go on passing — **on torn frames included**, which is the one thing
/// it exists to catch. `tests::the_picture_is_frame_keyed_or_verify_proves_nothing` pins the
/// property from outside the pair.
///
/// Returns the offending pixel's coordinates on the first disagreement, which is what turns a
/// failing assertion into a diagnosis.
pub fn verify(view: &FrameView<'_>) -> Result<(), String> {
    if view.format != PixelFormat::Rgba8UnormSrgb {
        return Err(format!("the simulator only draws RGBA8; this frame says {:?}", view.format));
    }
    for y in 0..view.height {
        let row = view.row(y);
        for x in 0..view.width {
            let got = &row[x as usize * 4..x as usize * 4 + 4];
            let want = pixel(x, y, view.frame_index);
            if got != want {
                return Err(format!(
                    "frame {} disagrees with itself at ({x}, {y}): {got:?} is not {want:?} — a \
                     blue channel of {} would belong to frame {}",
                    view.frame_index, got[2], got[2]
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🚨 **The premise [`verify`] rests on, stated from outside the pair it compares.**
    ///
    /// `draw` writes `pixel(x, y, index)` and `verify` checks against `pixel(x, y, index)`.
    /// That agreement is by construction and survives any change to `pixel` — including one
    /// that stopped the frame index reaching the output, after which `verify` would pass on a
    /// **torn** frame, which is the single thing the whole tear-detection suite is evidence
    /// for. ✏️ Third face of tonight's class, reported by the Ascent session: *a test that
    /// derives both sides of a comparison from the same function cannot see that function
    /// change.*
    #[test]
    fn the_picture_is_frame_keyed_or_verify_proves_nothing() {
        // Two frames must differ at every pixel, or a row-wise blend of them is undetectable.
        for (x, y) in [(0, 0), (1, 0), (0, 1), (37, 61), (255, 255), (256, 256)] {
            assert_ne!(
                pixel(x, y, 1),
                pixel(x, y, 2),
                "({x}, {y}) looks the same in frames 1 and 2, so a tear between them is invisible"
            );
        }
        // And it must vary across the picture, or a frame of one flat colour would verify.
        assert_ne!(pixel(0, 0, 1), pixel(1, 0, 1));
        assert_ne!(pixel(0, 0, 1), pixel(0, 1, 1));
        // The channel the frame index rides in is the one `verify`'s diagnostic names.
        assert_eq!(pixel(9, 9, 7)[2], 7, "the blue channel is what a torn frame disagrees in");
    }
}
