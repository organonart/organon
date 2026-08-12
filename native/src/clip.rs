//! MIDI-clip bridge. Every parameter is assigned a CC number; a preset can be
//! written as a Standard MIDI File containing a burst of those CCs at tick 0,
//! and the plugin reads incoming CCs to drive the visual (since a plugin can't
//! set its own host parameters, the CC overrides flow straight into the
//! shared-memory snapshot the visual reads).

use crate::ipc::Shared;

/// First CC number. Params occupy `CC_BASE .. CC_BASE + N`. Starts at 16 to
/// dodge the common controllers (mod/volume/pan/expression/sustain).
pub const CC_BASE: u8 = 16;
pub const N: usize = 32;

/// (min, max) for each parameter slot, in the canonical order below. Ranges are
/// the ones `params.rs` declares, and `taper_round_trips_against_the_engine_range`
/// (in `cli.rs`) pins them there — it reads each bound off the real param object
/// and fails the build on a disagreement, which is what this table went without
/// while `trans_amp` sat at ten times its true maximum. `rot_mod` is per-axis
/// rotation SPEED; slot 16 is the global speed (`loop_step` and per-axis
/// `angle_inc` were removed).
///
/// Two slots are **exempt from that pin, on purpose**, and the test names both:
///
/// - **16, effective global speed.** No parameter backs it. It carries
///   `rot_mod[3]`, which `param_table.rs` packs as the *expression*
///   `inc_scale × 10^speed_exp`. The product spans 0..1, but its default is
///   0.01 and anything past ~0.1 is a blur, so a full-span CC would spend most
///   of its 127 steps past usable — `(0, 0.1)` is a deliberate playable range
///   for this lane, not a copy of `inc_scale`'s 0..1 dial. The cost is that a
///   snapshot above 0.1 exports pinned at CC 127 and re-imports at 0.1.
/// - **26, reserved.** The Pulse Depth knob was removed; `to_shared` hard-codes
///   `pulse_depth: 0.0` and there is no param left to check against.
pub(crate) const RANGES: [(f32, f32); N] = [
    (1.0, 128.0), (1.0, 128.0), (1.0, 128.0), (0.0, 256.0), // loop_count x,y,z,q
    (0.0, 2160.0), (0.0, 2160.0), (0.0, 2160.0),            // rot_amp
    (-2.0, 2.0), (-2.0, 2.0), (-2.0, 2.0),                  // rot_mod (rotation speed)
    (0.0, 20.0), (0.0, 20.0), (0.0, 20.0),                  // trans_amp
    (-200.0, 200.0), (-200.0, 200.0), (-200.0, 200.0),      // trans_mod
    (0.0, 0.1),                                             // effective global speed (playable range — see above)
    (0.0, 0.5),                                             // scale_amp
    (0.0, 3.0), (0.0, 6.0), (0.0, 3.0),                     // ambient, key, fill
    (-90.0, 90.0), (-180.0, 180.0),                         // elevation, azimuth
    (0.0, 2.0), (0.0, 1.0),                                 // glow, opacity
    (40.0, 240.0), (0.0, 3.0),                              // tempo, pulse_depth (slot 26 reserved/inert)
    (0.0, 6.0), (0.0, 6.0), (0.0, 6.0),                     // rot/trans/scale func (sin..saw)
    (0.0, 1.0), (0.0, 1.0),                                 // animate, pulse
];

/// Map an incoming CC number to a parameter slot, if it's one of ours.
pub fn cc_index(cc: u8) -> Option<usize> {
    let lo = CC_BASE as usize;
    let i = cc as usize;
    (i >= lo && i < lo + N).then_some(i - lo)
}

/// Read slot `i`'s plain value out of the snapshot.
fn get(s: &Shared, i: usize) -> f32 {
    match i {
        0..=3 => s.loop_count[i],
        4..=6 => s.rot_amp[i - 4],
        7..=9 => s.rot_mod[i - 7],
        10..=12 => s.trans_amp[i - 10],
        13..=15 => s.trans_mod[i - 13],
        16 => s.rot_mod[3], // inc_scale
        17 => s.scale_amp,
        18..=24 => s.lighting[i - 18],
        25 => s.tempo,
        26 => s.pulse_depth,
        27 => s.rot_func as f32,
        28 => s.trans_func as f32,
        29 => s.scale_func as f32,
        30 => s.animate as f32,
        31 => s.pulse as f32,
        _ => 0.0,
    }
}

/// Write slot `i`'s plain value into the snapshot.
fn set(s: &mut Shared, i: usize, v: f32) {
    match i {
        0..=3 => s.loop_count[i] = v,
        4..=6 => s.rot_amp[i - 4] = v,
        7..=9 => s.rot_mod[i - 7] = v,
        10..=12 => s.trans_amp[i - 10] = v,
        13..=15 => s.trans_mod[i - 13] = v,
        16 => s.rot_mod[3] = v, // inc_scale
        17 => s.scale_amp = v,
        18..=24 => s.lighting[i - 18] = v,
        25 => s.tempo = v,
        26 => s.pulse_depth = v,
        27 => s.rot_func = v.round().clamp(0.0, 6.0) as u32,
        28 => s.trans_func = v.round().clamp(0.0, 6.0) as u32,
        29 => s.scale_func = v.round().clamp(0.0, 6.0) as u32,
        30 => s.animate = (v >= 0.5) as u32,
        31 => s.pulse = (v >= 0.5) as u32,
        _ => {}
    }
}

/// Apply a normalized CC value (0..1) to slot `i` of the snapshot.
pub fn apply_normalized(s: &mut Shared, i: usize, norm: f32) {
    let (lo, hi) = RANGES[i];
    set(s, i, lo + norm.clamp(0.0, 1.0) * (hi - lo));
}

/// Slot `i`'s current value as a 0..1 normalized number. Used to detect when the
/// user has moved a slider (so a clip's CC override can be released).
pub fn normalized(s: &Shared, i: usize) -> f32 {
    let (lo, hi) = RANGES[i];
    if hi > lo {
        ((get(s, i) - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The CC burst (controller, value) representing a snapshot's full state.
fn cc_burst(s: &Shared) -> Vec<(u8, u8)> {
    (0..N)
        .map(|i| {
            let (lo, hi) = RANGES[i];
            let norm = if hi > lo { (get(s, i) - lo) / (hi - lo) } else { 0.0 };
            let val = (norm.clamp(0.0, 1.0) * 127.0).round() as u8;
            (CC_BASE + i as u8, val)
        })
        .collect()
}

/// Write a single-track Standard MIDI File: all parameter CCs at tick 0, then a
/// short sustained note so the clip has visible length in the DAW.
/// (The per-preset `.mid` export button was removed from the presets rail —
/// unused feature; kept in case clip export returns.)
#[allow(dead_code)]
pub fn write_midi(s: &Shared, path: &std::path::Path) -> std::io::Result<()> {
    use midly::{
        num::{u4, u7},
        Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    };

    let mut track: Vec<TrackEvent> = Vec::with_capacity(N + 3);
    for (cc, val) in cc_burst(s) {
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: u4::new(0),
                message: MidiMessage::Controller {
                    controller: u7::new(cc & 0x7f),
                    value: u7::new(val & 0x7f),
                },
            },
        });
    }
    // A one-beat C4 note so the clip isn't zero-length.
    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Midi {
            channel: u4::new(0),
            message: MidiMessage::NoteOn { key: u7::new(60), vel: u7::new(100) },
        },
    });
    track.push(TrackEvent {
        delta: 480.into(),
        kind: TrackEventKind::Midi {
            channel: u4::new(0),
            message: MidiMessage::NoteOff { key: u7::new(60), vel: u7::new(0) },
        },
    });
    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let smf = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(480.into())),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    smf.write(&mut bytes).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}
