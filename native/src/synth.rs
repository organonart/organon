//! #339 Duo-Field synthesis — Tier 1: field probes (virtual microphones).
//!
//! The plugin's `process()` is a stereo pass-through that never wrote its output
//! buffer. This module *finally writes into it*: place stereo listener probes in
//! the radiating acoustic field the visual already renders, evaluate the field's
//! scalar **pressure** at each probe every sample (the SAME `math::acoustic_field_pu`
//! kernel the visual uses — never a fork), and sum the result into the passthrough
//! behind a wet level. The field you see radiating on screen becomes the field you
//! hear.
//!
//! Two play modes, present from this first tier:
//! - **Generative** — the generator's own multipole sources radiate; the probes
//!   hear them. Self-contained: no MIDI, no input.
//! - **Instrument** — each held MIDI note spawns a **voice = a radiating source**
//!   (note → wavenumber through the dispersion relation `ω = c·k`), heard through
//!   the same probes with an ADSR drive envelope. A chord is several interfering
//!   radiators mixed by the wave equation itself (superposition, not a summing bus).
//! - **Duet** — both at once (voices are just extra sources on the bed).
//!
//! Everything here is allocation-free and lock-free (audio-thread rule): all state
//! is preallocated, `render` only reads params + writes samples. `synth_on = off`
//! → the passthrough is byte-identical (an offline test pins this).

use crate::math::{self, cavity_field_pu, maxwell_axial_e, note_freq, MaxSource};
use glam::Vec3;

/// Fixed voice count — a preallocated `[Voice; N_VOICES]`, no `Vec` on the audio
/// thread. Chords beyond this steal the quietest-in-release voice.
pub const N_VOICES: usize = 8;

/// Per-voice float count published into `Shared.voices` (append-only; the spare
/// tail slots are reserved for MPE per-note expression). 8 voices × 8 = 64.
pub const VOICE_STRIDE: usize = 8;
/// Total floats in the published `Shared.voices` block.
pub const VOICES_LEN: usize = N_VOICES * VOICE_STRIDE;

/// Max radiating sources evaluated per probe per sample (generative multipole +
/// one monopole per voice). Preallocated scratch.
const MAX_SOURCES: usize = N_VOICES + 8;

/// Max oscillators in the Tier 2 lattice bank (preallocated).
pub const MAX_BANK: usize = 64;

/// #339 Tier 4 — scanned-geometry wavetable size (a closed shell ring, seam-free).
pub const WT_SIZE: usize = 256;
/// #339 Tier 4 — granular grain pool + field-probe cloud sizes (preallocated).
pub const MAX_GRAINS: usize = 48;
pub const N_CLOUD: usize = 32;

/// One granular grain (#339 Tier 4) — a windowed sine burst spawned from a field
/// probe. Preallocated; `active` gates it.
#[derive(Clone, Copy)]
struct Grain {
    active: bool,
    phase: f32,   // carrier phase (rad)
    freq: f32,    // Hz
    pos: f32,     // window position 0..1
    pos_inc: f32, // per-sample window advance (1 / grain length)
    amp: f32,
    pan: f32, // −1..1
}

impl Grain {
    const fn silent() -> Self {
        Grain { active: false, phase: 0.0, freq: 0.0, pos: 0.0, pos_inc: 0.0, amp: 0.0, pan: 0.0 }
    }
}

/// The time-lens "speed of sound" (world units per second). World units are read
/// as ≈ metres, so a probe pair a few centimetres apart yields a physically real
/// interaural delay (`Δr / C_SOUND`) and honest Doppler falls out of `t − r/c`.
pub const C_SOUND: f32 = 343.0;

/// Hard output ceiling the soft-knee limiter enforces so no field singularity can
/// ever slam the master (the `1/r` blow-up is a speaker-killer, not just a visual
/// instability). Slightly below full scale.
const LIMIT_CEIL: f32 = 0.95;

/// One-pole coefficient de-zippering each lattice oscillator's amplitude toward its
/// node's (control-rate) energy (~5 ms at 48 kHz).
const LAT_AMP_SMOOTH: f32 = 0.004;
/// Cap on a single lattice oscillator's amplitude so a node near a source
/// singularity can't dominate the bank (the audio near-field cap).
const LAT_AMP_CAP: f32 = 4.0;
/// Control-rate stride (samples) for re-sampling the field energy at each lattice
/// node — so the shell-breathe tracks `sn_shell_rate` smoothly, independent of the
/// host buffer size, without a per-sample field eval for every node (#339 Tier 2).
const LAT_DECIM: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    note: u8,
    vel: f32,
    /// Target frequency (Hz) from note + bend; `freq_cur` slews toward it (glide).
    freq: f32,
    freq_cur: f32,
    /// Per-voice audio carrier phase (rad) — each voice its own clock, so a chord
    /// is genuine superposition at different frequencies.
    phase: f32,
    /// ADSR level, 0..1.
    env: f32,
    /// Monotone age stamp for deterministic voice-stealing.
    age: u64,
}

impl Voice {
    const fn silent() -> Self {
        Voice {
            stage: Stage::Idle,
            note: 0,
            vel: 0.0,
            freq: 0.0,
            freq_cur: 0.0,
            phase: 0.0,
            env: 0.0,
            age: 0,
        }
    }
}

/// The per-block synthesis configuration, built from params in `process()`. Plain
/// data — no borrows of `self.params` reach the render loop.
#[derive(Clone, Copy)]
pub struct SynthConfig {
    pub on: bool,
    /// 0 = Generative, 1 = Instrument, 2 = Duet.
    pub play_mode: u32,
    /// Linear master gain (from dB) × wet level.
    pub level: f32,
    /// Acoustic multipole order for the generative bed (`math::AcousticKind`).
    pub source_kind: u32,
    /// When set, the bed follows the **Maxwell** field (an axial-E signed tone,
    /// `math::maxwell_axial_e`) instead of the acoustic pressure kernel — so the
    /// Maxwell generator's own sliders drive the sound.
    pub maxwell: bool,
    /// When set (Acoustic generator in **Cavity** model, #325 Tier 4), the bed
    /// sonifies the rectangular standing-wave eigenmode (`math::cavity_field_pu`)
    /// — the Chladni figure you see IS the tone you hear (#339 Tier 3 seed).
    pub cavity: bool,
    /// Cavity mode numbers `(nx, ny, nz)` and box half-extents `dims` (used when
    /// `cavity`). Pitch already derives from these via `gen_freq`.
    pub cav_modes: Vec3,
    pub cav_dims: Vec3,
    /// Maxwell layout: number of collinear sources (radiating dipoles for audio).
    pub src_count: u32,
    /// Generative base frequency (Hz) — the drone pitch. When following a field
    /// generator this is derived from its wavenumber `k` (dispersion ω = c·k).
    pub gen_freq: f32,
    /// Generative bed amplitude (0 mutes the bed; Instrument mode with a silent
    /// bed still sounds through voices).
    pub gen_amp: f32,
    /// Multipole extent (world units) — sets interference / beating structure.
    pub separation: f32,
    /// Near-field velocity weight passed to the kernel (kept small = far-field).
    pub near: f32,
    /// Near-source radius clamp so `1/r` stays finite.
    pub r_min: f32,
    /// The two listener probes (L, R). Spacing → interaural time difference.
    pub probe_l: Vec3,
    pub probe_r: Vec3,
    /// Concert-A reference (Hz) for equal temperament.
    pub a4: f32,
    /// Current pitch-bend in semitones (already scaled by the range dial).
    pub bend_semi: f32,
    /// Keyboard→X placement spread (0 = all voices stacked at the origin).
    pub place_spread: f32,
    /// ADSR times (seconds) + sustain level (0..1).
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    /// Portamento time (seconds) for the current-frequency slew.
    pub glide: f32,
    // --- visual-lens dials (published per voice for the picture) ---
    pub vis_pivot: f32,
    pub vis_anchor: f32,
    pub vis_slope: f32,
    pub vis_k_anchor: f32,
    pub vis_k_slope: f32,
    pub vis_quantize: u32,
    // --- #339 Tier 2: the oscillator lattice ---
    /// Synthesis mode: 0 = **Probes** (Tier 1 field mics), 1 = **Lattice** (Tier 2
    /// additive bank). Append-only.
    pub mode: u32,
    /// Number of oscillators in the bank (1..=`MAX_BANK`), each anchored to a shell node.
    pub bank_size: u32,
    /// Tuning layout (`math::lattice_freq`): 0 Octaves / 1 Harmonic / 2 Stretched / 3 Geometric.
    pub tuning_layout: u32,
    /// Tuning spread (octaves-per-step / geometric ratio).
    pub tune_spread: f32,
    /// Inharmonicity `B` for the Stretched layout.
    pub tune_stretch: f32,
    /// Radius of the sampling shell the lattice nodes sit on (world units).
    pub shell_r: f32,
    /// Field **breathing** rate (Hz) — the slow clock that modulates each
    /// oscillator's amplitude from its node's local energy (NOT the audio carrier;
    /// the carriers are the per-node tuned frequencies).
    pub shell_rate: f32,
    /// The generator's RAW wavenumber `k` (≈ the visual `ac_k`/`mx_k`, small) —
    /// sets the spatial nodal structure sampled on the shell (which nodes fall in a
    /// pressure null vs an antinode). Distinct from the audio pitch in `gen_freq`.
    pub field_k: f32,
    // --- #339 Tier 3: struck cavities (modal synthesis) ---
    /// Generative **mallet** strike strength this block (0 = no strike). Set by
    /// `process()` on a beat crossing or an input transient; the block's impulse
    /// into the modal bank. Note-on strikes come via `strike_modal` instead.
    pub mallet: f32,
    /// Modal bank output amplitude. Unlike `gen_amp` (the field bed, silenced off a
    /// field generator), modal is STRUCK — independent of the visual field — so it
    /// sounds on any generator and in Instrument mode.
    pub modal_amp: f32,
    /// Modal decay time (`-60 dB`, seconds) — the base ring length.
    pub t60: f32,
    /// Mallet **brightness** (0..1): how much high modes are excited (a hard vs soft
    /// mallet) and how much faster they decay (radiation efficiency → darker tail).
    pub bright: f32,
    // --- #339 Tier 4: granular aura + scanned-geometry wavetable ---
    /// A held note's frequency (Hz), or 0 if none — for Wavetable (playback rate)
    /// and Granular (grain-pitch centre). Mono.
    pub note_hz: f32,
    /// Grain length (seconds) for the granular aura.
    pub grain_size: f32,
    /// Grain density (0..1) — how thickly grains are scheduled (× the field flux).
    pub grain_density: f32,
}

impl Default for SynthConfig {
    fn default() -> Self {
        SynthConfig {
            on: false,
            play_mode: 0,
            level: 1.0,
            source_kind: 0,
            maxwell: false,
            cavity: false,
            cav_modes: Vec3::new(2.0, 2.0, 1.0),
            cav_dims: Vec3::splat(8.0),
            src_count: 2,
            gen_freq: 110.0,
            gen_amp: 1.0,
            separation: 0.6,
            near: 0.2,
            r_min: 0.15,
            probe_l: Vec3::new(-0.09, 0.0, 1.2),
            probe_r: Vec3::new(0.09, 0.0, 1.2),
            a4: 440.0,
            bend_semi: 0.0,
            place_spread: 0.0,
            attack: 0.01,
            decay: 0.2,
            sustain: 0.7,
            release: 0.4,
            glide: 0.0,
            vis_pivot: 110.0,
            vis_anchor: 0.5,
            vis_slope: 0.34,
            vis_k_anchor: 1.5,
            vis_k_slope: 0.34,
            vis_quantize: 0,
            mode: 0,
            bank_size: 32,
            tuning_layout: 0,
            tune_spread: 0.25,
            tune_stretch: 0.0,
            shell_r: 2.5,
            shell_rate: 1.0,
            field_k: 1.5,
            mallet: 0.0,
            modal_amp: 1.0,
            t60: 1.5,
            bright: 0.5,
            note_hz: 0.0,
            grain_size: 0.06,
            grain_density: 0.5,
        }
    }
}

/// The audio-thread synthesis engine. Preallocated once (in `Default`), configured
/// per block, rendered per sample. Owns the voice bank, the generative field clock,
/// a source scratch buffer, and the limiter state.
pub struct SynthEngine {
    sample_rate: f32,
    voices: [Voice; N_VOICES],
    age: u64,
    /// Generative field phase (rad) — one shared clock for the bed's multipole.
    gen_phase: f32,
    /// Preallocated source scratch (filled per sample, never allocates).
    src: [MaxSource; MAX_SOURCES],
    // --- #339 Tier 2 oscillator-lattice state (preallocated) ---
    /// Per-oscillator carrier phase (rad).
    osc_phase: [f32; MAX_BANK],
    /// Per-oscillator smoothed amplitude (de-zippered toward the node's energy).
    osc_amp: [f32; MAX_BANK],
    /// Slow field-breathe phase (rad) modulating the bank amplitudes.
    shell_phase: f32,
    // --- #339 Tier 3 modal-resonator state (preallocated) ---
    /// Two-pole resonator history per mode (`y[n-1]`, `y[n-2]`).
    mode_y1: [f32; MAX_BANK],
    mode_y2: [f32; MAX_BANK],
    /// The modal bank's current fundamental (Hz) — the strike sets it.
    modal_root: f32,
    /// Pending note strike (set by `strike_modal`, consumed at the next block):
    /// strength + the root frequency to retune to.
    pending_strike: f32,
    pending_root: f32,
    // --- #339 Tier 4 wavetable + granular state (preallocated) ---
    /// Scanned-geometry wavetable (a closed shell-ring cross-section) + read phase.
    wt_table: [f32; WT_SIZE],
    wt_phase: f32,
    /// Granular grain pool + the field-probe cloud (advected through the field), a
    /// grain-schedule accumulator, a deterministic spawn counter, and an init flag.
    grains: [Grain; MAX_GRAINS],
    cloud: [Vec3; N_CLOUD],
    cloud_energy: [f32; N_CLOUD],
    grain_accum: f32,
    spawn_ctr: u32,
    cloud_init: bool,
}

/// Stateless soft-knee limiter: linear below `knee` (= 0.6·ceil), then a `tanh`
/// shoulder that asymptotes to `LIMIT_CEIL`. `|out| < LIMIT_CEIL` unconditionally
/// (tanh < 1), so no field singularity can ever slam the master — the audio twin
/// of the visual near-field cap, and provably bounded (an offline test pins it).
fn soft_clip(x: f32) -> f32 {
    let knee = 0.6 * LIMIT_CEIL;
    let a = x.abs();
    if a <= knee {
        x
    } else {
        let range = LIMIT_CEIL - knee;
        (knee + range * ((a - knee) / range).tanh()).copysign(x)
    }
}

impl Default for SynthEngine {
    fn default() -> Self {
        SynthEngine {
            sample_rate: 48_000.0,
            voices: [Voice::silent(); N_VOICES],
            age: 0,
            gen_phase: 0.0,
            src: [MaxSource {
                pos: Vec3::ZERO,
                q: 0.0,
                phase: 0.0,
            }; MAX_SOURCES],
            osc_phase: [0.0; MAX_BANK],
            osc_amp: [0.0; MAX_BANK],
            shell_phase: 0.0,
            mode_y1: [0.0; MAX_BANK],
            mode_y2: [0.0; MAX_BANK],
            modal_root: 110.0,
            pending_strike: 0.0,
            pending_root: 110.0,
            wt_table: [0.0; WT_SIZE],
            wt_phase: 0.0,
            grains: [Grain::silent(); MAX_GRAINS],
            cloud: [Vec3::ZERO; N_CLOUD],
            cloud_energy: [0.0; N_CLOUD],
            grain_accum: 0.0,
            spawn_ctr: 0,
            cloud_init: false,
        }
    }
}

impl SynthEngine {
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(1.0);
    }

    /// Note-on: allocate (or steal) a voice, retune it, restart its envelope.
    /// `bend_semi` is baked into the target frequency; `a4` is the tuning reference.
    pub fn note_on(&mut self, note: u8, velocity: f32, bend_semi: f32, a4: f32) {
        let idx = self.pick_voice();
        self.age = self.age.wrapping_add(1);
        let freq = note_freq(note as f32, bend_semi, a4);
        let v = &mut self.voices[idx];
        // Legato glide: if a voice was already sounding, keep its phase + current
        // frequency so the pitch slides; a fresh voice starts at the target.
        if v.stage == Stage::Idle {
            v.freq_cur = freq;
            v.phase = 0.0;
        }
        v.stage = Stage::Attack;
        v.note = note;
        v.vel = velocity.clamp(0.0, 1.0);
        v.freq = freq;
        v.age = self.age;
    }

    /// Retune every sounding voice from its note + the current bend/reference —
    /// called once per block so pitch bend + tuning changes track continuously
    /// (portamento slews `freq_cur` toward the new target inside `render`).
    pub fn retune(&mut self, bend_semi: f32, a4: f32) {
        for v in self.voices.iter_mut() {
            if v.stage != Stage::Idle {
                v.freq = note_freq(v.note as f32, bend_semi, a4);
            }
        }
    }

    /// Strike the **modal bank** (#339 Tier 3) at `root` Hz with `velocity` — a
    /// note-on used as a mallet. Retunes the bank and queues an impulse consumed at
    /// the next block. Higher notes ⇒ higher root ⇒ (cavity pitch ∝ 1/size) a
    /// smaller visible cavity.
    pub fn strike_modal(&mut self, root: f32, velocity: f32) {
        self.pending_root = root.max(1.0);
        self.pending_strike = self.pending_strike.max(velocity.clamp(0.0, 1.0));
    }

    /// Note-off: move any matching sounding voice into its release stage.
    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut() {
            if v.note == note && v.stage != Stage::Idle && v.stage != Stage::Release {
                v.stage = Stage::Release;
            }
        }
    }

    /// All notes off (e.g. preset recall / panic) — release every voice and damp the
    /// modal bank so a held ring doesn't survive the reset.
    pub fn release_all(&mut self) {
        for v in self.voices.iter_mut() {
            if v.stage != Stage::Idle {
                v.stage = Stage::Release;
            }
        }
        self.mode_y1 = [0.0; MAX_BANK];
        self.mode_y2 = [0.0; MAX_BANK];
        self.pending_strike = 0.0;
    }

    /// Damp the modal ring + granular grains (+ drop a queued strike) WITHOUT touching
    /// held voices — called on a preset recall so a long tail doesn't bleed into the
    /// new patch, while held instrument notes survive (the #339 preset contract).
    pub fn damp_modal(&mut self) {
        self.mode_y1 = [0.0; MAX_BANK];
        self.mode_y2 = [0.0; MAX_BANK];
        self.pending_strike = 0.0;
        self.grains = [Grain::silent(); MAX_GRAINS];
        self.grain_accum = 0.0;
    }

    /// Test-only view of the scanned wavetable (to assert seam continuity).
    #[cfg(test)]
    pub fn wt_table(&self) -> &[f32] {
        &self.wt_table
    }

    /// Pick a free voice, else steal: prefer the quietest voice already in release,
    /// otherwise the quietest overall. Deterministic (ties break by lowest index).
    fn pick_voice(&self) -> usize {
        // 1) an idle slot
        if let Some(i) = self.voices.iter().position(|v| v.stage == Stage::Idle) {
            return i;
        }
        // 2) quietest-in-release
        let mut best: Option<(usize, f32)> = None;
        for (i, v) in self.voices.iter().enumerate() {
            if v.stage == Stage::Release {
                match best {
                    Some((_, e)) if v.env >= e => {}
                    _ => best = Some((i, v.env)),
                }
            }
        }
        if let Some((i, _)) = best {
            return i;
        }
        // 3) quietest overall
        let mut idx = 0;
        let mut lo = f32::INFINITY;
        for (i, v) in self.voices.iter().enumerate() {
            if v.env < lo {
                lo = v.env;
                idx = i;
            }
        }
        idx
    }

    /// The world position a voice radiates from (Keyboard-spread along X; stack at
    /// spread 0). Published so the picture shows where each note sits.
    fn voice_pos(note: u8, spread: f32) -> Vec3 {
        Vec3::new((note as f32 - 60.0) / 12.0 * spread, 0.0, 0.0)
    }

    /// Number of currently sounding voices (test/telemetry helper).
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.stage != Stage::Idle).count()
    }

    /// Advance one ADSR sample for a voice, returning its envelope level. Linear
    /// segments; times in seconds. Release completes exactly to Idle at 0.
    fn step_env(v: &mut Voice, cfg: &SynthConfig, sr: f32) -> f32 {
        let seg = |t: f32| 1.0 / (t.max(1.0e-4) * sr); // per-sample increment
        match v.stage {
            Stage::Idle => {
                v.env = 0.0;
            }
            Stage::Attack => {
                v.env += seg(cfg.attack);
                if v.env >= 1.0 {
                    v.env = 1.0;
                    v.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let s = cfg.sustain.clamp(0.0, 1.0);
                v.env -= seg(cfg.decay) * (1.0 - s);
                if v.env <= s {
                    v.env = s;
                    v.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                v.env = cfg.sustain.clamp(0.0, 1.0);
            }
            Stage::Release => {
                v.env -= seg(cfg.release);
                if v.env <= 0.0 {
                    v.env = 0.0;
                    v.stage = Stage::Idle;
                }
            }
        }
        v.env
    }

    /// Write signed monopoles for an acoustic multipole of `kind`, centred at
    /// `centre`, extent `sep`, strength `amp`, into `out[..]`; returns the count.
    /// Inlined (no `Vec`) sibling of `math::acoustic_sources` — same layout.
    fn write_multipole(out: &mut [MaxSource], kind: u32, centre: Vec3, sep: f32, amp: f32) -> usize {
        let h = 0.5 * sep.max(0.0);
        let mut n = 0;
        let mut push = |x: f32, y: f32, z: f32, q: f32| {
            if n < out.len() {
                out[n] = MaxSource {
                    pos: centre + Vec3::new(x, y, z),
                    q: q * amp,
                    phase: 0.0,
                };
                n += 1;
            }
        };
        match kind {
            1 => {
                // Dipole (axis Z)
                push(0.0, 0.0, h, 1.0);
                push(0.0, 0.0, -h, -1.0);
            }
            2 => {
                // Lateral quadrupole
                push(h, 0.0, h, 1.0);
                push(-h, 0.0, h, -1.0);
                push(h, 0.0, -h, -1.0);
                push(-h, 0.0, -h, 1.0);
            }
            3 => {
                // Longitudinal quadrupole
                push(0.0, 0.0, sep, 1.0);
                push(0.0, 0.0, 0.0, -2.0);
                push(0.0, 0.0, -sep, 1.0);
            }
            _ => {
                // Monopole
                push(0.0, 0.0, 0.0, 1.0);
            }
        }
        n
    }

    /// Write the **Maxwell** source layout (a collinear X-axis array of `count`
    /// radiating dipoles, extent `sep`, strength `amp`) into `out[..]`; returns the
    /// count. The allocation-free sibling of `math::maxwell_sources` (static layout,
    /// no swirl) — so the Maxwell generator's source count / separation drive the
    /// interference you hear.
    fn write_maxwell(out: &mut [MaxSource], count: u32, sep: f32, amp: f32) -> usize {
        let count = count.max(1).min(out.len() as u32) as usize;
        for i in 0..count {
            let x = (i as f32 - (count as f32 - 1.0) * 0.5) * sep;
            out[i] = MaxSource { pos: Vec3::new(x, 0.0, 0.0), q: amp, phase: 0.0 };
        }
        count
    }

    /// Build the field source layout for this generator into `self.src` (acoustic
    /// multipole / Maxwell array; cavity uses none); returns the count. (#339 Tier 4)
    fn build_field_sources(&mut self, cfg: &SynthConfig) -> usize {
        if cfg.cavity {
            0
        } else if cfg.maxwell {
            Self::write_maxwell(&mut self.src, cfg.src_count, cfg.separation, 1.0)
        } else {
            Self::write_multipole(&mut self.src, cfg.source_kind, Vec3::ZERO, cfg.separation, 1.0)
        }
    }

    /// (pressure, velocity) of the active field at `p`, dispatching on the generator
    /// model — the shared sampler for the Tier 4 wavetable scan + granular cloud.
    fn field_pu_at(srcs: &[MaxSource], cfg: &SynthConfig, p: Vec3, phase: f32) -> (f32, Vec3) {
        if cfg.cavity {
            cavity_field_pu(p, cfg.cav_modes, cfg.cav_dims, phase)
        } else if cfg.maxwell {
            let e = maxwell_axial_e(p, srcs, true, Vec3::Z, cfg.field_k, cfg.near, cfg.r_min, phase);
            (e, Vec3::ZERO)
        } else {
            math::acoustic_field_pu(p, srcs, cfg.field_k, cfg.near, cfg.r_min, phase)
        }
    }

    /// Render `n` stereo frames into `left`/`right`, summing the synth bus over the
    /// existing passthrough already present in the buffers. `synth_on = off` → the
    /// buffers are left byte-identical.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], cfg: &SynthConfig) {
        if !cfg.on {
            return;
        }
        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let two_pi = std::f32::consts::TAU;
        let n = left.len().min(right.len());

        let gen = cfg.play_mode == 0 || cfg.play_mode == 2;
        let inst = cfg.play_mode == 1 || cfg.play_mode == 2;

        // Frequency-slew coefficient (portamento). glide 0 → snap.
        let glide_coef = if cfg.glide > 1.0e-4 {
            (-dt / cfg.glide).exp()
        } else {
            0.0
        };

        // Generative field wavenumber (dispersion ω = c·k, so k = 2πf / c).
        let gen_k = (two_pi * cfg.gen_freq / C_SOUND).max(0.0);
        let gen_w = two_pi * cfg.gen_freq;

        // --- #339 Tier 2: precompute the oscillator-lattice bank ---
        // Each oscillator sits on a shell node; its carrier = the tuned frequency
        // and its pan = the node's X (both phase-independent, so computed once here).
        // Its amplitude = √(local field energy) at that node is re-sampled INSIDE the
        // loop every `LAT_DECIM` samples at the live shell-breathe phase (below), so
        // the breathe tracks `sn_shell_rate` smoothly regardless of host buffer size.
        let use_lattice = gen && cfg.mode == 1 && cfg.gen_amp > 0.0;
        let bank = if use_lattice {
            (cfg.bank_size.max(1) as usize).min(MAX_BANK)
        } else {
            0
        };
        let shell_r = cfg.shell_r.max(1.0e-3);
        let mut lat_freq = [0.0f32; MAX_BANK];
        let mut lat_amp = [0.0f32; MAX_BANK]; // target amplitude (re-sampled in-loop)
        let mut lat_pan = [0.0f32; MAX_BANK];
        let mut lat_node = [Vec3::ZERO; MAX_BANK];
        if use_lattice {
            for i in 0..bank {
                let node = math::fib_sphere_node(i, bank) * shell_r;
                lat_node[i] = node;
                // Clamp each carrier below Nyquist. Octave/geometric layouts grow
                // exponentially in `i` and can overflow f32 to +Inf (→ phase Inf →
                // sin = NaN → corrupted output); this also stops aliasing.
                let f = math::lattice_freq(cfg.tuning_layout, cfg.gen_freq, i, cfg.tune_spread, cfg.tune_stretch);
                lat_freq[i] = if f.is_finite() { f.clamp(0.0, sr * 0.49) } else { sr * 0.49 };
                lat_pan[i] = (node.x / shell_r).clamp(-1.0, 1.0);
            }
        }
        let shell_w = two_pi * cfg.shell_rate;
        // Power-normalise the bank so N oscillators don't sum to N× loudness.
        let bank_norm = if bank > 0 { 1.0 / (bank as f32).sqrt() } else { 1.0 };

        // --- #339 Tier 3: precompute the modal resonator bank (struck cavities) ---
        // A bank of damped two-pole resonators tuned to the cavity eigenmodes
        // (`rect_mode_ratios`), struck by the beat/transient mallet (generative) or a
        // note-on (`strike_modal`). One mode table, two outputs: the partials you
        // hear are the nodal pattern you watch reorganise.
        // Modal is STRUCK (not a field bed), so it runs whenever the engine is
        // Modal — independent of the generator / `gen_amp`, so Instrument note
        // strikes and non-field generators still sound.
        let use_modal = cfg.mode == 2 && (gen || inst);
        let modes = if use_modal {
            (cfg.bank_size.max(1) as usize).min(MAX_BANK)
        } else {
            0
        };
        let mut mode_a1 = [0.0f32; MAX_BANK];
        let mut mode_a2 = [0.0f32; MAX_BANK];
        let mut mode_exc = [0.0f32; MAX_BANK];
        let mut mode_pan = [0.0f32; MAX_BANK];
        let mut strike_now = 0.0f32;
        if use_modal {
            // Resolve this block's strike: a queued note strike retunes + wins;
            // else the generative beat/transient mallet strikes at `gen_freq`.
            if self.pending_strike > 0.0 {
                self.modal_root = self.pending_root.max(1.0);
                strike_now = self.pending_strike;
                self.pending_strike = 0.0;
            } else if cfg.mallet > 0.0 {
                self.modal_root = cfg.gen_freq.max(1.0);
                strike_now = cfg.mallet;
            }
            let mut ratios = [0.0f32; MAX_BANK];
            math::rect_mode_ratios(&mut ratios[..modes]);
            for m in 0..modes {
                // Ran out of distinct eigenmodes (bank_size > the ratio table): leave
                // the slot inert instead of running a near-DC resonator.
                if ratios[m] <= 0.0 {
                    mode_a1[m] = 0.0;
                    mode_a2[m] = 0.0;
                    mode_exc[m] = 0.0;
                    continue;
                }
                let f = (self.modal_root * ratios[m]).clamp(0.0, sr * 0.49);
                // Higher modes ring shorter with brightness (radiation efficiency).
                let t60m = cfg.t60 / (1.0 + cfg.bright * m as f32 * 0.5);
                let r = math::t60_to_r(t60m, sr);
                let w = two_pi * f / sr;
                mode_a1[m] = 2.0 * r * w.cos();
                mode_a2[m] = -(r * r);
                // Soft (bright 0, low modes only) → hard (bright 1, flat) mallet.
                // Scale the strike impulse by sin(ω): a two-pole resonator's impulse
                // response peaks at ≈1/sin(ω), so this normalises every mode's RING
                // to ≈ the strike level regardless of frequency or T60 (the old
                // `1−r` factor made long-decay rings ~−80 dB — inaudible).
                let roll = (-(1.0 - cfg.bright) * m as f32 * 0.5).exp();
                mode_exc[m] = roll * w.sin().max(1.0e-3);
                // Spread the partials across the stereo field for width.
                let frac = if modes > 1 { m as f32 / (modes - 1) as f32 } else { 0.5 };
                mode_pan[m] = (frac * 2.0 - 1.0) * 0.6;
            }
        }
        let modal_norm = if modes > 0 { 1.0 / (modes as f32).sqrt() } else { 1.0 };

        // --- #339 Tier 4: wavetable + granular precompute (control-rate) ---
        // Wavetable = the field's cross-section scanned around a closed shell ring
        // (seam-free) and played at f₀. Granular = a probe cloud advected through the
        // field, spraying windowed-sine grains whose pitch/pan/amp follow the flow.
        let use_wt = cfg.mode == 4 && (gen || inst);
        let use_gran = cfg.mode == 3 && (gen || inst);
        // Playback pitch / grain-pitch centre: a held note, else the generator's f₀.
        let base_hz = if cfg.note_hz > 0.0 { cfg.note_hz } else { cfg.gen_freq.max(1.0) };
        if use_wt || use_gran {
            // The geometry evolves on the slow shell-breathe clock, refilled per block.
            self.shell_phase += two_pi * cfg.shell_rate * dt * n as f32;
            if self.shell_phase > two_pi * 1024.0 {
                self.shell_phase -= two_pi * 1024.0;
            }
            let m = self.build_field_sources(cfg);
            let srcs = self.src; // Copy — free the borrow so we can mutate wt/cloud.
            let phase = self.shell_phase;
            let shell_r = cfg.shell_r.max(1.0e-3);

            if use_wt {
                // Scan the pressure around a closed ring → the wavetable; peak-normalise.
                let mut peak = 1.0e-6f32;
                for i in 0..WT_SIZE {
                    let p = math::ring_point(i, WT_SIZE, shell_r);
                    let v = Self::field_pu_at(&srcs[..m], cfg, p, phase).0;
                    self.wt_table[i] = v;
                    peak = peak.max(v.abs());
                }
                let g = 1.0 / peak;
                for v in self.wt_table.iter_mut() {
                    *v *= g;
                }
            }

            if use_gran {
                // Seed the cloud on the shell (Fibonacci), once.
                if !self.cloud_init {
                    for c in 0..N_CLOUD {
                        self.cloud[c] = math::fib_sphere_node(c, N_CLOUD) * shell_r;
                    }
                    self.cloud_init = true;
                }
                // Advect each probe along the field flow + record local energy.
                let step = shell_r * 0.03;
                for c in 0..N_CLOUD {
                    let (pres, u) = Self::field_pu_at(&srcs[..m], cfg, self.cloud[c], phase);
                    self.cloud_energy[c] = (pres * pres + u.length_squared()).min(8.0);
                    let flow = u.normalize_or_zero();
                    let mut p = self.cloud[c] + flow * step;
                    // Respawn a probe that drifts off the shell.
                    let r = p.length();
                    if r > shell_r * 2.5 || r < shell_r * 0.15 {
                        p = math::fib_sphere_node(self.spawn_ctr as usize % N_CLOUD, N_CLOUD) * shell_r;
                    }
                    self.cloud[c] = p;
                }
                // Schedule grains at a density-driven rate (× the field flux).
                let flux = self.cloud_energy.iter().sum::<f32>() / N_CLOUD as f32;
                let per_sec = cfg.grain_density.max(0.0) * (4.0 + 90.0 * flux);
                self.grain_accum += per_sec * (n as f32 / sr);
                while self.grain_accum >= 1.0 {
                    self.grain_accum -= 1.0;
                    let Some(gi) = self.grains.iter().position(|g| !g.active) else {
                        break; // pool full
                    };
                    let ci = self.spawn_ctr as usize % N_CLOUD;
                    self.spawn_ctr = self.spawn_ctr.wrapping_add(1);
                    let e = self.cloud_energy[ci];
                    let probe = self.cloud[ci];
                    self.grains[gi] = Grain {
                        active: true,
                        phase: 0.0,
                        freq: (base_hz * (0.5 + e.min(2.0))).clamp(20.0, sr * 0.49),
                        pos: 0.0,
                        pos_inc: 1.0 / (cfg.grain_size.max(0.002) * sr),
                        amp: e.sqrt().min(1.0),
                        pan: (probe.x / shell_r).clamp(-1.0, 1.0),
                    };
                }
            }
        }
        let grain_norm = 1.0 / (MAX_GRAINS as f32).sqrt();

        for i in 0..n {
            let mut sl = 0.0f32;
            let mut sr_out = 0.0f32;

            // --- Tier 2 oscillator lattice: the breathing additive bank ---
            if use_lattice {
                self.shell_phase += shell_w * dt;
                if self.shell_phase > two_pi * 1024.0 {
                    self.shell_phase -= two_pi * 1024.0;
                }
                // Re-sample each node's field energy at the LIVE shell phase every
                // `LAT_DECIM` samples (control rate), so the breathe tracks
                // `sn_shell_rate` smoothly, not the host buffer boundary. The source
                // scratch is rebuilt here (the voice loop below reuses `self.src[0]`).
                if i % LAT_DECIM == 0 {
                    let m = if cfg.cavity {
                        0
                    } else if cfg.maxwell {
                        Self::write_maxwell(&mut self.src, cfg.src_count, cfg.separation, 1.0)
                    } else {
                        Self::write_multipole(&mut self.src, cfg.source_kind, Vec3::ZERO, cfg.separation, 1.0)
                    };
                    for b in 0..bank {
                        let node = lat_node[b];
                        let energy = if cfg.cavity {
                            math::acoustic_cavity_energy(node, cfg.cav_modes, cfg.cav_dims, 0.0, self.shell_phase)
                        } else if cfg.maxwell {
                            let e = maxwell_axial_e(node, &self.src[..m], true, Vec3::Z, cfg.field_k, cfg.near, cfg.r_min, self.shell_phase);
                            e * e
                        } else {
                            math::acoustic_energy_density(node, &self.src[..m], 0.0, cfg.field_k, cfg.near, cfg.r_min, self.shell_phase)
                        };
                        lat_amp[b] = energy.max(0.0).sqrt().min(LAT_AMP_CAP);
                    }
                }
                let mut la = 0.0f32;
                let mut ra = 0.0f32;
                for b in 0..bank {
                    self.osc_phase[b] += two_pi * lat_freq[b] * dt;
                    if self.osc_phase[b] > two_pi * 1024.0 {
                        self.osc_phase[b] -= two_pi * 1024.0;
                    }
                    // De-zipper the amplitude toward the node's energy.
                    self.osc_amp[b] += (lat_amp[b] - self.osc_amp[b]) * LAT_AMP_SMOOTH;
                    let s = self.osc_phase[b].sin() * self.osc_amp[b];
                    // Equal-power pan by the node's X.
                    let p = (lat_pan[b] * 0.5 + 0.5).clamp(0.0, 1.0);
                    la += s * (1.0 - p).sqrt();
                    ra += s * p.sqrt();
                }
                sl += la * bank_norm * cfg.gen_amp;
                sr_out += ra * bank_norm * cfg.gen_amp;
            }

            // --- Tier 3 modal bank: struck resonators ring their eigenmodes ---
            if use_modal {
                // The mallet impulse lands on the first sample of the struck block.
                let x0 = if i == 0 { strike_now } else { 0.0 };
                let mut ml = 0.0f32;
                let mut mr = 0.0f32;
                for m in 0..modes {
                    let x = x0 * mode_exc[m];
                    let y = mode_a1[m] * self.mode_y1[m] + mode_a2[m] * self.mode_y2[m] + x;
                    self.mode_y2[m] = self.mode_y1[m];
                    self.mode_y1[m] = y;
                    let p = (mode_pan[m] * 0.5 + 0.5).clamp(0.0, 1.0);
                    ml += y * (1.0 - p).sqrt();
                    mr += y * p.sqrt();
                }
                sl += ml * modal_norm * cfg.modal_amp;
                sr_out += mr * modal_norm * cfg.modal_amp;
            }

            // --- Tier 4 wavetable: play the scanned shell cross-section at f₀ ---
            if use_wt {
                self.wt_phase += base_hz * dt;
                if self.wt_phase >= 1.0 {
                    self.wt_phase -= self.wt_phase.floor();
                }
                let x = self.wt_phase * WT_SIZE as f32;
                let i0 = (x as usize) % WT_SIZE;
                let i1 = (i0 + 1) % WT_SIZE;
                let frac = x - x.floor();
                let s = self.wt_table[i0] * (1.0 - frac) + self.wt_table[i1] * frac;
                sl += s * cfg.gen_amp;
                sr_out += s * cfg.gen_amp;
            }

            // --- Tier 4 granular: sum the active windowed-sine grains ---
            if use_gran {
                let mut gl = 0.0f32;
                let mut gr = 0.0f32;
                for g in self.grains.iter_mut() {
                    if !g.active {
                        continue;
                    }
                    let s = g.phase.sin() * math::hann_window(g.pos) * g.amp;
                    g.phase += two_pi * g.freq * dt;
                    if g.phase > two_pi * 1024.0 {
                        g.phase -= two_pi * 1024.0;
                    }
                    g.pos += g.pos_inc;
                    if g.pos >= 1.0 {
                        g.active = false;
                    }
                    let p = (g.pan * 0.5 + 0.5).clamp(0.0, 1.0);
                    gl += s * (1.0 - p).sqrt();
                    gr += s * p.sqrt();
                }
                sl += gl * grain_norm * cfg.gen_amp;
                sr_out += gr * grain_norm * cfg.gen_amp;
            }

            // --- generative bed (Probes mode): sonify the SAME field the visual renders ---
            if gen && cfg.mode == 0 && cfg.gen_amp > 0.0 {
                self.gen_phase += gen_w * dt;
                if self.gen_phase > two_pi * 1024.0 {
                    self.gen_phase -= two_pi * 1024.0;
                }
                if cfg.cavity {
                    // Acoustic Cavity model (#325 Tier 4): the rectangular standing
                    // wave — the Chladni pattern you see is the tone you hear. Bounded
                    // (no 1/r), so amplitude is stable; probes moving through a nodal
                    // plane fall silent, antinodes bloom.
                    let (pl, _) = cavity_field_pu(cfg.probe_l, cfg.cav_modes, cfg.cav_dims, self.gen_phase);
                    let (pr, _) = cavity_field_pu(cfg.probe_r, cfg.cav_modes, cfg.cav_dims, self.gen_phase);
                    sl += pl * cfg.gen_amp;
                    sr_out += pr * cfg.gen_amp;
                } else if cfg.maxwell {
                    // Maxwell generator: the axial E of its radiating source array
                    // (signed → a clean tone), so mx_sources / mx_separation / mx_k
                    // shape the sound.
                    let m = Self::write_maxwell(&mut self.src, cfg.src_count, cfg.separation, cfg.gen_amp);
                    let srcs = &self.src[..m];
                    sl += maxwell_axial_e(cfg.probe_l, srcs, true, Vec3::Z, gen_k, cfg.near, cfg.r_min, self.gen_phase);
                    sr_out += maxwell_axial_e(cfg.probe_r, srcs, true, Vec3::Z, gen_k, cfg.near, cfg.r_min, self.gen_phase);
                } else {
                    // Acoustic generator (or manual bed): its multipole pressure.
                    let m = Self::write_multipole(
                        &mut self.src,
                        cfg.source_kind,
                        Vec3::ZERO,
                        cfg.separation,
                        cfg.gen_amp,
                    );
                    let srcs = &self.src[..m];
                    let (pl, _) =
                        math::acoustic_field_pu(cfg.probe_l, srcs, gen_k, cfg.near, cfg.r_min, self.gen_phase);
                    let (pr, _) =
                        math::acoustic_field_pu(cfg.probe_r, srcs, gen_k, cfg.near, cfg.r_min, self.gen_phase);
                    sl += pl;
                    sr_out += pr;
                }
            }

            // --- instrument voices: each note a radiating monopole ---
            if inst {
                for vi in 0..N_VOICES {
                    // Copy the small fields we need, then drop the borrow before
                    // touching the shared source scratch (no aliasing).
                    let (stage, freq_target) = {
                        let v = &self.voices[vi];
                        (v.stage, v.freq)
                    };
                    if stage == Stage::Idle {
                        continue;
                    }
                    // Slew frequency + advance envelope + phase.
                    let (env, freq_cur, phase, note) = {
                        let v = &mut self.voices[vi];
                        v.freq_cur = if glide_coef > 0.0 {
                            freq_target + (v.freq_cur - freq_target) * glide_coef
                        } else {
                            freq_target
                        };
                        let env = Self::step_env(v, cfg, sr);
                        let w = two_pi * v.freq_cur;
                        v.phase += w * dt;
                        if v.phase > two_pi * 1024.0 {
                            v.phase -= two_pi * 1024.0;
                        }
                        (env, v.freq_cur, v.phase, v.note)
                    };
                    if env <= 0.0 {
                        continue;
                    }
                    let vk = (two_pi * freq_cur / C_SOUND).max(0.0);
                    let pos = Self::voice_pos(note, cfg.place_spread);
                    let amp = env * self.voices[vi].vel;
                    // A single radiating monopole per voice (velocity-scaled).
                    self.src[0] = MaxSource { pos, q: amp, phase: 0.0 };
                    let srcs = &self.src[..1];
                    let (pl, _) =
                        math::acoustic_field_pu(cfg.probe_l, srcs, vk, cfg.near, cfg.r_min, phase);
                    let (pr, _) =
                        math::acoustic_field_pu(cfg.probe_r, srcs, vk, cfg.near, cfg.r_min, phase);
                    sl += pl;
                    sr_out += pr;
                }
            }

            // Master level, then the soft-knee limiter (last-in-chain, non-optional).
            left[i] += soft_clip(sl * cfg.level);
            right[i] += soft_clip(sr_out * cfg.level);
        }
    }

    /// Publish the current voice bank into `Shared.voices` (per voice: gate, lensed
    /// visual wavenumber, lensed visual rate, drive, x, y, z, reserved-for-MPE), so
    /// the visual can append each note as a radiating shell it draws at a rate the
    /// eye enjoys. Called once per block (control-rate) — allocation-free.
    pub fn write_voices(&self, out: &mut [f32; VOICES_LEN], cfg: &SynthConfig) {
        for vi in 0..N_VOICES {
            let v = &self.voices[vi];
            let base = vi * VOICE_STRIDE;
            let sounding = v.stage != Stage::Idle;
            let f = if sounding { v.freq_cur.max(1.0) } else { 0.0 };
            let (rate_vis, k_vis) = if sounding {
                math::visual_lens(
                    f,
                    cfg.vis_pivot,
                    cfg.vis_anchor,
                    cfg.vis_slope,
                    cfg.vis_k_anchor,
                    cfg.vis_k_slope,
                    cfg.vis_quantize,
                )
            } else {
                (0.0, 0.0)
            };
            let pos = Self::voice_pos(v.note, cfg.place_spread);
            out[base] = if sounding { 1.0 } else { 0.0 };
            out[base + 1] = k_vis;
            out[base + 2] = rate_vis;
            out[base + 3] = v.env * v.vel;
            out[base + 4] = pos.x;
            out[base + 5] = pos.y;
            out[base + 6] = pos.z;
            out[base + 7] = 0.0; // reserved (MPE per-note expression)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_gen(freq: f32) -> SynthConfig {
        SynthConfig {
            on: true,
            play_mode: 0,
            gen_freq: freq,
            gen_amp: 1.0,
            source_kind: 0, // monopole
            level: 0.5,
            ..SynthConfig::default()
        }
    }

    /// Goertzel single-bin power at `freq` over `x` sampled at `sr`.
    fn goertzel(x: &[f32], freq: f32, sr: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / sr;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &v in x {
            let s0 = v + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0)
    }

    fn cfg_lattice(f0: f32, bank: u32, layout: u32, spread: f32) -> SynthConfig {
        SynthConfig {
            on: true,
            play_mode: 0,
            mode: 1, // Lattice
            gen_freq: f0,
            gen_amp: 1.0,
            level: 0.5,
            bank_size: bank,
            tuning_layout: layout,
            tune_spread: spread,
            source_kind: 0, // monopole → isotropic energy on the shell
            field_k: 1.0,
            shell_r: 2.0,
            shell_rate: 1.0,
            near: 0.0,
            r_min: 0.15,
            ..SynthConfig::default()
        }
    }

    #[test]
    fn lattice_octave_layout_has_the_expected_peaks() {
        // 4 oscillators, octave spread (×2 per step) → carriers at 110/220/440/880.
        let mut eng = SynthEngine::default();
        let cfg = cfg_lattice(110.0, 4, 0, 1.0);
        let (l, _) = render_mono(&mut eng, &cfg, 48_000);
        let peak = |f| goertzel(&l, f, 48_000.0);
        for &f in &[110.0, 220.0, 440.0, 880.0] {
            assert!(peak(f) > 5.0 * peak(f * 1.5).max(1e-9), "octave carrier present at {f} Hz");
        }
    }

    #[test]
    fn lattice_output_is_bounded_under_pathological_params() {
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_lattice(90.0, MAX_BANK as u32, 0, 0.25);
        cfg.level = 8.0; // hot
        cfg.gen_amp = 4.0;
        cfg.shell_r = 0.05; // nodes crammed near the source singularity
        cfg.r_min = 0.02;
        let (l, r) = render_mono(&mut eng, &cfg, 24_000);
        let pk = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(pk <= LIMIT_CEIL + 1e-3, "lattice bounded by the limiter (peak {pk})");
    }

    #[test]
    fn lattice_carriers_never_overflow_to_nan() {
        // Octave spread 1 over a full 64-osc bank sends `lattice_freq` past f32's
        // range (f0·2^63 = +Inf). The Nyquist clamp must keep every sample finite.
        let mut eng = SynthEngine::default();
        let cfg = cfg_lattice(220.0, MAX_BANK as u32, 0, 1.0);
        let (l, r) = render_mono(&mut eng, &cfg, 8_000);
        assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "no NaN/Inf from overflowing carriers");
        let pk = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(pk <= LIMIT_CEIL + 1e-3, "still bounded (peak {pk})");
    }

    #[test]
    fn lattice_is_deterministic() {
        let cfg = cfg_lattice(120.0, 24, 1, 0.0);
        let (mut a, mut b) = (SynthEngine::default(), SynthEngine::default());
        let (l1, r1) = render_mono(&mut a, &cfg, 4_000);
        let (l2, r2) = render_mono(&mut b, &cfg, 4_000);
        assert_eq!(l1, l2, "same params + fresh state → identical L");
        assert_eq!(r1, r2, "same params + fresh state → identical R");
    }

    fn cfg_modal(t60: f32, root: f32) -> SynthConfig {
        SynthConfig {
            on: true,
            play_mode: 0,
            mode: 2, // Modal
            gen_freq: root,
            gen_amp: 1.0,
            level: 0.5,
            bank_size: 8,
            t60,
            bright: 0.3,
            ..SynthConfig::default()
        }
    }
    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn modal_strike_rings_then_decays() {
        let mut eng = SynthEngine::default();
        let cfg = cfg_modal(0.3, 220.0);
        eng.strike_modal(220.0, 1.0);
        let (l, _) = render_mono(&mut eng, &cfg, 48_000); // 1 s
        let early = rms(&l[400..4_800]);
        let late = rms(&l[43_200..48_000]);
        assert!(early > 1e-4, "the strike rings (early RMS {early})");
        assert!(early > 20.0 * late.max(1e-12), "decays over ~T60 (early {early}, late {late})");
    }

    #[test]
    fn modal_silence_in_silence_out() {
        // No strike → the resonators stay at rest → silence (no self-oscillation).
        let mut eng = SynthEngine::default();
        let cfg = cfg_modal(2.0, 220.0);
        let (l, r) = render_mono(&mut eng, &cfg, 4_800);
        let e: f32 = l.iter().chain(r.iter()).map(|v| v * v).sum();
        assert!(e < 1e-9, "silent without a strike (energy {e})");
    }

    #[test]
    fn modal_note_scales_the_root() {
        // The fundamental partial = root × ratio[0] (= 1), so a strike at 330 Hz
        // rings a partial there, not at 220.
        let mut eng = SynthEngine::default();
        let cfg = cfg_modal(1.5, 220.0);
        eng.strike_modal(330.0, 1.0);
        let (l, _) = render_mono(&mut eng, &cfg, 24_000);
        let p330 = goertzel(&l, 330.0, 48_000.0);
        let p220 = goertzel(&l, 220.0, 48_000.0);
        assert!(p330 > 8.0 * p220.max(1e-12), "fundamental follows the struck root");
    }

    #[test]
    fn modal_sounds_in_instrument_mode_off_a_field_generator() {
        // Regression: modal is struck, not a field bed — a non-field generator zeroes
        // `gen_amp`, but modal must still ring (via `modal_amp`) in Instrument mode.
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_modal(0.5, 220.0);
        cfg.gen_amp = 0.0; // as forced off a field generator
        cfg.play_mode = 1; // Instrument
        eng.strike_modal(220.0, 1.0);
        let (l, _) = render_mono(&mut eng, &cfg, 4_800);
        assert!(rms(&l[200..]) > 1e-3, "modal rings in Instrument mode with gen_amp=0");
    }

    #[test]
    fn modal_output_is_bounded() {
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_modal(6.0, 90.0);
        cfg.level = 8.0;
        cfg.gen_amp = 4.0;
        cfg.bank_size = MAX_BANK as u32;
        for _ in 0..8 {
            eng.strike_modal(90.0, 1.0);
            let (l, r) = render_mono(&mut eng, &cfg, 2_000);
            let pk = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(pk <= LIMIT_CEIL + 1e-3, "modal bank bounded (peak {pk})");
        }
    }

    fn cfg_wavetable(f0: f32) -> SynthConfig {
        SynthConfig {
            on: true,
            play_mode: 0,
            mode: 4, // Wavetable
            gen_freq: f0,
            gen_amp: 1.0,
            level: 0.5,
            source_kind: 1, // dipole → the ring cross-section varies (not flat)
            field_k: 1.0,
            shell_r: 2.0,
            ..SynthConfig::default()
        }
    }

    #[test]
    fn wavetable_plays_at_the_base_frequency() {
        let mut eng = SynthEngine::default();
        let cfg = cfg_wavetable(220.0);
        let (l, _) = render_mono(&mut eng, &cfg, 48_000);
        let p220 = goertzel(&l, 220.0, 48_000.0);
        let p165 = goertzel(&l, 165.0, 48_000.0);
        assert!(p220 > 8.0 * p165.max(1e-9), "fundamental at f0 (220)");
    }

    #[test]
    fn wavetable_seam_is_continuous() {
        // The ring is closed, so no single step around the table (incl. the wrap) is
        // a big jump — the scan can't click at the seam.
        let mut eng = SynthEngine::default();
        let cfg = cfg_wavetable(200.0);
        let _ = render_mono(&mut eng, &cfg, 1_024);
        let t = eng.wt_table();
        let mut max_step = 0.0f32;
        for i in 0..t.len() {
            let d = (t[(i + 1) % t.len()] - t[i]).abs();
            max_step = max_step.max(d);
        }
        assert!(max_step < 0.2, "no seam click (max adjacent step {max_step} of ±1)");
    }

    fn cfg_granular() -> SynthConfig {
        SynthConfig {
            on: true,
            play_mode: 0,
            mode: 3, // Granular
            gen_freq: 220.0,
            gen_amp: 1.0,
            level: 0.5,
            source_kind: 1,
            field_k: 1.0,
            shell_r: 2.0,
            grain_size: 0.05,
            grain_density: 0.6,
            ..SynthConfig::default()
        }
    }

    #[test]
    fn granular_silent_with_no_density() {
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_granular();
        cfg.grain_density = 0.0; // no grains scheduled
        let (l, r) = render_mono(&mut eng, &cfg, 4_800);
        let e: f32 = l.iter().chain(r.iter()).map(|v| v * v).sum();
        assert!(e < 1e-9, "no grains → silence (energy {e})");
    }

    #[test]
    fn granular_produces_bounded_texture() {
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_granular();
        cfg.level = 8.0;
        cfg.grain_density = 1.0;
        let (l, r) = render_mono(&mut eng, &cfg, 24_000);
        let energy: f32 = l.iter().map(|v| v * v).sum();
        let pk = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(energy > 1e-4, "grains make sound");
        assert!(pk <= LIMIT_CEIL + 1e-3, "bounded (peak {pk})");
    }

    #[test]
    fn tier4_is_deterministic() {
        for cfg in [cfg_wavetable(180.0), cfg_granular()] {
            let (mut a, mut b) = (SynthEngine::default(), SynthEngine::default());
            let (l1, r1) = render_mono(&mut a, &cfg, 4_000);
            let (l2, r2) = render_mono(&mut b, &cfg, 4_000);
            assert_eq!((l1, r1), (l2, r2), "same params + fresh state → identical");
        }
    }

    fn render_mono(eng: &mut SynthEngine, cfg: &SynthConfig, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        eng.render(&mut l, &mut r, cfg);
        (l, r)
    }

    #[test]
    fn synth_off_is_byte_identical() {
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_gen(220.0);
        cfg.on = false;
        let mut l = vec![0.11f32, -0.3, 0.7, -0.9, 0.05];
        let mut r = vec![-0.2f32, 0.4, -0.6, 0.8, -0.01];
        let l0 = l.clone();
        let r0 = r.clone();
        eng.render(&mut l, &mut r, &cfg);
        assert_eq!(l, l0, "off must not touch the left passthrough");
        assert_eq!(r, r0, "off must not touch the right passthrough");
    }

    #[test]
    fn generative_tone_peaks_at_the_base_frequency() {
        let mut eng = SynthEngine::default();
        let cfg = cfg_gen(440.0);
        let (l, _) = render_mono(&mut eng, &cfg, 48_000);
        let p_base = goertzel(&l, 440.0, 48_000.0);
        let p_oct = goertzel(&l, 880.0, 48_000.0);
        let p_sub = goertzel(&l, 220.0, 48_000.0);
        assert!(p_base > 50.0 * p_oct.max(1e-9), "peak at 440, not 880");
        assert!(p_base > 50.0 * p_sub.max(1e-9), "peak at 440, not 220");
    }

    #[test]
    fn played_note_produces_its_equal_tempered_frequency() {
        let mut eng = SynthEngine::default();
        let mut cfg = SynthConfig {
            on: true,
            play_mode: 1, // instrument only (silent bed)
            attack: 0.001,
            sustain: 1.0,
            level: 0.5,
            ..SynthConfig::default()
        };
        cfg.gen_amp = 0.0;
        eng.note_on(69, 1.0, 0.0, 440.0); // A4 = 440
        let (l, _) = render_mono(&mut eng, &cfg, 48_000);
        let p_440 = goertzel(&l, 440.0, 48_000.0);
        let p_466 = goertzel(&l, 466.16, 48_000.0);
        let p_415 = goertzel(&l, 415.30, 48_000.0);
        assert!(p_440 > 30.0 * p_466.max(1e-9), "played A4 peaks at 440");
        assert!(p_440 > 30.0 * p_415.max(1e-9), "not the neighbouring semitones");
    }

    #[test]
    fn one_over_r_amplitude_falloff() {
        // A monopole at the origin: RMS pressure ∝ 1/r. Put both probes on +Z at
        // r_near and r_far (2×), expect the near probe ≈ 2× louder.
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_gen(300.0);
        cfg.near = 0.0; // far-field only (pure 1/r pressure term)
        cfg.probe_l = Vec3::new(0.0, 0.0, 1.0);
        cfg.probe_r = Vec3::new(0.0, 0.0, 2.0);
        let (l, r) = render_mono(&mut eng, &cfg, 24_000);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let ratio = rms(&l) / rms(&r).max(1e-9);
        assert!((ratio - 2.0).abs() < 0.15, "1/r falloff: near/far ≈ 2 (got {ratio})");
    }

    #[test]
    fn spaced_probes_give_an_interaural_delay() {
        // Two probes at different radii from a monopole differ by the retarded
        // phase k·Δr → a time lag Δr/C_SOUND. Cross-correlate to recover it.
        let mut eng = SynthEngine::default();
        let mut cfg = cfg_gen(200.0);
        cfg.near = 0.0;
        // R sits ~10 samples farther out along Z: Δr = 10·C/sr.
        let dr = 10.0 * C_SOUND / 48_000.0;
        cfg.probe_l = Vec3::new(0.0, 0.0, 1.0);
        cfg.probe_r = Vec3::new(0.0, 0.0, 1.0 + dr);
        let (l, r) = render_mono(&mut eng, &cfg, 8_000);
        // Best integer lag `d` maximizing Σ l[t]·r[t+d] over a window (R lags L).
        let (mut best_d, mut best_c) = (0i32, f32::NEG_INFINITY);
        for d in 0..24 {
            let mut c = 0.0f32;
            for t in 1000..(l.len() - 24) {
                c += l[t] * r[t + d as usize];
            }
            if c > best_c {
                best_c = c;
                best_d = d;
            }
        }
        assert!((best_d - 10).abs() <= 1, "interaural lag ≈ 10 samples (got {best_d})");
    }

    #[test]
    fn limiter_bounds_output_under_max_drive() {
        // 8 voices struck at max velocity into a near-field probe (the stress case).
        let mut eng = SynthEngine::default();
        let mut cfg = SynthConfig {
            on: true,
            play_mode: 1,
            attack: 0.0001,
            sustain: 1.0,
            level: 8.0, // deliberately hot
            r_min: 0.02,
            ..SynthConfig::default()
        };
        cfg.gen_amp = 0.0;
        cfg.probe_l = Vec3::new(0.0, 0.0, 0.05);
        cfg.probe_r = Vec3::new(0.0, 0.0, 0.05);
        for i in 0..N_VOICES {
            eng.note_on(48 + i as u8 * 3, 1.0, 0.0, 440.0);
        }
        let (l, r) = render_mono(&mut eng, &cfg, 24_000);
        let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= LIMIT_CEIL + 1e-3, "limiter holds the ceiling (peak {peak})");
    }

    #[test]
    fn voice_steal_is_deterministic_and_bounded() {
        let mut eng = SynthEngine::default();
        for i in 0..(N_VOICES + 3) {
            eng.note_on(60 + i as u8, 1.0, 0.0, 440.0);
        }
        assert_eq!(eng.active_voices(), N_VOICES, "never exceeds the voice bank");
    }

    #[test]
    fn release_completes_to_silence() {
        let mut eng = SynthEngine::default();
        let mut cfg = SynthConfig {
            on: true,
            play_mode: 1,
            attack: 0.001,
            release: 0.02,
            sustain: 1.0,
            level: 0.5,
            ..SynthConfig::default()
        };
        cfg.gen_amp = 0.0;
        eng.note_on(72, 1.0, 0.0, 440.0);
        let _ = render_mono(&mut eng, &cfg, 2_000);
        eng.note_off(72);
        let (l, r) = render_mono(&mut eng, &cfg, 48_000);
        assert_eq!(eng.active_voices(), 0, "voice returns to idle after release");
        let tail = &l[l.len() - 256..];
        let tail_r = &r[r.len() - 256..];
        let e: f32 = tail.iter().chain(tail_r.iter()).map(|v| v * v).sum();
        assert!(e < 1e-9, "output decays to silence (residual energy {e})");
    }
}
