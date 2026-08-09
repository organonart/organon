//! Real-time audio feature extraction for the audio-reactive visual.
//!
//! Lives on the audio thread inside the plugin's `process()`. Recent input
//! samples (summed to mono) accumulate in a ring buffer; once per block we run a
//! Hann-windowed real FFT over the latest `FFT_SIZE` samples, integrate the
//! magnitude spectrum into log-spaced frequency bands, and smooth each band with
//! an attack/release envelope follower. Everything is pre-allocated in `new`, so
//! `push_sample`/`analyze` never allocate or lock — they're hard-real-time safe.
//!
//! The point is the *envelope*: a fast-attack / slow-release follower on the
//! bass band swells on each kick and decays in the gap, so it pulses with the
//! music's real rhythm (no BPM knowledge needed). The visual reads the smoothed
//! bands from the IPC snapshot and can swap them in as its pulse source.

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// FFT window length. 2048 @ 48 kHz ≈ 43 ms — enough low-frequency resolution to
/// separate the sub/bass bins, while staying cheap to run every block.
pub const FFT_SIZE: usize = 2048;

/// Number of analysis bands. Six edges → five bands: sub, bass, low-mid, mid, high.
pub const NUM_BANDS: usize = 5;

/// Band edge frequencies (Hz). Log-spaced, matching how we hear; "the bass" is
/// the bottom two bands (sub 20–60, bass 60–160).
const BAND_HZ: [f32; NUM_BANDS + 1] = [20.0, 60.0, 160.0, 500.0, 2000.0, 16000.0];

/// Human-readable labels for the analysis bands (UI).
pub const BAND_LABELS: [&str; NUM_BANDS] = ["sub", "bass", "low-mid", "mid", "high"];

/// Number of bars in the display spectrum. Finer than the analysis bands so the
/// editor's spectrum analyzer reads as a smooth curve, not five blocks. Display
/// only — the routing/pulse machinery uses the five bands, not these.
pub const DISP_BINS: usize = 32;

/// Frequency span of the display spectrum (Hz), log-spaced across `DISP_BINS`.
const DISP_LO_HZ: f32 = 30.0;
const DISP_HI_HZ: f32 = 16000.0;

/// Windowed-FFT band analyzer with per-band attack/release envelope followers.
pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,             // Hann, len FFT_SIZE
    win_energy: f32,              // Σw² (band-power → dBFS calibration, #333 T2)
    ring: Vec<f32>,               // recent mono samples, len FFT_SIZE
    write: usize,                 // ring write head (oldest sample)
    input: Vec<f32>,              // FFT input scratch, len FFT_SIZE
    spectrum: Vec<Complex32>,     // FFT output, len FFT_SIZE/2 + 1
    scratch: Vec<Complex32>,      // realfft internal scratch
    edges: [usize; NUM_BANDS + 1],// band → spectrum-bin boundaries
    env: [f32; NUM_BANDS],        // smoothed band levels (the output)
    disp_edges: [usize; DISP_BINS + 1], // display-bar → spectrum-bin boundaries
    disp: [f32; DISP_BINS],       // latest display-spectrum magnitudes (post-gain)
    level: f32,                   // smoothed broadband input level (post-gain)
    peak: f32,                    // latest block peak amplitude (post-gain, unsmoothed)
    centroid: f32,                // smoothed spectral centroid, 0..1 on the log axis (#248 T3)
    bal: f32,                     // smoothed stereo balance, −1 (L) .. +1 (R) (#248 T3)
    sample_rate: f32,
    atk_tau: f32,                 // attack time constant (s)
    rel_tau: f32,                 // release time constant (s)
    tempo: TempoEstimator,        // onset-autocorrelation BPM estimator (#307)
    prev_low: f32,                // previous block's low-band level (onset flux)
}

/// Onset-autocorrelation tempo (BPM) estimator (#307 Tier 1). It's fed a
/// low-frequency **onset strength** each analysis block, accumulates it into
/// fixed-rate bins (so the estimate is sample-rate / block-size independent),
/// autocorrelates the recent bin history over a musical lag range, and reports the
/// beat period as a BPM plus a confidence (how sharply the autocorrelation peaks).
///
/// EDM's steady four-on-the-floor kick is the target: a clear peak at the beat lag.
/// When the kick drops out (a breakdown) the autocorrelation flattens, confidence
/// falls, and the **last good BPM is held** rather than re-estimated to nonsense —
/// the visual keeps time until the beat returns.
pub struct TempoEstimator {
    hist: Vec<f32>,   // ring of per-bin onset energy (len BINS)
    scratch: Vec<f32>, // chronological copy for autocorrelation (len BINS)
    head: usize,      // ring write index
    filled: usize,    // bins written so far (caps at BINS)
    acc: f32,         // onset accumulating into the current bin
    acc_dt: f32,      // seconds accumulated into the current bin
    since_update: u32, // bins since the last estimate
    bpm: f32,         // last reported BPM (held on low confidence)
    conf: f32,        // last confidence, 0..1
}

impl TempoEstimator {
    /// Bins per second the onset envelope is resampled to (10 ms bins).
    const BIN_HZ: f32 = 100.0;
    /// History length in bins (~6 s) — enough for a stable low-tempo autocorrelation.
    const BINS: usize = 600;
    /// Re-estimate every this many bins (~250 ms).
    const UPDATE_EVERY: u32 = 25;
    /// Tempo search window (BPM). Wide enough for half/double-time; folded to a
    /// musical octave afterwards.
    const BPM_MIN: f32 = 70.0;
    const BPM_MAX: f32 = 190.0;
    /// Confidence needed to trust a fresh estimate (else hold the last BPM).
    const CONF_TRUST: f32 = 0.15;

    pub fn new() -> TempoEstimator {
        TempoEstimator {
            hist: vec![0.0; Self::BINS],
            scratch: vec![0.0; Self::BINS],
            head: 0,
            filled: 0,
            acc: 0.0,
            acc_dt: 0.0,
            since_update: 0,
            bpm: 0.0,
            conf: 0.0,
        }
    }

    /// The last detected BPM (0 until the first lock; held through breakdowns).
    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    /// The last confidence, 0..1 (decays while a held BPM coasts through a gap).
    pub fn confidence(&self) -> f32 {
        self.conf
    }

    /// Feed one block's onset strength (`>= 0`) and its duration in seconds. RT-safe
    /// (no allocation, no lock). Accumulates into fixed-rate bins and periodically
    /// re-estimates.
    pub fn push(&mut self, onset: f32, dt: f32) {
        let bin_dt = 1.0 / Self::BIN_HZ;
        self.acc += onset.max(0.0);
        self.acc_dt += dt.max(0.0);
        // Emit as many whole bins as have elapsed (guards against long blocks). When a
        // single push spans more than one bin (e.g. a 1024-sample block ≈ 21 ms at
        // 48 kHz covers 2+ of the 10 ms bins), the accumulated onset is spread across
        // those bins **proportionally to the time each covers**, not dumped whole into
        // the first bin — otherwise the later bins record zero while time advances,
        // skewing the onset timeline the autocorrelation reads.
        let mut guard = 0;
        while self.acc_dt >= bin_dt && guard < Self::BINS {
            // This bin covers `bin_dt` of the `acc_dt` still pooled in `acc`.
            let bin_energy = self.acc * (bin_dt / self.acc_dt);
            self.hist[self.head] = bin_energy;
            self.head = (self.head + 1) % Self::BINS;
            self.filled = (self.filled + 1).min(Self::BINS);
            self.acc -= bin_energy;
            self.acc_dt -= bin_dt;
            self.since_update += 1;
            guard += 1;
            if self.since_update >= Self::UPDATE_EVERY {
                self.since_update = 0;
                self.estimate();
            }
        }
    }

    /// Autocorrelate the onset history and update `bpm`/`conf`. Holds the BPM when
    /// the peak isn't prominent enough (a breakdown).
    fn estimate(&mut self) {
        let lag_min = (Self::BIN_HZ * 60.0 / Self::BPM_MAX).floor().max(1.0) as usize;
        let lag_max = (Self::BIN_HZ * 60.0 / Self::BPM_MIN).ceil() as usize;
        // Need a couple of periods of history to trust the correlation.
        if self.filled < lag_max * 2 {
            self.conf *= 0.9; // decay confidence while we warm up; hold bpm
            return;
        }
        let n = self.filled;
        // Copy ring → scratch in chronological order (oldest first).
        let start = if self.filled < Self::BINS {
            0
        } else {
            self.head // ring full: oldest sits at head
        };
        for i in 0..n {
            self.scratch[i] = self.hist[(start + i) % Self::BINS];
        }
        // Mean-remove so a DC onset floor doesn't swamp the correlation.
        let mean: f32 = self.scratch[..n].iter().sum::<f32>() / n as f32;
        for v in self.scratch[..n].iter_mut() {
            *v -= mean;
        }
        let mut best_lag = lag_min;
        let mut best_r = f32::MIN;
        let mut r_sum = 0.0f32;
        let mut r_cnt = 0.0f32;
        for lag in lag_min..=lag_max.min(n - 1) {
            let mut r = 0.0f32;
            for i in lag..n {
                r += self.scratch[i] * self.scratch[i - lag];
            }
            r /= (n - lag) as f32;
            r_sum += r;
            r_cnt += 1.0;
            if r > best_r {
                best_r = r;
                best_lag = lag;
            }
        }
        let mean_r = if r_cnt > 0.0 { r_sum / r_cnt } else { 0.0 };
        // Confidence: how far the peak stands above the average correlation.
        let conf = if best_r > 0.0 {
            ((best_r - mean_r) / best_r).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if conf >= Self::CONF_TRUST {
            let mut bpm = Self::BIN_HZ * 60.0 / best_lag as f32;
            // Fold into a musical octave [90, 180) so half/double-time locks agree.
            while bpm < 90.0 {
                bpm *= 2.0;
            }
            while bpm >= 180.0 {
                bpm *= 0.5;
            }
            if self.bpm <= 0.0 {
                self.bpm = bpm;
            } else {
                // Ease toward the estimate; snap if it's a big (real) change.
                if (bpm - self.bpm).abs() > 8.0 {
                    self.bpm = bpm;
                } else {
                    self.bpm += (bpm - self.bpm) * 0.25;
                }
            }
            self.conf = conf;
        } else {
            // Breakdown / no clear beat: hold the last BPM, let confidence decay.
            self.conf *= 0.9;
        }
    }
}

impl Default for TempoEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Analyzer {
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let input = fft.make_input_vec();
        let spectrum = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        // Hann window: sin²(πi/(N−1)).
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let s = (std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0)).sin();
                s * s
            })
            .collect();
        let win_energy = window.iter().map(|w| w * w).sum();
        let mut a = Analyzer {
            fft,
            window,
            win_energy,
            ring: vec![0.0; FFT_SIZE],
            write: 0,
            input,
            spectrum,
            scratch,
            edges: [0; NUM_BANDS + 1],
            env: [0.0; NUM_BANDS],
            disp_edges: [0; DISP_BINS + 1],
            disp: [0.0; DISP_BINS],
            level: 0.0,
            peak: 0.0,
            centroid: 0.0,
            bal: 0.0,
            sample_rate: sample_rate.max(1.0),
            atk_tau: 0.010,
            rel_tau: 0.200,
            tempo: TempoEstimator::new(),
            prev_low: 0.0,
        };
        a.recompute_edges();
        a
    }

    /// Recompute the band → bin mappings (analysis bands + display bars) for the
    /// current sample rate.
    fn recompute_edges(&mut self) {
        let nyq_bin = FFT_SIZE / 2;
        let hz_to_bin = |hz: f32| {
            ((hz * FFT_SIZE as f32 / self.sample_rate).round() as usize).min(nyq_bin)
        };
        for (k, &hz) in BAND_HZ.iter().enumerate() {
            self.edges[k] = hz_to_bin(hz);
        }
        // Log-spaced display bars across [DISP_LO_HZ, DISP_HI_HZ].
        let ratio = (DISP_HI_HZ / DISP_LO_HZ).ln();
        for k in 0..=DISP_BINS {
            let hz = DISP_LO_HZ * (ratio * k as f32 / DISP_BINS as f32).exp();
            self.disp_edges[k] = hz_to_bin(hz);
        }
    }

    /// Update the sample rate (and the band-edge bins) if it changed.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        if (sr - self.sample_rate).abs() > 0.5 {
            self.sample_rate = sr;
            self.recompute_edges();
        }
    }

    /// Set the envelope follower attack/release in milliseconds. RT-safe.
    pub fn set_times(&mut self, attack_ms: f32, release_ms: f32) {
        self.atk_tau = attack_ms.max(0.1) / 1000.0;
        self.rel_tau = release_ms.max(0.1) / 1000.0;
    }

    /// Append one mono sample to the ring buffer. RT-safe.
    #[inline]
    pub fn push_sample(&mut self, x: f32) {
        self.ring[self.write] = x;
        self.write = (self.write + 1) % FFT_SIZE;
    }

    /// Run the FFT over the latest window, advance the per-band envelopes by `dt`
    /// seconds, and return them. `gain` scales the input sensitivity. RT-safe (no
    /// allocation, no lock). The returned levels peak around 1 for a full-scale
    /// signal at unity gain; the caller exposes `gain` so quiet room sound still
    /// drives the visual.
    pub fn analyze(&mut self, gain: f32, dt: f32) -> [f32; NUM_BANDS] {
        // Copy ring → input in chronological order (oldest at `write`), windowed.
        for i in 0..FFT_SIZE {
            let s = self.ring[(self.write + i) % FFT_SIZE];
            self.input[i] = s * self.window[i];
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return self.env;
        }

        // Envelope coefficients from the block duration (frame-rate independent).
        let atk = 1.0 - (-dt / self.atk_tau).exp();
        let rel = 1.0 - (-dt / self.rel_tau).exp();
        // Bin magnitude → ~amplitude: a Hann-windowed bin peaks at ≈ A·N/4, so
        // ×(2/N) lands a full-scale tone near unity after the gain knob.
        let scale = gain * 2.0 / FFT_SIZE as f32;
        let mut raw_low = 0.0f32; // instantaneous sub+bass energy (for onset flux)
        for b in 0..NUM_BANDS {
            let lo = self.edges[b].max(1); // skip DC
            let hi = self.edges[b + 1].max(lo + 1);
            let mut sum = 0.0f32;
            for k in lo..hi {
                sum += self.spectrum[k].norm();
            }
            // Mean magnitude across the band (density), so wide bands don't
            // dominate purely by spanning more bins.
            let level = (sum / (hi - lo) as f32) * scale;
            if b < 2 {
                raw_low += level; // sub (0) + bass (1) — where the kick lives
            }
            let coeff = if level > self.env[b] { atk } else { rel };
            self.env[b] += (level - self.env[b]) * coeff;
        }

        // BPM detection (#307): feed the rising edge (positive flux) of the raw
        // low-band energy — a kick makes a sharp onset. The estimator bins it at a
        // fixed rate and autocorrelates for the beat period; it holds the last good
        // BPM through a breakdown (no clear kick). Only runs while audio-reactive is
        // on (this whole method does), so it costs nothing otherwise.
        let onset = (raw_low - self.prev_low).max(0.0);
        self.prev_low = raw_low;
        self.tempo.push(onset, dt);

        // Display spectrum: same mean-magnitude density per (finer) log bar. These
        // are instantaneous (unsmoothed) — the editor applies a peak-hold/decay for
        // the falling-bar look, decoupled from the audio block rate.
        for d in 0..DISP_BINS {
            let lo = self.disp_edges[d].max(1);
            let hi = self.disp_edges[d + 1].max(lo + 1);
            let mut sum = 0.0f32;
            for k in lo..hi {
                sum += self.spectrum[k].norm();
            }
            self.disp[d] = (sum / (hi - lo) as f32) * scale;
        }

        // Spectral centroid (#248 Tier 3): the magnitude-weighted mean frequency,
        // mapped to 0..1 on the same log axis as the display spectrum — the
        // "brightness" of the sound (the honest pitch proxy that drives the
        // visible, scaled oscillation rate). Held when the input is essentially
        // silent (a meaningless centroid shouldn't jitter the rate).
        let mut wsum = 0.0f32;
        let mut msum = 0.0f32;
        let nyq = FFT_SIZE / 2;
        for k in 1..=nyq {
            let mag = self.spectrum[k].norm();
            let hz = k as f32 * self.sample_rate / FFT_SIZE as f32;
            wsum += mag * hz;
            msum += mag;
        }
        if msum * scale > 1.0e-4 {
            let hz = (wsum / msum).clamp(DISP_LO_HZ, DISP_HI_HZ);
            let c01 = (hz / DISP_LO_HZ).ln() / (DISP_HI_HZ / DISP_LO_HZ).ln();
            let coeff = if c01 > self.centroid { atk } else { rel };
            self.centroid += (c01 - self.centroid) * coeff;
        }

        // Broadband input level: RMS over the (un-windowed) recent samples, post-
        // gain, attack/release-smoothed for a steady meter; plus the raw block peak
        // for a clip indicator.
        let mut sq = 0.0f32;
        let mut pk = 0.0f32;
        for &x in &self.ring {
            sq += x * x;
            pk = pk.max(x.abs());
        }
        let rms = (sq / FFT_SIZE as f32).sqrt() * gain;
        let coeff = if rms > self.level { atk } else { rel };
        self.level += (rms - self.level) * coeff;
        self.peak = pk * gain;

        self.env
    }

    /// Latest display-spectrum magnitudes (finer than the analysis bands). Display
    /// only; instantaneous (the caller decays them for the bar-cap look).
    pub fn display_spectrum(&self) -> &[f32; DISP_BINS] {
        &self.disp
    }

    /// Smoothed broadband input level (post-gain), ~1 at full scale.
    pub fn input_level(&self) -> f32 {
        self.level
    }

    /// Raw block peak amplitude (post-gain) — for the clip indicator.
    pub fn input_peak(&self) -> f32 {
        self.peak
    }

    /// The most recent smoothed band envelopes (without re-running the FFT).
    pub fn bands(&self) -> [f32; NUM_BANDS] {
        self.env
    }

    /// The most recent detected tempo in BPM (#307). Held through a breakdown; 0
    /// until the estimator has locked at least once.
    pub fn tempo_bpm(&self) -> f32 {
        self.tempo.bpm()
    }

    /// Confidence (0..1) in the detected tempo (#307): how sharply the onset
    /// autocorrelation peaks. Decays through a breakdown while the BPM is held.
    pub fn tempo_confidence(&self) -> f32 {
        self.tempo.confidence()
    }

    /// Smoothed spectral centroid on the log frequency axis, 0 (≈30 Hz) .. 1
    /// (≈16 kHz) — the "brightness" of the sound (#248 Tier 3). Held during
    /// silence.
    pub fn spectral_centroid(&self) -> f32 {
        self.centroid
    }

    /// Feed one block's per-channel energies (sums of squares) and advance the
    /// smoothed **stereo balance**: −1 = hard left, +1 = hard right, 0 = centred
    /// or mono (#248 Tier 3). Uses the release time constant both ways (the field
    /// should lean smoothly, not snap). RT-safe. Near-silence holds the last
    /// balance instead of collapsing to a noisy ratio.
    pub fn update_balance(&mut self, l_sq: f32, r_sq: f32, dt: f32) -> f32 {
        let tot = l_sq + r_sq;
        if tot > 1.0e-12 {
            let raw = ((r_sq.sqrt() - l_sq.sqrt()) / (r_sq.sqrt() + l_sq.sqrt() + 1.0e-12))
                .clamp(-1.0, 1.0);
            let coeff = 1.0 - (-dt / self.rel_tau).exp();
            self.bal += (raw - self.bal) * coeff;
        }
        self.bal
    }

    /// The most recent smoothed stereo balance (−1 left .. +1 right).
    pub fn stereo_balance(&self) -> f32 {
        self.bal
    }

    /// Raw one-sided FFT bins from the most recent `analyze()` (len FFT_SIZE/2 + 1)
    /// — the input to the calibrated spectrum (#333 T2).
    pub fn raw_bins(&self) -> &[Complex32] {
        &self.spectrum
    }
    /// Current sample rate.
    pub fn sr(&self) -> f32 {
        self.sample_rate
    }
    /// Hann window energy Σw² (band-power → dBFS calibration).
    pub fn window_energy(&self) -> f32 {
        self.win_energy
    }
}

/// Largest fractional-octave band count we pre-allocate — 1/12-octave from ~20 Hz
/// to ~20 kHz needs ≈ 120 bands (#333 T2).
pub const MAX_RTA_BANDS: usize = 128;

/// A-weighting gain in dB at `f` Hz (IEC 61672).
pub fn a_weight_db(f: f32) -> f32 {
    let f2 = f * f;
    let num = 12194.0f32.powi(2) * f2 * f2;
    let den = (f2 + 20.6f32.powi(2))
        * ((f2 + 107.7f32.powi(2)) * (f2 + 737.9f32.powi(2))).sqrt()
        * (f2 + 12194.0f32.powi(2));
    20.0 * (num / den).log10() + 2.00
}

/// C-weighting gain in dB at `f` Hz (IEC 61672).
pub fn c_weight_db(f: f32) -> f32 {
    let f2 = f * f;
    let num = 12194.0f32.powi(2) * f2;
    let den = (f2 + 20.6f32.powi(2)) * (f2 + 12194.0f32.powi(2));
    20.0 * (num / den).log10() + 0.06
}

/// Calibrated fractional-octave RTA (#333 Tier 2). Turns the raw FFT power into a
/// real dB-per-band spectrum: **band power** (Σ|X|², window/calibration corrected
/// so a full-scale sine reads 0 dBFS in its band), integrated into **IEC 61260
/// fractional-octave** bands (1/1, 1/3, 1/6, 1/12), with selectable **A/C/Z
/// weighting** and **fast/slow/peak-hold/Leq** averaging. Fed the analyzer's raw
/// bins once per block; RT-safe (pre-allocated). All display state; not in the hot
/// per-sample path.
pub struct CalibratedSpectrum {
    sr: f32,
    resolution: u32, // octave denominator: 1, 3, 6, 12 (0 → 3 default)
    weighting: u32,  // 0 = Z (flat), 1 = A, 2 = C
    averaging: u32,  // 0 = fast, 1 = slow, 2 = peak-hold, 3 = Leq (infinite)
    centers: Vec<f32>,
    lo_hz: Vec<f32>,
    hi_hz: Vec<f32>,
    weight_db: Vec<f32>, // cached weighting per band
    n_bands: usize,
    inst_db: Vec<f32>,   // instantaneous calibrated dB per band (this block)
    disp_db: Vec<f32>,   // averaged/held dB per band (the output)
    leq_pow: Vec<f64>,   // linear power accumulator for Leq
    leq_n: u64,
}

impl CalibratedSpectrum {
    pub fn new(sr: f32) -> CalibratedSpectrum {
        let mut c = CalibratedSpectrum {
            sr: sr.max(1.0),
            resolution: 3,
            weighting: 0,
            averaging: 0,
            centers: Vec::with_capacity(MAX_RTA_BANDS),
            lo_hz: Vec::with_capacity(MAX_RTA_BANDS),
            hi_hz: Vec::with_capacity(MAX_RTA_BANDS),
            weight_db: Vec::with_capacity(MAX_RTA_BANDS),
            n_bands: 0,
            inst_db: vec![f32::NEG_INFINITY; MAX_RTA_BANDS],
            disp_db: vec![f32::NEG_INFINITY; MAX_RTA_BANDS],
            leq_pow: vec![0.0; MAX_RTA_BANDS],
            leq_n: 0,
        };
        c.rebuild_bands();
        c
    }

    pub fn n_bands(&self) -> usize {
        self.n_bands
    }
    pub fn center_hz(&self, i: usize) -> f32 {
        self.centers.get(i).copied().unwrap_or(0.0)
    }
    /// The averaged calibrated levels, dBFS per band (0 dBFS = full-scale sine).
    pub fn levels_db(&self) -> &[f32] {
        &self.disp_db[..self.n_bands]
    }

    /// Set the RTA controls; rebuilds the band table if resolution/weighting/sr
    /// changed. Cheap and rare (called from the block loop with param values).
    pub fn configure(&mut self, sr: f32, resolution: u32, weighting: u32, averaging: u32) {
        let res = match resolution {
            0 | 1 | 3 | 6 | 12 => resolution, // 0 = raw linear FFT axis
            _ => 3,
        };
        let sr = sr.max(1.0);
        let dirty = (sr - self.sr).abs() > 0.5 || res != self.resolution || weighting != self.weighting;
        let avg_changed = averaging != self.averaging;
        self.sr = sr;
        self.resolution = res;
        self.weighting = weighting;
        self.averaging = averaging;
        if dirty {
            // rebuild_bands also drops the averaging state (new band table).
            self.rebuild_bands();
        } else if avg_changed {
            // Switching the averaging mode (e.g. Fast → Leq) must not blend fresh
            // blocks with stale Leq / peak-hold accumulators from the old mode.
            self.reset_averaging();
        }
    }

    fn rebuild_bands(&mut self) {
        self.centers.clear();
        self.lo_hz.clear();
        self.hi_hz.clear();
        self.weight_db.clear();
        // Build (lo, hi) edges for each band, then fill the center/weight tables.
        let nyq = self.sr * 0.5;
        let hi_hz = 20_000f32.min(nyq * 0.99);
        if self.resolution == 0 {
            // Raw linear-FFT axis: MAX_RTA_BANDS equal-width bins, 20 Hz → Nyquist.
            let lo0 = 20.0f32;
            let bw = (hi_hz - lo0) / MAX_RTA_BANDS as f32;
            for i in 0..MAX_RTA_BANDS {
                let lo = lo0 + i as f32 * bw;
                self.lo_hz.push(lo);
                self.hi_hz.push(lo + bw);
            }
        } else {
            // IEC 61260 base-2 fractional-octave bands: fm = 1000·2^(k/b).
            let b = self.resolution as f32;
            let half = 2f32.powf(0.5 / b);
            let mut k = (b * (20.0f32 / 1000.0).log2()).round() as i32; // start near 20 Hz
            while self.lo_hz.len() < MAX_RTA_BANDS {
                let fc = 1000.0 * 2f32.powf(k as f32 / b);
                if fc > hi_hz {
                    break;
                }
                if fc >= 15.0 {
                    self.lo_hz.push(fc / half);
                    self.hi_hz.push(fc * half);
                }
                k += 1;
            }
        }
        for i in 0..self.lo_hz.len() {
            let fc = (self.lo_hz[i] * self.hi_hz[i]).sqrt(); // geometric-mean center
            self.centers.push(fc);
            self.weight_db.push(match self.weighting {
                1 => a_weight_db(fc),
                2 => c_weight_db(fc),
                _ => 0.0,
            });
        }
        self.n_bands = self.centers.len();
        // Fresh band table → drop stale averaging state.
        for v in self.inst_db.iter_mut() {
            *v = f32::NEG_INFINITY;
        }
        for v in self.disp_db.iter_mut() {
            *v = f32::NEG_INFINITY;
        }
        self.leq_pow.iter_mut().for_each(|p| *p = 0.0);
        self.leq_n = 0;
    }

    /// Reset the Leq / peak-hold accumulation.
    pub fn reset_averaging(&mut self) {
        self.leq_pow.iter_mut().for_each(|p| *p = 0.0);
        self.leq_n = 0;
        for v in self.disp_db.iter_mut() {
            *v = f32::NEG_INFINITY;
        }
    }

    /// Integrate one block's raw FFT power into the calibrated bands + apply
    /// weighting and averaging. `bins` = one-sided FFT, `win_energy` = Σw², `dt` =
    /// block duration (s) for the time-averaging coefficients.
    pub fn update(&mut self, bins: &[Complex32], n_fft: usize, win_energy: f32, dt: f32) {
        // Calibration: full-scale sine (amp 1) → 0 dBFS in its band. The one-sided
        // Hann-windowed sine's summed power relates to A²/2 by 8/(win_energy·N);
        // ×2 more references it to the sine's peak so a full-scale tone reads 0 dBFS
        // (verified against a bin-centred full-scale tone in the unit tests).
        let cal = 16.0 / (win_energy.max(1.0e-9) * n_fft as f32);
        let bin_hz = self.sr / n_fft as f32;
        let nb = bins.len();
        for band in 0..self.n_bands {
            let lo = self.lo_hz[band];
            let hi = self.hi_hz[band];
            let k_lo = ((lo / bin_hz).floor() as usize).max(1);
            let k_hi = ((hi / bin_hz).ceil() as usize).min(nb - 1);
            let mut p = 0.0f32;
            for k in k_lo..=k_hi {
                p += bins[k].norm_sqr();
            }
            let db = if p > 1.0e-20 {
                10.0 * (p * cal).log10() + self.weight_db[band]
            } else {
                f32::NEG_INFINITY
            };
            self.inst_db[band] = db;
        }
        self.apply_averaging(dt);
    }

    fn apply_averaging(&mut self, dt: f32) {
        // Time constants: fast ≈ 125 ms, slow ≈ 1 s (IEC 61672 exponential).
        let tau = match self.averaging {
            1 => 1.0,   // slow
            _ => 0.125, // fast (also the rise for peak-hold)
        };
        let coeff = 1.0 - (-dt / tau).exp();
        match self.averaging {
            2 => {
                // Peak-hold: instant rise, slow decay (~1.5 s).
                let dec = 1.0 - (-dt / 1.5).exp();
                for i in 0..self.n_bands {
                    let cur = self.disp_db[i];
                    let inst = self.inst_db[i];
                    self.disp_db[i] = if inst > cur {
                        inst
                    } else if cur.is_finite() {
                        cur + (inst.max(-120.0) - cur) * dec
                    } else {
                        inst
                    };
                }
            }
            3 => {
                // Leq: running average of linear power since reset.
                self.leq_n += 1;
                for i in 0..self.n_bands {
                    let lin = if self.inst_db[i].is_finite() {
                        10f64.powf((self.inst_db[i] as f64) / 10.0)
                    } else {
                        0.0
                    };
                    self.leq_pow[i] += lin;
                    let mean = self.leq_pow[i] / self.leq_n as f64;
                    self.disp_db[i] = if mean > 1.0e-20 {
                        10.0 * mean.log10() as f32
                    } else {
                        f32::NEG_INFINITY
                    };
                }
            }
            _ => {
                // Fast / slow exponential smoothing (in the dB domain, clamped).
                for i in 0..self.n_bands {
                    let inst = self.inst_db[i].max(-120.0);
                    let cur = self.disp_db[i];
                    self.disp_db[i] = if cur.is_finite() {
                        cur + (inst - cur) * coeff
                    } else {
                        inst
                    };
                }
            }
        }
    }
}

// ===========================================================================
// Calibrated / analytical metering (#333 Tiers 1–2)
// ===========================================================================
//
// A metrologically honest measurement layer that runs alongside the expressive
// `Analyzer`. Everything below is defined against digital full scale (dBFS) and
// the ITU-R BS.1770-4 / EBU R128 loudness reference — reproducible numbers, not
// gain-scaled aesthetics. Like `Analyzer`, all state is pre-allocated in `new`
// (sized for a max sample rate) so the hot path never allocates or locks.

/// Highest sample rate we pre-allocate loudness windows for (so a sample-rate
/// change never reallocates on the audio thread).
const MAX_SR: usize = 192_000;
/// Loudness reference offset (BS.1770): `L = -0.691 + 10·log10(Σ z)`.
const LKFS_OFFSET: f32 = -0.691;

/// A Direct-Form-I biquad with per-channel state (stereo). Coefficients are
/// normalised so `a0 = 1`.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // Per-channel delay line [x1, x2, y1, y2] for up to 2 channels.
    s: [[f32; 4]; 2],
}

impl Biquad {
    #[inline]
    fn process(&mut self, ch: usize, x: f32) -> f32 {
        let st = &mut self.s[ch];
        let y = self.b0 * x + self.b1 * st[0] + self.b2 * st[1] - self.a1 * st[2] - self.a2 * st[3];
        st[1] = st[0];
        st[0] = x;
        st[3] = st[2];
        st[2] = y;
        y
    }
    fn reset(&mut self) {
        self.s = [[0.0; 4]; 2];
    }
    /// RBJ high-shelf. `fc` = corner Hz, `q` = Q, `gain_db` = shelf gain.
    fn high_shelf(sr: f32, fc: f32, q: f32, gain_db: f32) -> Biquad {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = std::f32::consts::TAU * fc / sr;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) + (a - 1.0) * cs + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cs);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cs - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cs + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cs);
        let a2 = (a + 1.0) - (a - 1.0) * cs - two_sqrt_a_alpha;
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s: [[0.0; 4]; 2] }
    }
    /// RBJ high-pass. `fc` = corner Hz, `q` = Q.
    fn high_pass(sr: f32, fc: f32, q: f32) -> Biquad {
        let w0 = std::f32::consts::TAU * fc / sr;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let b0 = (1.0 + cs) / 2.0;
        let b1 = -(1.0 + cs);
        let b2 = (1.0 + cs) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cs;
        let a2 = 1.0 - alpha;
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, s: [[0.0; 4]; 2] }
    }
    /// Magnitude response at `f` Hz (for calibration / tests).
    #[cfg(test)]
    fn mag_at(&self, sr: f32, f: f32) -> f32 {
        let w = std::f32::consts::TAU * f / sr;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        // H(e^jw) = (b0 + b1 e^-jw + b2 e^-2jw) / (1 + a1 e^-jw + a2 e^-2jw)
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt()
    }
}

/// The BS.1770 **K-weighting** pre-filter parameters (a +4 dB high-shelf ≈ 1.68 kHz
/// then an RLB high-pass ≈ 38 Hz). These analog constants reproduce the standard's
/// 48 kHz reference coefficients and generalise to any sample rate via the RBJ
/// bilinear formulas.
fn k_weight_filters(sr: f32) -> (Biquad, Biquad) {
    let shelf = Biquad::high_shelf(sr, 1681.974_4, 0.707_175_2, 3.999_843_8);
    let hp = Biquad::high_pass(sr, 38.135_47, 0.500_327);
    (shelf, hp)
}

/// Histogram of block-loudness values for the gated integrated-loudness and LRA
/// computations (BS.1770 / EBU R128). Bins span [-70, +6) LUFS at 0.1 LU.
struct LoudnessHistogram {
    counts: Vec<u32>,
}

impl LoudnessHistogram {
    const LO: f32 = -70.0;
    const HI: f32 = 6.0;
    const STEP: f32 = 0.1;
    fn new() -> Self {
        let n = ((Self::HI - Self::LO) / Self::STEP).round() as usize;
        LoudnessHistogram { counts: vec![0; n] }
    }
    fn clear(&mut self) {
        self.counts.iter_mut().for_each(|c| *c = 0);
    }
    #[inline]
    fn bin_center(i: usize) -> f32 {
        Self::LO + (i as f32 + 0.5) * Self::STEP
    }
    fn add(&mut self, loudness: f32) {
        if loudness < Self::LO || !loudness.is_finite() {
            return;
        }
        let i = (((loudness - Self::LO) / Self::STEP) as usize).min(self.counts.len() - 1);
        self.counts[i] += 1;
    }
    /// Gated integrated loudness: absolute gate at `abs_gate`, then a relative gate
    /// `rel_lu` below the mean of the abs-gated blocks (BS.1770: rel_lu = 10).
    fn integrated(&self, abs_gate: f32, rel_lu: f32) -> f32 {
        let energy = |l: f32| 10f32.powf((l + 0.691) / 10.0);
        // Stage 1: mean energy of blocks above the absolute gate.
        let (mut e_sum, mut n) = (0.0f64, 0u64);
        for (i, &c) in self.counts.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let l = Self::bin_center(i);
            if l >= abs_gate {
                e_sum += energy(l) as f64 * c as f64;
                n += c as u64;
            }
        }
        if n == 0 {
            return f32::NEG_INFINITY;
        }
        let mean1 = e_sum / n as f64;
        let rel_gate = (LKFS_OFFSET as f64 + 10.0 * mean1.log10()) as f32 - rel_lu;
        // Stage 2: mean energy of blocks above max(abs, relative) gate.
        let gate = abs_gate.max(rel_gate);
        let (mut e2, mut n2) = (0.0f64, 0u64);
        for (i, &c) in self.counts.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let l = Self::bin_center(i);
            if l >= gate {
                e2 += energy(l) as f64 * c as f64;
                n2 += c as u64;
            }
        }
        if n2 == 0 {
            return f32::NEG_INFINITY;
        }
        LKFS_OFFSET + 10.0 * (e2 / n2 as f64).log10() as f32
    }
    /// Loudness range (EBU Tech 3342): the 10th→95th percentile span of the
    /// short-term distribution, gated at `abs_gate` and `rel_lu` below the mean.
    fn lra(&self, abs_gate: f32, rel_lu: f32) -> f32 {
        let energy = |l: f32| 10f32.powf((l + 0.691) / 10.0);
        let (mut e_sum, mut n) = (0.0f64, 0u64);
        for (i, &c) in self.counts.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let l = Self::bin_center(i);
            if l >= abs_gate {
                e_sum += energy(l) as f64 * c as f64;
                n += c as u64;
            }
        }
        if n == 0 {
            return 0.0;
        }
        let rel_gate = (LKFS_OFFSET as f64 + 10.0 * (e_sum / n as f64).log10()) as f32 - rel_lu;
        let gate = abs_gate.max(rel_gate);
        // Cumulative distribution over gated bins.
        let total: u64 = self
            .counts
            .iter()
            .enumerate()
            .filter(|(i, _)| Self::bin_center(*i) >= gate)
            .map(|(_, &c)| c as u64)
            .sum();
        if total == 0 {
            return 0.0;
        }
        let pct = |p: f64| -> f32 {
            let target = p * total as f64;
            let mut acc = 0u64;
            for (i, &c) in self.counts.iter().enumerate() {
                if Self::bin_center(i) < gate || c == 0 {
                    continue;
                }
                acc += c as u64;
                if acc as f64 >= target {
                    return Self::bin_center(i);
                }
            }
            Self::HI
        };
        (pct(0.95) - pct(0.10)).max(0.0)
    }
}

/// BS.1770-4 / EBU R128 loudness meter + BS.1770 Annex 2 true-peak, plus stereo
/// correlation and L/R/M/S levels. Fed one stereo frame at a time from `process()`;
/// RT-safe (pre-allocated, no alloc / no lock in `push`).
pub struct LoudnessMeter {
    sr: f32,
    kw_shelf: Biquad,
    kw_hp: Biquad,
    // Sliding-window mean-square of the K-weighted stereo power (kL²+kR²).
    ring: Vec<f32>,
    wr: usize,          // write head
    filled: usize,      // samples written (caps at ring len)
    n_mom: usize,       // 400 ms window length (samples)
    n_short: usize,     // 3 s window length (samples)
    sum_mom: f64,       // running sum over the last n_mom samples
    sum_short: f64,     // running sum over the last n_short samples
    // Gating: every 100 ms push the current 400 ms / 3 s block to the histograms.
    n_block: usize,     // 100 ms in samples
    since_block: usize,
    hist_i: LoudnessHistogram,  // 400 ms blocks → integrated
    hist_lra: LoudnessHistogram, // 3 s blocks → LRA
    // True peak (4× oversampled, per channel).
    tp: TruePeak,
    tp_hold: f32,
    // Per-block stereo accumulators → smoothed correlation + L/R/M/S dBFS.
    bl_l2: f64,
    bl_r2: f64,
    bl_lr: f64,
    bl_n: u32,
    corr: f32,
    lvl_l: f32,
    lvl_r: f32,
    lvl_m: f32,
    lvl_s: f32,
}

impl LoudnessMeter {
    pub fn new(sr: f32) -> LoudnessMeter {
        let (kw_shelf, kw_hp) = k_weight_filters(sr.max(1.0));
        let ring = vec![0.0; (MAX_SR as f32 * 3.0) as usize + 1];
        let mut m = LoudnessMeter {
            sr: sr.max(1.0),
            kw_shelf,
            kw_hp,
            ring,
            wr: 0,
            filled: 0,
            n_mom: 0,
            n_short: 0,
            sum_mom: 0.0,
            sum_short: 0.0,
            n_block: 0,
            since_block: 0,
            hist_i: LoudnessHistogram::new(),
            hist_lra: LoudnessHistogram::new(),
            tp: TruePeak::new(),
            tp_hold: 0.0,
            bl_l2: 0.0,
            bl_r2: 0.0,
            bl_lr: 0.0,
            bl_n: 0,
            corr: 0.0,
            lvl_l: f32::NEG_INFINITY,
            lvl_r: f32::NEG_INFINITY,
            lvl_m: f32::NEG_INFINITY,
            lvl_s: f32::NEG_INFINITY,
        };
        m.set_sample_rate(sr);
        m
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        let sr = sr.max(1.0);
        let changed = (sr - self.sr).abs() > 0.5 || self.n_mom == 0;
        self.sr = sr;
        if !changed {
            return;
        }
        let (kw_shelf, kw_hp) = k_weight_filters(sr);
        self.kw_shelf = kw_shelf;
        self.kw_hp = kw_hp;
        self.kw_shelf.reset();
        self.kw_hp.reset();
        self.n_mom = ((sr * 0.4) as usize).max(1).min(self.ring.len() - 1);
        self.n_short = ((sr * 3.0) as usize).max(1).min(self.ring.len() - 1);
        self.n_block = ((sr * 0.1) as usize).max(1);
        // Reset the sliding windows (stale samples were at the old rate).
        self.ring.iter_mut().for_each(|v| *v = 0.0);
        self.wr = 0;
        self.filled = 0;
        self.sum_mom = 0.0;
        self.sum_short = 0.0;
        self.since_block = 0;
    }

    /// Reset the integrated/LRA session (histograms + true-peak hold).
    pub fn reset_integration(&mut self) {
        self.hist_i.clear();
        self.hist_lra.clear();
        self.tp_hold = 0.0;
    }

    /// Feed one stereo frame (mono: pass `l == r`). RT-safe.
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        // K-weight each channel, accumulate summed power into the sliding windows.
        let kl = self.kw_hp.process(0, self.kw_shelf.process(0, l));
        let kr = self.kw_hp.process(1, self.kw_shelf.process(1, r));
        let p = (kl * kl + kr * kr) as f64;
        let len = self.ring.len();
        // Subtract the samples leaving each window before overwriting.
        let leave_mom = self.ring[(self.wr + len - self.n_mom) % len] as f64;
        let leave_short = self.ring[(self.wr + len - self.n_short) % len] as f64;
        self.ring[self.wr] = p as f32;
        self.wr = (self.wr + 1) % len;
        self.filled = (self.filled + 1).min(len);
        self.sum_mom += p - if self.filled > self.n_mom { leave_mom } else { 0.0 };
        self.sum_short += p - if self.filled > self.n_short { leave_short } else { 0.0 };
        self.sum_mom = self.sum_mom.max(0.0);
        self.sum_short = self.sum_short.max(0.0);

        // True peak + stereo accumulators.
        let tp = self.tp.push(l, r);
        self.tp_hold = self.tp_hold.max(tp);
        self.bl_l2 += (l * l) as f64;
        self.bl_r2 += (r * r) as f64;
        self.bl_lr += (l * r) as f64;
        self.bl_n += 1;

        // Every 100 ms: push the current 400 ms / 3 s blocks to the gating
        // histograms and refresh the stereo readouts.
        self.since_block += 1;
        if self.since_block >= self.n_block {
            self.since_block = 0;
            if self.filled >= self.n_mom {
                self.hist_i.add(self.momentary());
            }
            if self.filled >= self.n_short {
                self.hist_lra.add(self.short_term());
            }
            self.flush_stereo();
        }
    }

    fn flush_stereo(&mut self) {
        if self.bl_n == 0 {
            return;
        }
        let n = self.bl_n as f64;
        let (l2, r2, lr) = (self.bl_l2 / n, self.bl_r2 / n, self.bl_lr / n);
        // Pearson correlation of L/R (−1..+1); undefined on silence → hold.
        let denom = (l2 * r2).sqrt();
        if denom > 1.0e-12 {
            self.corr = (lr / denom).clamp(-1.0, 1.0) as f32;
        }
        let dbfs = |ms: f64| -> f32 {
            if ms > 1.0e-20 {
                10.0 * ms.log10() as f32
            } else {
                f32::NEG_INFINITY
            }
        };
        // M = (L+R)/2, S = (L−R)/2 mean-square from the sums.
        let m2 = (l2 + r2 + 2.0 * lr) / 4.0;
        let s2 = (l2 + r2 - 2.0 * lr) / 4.0;
        self.lvl_l = dbfs(l2);
        self.lvl_r = dbfs(r2);
        self.lvl_m = dbfs(m2);
        self.lvl_s = dbfs(s2);
        self.bl_l2 = 0.0;
        self.bl_r2 = 0.0;
        self.bl_lr = 0.0;
        self.bl_n = 0;
    }

    /// Momentary loudness (400 ms window), LUFS. Divides by the filled count while
    /// the window ramps up, so it's correct before 400 ms of audio has arrived.
    pub fn momentary(&self) -> f32 {
        let cnt = self.filled.min(self.n_mom).max(1);
        let z = self.sum_mom / cnt as f64;
        if z > 1.0e-12 {
            LKFS_OFFSET + 10.0 * z.log10() as f32
        } else {
            f32::NEG_INFINITY
        }
    }
    /// Short-term loudness (3 s window), LUFS. Ramp-up handled like `momentary`.
    pub fn short_term(&self) -> f32 {
        let cnt = self.filled.min(self.n_short).max(1);
        let z = self.sum_short / cnt as f64;
        if z > 1.0e-12 {
            LKFS_OFFSET + 10.0 * z.log10() as f32
        } else {
            f32::NEG_INFINITY
        }
    }
    /// Gated integrated loudness (BS.1770), LUFS.
    pub fn integrated(&self) -> f32 {
        self.hist_i.integrated(-70.0, 10.0)
    }
    /// Loudness range (EBU 3342), LU.
    pub fn lra(&self) -> f32 {
        self.hist_lra.lra(-70.0, 20.0)
    }
    /// True-peak hold, dBTP (0 dBTP = full scale).
    pub fn true_peak_db(&self) -> f32 {
        if self.tp_hold > 1.0e-9 {
            20.0 * self.tp_hold.log10()
        } else {
            f32::NEG_INFINITY
        }
    }
    pub fn correlation(&self) -> f32 {
        self.corr
    }
    /// L/R/M/S levels in dBFS (RMS).
    pub fn levels_lrms(&self) -> [f32; 4] {
        [self.lvl_l, self.lvl_r, self.lvl_m, self.lvl_s]
    }
    /// K-weighting magnitude at `f` Hz (test/calibration helper).
    #[cfg(test)]
    fn k_gain(&self, f: f32) -> f32 {
        self.kw_shelf.mag_at(self.sr, f) * self.kw_hp.mag_at(self.sr, f)
    }
}

/// 4× oversampled inter-sample true-peak per BS.1770-4 Annex 2. A windowed-sinc
/// polyphase interpolator reconstructs the 3 samples between each pair and takes
/// the max |value|, so a signal that clips *between* samples is caught. Per-channel
/// history, RT-safe.
struct TruePeak {
    // Polyphase coefficients: PHASES × TAPS.
    coeffs: [[f32; Self::TAPS]; Self::PHASES],
    hist: [[f32; Self::TAPS]; 2], // per-channel sample history (newest at [0])
}

impl TruePeak {
    const PHASES: usize = 4;
    const TAPS: usize = 12;
    fn new() -> TruePeak {
        // Windowed-sinc low-pass at the original Nyquist, split into 4 phases.
        let taps = Self::PHASES * Self::TAPS; // 48-tap prototype
        let fc = 0.5 / Self::PHASES as f32; // cutoff = orig Nyquist in the 4× domain
        let mut proto = vec![0.0f32; taps];
        let mut sum = 0.0f32;
        for (n, p) in proto.iter_mut().enumerate() {
            let x = n as f32 - (taps as f32 - 1.0) / 2.0;
            let sinc = if x.abs() < 1.0e-6 {
                2.0 * fc
            } else {
                (std::f32::consts::TAU * fc * x).sin() / (std::f32::consts::PI * x)
            };
            // Blackman window.
            let w = 0.42
                - 0.5 * (std::f32::consts::TAU * n as f32 / (taps as f32 - 1.0)).cos()
                + 0.08 * (2.0 * std::f32::consts::TAU * n as f32 / (taps as f32 - 1.0)).cos();
            *p = sinc * w;
            sum += *p;
        }
        // Normalise so each phase preserves amplitude (total gain = PHASES).
        let norm = Self::PHASES as f32 / sum;
        let mut coeffs = [[0.0f32; Self::TAPS]; Self::PHASES];
        for ph in 0..Self::PHASES {
            for t in 0..Self::TAPS {
                // Phase `ph` gathers every PHASES-th tap.
                coeffs[ph][t] = proto[t * Self::PHASES + ph] * norm;
            }
        }
        TruePeak { coeffs, hist: [[0.0; Self::TAPS]; 2] }
    }

    /// Push one stereo frame, return the max |inter-sample value| across both
    /// channels and all 4 phases.
    #[inline]
    fn push(&mut self, l: f32, r: f32) -> f32 {
        let mut peak = 0.0f32;
        for (ch, x) in [l, r].into_iter().enumerate() {
            let h = &mut self.hist[ch];
            // Shift newest in at [0].
            for i in (1..Self::TAPS).rev() {
                h[i] = h[i - 1];
            }
            h[0] = x;
            for ph in 0..Self::PHASES {
                let mut acc = 0.0f32;
                for t in 0..Self::TAPS {
                    acc += self.coeffs[ph][t] * h[t];
                }
                peak = peak.max(acc.abs());
            }
        }
        peak
    }
}

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Lock-free one-writer/one-reader channel carrying the live analysis to the
/// editor's Audio panel. The audio thread `publish`es each block; the GUI thread
/// `snapshot`s each repaint. f32s ride as bit patterns in `AtomicU32`; all loads
/// are `Relaxed` (a torn frame is harmless for a meter).
pub struct AudioViz {
    active: AtomicBool,
    level: AtomicU32,
    peak: AtomicU32,
    bands: [AtomicU32; NUM_BANDS],
    spectrum: [AtomicU32; DISP_BINS],
    // #333: calibrated meters — [M, S, I, LRA, dBTP, corr, L, R, M, S] dBFS/LUFS.
    meters: [AtomicU32; 10],
    // #333: calibrated RTA — band levels (dBFS), count, resolution + band-0 center.
    rta: [AtomicU32; MAX_RTA_BANDS],
    rta_n: AtomicU32,
    rta_res: AtomicU32,
    rta_c0: AtomicU32,
}

/// A consistent-enough read of the live analysis for one repaint.
#[derive(Clone)]
pub struct VizFrame {
    pub active: bool,
    pub level: f32,
    pub peak: f32,
    pub bands: [f32; NUM_BANDS],
    pub spectrum: [f32; DISP_BINS],
    /// [momentary, short, integrated, LRA, true_peak_dBTP, correlation, L, R, M, S].
    pub meters: [f32; 10],
    /// Calibrated RTA band levels, dBFS (first `rta_n` valid).
    pub rta: [f32; MAX_RTA_BANDS],
    pub rta_n: usize,
    pub rta_res: u32,
    pub rta_c0: f32,
}

/// The calibrated meters in their **silent / inactive** state (#333): dB fields at
/// the −120 floor, LRA + correlation at 0. Used to seed `AudioViz` and to publish
/// when Audio Reactive is off — so the editor never misreads the zero-bit default as
/// a full-scale 0 dBFS (which would falsely trip the true-peak-over alarm).
/// `[M, S, I, LRA, dBTP, corr, L, R, M, S]`.
pub const SILENT_METERS: [f32; 10] =
    [-120.0, -120.0, -120.0, 0.0, -120.0, 0.0, -120.0, -120.0, -120.0, -120.0];

impl AudioViz {
    pub fn new() -> Self {
        AudioViz {
            active: AtomicBool::new(false),
            level: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            bands: std::array::from_fn(|_| AtomicU32::new(0)),
            spectrum: std::array::from_fn(|_| AtomicU32::new(0)),
            // Seed the meters to the silent floor, not 0.0 (= full scale in dBFS).
            meters: std::array::from_fn(|i| AtomicU32::new(SILENT_METERS[i].to_bits())),
            rta: std::array::from_fn(|_| AtomicU32::new((-120.0f32).to_bits())),
            rta_n: AtomicU32::new(0),
            rta_res: AtomicU32::new(3),
            rta_c0: AtomicU32::new(0),
        }
    }

    /// Publish the latest analysis (audio thread, RT-safe — just atomic stores).
    pub fn publish(
        &self,
        active: bool,
        level: f32,
        peak: f32,
        bands: &[f32; NUM_BANDS],
        spectrum: &[f32; DISP_BINS],
    ) {
        self.active.store(active, Ordering::Relaxed);
        self.level.store(level.to_bits(), Ordering::Relaxed);
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
        for i in 0..NUM_BANDS {
            self.bands[i].store(bands[i].to_bits(), Ordering::Relaxed);
        }
        for i in 0..DISP_BINS {
            self.spectrum[i].store(spectrum[i].to_bits(), Ordering::Relaxed);
        }
    }

    /// Publish the calibrated meters + RTA (#333). Separate call so the expressive
    /// `publish` above stays untouched. Non-finite dB is floored for display.
    pub fn publish_meters(&self, meters: &[f32; 10], rta: &[f32], res: u32, c0: f32) {
        for i in 0..10 {
            let v = if meters[i].is_finite() { meters[i] } else { -120.0 };
            self.meters[i].store(v.to_bits(), Ordering::Relaxed);
        }
        let n = rta.len().min(MAX_RTA_BANDS);
        for i in 0..n {
            let v = if rta[i].is_finite() { rta[i] } else { -120.0 };
            self.rta[i].store(v.to_bits(), Ordering::Relaxed);
        }
        self.rta_n.store(n as u32, Ordering::Relaxed);
        self.rta_res.store(res, Ordering::Relaxed);
        self.rta_c0.store(c0.to_bits(), Ordering::Relaxed);
    }

    /// Read the latest analysis (GUI thread).
    pub fn snapshot(&self) -> VizFrame {
        VizFrame {
            active: self.active.load(Ordering::Relaxed),
            level: f32::from_bits(self.level.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.peak.load(Ordering::Relaxed)),
            bands: std::array::from_fn(|i| f32::from_bits(self.bands[i].load(Ordering::Relaxed))),
            spectrum: std::array::from_fn(|i| {
                f32::from_bits(self.spectrum[i].load(Ordering::Relaxed))
            }),
            meters: std::array::from_fn(|i| f32::from_bits(self.meters[i].load(Ordering::Relaxed))),
            rta: std::array::from_fn(|i| f32::from_bits(self.rta[i].load(Ordering::Relaxed))),
            rta_n: self.rta_n.load(Ordering::Relaxed) as usize,
            rta_res: self.rta_res.load(Ordering::Relaxed),
            rta_c0: f32::from_bits(self.rta_c0.load(Ordering::Relaxed)),
        }
    }
}

impl Default for AudioViz {
    fn default() -> Self {
        Self::new()
    }
}

/// Lock-free raw-waveform ring for the oscilloscope (#333, s(M)exoscope-style). The
/// audio thread `push`es every stereo frame; the GUI reads the most recent window
/// each repaint and does all scope processing (zoom / trigger / freeze) display-side.
/// Single writer (audio thread), single reader (GUI) — torn reads are harmless for a
/// scope. Pre-allocated, no alloc / no lock in `push`.
pub struct ScopeRing {
    l: Vec<AtomicU32>,
    r: Vec<AtomicU32>,
    w: AtomicUsize, // total frames written (monotonic)
    cap: usize,
    sr: AtomicU32, // sample rate (f32 bits), for the scope's time axis / triggers
}

impl ScopeRing {
    /// ~1.4 s of stereo history at 48 kHz — plenty for a live scope's widest sweep.
    pub const CAP: usize = 1 << 16;

    pub fn new() -> ScopeRing {
        ScopeRing {
            l: (0..Self::CAP).map(|_| AtomicU32::new(0)).collect(),
            r: (0..Self::CAP).map(|_| AtomicU32::new(0)).collect(),
            w: AtomicUsize::new(0),
            cap: Self::CAP,
            sr: AtomicU32::new(48_000f32.to_bits()),
        }
    }

    pub fn set_sample_rate(&self, sr: f32) {
        self.sr.store(sr.max(1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sr.load(Ordering::Relaxed))
    }

    /// Push one stereo frame (audio thread). Store the sample, then bump the count so
    /// the reader never sees a slot before it's written.
    #[inline]
    pub fn push(&self, l: f32, r: f32) {
        let seq = self.w.load(Ordering::Relaxed);
        let i = seq % self.cap;
        self.l[i].store(l.to_bits(), Ordering::Relaxed);
        self.r[i].store(r.to_bits(), Ordering::Relaxed);
        self.w.store(seq.wrapping_add(1), Ordering::Relaxed);
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }
    pub fn written(&self) -> usize {
        self.w.load(Ordering::Relaxed)
    }

    /// Copy the most recent `n` samples of channel `ch` (0 = L, 1 = R) into `out`,
    /// oldest first. Fewer than `n` early on; capped at the ring capacity.
    pub fn read_recent(&self, ch: usize, n: usize, out: &mut Vec<f32>) {
        out.clear();
        let total = self.written();
        let n = n.min(self.cap).min(total);
        if n == 0 {
            return;
        }
        let start = total - n;
        let src = if ch == 1 { &self.r } else { &self.l };
        out.reserve(n);
        for i in 0..n {
            let idx = (start + i) % self.cap;
            out.push(f32::from_bits(src[idx].load(Ordering::Relaxed)));
        }
    }

    /// Like `read_recent`, but `ch == 2` reads the **Mid** (L+R)/2 (the Field
    /// Chamber wall scope's default). `out`'s capacity is reused across calls, so
    /// after the first call there is no allocation (audio-thread safe).
    pub fn read_recent_mid(&self, ch: usize, n: usize, out: &mut Vec<f32>) {
        if ch != 2 {
            self.read_recent(ch, n, out);
            return;
        }
        out.clear();
        let total = self.written();
        let n = n.min(self.cap).min(total);
        if n == 0 {
            return;
        }
        let start = total - n;
        out.reserve(n);
        for i in 0..n {
            let idx = (start + i) % self.cap;
            let l = f32::from_bits(self.l[idx].load(Ordering::Relaxed));
            let r = f32::from_bits(self.r[idx].load(Ordering::Relaxed));
            out.push(0.5 * (l + r));
        }
    }
}

/// Build a triggered, downsampled oscilloscope **display frame** for the Field
/// Chamber wall (#346). `src` = recent samples (oldest→newest); its length should
/// be at least `span` (ideally `span` + a search margin so the trigger has room).
/// `span` = how many source samples the display window covers. `trigger`:
/// 0 = Free (newest `span` samples), 1 = Rising, 2 = Falling (align the window to
/// the first `level`-crossing edge in the search margin). Writes exactly `out.len()`
/// points into `out` (linearly resampling the `span`-sample window), and returns the
/// number written + whether a trigger edge was locked. Pure + deterministic
/// (unit-tested); no allocation.
pub fn scope_frame_into(src: &[f32], span: usize, out: &mut [f32], trigger: u32, level: f32) -> (usize, bool) {
    let out_len = out.len();
    if out_len == 0 {
        return (0, false);
    }
    if src.is_empty() {
        for v in out.iter_mut() {
            *v = 0.0;
        }
        return (0, false);
    }
    let span = span.clamp(2, src.len());
    // Trigger search window is everything before the last `span` samples.
    let search_end = src.len() - span;
    let mut start = search_end; // Free / no-edge fallback = newest `span` samples.
    let mut locked = false;
    if trigger == 1 || trigger == 2 {
        // Scan the search window from the MOST RECENT candidate backward and lock the
        // LATEST edge before the live region — so the wall trace tracks "now", not the
        // oldest crossing in the buffer (which would show stale waveform). (Bugbot)
        let mut i = search_end;
        while i >= 1 {
            let a = src[i - 1];
            let b = src[i];
            let edge = if trigger == 1 {
                a < level && b >= level
            } else {
                a > level && b <= level
            };
            if edge {
                start = i;
                locked = true;
                break;
            }
            i -= 1;
        }
    }
    // Linearly resample the `span`-sample window [start, start+span) → `out_len` points
    // (proper interpolation between the two straddling samples, not nearest-neighbor).
    let last = src.len() - 1;
    for (j, o) in out.iter_mut().enumerate() {
        let t = if out_len > 1 {
            j as f32 * (span as f32 - 1.0) / (out_len as f32 - 1.0)
        } else {
            0.0
        };
        let pos = start as f32 + t;
        let i0 = (pos.floor() as usize).min(last);
        let i1 = (i0 + 1).min(last);
        let frac = pos - i0 as f32;
        *o = src[i0] * (1.0 - frac) + src[i1] * frac;
    }
    (out_len, locked)
}

impl Default for ScopeRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the analyzer with `cycles` worth of a sine at `freq` Hz, one block.
    fn feed_sine(a: &mut Analyzer, sr: f32, freq: f32, amp: f32, n: usize, phase0: f32) -> f32 {
        let mut phase = phase0;
        let step = std::f32::consts::TAU * freq / sr;
        for _ in 0..n {
            a.push_sample(amp * phase.sin());
            phase += step;
        }
        phase
    }

    #[test]
    fn bass_tone_lights_the_bass_band() {
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(5.0, 200.0);
        // Fill the ring with a 80 Hz tone (lands in the "bass" band, index 1).
        let mut ph = 0.0;
        for _ in 0..(FFT_SIZE / 256 + 4) {
            ph = feed_sine(&mut a, sr, 80.0, 1.0, 256, ph);
            a.analyze(1.0, 256.0 / sr);
        }
        let bands = a.bands();
        // The bass band should clearly dominate the high band.
        assert!(bands[1] > 0.05, "bass band too quiet: {bands:?}");
        assert!(bands[1] > bands[4] * 4.0, "bass not dominant: {bands:?}");
    }

    #[test]
    fn envelope_releases_on_silence() {
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(5.0, 80.0);
        let mut ph = 0.0;
        for _ in 0..16 {
            ph = feed_sine(&mut a, sr, 80.0, 1.0, 256, ph);
            a.analyze(1.0, 256.0 / sr);
        }
        let loud = a.bands()[1];
        // Now feed silence and let it decay.
        for _ in 0..40 {
            for _ in 0..256 {
                a.push_sample(0.0);
            }
            a.analyze(1.0, 256.0 / sr);
        }
        let quiet = a.bands()[1];
        assert!(quiet < loud * 0.2, "envelope did not release: {loud} → {quiet}");
    }

    #[test]
    fn level_and_spectrum_track_a_tone() {
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(5.0, 200.0);
        // Silence first: level + spectrum stay at zero.
        for _ in 0..8 {
            for _ in 0..256 {
                a.push_sample(0.0);
            }
            a.analyze(1.0, 256.0 / sr);
        }
        assert!(a.input_level() < 1.0e-3, "level not zero on silence");
        // Now a sustained bass tone: the broadband level rises and the display
        // spectrum lights up somewhere in its low half (80 Hz).
        let mut ph = 0.0;
        for _ in 0..20 {
            ph = feed_sine(&mut a, sr, 80.0, 1.0, 256, ph);
            a.analyze(1.0, 256.0 / sr);
        }
        assert!(a.input_level() > 0.05, "level did not rise: {}", a.input_level());
        let disp = a.display_spectrum();
        let low: f32 = disp[..DISP_BINS / 2].iter().cloned().fold(0.0, f32::max);
        let high: f32 = disp[DISP_BINS / 2..].iter().cloned().fold(0.0, f32::max);
        assert!(low > 0.01, "display spectrum dark in the low half: {disp:?}");
        assert!(low > high, "80 Hz tone should peak in the low half: low={low} high={high}");
    }

    #[test]
    fn centroid_tracks_brightness_and_holds_on_silence() {
        // #248 Tier 3: a bass tone parks the centroid low; a bright tone drives
        // it high; silence holds the last value rather than jittering.
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(5.0, 50.0);
        let mut ph = 0.0;
        for _ in 0..30 {
            ph = feed_sine(&mut a, sr, 80.0, 1.0, 256, ph);
            a.analyze(1.0, 256.0 / sr);
        }
        let low = a.spectral_centroid();
        assert!(low < 0.4, "80 Hz should read dark: {low}");
        let mut a2 = Analyzer::new(sr);
        a2.set_times(5.0, 50.0);
        let mut ph2 = 0.0;
        for _ in 0..30 {
            ph2 = feed_sine(&mut a2, sr, 8000.0, 1.0, 256, ph2);
            a2.analyze(1.0, 256.0 / sr);
        }
        let high = a2.spectral_centroid();
        assert!(high > 0.6, "8 kHz should read bright: {high}");
        assert!(high > low + 0.2, "centroid must separate dark from bright");
        // Silence: the centroid holds (the envelopes release, the centroid doesn't).
        for _ in 0..40 {
            for _ in 0..256 {
                a2.push_sample(0.0);
            }
            a2.analyze(1.0, 256.0 / sr);
        }
        assert!((a2.spectral_centroid() - high).abs() < 0.05, "centroid must hold on silence");
    }

    #[test]
    fn balance_leans_with_the_mix_and_holds_on_silence() {
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(5.0, 30.0);
        // Hard-left energy for a while → balance swings negative.
        for _ in 0..60 {
            a.update_balance(1.0, 0.0, 256.0 / sr);
        }
        let l = a.stereo_balance();
        assert!(l < -0.8, "hard left must lean the balance left: {l}");
        // Swing right.
        for _ in 0..60 {
            a.update_balance(0.0, 1.0, 256.0 / sr);
        }
        assert!(a.stereo_balance() > 0.8);
        // Mono/centred energy re-centres…
        for _ in 0..60 {
            a.update_balance(0.5, 0.5, 256.0 / sr);
        }
        assert!(a.stereo_balance().abs() < 0.1);
        // …and silence holds instead of snapping.
        let held = a.stereo_balance();
        for _ in 0..20 {
            a.update_balance(0.0, 0.0, 256.0 / sr);
        }
        assert_eq!(a.stereo_balance(), held);
    }

    #[test]
    fn attack_is_faster_than_release() {
        // With a step input, a fast attack reaches a higher fraction in N blocks
        // than a slow release falls back over the same N blocks.
        let sr = 48_000.0;
        let mut a = Analyzer::new(sr);
        a.set_times(2.0, 500.0);
        // Prime to steady state on a loud tone.
        let mut ph = 0.0;
        for _ in 0..30 {
            ph = feed_sine(&mut a, sr, 80.0, 1.0, 256, ph);
            a.analyze(1.0, 256.0 / sr);
        }
        let steady = a.bands()[1];
        // One block of silence: slow release should barely move it.
        for _ in 0..256 {
            a.push_sample(0.0);
        }
        a.analyze(1.0, 256.0 / sr);
        let after_one = a.bands()[1];
        assert!(after_one > steady * 0.5, "release too fast: {steady} → {after_one}");
    }

    // ---- #307 Tier 1: BPM detection ----

    #[test]
    fn tempo_estimator_locks_to_an_impulse_train() {
        // 120 BPM = a beat every 0.5 s = every 50 bins at 100 Hz. Feed a clean
        // onset impulse train (one bin per push, so 1 push = 1 bin).
        let bin_dt = 1.0 / TempoEstimator::BIN_HZ;
        let mut te = TempoEstimator::new();
        for i in 0..900 {
            let onset = if i % 50 == 0 { 1.0 } else { 0.0 };
            te.push(onset, bin_dt);
        }
        assert!(
            (te.bpm() - 120.0).abs() < 3.0,
            "expected ~120 BPM, got {} (conf {})",
            te.bpm(),
            te.confidence()
        );
        assert!(te.confidence() > TempoEstimator::CONF_TRUST, "no lock");
    }

    #[test]
    fn tempo_estimator_holds_bpm_through_a_breakdown() {
        let bin_dt = 1.0 / TempoEstimator::BIN_HZ;
        let mut te = TempoEstimator::new();
        // Lock to 128 BPM (a beat every ~46.9 bins).
        let period = (TempoEstimator::BIN_HZ * 60.0 / 128.0).round() as i32;
        for i in 0..900i32 {
            let onset = if i % period == 0 { 1.0 } else { 0.0 };
            te.push(onset, bin_dt);
        }
        let locked = te.bpm();
        assert!(locked > 100.0, "did not lock: {locked}");
        // Breakdown: pure silence for ~4 s. The BPM must be held, not zeroed.
        for _ in 0..400 {
            te.push(0.0, bin_dt);
        }
        assert!(
            (te.bpm() - locked).abs() < 1.0,
            "BPM not held through breakdown: {locked} → {}",
            te.bpm()
        );
    }

    // --- #333 Tier 1: calibrated loudness + true-peak ----------------------

    /// Drive the loudness meter with a stereo sine for `secs` seconds.
    fn feed_loudness(m: &mut LoudnessMeter, sr: f32, freq: f32, amp: f32, secs: f32) {
        let n = (sr * secs) as usize;
        let step = std::f32::consts::TAU * freq / sr;
        let mut ph = 0.0f32;
        for _ in 0..n {
            let s = amp * ph.sin();
            m.push(s, s);
            ph += step;
        }
    }

    #[test]
    fn k_weighting_is_near_unity_at_1khz() {
        let m = LoudnessMeter::new(48_000.0);
        let g = m.k_gain(1000.0);
        // K-weighting is defined to be ~0 dB near 1 kHz.
        assert!((20.0 * g.log10()).abs() < 0.7, "K-gain@1k = {} dB", 20.0 * g.log10());
        // And it boosts the highs (high-shelf) and cuts the lows (RLB HP).
        assert!(m.k_gain(6000.0) > g, "no HF shelf boost");
        assert!(m.k_gain(40.0) < g, "no LF rolloff");
    }

    #[test]
    fn loudness_of_minus20dbfs_1khz_sine() {
        let sr = 48_000.0;
        let mut m = LoudnessMeter::new(sr);
        // −20 dBFS = amplitude 0.1, in both channels. Feed > 3 s so the short-term
        // window fills and the integrated histogram accumulates gated blocks.
        feed_loudness(&mut m, sr, 1000.0, 0.1, 4.0);
        // Expected: L = −0.691 + 10log10(2·(0.1·kgain)²/2). kgain≈1 → ≈ −20.7 LUFS.
        let kg = m.k_gain(1000.0);
        let expected = LKFS_OFFSET + 10.0 * (2.0 * (0.1 * kg).powi(2) / 2.0).log10();
        assert!((m.momentary() - expected).abs() < 0.3, "M {} vs {}", m.momentary(), expected);
        assert!((m.short_term() - expected).abs() < 0.3, "S {} vs {}", m.short_term(), expected);
        assert!((m.integrated() - expected).abs() < 0.5, "I {} vs {}", m.integrated(), expected);
        // Steady tone → LRA ≈ 0.
        assert!(m.lra() < 1.0, "LRA {} should be ~0 for a steady tone", m.lra());
    }

    #[test]
    fn loudness_scales_by_6db_when_amplitude_doubles() {
        let sr = 48_000.0;
        let mut a = LoudnessMeter::new(sr);
        let mut b = LoudnessMeter::new(sr);
        feed_loudness(&mut a, sr, 1000.0, 0.1, 1.0);
        feed_loudness(&mut b, sr, 1000.0, 0.2, 1.0);
        assert!(((b.momentary() - a.momentary()) - 6.02).abs() < 0.2, "doubling ≠ +6 dB");
    }

    #[test]
    fn true_peak_catches_inter_sample_over() {
        let sr = 48_000.0;
        let mut m = LoudnessMeter::new(sr);
        // The canonical inter-sample-over signal: a tone at fs/4 phase-shifted 45°,
        // so the SAMPLES sit at ±amp·0.707 (−3 dB) while the true waveform peaks at
        // amp. A good true-peak meter reads ~3 dB higher than the sample peak.
        let amp = 0.98;
        // Exact 4-sample pattern for fs/4 at 45°: ±amp·0.7071 (no phase drift).
        let pat = [
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
            -std::f32::consts::FRAC_1_SQRT_2,
            -std::f32::consts::FRAC_1_SQRT_2,
        ];
        let mut sample_peak = 0.0f32;
        for n in 0..(sr as usize) {
            let s = amp * pat[n % 4];
            sample_peak = sample_peak.max(s.abs());
            m.push(s, s);
        }
        let sample_db = 20.0 * sample_peak.log10();
        let tp_db = m.true_peak_db();
        // The reconstructed peak must be well above the sample peak (the over is
        // caught) and land near the true 0.98 amplitude (−0.18 dBTP), not overshoot.
        assert!(tp_db > sample_db + 1.5, "TP {tp_db} not clearly > sample {sample_db}");
        assert!(tp_db > -1.2 && tp_db < 0.3, "TP {tp_db} should be ≈ −0.18 dBTP");
    }

    #[test]
    fn stereo_correlation_is_plus_one_for_mono_minus_one_for_antiphase() {
        let sr = 48_000.0;
        let step = std::f32::consts::TAU * 400.0 / sr;
        let mut mono = LoudnessMeter::new(sr);
        let mut anti = LoudnessMeter::new(sr);
        let mut ph = 0.0f32;
        for _ in 0..(sr as usize / 2) {
            let s = 0.5 * ph.sin();
            mono.push(s, s);
            anti.push(s, -s);
            ph += step;
        }
        assert!(mono.correlation() > 0.99, "mono corr {}", mono.correlation());
        assert!(anti.correlation() < -0.99, "antiphase corr {}", anti.correlation());
    }

    // --- #333 Tier 2: calibrated spectrum ---------------------------------

    #[test]
    fn a_weighting_curve_shape() {
        assert!(a_weight_db(1000.0).abs() < 0.05, "A(1k) ≠ 0");
        assert!(a_weight_db(100.0) < -15.0, "A(100) not strongly cut");
        assert!(c_weight_db(1000.0).abs() < 0.05, "C(1k) ≠ 0");
        assert!(c_weight_db(100.0) > a_weight_db(100.0), "C should cut less than A at 100 Hz");
    }

    #[test]
    fn calibrated_spectrum_full_scale_sine_reads_near_0dbfs() {
        let sr = 48_000.0;
        let mut an = Analyzer::new(sr);
        an.set_times(1.0, 1.0);
        let mut cs = CalibratedSpectrum::new(sr);
        cs.configure(sr, 3, 0, 0); // 1/3-octave, Z-weight, fast
        // Bin-centered 1 kHz full-scale sine.
        let bin_hz = sr / FFT_SIZE as f32;
        let freq = (1000.0 / bin_hz).round() * bin_hz;
        let mut ph = 0.0;
        for _ in 0..40 {
            ph = feed_sine(&mut an, sr, freq, 1.0, 256, ph);
            an.analyze(1.0, 256.0 / sr);
            cs.update(an.raw_bins(), FFT_SIZE, an.window_energy(), 256.0 / sr);
        }
        // Find the band containing 1 kHz and check it reads ≈ 0 dBFS.
        let levels = cs.levels_db();
        let mut best = (0usize, f32::MAX);
        for i in 0..cs.n_bands() {
            let d = (cs.center_hz(i) - freq).abs();
            if d < best.1 {
                best = (i, d);
            }
        }
        let db = levels[best.0];
        assert!((db - 0.0).abs() < 1.5, "full-scale tone band = {db} dBFS (want ≈ 0)");
        // Neighbouring bands (no signal) must be far below.
        if best.0 + 2 < cs.n_bands() {
            assert!(levels[best.0 + 2] < db - 20.0, "energy leaked into a far band");
        }
    }

    #[test]
    fn audioviz_meters_default_to_silent_not_full_scale() {
        // Before any publish, the editor must read a SILENT meter, not the zero-bit
        // default (= 0.0 = full-scale 0 dBFS), which would false-trip the TP alarm.
        let f = AudioViz::new().snapshot();
        assert!(f.meters[4] <= -1.0, "dBTP init {} would false-trip the over alarm", f.meters[4]);
        for i in [0, 1, 2, 6, 7, 8, 9] {
            assert!(f.meters[i] <= -100.0, "meter {i} init not floored: {}", f.meters[i]);
        }
        assert_eq!(f.meters[3], 0.0, "LRA init should be 0");
        assert_eq!(f.meters[5], 0.0, "correlation init should be 0");
        assert!(f.rta.iter().all(|&d| d <= -100.0), "RTA bins init not floored");
    }

    #[test]
    fn averaging_mode_switch_resets_leq_accumulators() {
        let sr = 48_000.0;
        let mut cs = CalibratedSpectrum::new(sr);
        let bins = vec![Complex32::new(0.0, 0.0); FFT_SIZE / 2 + 1];
        let win = 0.375 * FFT_SIZE as f32;
        cs.configure(sr, 3, 0, 3); // Leq
        for _ in 0..5 {
            cs.update(&bins, FFT_SIZE, win, 0.01);
        }
        assert!(cs.leq_n > 0, "Leq should have accumulated blocks");
        // Switch averaging only (same res/weight): must reset the Leq state, not blend.
        cs.configure(sr, 3, 0, 0); // Fast
        assert_eq!(cs.leq_n, 0, "Leq accumulators must reset on an averaging-mode switch");
        assert!(cs.leq_pow.iter().all(|&p| p == 0.0), "leq_pow not cleared");
    }

    #[test]
    fn calibrated_spectrum_band_count_grows_with_resolution() {
        let sr = 48_000.0;
        let mut cs = CalibratedSpectrum::new(sr);
        cs.configure(sr, 1, 0, 0);
        let one = cs.n_bands();
        cs.configure(sr, 12, 0, 0);
        let twelve = cs.n_bands();
        assert!(twelve > one * 6, "1/12 ({twelve}) should be ~12× 1/1 ({one})");
        assert!(twelve <= MAX_RTA_BANDS, "band count overflow");
        // Linear (raw FFT) mode fills the full band table.
        cs.configure(sr, 0, 0, 0);
        assert_eq!(cs.n_bands(), MAX_RTA_BANDS, "linear mode should be full width");
    }

    /// Feed the analyzer + calibrated spectrum an equal-power-per-octave signal:
    /// octave-spaced, equal-amplitude sines. With one tone per octave band, equal
    /// amplitude ⇒ equal band power ⇒ a flat octave RTA (the pink-noise property).
    fn feed_octave_tones(an: &mut Analyzer, cs: &mut CalibratedSpectrum, sr: f32, blocks: usize) {
        let freqs: Vec<f32> = (0..9).map(|k| 31.25 * 2f32.powi(k)).collect();
        let mut phs = vec![0.0f32; freqs.len()];
        for _ in 0..blocks {
            for _ in 0..256 {
                let mut s = 0.0f32;
                for (i, &f) in freqs.iter().enumerate() {
                    s += phs[i].sin(); // equal amplitude → equal power per octave
                    phs[i] += std::f32::consts::TAU * f / sr;
                }
                an.push_sample(s * 0.05);
            }
            an.analyze(1.0, 256.0 / sr);
            cs.update(an.raw_bins(), FFT_SIZE, an.window_energy(), 256.0 / sr);
        }
    }

    #[test]
    fn pink_noise_reads_flat_per_octave_under_z_weighting() {
        let sr = 48_000.0;
        let mut an = Analyzer::new(sr);
        an.set_times(1.0, 1.0);
        let mut cs = CalibratedSpectrum::new(sr);
        cs.configure(sr, 1, 0, 3); // 1/1-octave, Z-weight, Leq
        feed_octave_tones(&mut an, &mut cs, sr, 60);
        let levels = cs.levels_db();
        // Equal power per octave → the octave bands carrying the tones read within a
        // few dB of each other (a flat RTA). Compare the mid octaves (indices 2..7).
        let mid: Vec<f32> = (0..cs.n_bands())
            .filter(|&i| cs.center_hz(i) >= 100.0 && cs.center_hz(i) <= 4000.0)
            .map(|i| levels[i])
            .filter(|v| v.is_finite() && *v > -60.0)
            .collect();
        assert!(mid.len() >= 4, "not enough populated octave bands: {mid:?}");
        let max = mid.iter().cloned().fold(f32::MIN, f32::max);
        let min = mid.iter().cloned().fold(f32::MAX, f32::min);
        assert!(max - min < 4.0, "pink noise not flat per octave: span {:.1} dB {mid:?}", max - min);
    }

    #[test]
    fn swept_tone_lands_in_the_right_third_octave_band() {
        let sr = 48_000.0;
        let bin_hz = sr / FFT_SIZE as f32;
        // Test a few frequencies across the spectrum.
        for &target in &[250.0f32, 1000.0, 4000.0] {
            let freq = (target / bin_hz).round() * bin_hz; // bin-centered
            let mut an = Analyzer::new(sr);
            an.set_times(1.0, 1.0);
            let mut cs = CalibratedSpectrum::new(sr);
            cs.configure(sr, 3, 0, 0); // 1/3-octave, Z, fast
            let mut ph = 0.0;
            for _ in 0..40 {
                ph = feed_sine(&mut an, sr, freq, 1.0, 256, ph);
                an.analyze(1.0, 256.0 / sr);
                cs.update(an.raw_bins(), FFT_SIZE, an.window_energy(), 256.0 / sr);
            }
            let levels = cs.levels_db();
            // The loudest band must be the one whose 1/3-octave span contains `freq`,
            // and it must read ≈ 0 dBFS (full-scale tone).
            let loudest = (0..cs.n_bands()).max_by(|&a, &b| levels[a].total_cmp(&levels[b])).unwrap();
            let ratio = freq / cs.center_hz(loudest);
            let half = 2f32.powf(0.5 / 3.0);
            assert!(ratio > 1.0 / half && ratio < half, "{freq} Hz not in the loudest band");
            assert!((levels[loudest]).abs() < 1.5, "{freq} Hz band = {} dBFS (want ≈ 0)", levels[loudest]);
        }
    }

    // #346 Field Chamber: the oscilloscope display-frame builder is deterministic.
    #[test]
    fn scope_frame_free_takes_the_newest_window() {
        // src = 0,1,2,...,99; free trigger, span 10 → newest 10 samples resampled.
        let src: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let mut out = [0.0f32; 10];
        let (n, locked) = scope_frame_into(&src, 10, &mut out, 0, 0.0);
        assert_eq!(n, 10);
        assert!(!locked, "free mode never locks");
        // window starts at src.len()-span = 90; endpoints exact.
        assert_eq!(out[0], 90.0);
        assert_eq!(out[9], 99.0);
    }

    #[test]
    fn scope_frame_rising_locks_on_the_zero_crossing() {
        // A sine with several periods; a rising-edge trigger should start the window
        // at (near) a zero-up-crossing, so out[0] ≈ 0 and the next point is > 0.
        let sr = 48_000.0f32;
        let freq = 1000.0f32;
        let n = 2048usize;
        let src: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / sr).sin())
            .collect();
        let span = 256usize;
        let mut out = [0.0f32; 256];
        let (_c, locked) = scope_frame_into(&src, span, &mut out, 1, 0.0);
        assert!(locked, "a multi-period sine must present a rising edge");
        assert!(out[0].abs() < 0.05, "trigger point not near zero: {}", out[0]);
        assert!(out[1] > out[0], "not a rising edge: {} → {}", out[0], out[1]);
    }

    #[test]
    fn scope_frame_handles_short_and_empty() {
        let mut out = [9.0f32; 8];
        let (n, locked) = scope_frame_into(&[], 10, &mut out, 1, 0.0);
        assert_eq!((n, locked), (0, false));
        assert!(out.iter().all(|&v| v == 0.0), "empty src must clear out");
        // span longer than src is clamped, still fills out.
        let src = [0.0, 1.0, 0.0, -1.0];
        let mut o2 = [0.0f32; 4];
        let (n2, _) = scope_frame_into(&src, 100, &mut o2, 0, 0.0);
        assert_eq!(n2, 4);
    }
}
