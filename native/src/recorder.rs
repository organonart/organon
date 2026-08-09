//! In-app production recorder (#430 Tier 0).
//!
//! Reads the **production texture** (the fixed-resolution, aspect-correct offscreen
//! target `capture.rs` already renders the scene+composite into) back to the CPU each
//! frame and pipes it to a spawned **ffmpeg** process, producing a finished video file
//! — no external screen capture in the path, so the recording is frame-exact and never
//! goes through the compositor's display tone-map. Beat-synced start/stop is driven by
//! the caller (`bin/visual.rs`), which counts bars off `beat_pos`.
//!
//! Visual-only, `#[path]`-included like `render.rs` (never compiled into the plugin
//! cdylib). The colour maths (linear-EDR → Rec.2020 PQ), the CFR frame pacing, and the
//! ffmpeg arg builder are pure functions, unit-tested below.
//!
//! ## Why recording doesn't cost frames
//! The readback is **pipelined**, and every per-frame CPU cost lives on a worker thread.
//! Per recorded frame the render thread does only: copy the texture into a free ring slot,
//! submit, and start an **async** map. A non-blocking `poll` later fires the map callback,
//! which hands the mapped buffer to the encode worker — the worker owns the (expensive,
//! per-pixel) HDR PQ/Rec.2020 conversion and the blocking write into ffmpeg's pipe, then
//! unmaps and returns the slot to the ring.
//!
//! The naive version of this (map + `poll(Wait)` + encode + write, all inline) is a full
//! CPU↔GPU sync every frame and cost ~a third of the frame rate. Nothing here waits on the
//! GPU: if all ring slots are still in flight the frame is skipped and the CFR clock
//! duplicates, so a hitch changes neither playback speed nor the live frame rate.
//!
//! ## Fidelity
//! - **SDR** (production texture is an sRGB 8-bit surface): the bytes are already the
//!   finished, gamma-encoded image — we strip the wgpu row padding and pipe them
//!   verbatim to an H.264 Rec.709 encode. Byte-faithful to Organon's SDR look.
//! - **HDR** (production texture is `Rgba16Float`, extended-linear EDR radiance): we
//!   apply the SMPTE-2084 **PQ** transfer over Rec.2020 primaries **on the CPU** (a
//!   known, testable formula) to 16-bit `rgba64le`, tag the stream as bt2020/PQ, and
//!   let ffmpeg quantize to 10-bit `p010` HEVC. So the file carries the generated
//!   highlight range as real HDR10, not a second unknown display tone-map.
//!
//! ## Audio (Tier 2)
//! The plugin streams its post-synth stereo output through the `audio_ring` mmap channel;
//! this recorder drains it per frame into `audio_buf`, and at stop writes a float32 WAV and
//! muxes it with the video (a second ffmpeg pass — video copied, audio → AAC). The live
//! video therefore encodes to a temp file (`video_tmp`); `finish` produces the final muxed
//! `out_path`, or just promotes the temp video if no plugin was feeding audio.
//!
//! Remaining scope: no overlay bake yet (Tier 1); the display-decoupled HDR mastering
//! target is driven from `bin/visual.rs` via `record_headroom()`.

use organic_math_native::audio_ring::AudioRingReader;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// EDR `1.0` (SDR diffuse white out of the composite) maps to this many cd/m² in the
/// recorded HDR signal. BT.2408 graphics white is 203 nits; a tunable starting point
/// (Tier 3 exposes it). Highlights that re-expanded above 1.0 scale linearly from here.
const REF_WHITE_NITS: f32 = 203.0;
/// Mastering peak: linear nits are clamped here before the PQ curve. 1000 nits is the
/// common HDR10 mastering target (Tier 3 exposes it).
const DEFAULT_MASTER_NITS: f32 = 1000.0;

// The encoded file has a constant frame rate; the render loop free-runs at the vsync rate.
// `frames_due` / `frames_due_at` pace to that CFR so a readback hitch never changes playback
// speed (it duplicates the current frame instead). The rate itself is per-take — see [`Fps`]
// and [`DEFAULT_FPS`].

/// A record frame rate as an exact rational, so the broadcast rates stay exact: 23.976 is
/// **24000/1001**, not a rounded 24. ffmpeg is given the fraction verbatim (`-r 24000/1001`),
/// so the file's rate tag matches an NLE sequence exactly instead of drifting a frame every
/// ~42 seconds against it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const fn new(num: u32, den: u32) -> Fps {
        Fps { num, den }
    }
    /// The rate as a plain number (for pacing maths).
    pub fn value(self) -> f64 {
        self.num as f64 / self.den.max(1) as f64
    }
    /// ffmpeg's `-r` argument — the exact fraction for the 1001-denominator rates.
    pub fn rate_arg(self) -> String {
        if self.den <= 1 {
            self.num.to_string()
        } else {
            format!("{}/{}", self.num, self.den)
        }
    }
    /// Short human label for the on-screen toast.
    pub fn label(self) -> String {
        if self.den <= 1 {
            format!("{} fps", self.num)
        } else {
            format!("{:.3} fps", self.value())
        }
    }
}

/// Selectable record rates, in cycle order (the **V** key). Ordered slowest→fastest so the
/// cinematic rates — the ones that matter for cutting into an NLE timeline — come first.
pub const FPS_CHOICES: &[Fps] = &[
    Fps::new(24000, 1001), // 23.976
    Fps::new(24, 1),
    Fps::new(25, 1),
    Fps::new(30000, 1001), // 29.97
    Fps::new(30, 1),
    Fps::new(60, 1),
];

/// Default record rate: 60, matching the pre-#430-chunk behaviour.
pub const DEFAULT_FPS: Fps = Fps::new(60, 1);

/// Next rate in the [`FPS_CHOICES`] cycle (wraps; an unknown rate restarts the cycle).
pub fn next_fps(cur: Fps) -> Fps {
    match FPS_CHOICES.iter().position(|f| *f == cur) {
        Some(i) => FPS_CHOICES[(i + 1) % FPS_CHOICES.len()],
        None => FPS_CHOICES[0],
    }
}

/// What kind of file we are producing (drives the pixel format + ffmpeg args).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordFormat {
    /// H.264 Rec.709 `.mp4` from an sRGB 8-bit production texture.
    Sdr,
    /// HEVC 10-bit Rec.2020 **PQ** (HDR10) `.mov` from an `Rgba16Float` production texture.
    HdrPq,
}

impl RecordFormat {
    fn ext(self) -> &'static str {
        match self {
            RecordFormat::Sdr => "mp4",
            RecordFormat::HdrPq => "mov",
        }
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/// Bytes per texel for the production-texture formats we read back.
pub fn bytes_per_pixel(format: wgpu::TextureFormat) -> u32 {
    match format {
        wgpu::TextureFormat::Rgba16Float => 8,
        _ => 4, // the sRGB 8-bit surfaces (Bgra8/Rgba8 Unorm[Srgb])
    }
}

/// Row stride wgpu requires for `copy_texture_to_buffer` — the unpadded row rounded up
/// to `COPY_BYTES_PER_ROW_ALIGNMENT` (256).
pub fn padded_bytes_per_row(width: u32, bpp: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256
    let unpadded = width * bpp;
    unpadded.div_ceil(align) * align
}

/// The ffmpeg rawvideo `-pix_fmt` name for an SDR production-texture format (byte order
/// as it sits in memory). HDR always feeds `rgba64le` after the CPU PQ transform.
pub fn sdr_pix_fmt(format: wgpu::TextureFormat) -> &'static str {
    match format {
        wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm => "bgra",
        _ => "rgba", // Rgba8Unorm[Srgb] and any other 4-byte fallback
    }
}

/// CFR pacing: how many encoded frames to emit *this* render frame so the output stays
/// at `fps` against the wall clock. `elapsed` is seconds since record start; `emitted`
/// is frames written so far. Returns 0 (we're ahead — drop this render frame) up to a
/// small cap (we fell behind — duplicate the current frame to hold real-time duration).
pub fn frames_due(elapsed_secs: f64, fps: f64, emitted: u64) -> u32 {
    // Frames that *should* have been shown by now (1-based: at t=0 we owe frame #1).
    let target = (elapsed_secs.max(0.0) * fps).floor() as i64 + 1;
    let n = target - emitted as i64;
    n.clamp(0, 4) as u32
}

// --- Phrase-chunk maths (#430 chunk mode) -----------------------------------
//
// Cutting Organon into clips that drop straight onto a music-video timeline means every
// chunk must start on a musical boundary AND be an exact whole number of frames. Those two
// demands fight: a phrase is almost never a whole number of frames. 8 beats at 172 BPM is
// 2.790698 s = **66.977 frames at 24 fps**. So chunk lengths must alternate between the
// integers bracketing the true length.
//
// The rule that makes it work: derive every boundary from **absolute** musical position
// (`round(k · L)`), never by accumulating (`+L` each chunk). Absolute rounding keeps every
// cut within ±½ frame of true forever; accumulation compounds the error and walks the
// clips off the grid over a four-minute track.

/// Exact (generally non-integer) frame length of one phrase.
pub fn phrase_frames(phrase_beats: f64, bpm: f64, fps: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    phrase_beats.max(0.0) * 60.0 / bpm * fps.max(0.0)
}

/// Absolute frame index where chunk `k` begins — rounded from `k · phrase_frames`, so the
/// rounding error is bounded at ±½ frame and never accumulates.
pub fn chunk_start_frame(k: u64, phrase_beats: f64, bpm: f64, fps: f64) -> u64 {
    (k as f64 * phrase_frames(phrase_beats, bpm, fps)).round().max(0.0) as u64
}

/// Exact frame count of chunk `k` — the gap between consecutive absolute boundaries. For
/// 8 beats at 172 BPM / 24 fps this is 67 for most chunks and 66 on a 43-chunk cycle,
/// averaging the true 66.977.
pub fn chunk_frames(k: u64, phrase_beats: f64, bpm: f64, fps: f64) -> u64 {
    let a = chunk_start_frame(k, phrase_beats, bpm, fps);
    let b = chunk_start_frame(k + 1, phrase_beats, bpm, fps);
    b.saturating_sub(a).max(1)
}

/// **Beat-driven CFR**: the frame index the file owes after `beats_elapsed` of musical time.
/// Chunk mode paces off this instead of the wall clock, so the file's timeline is the
/// *song's* timeline — a tempo change or a slow audio-device clock re-times the capture with
/// it rather than sliding the cuts off the grid.
pub fn ideal_frame(beats_elapsed: f64, bpm: f64, fps: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    beats_elapsed.max(0.0) * 60.0 / bpm * fps.max(0.0)
}

/// [`frames_due`] for the beat-driven clock: how many frames to emit given the ideal frame
/// index we've reached and how many we've written. Same 0..4 clamp — 0 means we're ahead
/// (drop this render frame), >1 means a hitch to cover by duplication.
pub fn frames_due_at(ideal: f64, emitted: u64) -> u32 {
    let target = ideal.max(0.0).floor() as i64 + 1;
    (target - emitted as i64).clamp(0, 4) as u32
}

/// The first phrase boundary strictly after `beat`, on the absolute grid
/// `offset_beats + n · phrase_beats`. This is what makes a take start on the downbeat of a
/// phrase rather than wherever the key happened to be pressed.
///
/// The grid is absolute *song* position, so chunk boundaries line up with the arrangement
/// even across a stop/restart. Note the host hands us `pos_beats` wrapped **mod 1024**;
/// 1024 divides evenly by 4/8/16/32/64, so power-of-two phrases survive the wrap invisibly
/// (a 3-bar phrase would hiccup once every 1024 beats).
pub fn next_boundary(beat: f64, phrase_beats: f64, offset_beats: f64) -> f64 {
    let p = phrase_beats.max(1.0e-6);
    let rel = beat - offset_beats;
    let n = (rel / p).floor() + 1.0;
    offset_beats + n * p
}

/// Whether an N-bar recording is complete: the continuous beat position has advanced by
/// `bars · beats_per_bar` since it started. `>=` is safe against the PLL nudging
/// `beat_pos` slightly backward (the caller also guards with a `recording` flag).
pub fn record_done(start_beat: f64, cur_beat: f64, bars: u32, beats_per_bar: f64) -> bool {
    if bars == 0 {
        return false; // 0 bars = free-run (manual stop only)
    }
    (cur_beat - start_beat) >= bars as f64 * beats_per_bar.max(1.0)
}

/// SMPTE ST-2084 (PQ) OETF: linear luminance in cd/m² → non-linear code in `[0,1]`.
pub fn pq_encode(nits: f32) -> f32 {
    const M1: f32 = 0.159_301_76; // 2610/16384
    const M2: f32 = 78.843_75; // 2523/32 * 128
    const C1: f32 = 0.835_937_5; // 3424/4096
    const C2: f32 = 18.851_562_5; // 2413/4096 * 32
    const C3: f32 = 18.687_5; // 2392/4096 * 32
    let y = (nits.max(0.0) / 10_000.0).clamp(0.0, 1.0);
    let yp = y.powf(M1);
    ((C1 + C2 * yp) / (1.0 + C3 * yp)).powf(M2)
}

/// Linear Rec.709 → linear Rec.2020 primaries (BT.2087). White maps to white.
pub fn rec709_to_2020(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    [
        0.6274 * r + 0.3293 * g + 0.0433 * b,
        0.0691 * r + 0.9195 * g + 0.0114 * b,
        0.0164 * r + 0.0880 * g + 0.8956 * b,
    ]
}

/// One HDR pixel: extended-linear EDR RGB (1.0 = SDR white) → Rec.2020 PQ 16-bit code.
/// `wide` = the source is already Rec.2020 (the `hdr_wide` path), so skip the 709→2020
/// matrix rather than double-converting.
pub fn linear_edr_to_pq_u16(rgb: [f32; 3], ref_white: f32, master_nits: f32, wide: bool) -> [u16; 3] {
    let rec2020 = if wide { rgb } else { rec709_to_2020(rgb) };
    let mut out = [0u16; 3];
    for i in 0..3 {
        let nits = (rec2020[i].max(0.0) * ref_white).min(master_nits);
        out[i] = (pq_encode(nits) * 65535.0 + 0.5) as u16;
    }
    out
}

/// Convert one mapped readback (with wgpu row padding) into the exact byte stream ffmpeg
/// expects on stdin, into `dst`. Pure so it can be unit-tested without a GPU:
/// - SDR: strip the per-row padding; bytes pass through unchanged.
/// - HDR: read 4×`f16` per texel, PQ-encode RGB to Rec.2020, emit `rgba64le`
///   (16-bit LE, opaque alpha).
pub fn encode_rows(
    src: &[u8],
    dst: &mut Vec<u8>,
    width: u32,
    height: u32,
    padded_bpr: u32,
    fmt: RecordFormat,
    wide: bool,
    ref_white: f32,
    master_nits: f32,
) {
    let (w, h, pbr) = (width as usize, height as usize, padded_bpr as usize);
    dst.clear();
    match fmt {
        RecordFormat::Sdr => {
            let row_bytes = w * 4;
            dst.reserve(row_bytes * h);
            for y in 0..h {
                let start = y * pbr;
                dst.extend_from_slice(&src[start..start + row_bytes]);
            }
        }
        RecordFormat::HdrPq => {
            dst.reserve(w * h * 8);
            for y in 0..h {
                let row = y * pbr;
                for x in 0..w {
                    let p = row + x * 8; // 4 channels × f16 (2 bytes)
                    let r = half::f16::from_le_bytes([src[p], src[p + 1]]).to_f32();
                    let g = half::f16::from_le_bytes([src[p + 2], src[p + 3]]).to_f32();
                    let b = half::f16::from_le_bytes([src[p + 4], src[p + 5]]).to_f32();
                    let [rr, gg, bb] = linear_edr_to_pq_u16([r, g, b], ref_white, master_nits, wide);
                    dst.extend_from_slice(&rr.to_le_bytes());
                    dst.extend_from_slice(&gg.to_le_bytes());
                    dst.extend_from_slice(&bb.to_le_bytes());
                    dst.extend_from_slice(&u16::MAX.to_le_bytes()); // opaque alpha
                }
            }
        }
    }
}

/// Build the ffmpeg command-line (excluding the binary). The video comes from stdin as
/// rawvideo; hardware VideoToolbox encode; colour tags describe the stream we feed.
pub fn ffmpeg_args(width: u32, height: u32, fps: Fps, fmt: RecordFormat, out: &Path) -> Vec<String> {
    let s = format!("{width}x{height}");
    let r = fps.rate_arg();
    let mut a: Vec<String> = vec!["-y".into(), "-f".into(), "rawvideo".into()];
    // Input color tags (before -i) describe the raw stream so ffmpeg does not re-transform.
    // SDR's `-pix_fmt` is a `PIXFMT` placeholder the caller replaces once it knows the
    // production texture's byte order (bgra vs rgba); HDR always feeds `rgba64le`.
    match fmt {
        RecordFormat::Sdr => {
            a.extend(["-pix_fmt".into(), "PIXFMT".into()]);
            a.extend(["-s".into(), s.clone(), "-r".into(), r.clone()]);
            a.extend([
                "-colorspace".into(), "bt709".into(),
                "-color_primaries".into(), "bt709".into(),
                "-color_trc".into(), "bt709".into(),
            ]);
            a.extend(["-i".into(), "-".into(), "-an".into()]);
            a.extend([
                "-c:v".into(), "h264_videotoolbox".into(),
                "-q:v".into(), "60".into(),
                "-pix_fmt".into(), "yuv420p".into(),
                "-colorspace".into(), "bt709".into(),
                "-color_primaries".into(), "bt709".into(),
                "-color_trc".into(), "bt709".into(),
                "-tag:v".into(), "avc1".into(),
                "-movflags".into(), "+faststart".into(),
            ]);
        }
        RecordFormat::HdrPq => {
            a.extend(["-pix_fmt".into(), "rgba64le".into()]);
            a.extend(["-s".into(), s.clone(), "-r".into(), r.clone()]);
            a.extend([
                "-colorspace".into(), "bt2020nc".into(),
                "-color_primaries".into(), "bt2020".into(),
                "-color_trc".into(), "smpte2084".into(),
                "-color_range".into(), "tv".into(),
            ]);
            a.extend(["-i".into(), "-".into(), "-an".into()]);
            a.extend([
                "-c:v".into(), "hevc_videotoolbox".into(),
                "-profile:v".into(), "main10".into(),
                "-q:v".into(), "60".into(),
                "-pix_fmt".into(), "p010le".into(),
                "-colorspace".into(), "bt2020nc".into(),
                "-color_primaries".into(), "bt2020".into(),
                "-color_trc".into(), "smpte2084".into(),
                "-color_range".into(), "tv".into(),
                "-tag:v".into(), "hvc1".into(),
                "-movflags".into(), "+faststart".into(),
            ]);
        }
    }
    a.push(out.to_string_lossy().into_owned());
    a
}

/// The composite HDR headroom (multiple of SDR white) the visual drives while recording,
/// **decoupled from the attached display**: highlights re-expand up to the recording master
/// peak, not the panel's EDR limit, so the file holds the range the renderer generates even
/// beyond what the screen can show. Matches the recorder's `master_nits` clamp, so composite
/// output ↔ PQ nits agree.
pub fn record_headroom() -> f32 {
    DEFAULT_MASTER_NITS / REF_WHITE_NITS
}

/// Build a 32-bit float WAV (`WAVE_FORMAT_IEEE_FLOAT`) from interleaved samples. Total +
/// pure so the header layout is unit-testable; the recorder buffers the take in RAM (seconds
/// of audio = a few MB) and writes it once at stop.
pub fn build_wav_f32(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits = 32u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * block_align as u32;
    let data_bytes = (samples.len() * 4) as u32;
    let riff_size = 36 + data_bytes; // 4 + (8+16) + (8+data)
    let mut v = Vec::with_capacity(44 + samples.len() * 4);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&riff_size.to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    v.extend_from_slice(&3u16.to_le_bytes()); // 3 = IEEE float
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_bytes.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// ffmpeg args to mux an already-encoded video with a WAV audio track into the final file
/// (video copied, audio → AAC). `-shortest` trims to the shorter stream so a sample of A/V
/// drift never leaves a trailing gap.
pub fn ffmpeg_mux_args(video: &Path, audio: &Path, out: &Path) -> Vec<String> {
    vec![
        "-y".into(),
        "-i".into(), video.to_string_lossy().into_owned(),
        "-i".into(), audio.to_string_lossy().into_owned(),
        "-map".into(), "0:v:0".into(),
        "-map".into(), "1:a:0".into(),
        "-c:v".into(), "copy".into(),
        "-c:a".into(), "aac".into(),
        "-b:a".into(), "320k".into(),
        "-shortest".into(),
        "-movflags".into(), "+faststart".into(),
        out.to_string_lossy().into_owned(),
    ]
}

// ---------------------------------------------------------------------------
// The live recorder (GPU readback + ffmpeg pipe)
// ---------------------------------------------------------------------------

/// Well-known absolute ffmpeg installs, probed after `$ORGANON_FFMPEG` and a bundled copy
/// but BEFORE falling back to a bare `ffmpeg` on `PATH`.
///
/// This matters because the visual is normally spawned **by the plugin**, and a macOS GUI
/// process inherits a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`) that does NOT include
/// Homebrew's prefix — so a bare `ffmpeg` fails to launch even though `which ffmpeg` works
/// fine in a terminal. Probing the known prefixes makes "brew install ffmpeg" just work for
/// a plugin-launched visual, without the user having to set an env var Ableton never sees.
#[cfg(not(windows))]
pub const FFMPEG_FALLBACKS: &[&str] = &[
    "/opt/homebrew/bin/ffmpeg", // Homebrew (Apple Silicon)
    "/usr/local/bin/ffmpeg",    // Homebrew (Intel) / manual install
    "/opt/local/bin/ffmpeg",    // MacPorts
];

/// The Windows probe list (#658 Tier 1). Deliberately **short**, because the pathology the
/// macOS list answers mostly does not exist here — and saying why is the point of this
/// comment, since "no entries at all" was a live option.
///
/// The macOS list exists because `launchd` hands a GUI app a sanitized `PATH`, so the
/// Homebrew prefix the user installed ffmpeg into is invisible to Ableton *and to everything
/// Ableton spawns*, no matter what the user's shell profile says. Windows does the opposite:
/// a process launched from Explorer inherits the composed machine+user `Path` out of the
/// registry, and a plugin loaded into Live inherits that in turn. **So on Windows a bare
/// `ffmpeg` is the normal, working answer** — these are the two exceptions to it:
///
/// - **`Path` is snapshotted at process start, never read live.** An installer broadcasts
///   `WM_SETTINGCHANGE`, but a Live (or Explorer) instance already running keeps the
///   environment block it launched with. `winget install ffmpeg` and then hitting record in a
///   session that predates it is precisely the Homebrew failure in Windows clothing, and it
///   is the most likely way anyone here meets it. winget's own shim directory is therefore
///   the first entry, not an afterthought.
/// - **A zip install is on nobody's `Path`.** The canonical Windows ffmpeg is a downloaded
///   build extracted by hand; every guide says `C:\ffmpeg`, and editing `Path` afterwards is
///   a separate manual step plenty of people skip.
///
/// Entries are `%NAME%` templates rather than literal `C:\…` paths because **none of these
/// roots are guaranteed**: Windows installs on any drive letter, `%ProgramFiles%` is
/// localized on some SKUs, and a domain profile puts `%USERPROFILE%` on a server. An entry
/// whose variable is unset resolves to nothing and is skipped — see
/// [`expand_env_placeholders`]. (Lookup is `GetEnvironmentVariableW` under the hood, which is
/// case-insensitive, so the canonical casing here is for the reader, not the resolver.)
///
/// A relocated Chocolatey or Scoop root is out of scope for a probe list: that is what
/// `$ORGANON_FFMPEG` is for.
#[cfg(windows)]
pub const FFMPEG_FALLBACKS: &[&str] = &[
    r"%LOCALAPPDATA%\Microsoft\WinGet\Links\ffmpeg.exe", // winget's shim dir — the stale-`Path` case
    r"%ProgramData%\chocolatey\bin\ffmpeg.exe",          // Chocolatey (default root)
    r"%USERPROFILE%\scoop\shims\ffmpeg.exe",             // Scoop (default root)
    r"%ProgramFiles%\ffmpeg\bin\ffmpeg.exe",             // a zip extracted under Program Files
    r"%SystemDrive%\ffmpeg\bin\ffmpeg.exe",              // …or at the drive root, the tutorial path
];

/// Expand `%NAME%` placeholders in a [`FFMPEG_FALLBACKS`] entry against `lookup`, or `None`
/// if any named variable is unset **or set to empty** — a candidate we cannot resolve is one
/// we skip, never one we probe with a literal `%LOCALAPPDATA%` still embedded in it (which
/// would silently test a nonsense path and always miss). Empty counts as unset because
/// `%ProgramFiles%` = `""` would collapse the entry to a plausible-looking `\ffmpeg\bin\…`.
///
/// The environment arrives as a closure so this stays **pure**: the Windows list is then
/// unit-testable from macOS and Linux, which is the only place anyone will ever read these
/// tests. Unix entries contain no `%` at all and come back byte-identical, so `ffmpeg_bin`
/// keeps one probe loop on every platform instead of growing a `cfg`'d second copy.
///
/// Stray percents follow `cmd`'s own rule: an unpaired `%` is literal (`50%` stays `50%`) and
/// `%%` is a literal `%`.
pub fn expand_env_placeholders(template: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    if !template.contains('%') {
        return Some(template.to_string()); // the Unix entries: nothing to expand
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('%') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('%') {
            Some(0) => {
                out.push('%'); // `%%` → one literal percent
                rest = &after[1..];
            }
            Some(close) => {
                let val = lookup(&after[..close])?;
                if val.is_empty() {
                    return None;
                }
                out.push_str(&val);
                rest = &after[close + 1..];
            }
            None => {
                out.push('%'); // unpaired trailing `%` — literal, and the loop then ends
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// The filename of a bundled ffmpeg sitting next to the visual binary. `EXE_SUFFIX` is `""`
/// on Unix, so this is the same `"ffmpeg"` macOS and Linux have always probed.
///
/// On Windows it is `"ffmpeg.exe"`, and without the suffix the bundled branch **can never
/// fire** (#658 Tier 1): the probe gates on `.exists()`, and `…\ffmpeg` does not exist even
/// when `…\ffmpeg.exe` does — so a bundled copy would be shipped and then silently ignored.
pub fn bundled_ffmpeg_name() -> String {
    format!("ffmpeg{}", std::env::consts::EXE_SUFFIX)
}

/// Resolve the ffmpeg binary: `$ORGANON_FFMPEG` override, else a copy next to the visual
/// binary (Tier 5 bundles one there), else a well-known install (see [`FFMPEG_FALLBACKS`]),
/// else plain `ffmpeg` on `PATH`.
fn ffmpeg_bin() -> String {
    if let Ok(p) = std::env::var("ORGANON_FFMPEG") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join(bundled_ffmpeg_name());
            if bundled.exists() {
                return bundled.to_string_lossy().into_owned();
            }
        }
    }
    for cand in FFMPEG_FALLBACKS {
        // Unix entries pass through verbatim; a Windows entry whose `%VAR%` is unset yields
        // `None` and is skipped rather than probed with the placeholder intact.
        let Some(path) = expand_env_placeholders(cand, |k| std::env::var(k).ok()) else {
            continue;
        };
        if std::path::Path::new(&path).exists() {
            return path;
        }
    }
    "ffmpeg".into()
}

/// The OS's own videos folder + `Organon/` (created if missing), else the current dir.
///
/// `dirs::video_dir()` rather than a hand-rolled `home_dir().join("Movies")` (#658 Tier 1).
/// On macOS the two are *the same string*: dirs defines the macOS videos dir as exactly
/// `$HOME/Movies`, so takes keep landing in `~/Movies/Organon`. On Windows there is no
/// `Movies` at all — and the right answer is not a hardcoded `%USERPROFILE%\Videos` either,
/// because `Videos` is a **Known Folder** that OneDrive backup, folder redirection or a
/// roaming profile routinely moves somewhere else. Only `SHGetKnownFolderPath` (what dirs
/// calls) names the folder the user will actually go looking in. On Linux it is
/// `$XDG_VIDEOS_DIR`, normally `~/Videos`.
fn output_dir() -> PathBuf {
    let base = output_base(dirs::video_dir(), dirs::home_dir());
    let _ = std::fs::create_dir_all(&base);
    base
}

/// [`output_dir`]'s *choice*, as a pure function of what `dirs` reported — so the fallback
/// chain is unit-testable without touching the real environment or creating directories.
///
/// The `home/Videos` middle step covers the one case `dirs::video_dir()` returns `None`: a
/// Linux box with no `xdg-user-dirs` configured, i.e. every headless one. It can never fire
/// on macOS or Windows, where the videos dir is unconditional. Dropping straight to `.` there
/// would scatter takes into whatever directory the visual happened to be launched from, so
/// name the conventional folder first and keep `.` as the true last resort.
fn output_base(video: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    video
        .or_else(|| home.map(|h| h.join("Videos")))
        .map(|v| v.join("Organon"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Readback ring depth. Three lets the GPU run ~2 frames ahead of the encoder: the render
/// thread copies into a free slot and moves on, and a slot only returns once the worker has
/// encoded it. Deeper just costs a full padded frame of memory each for no extra headroom.
const RING: usize = 3;

/// One mapped frame handed to the encode worker: which ring slot it occupies, its buffer,
/// and how many times to emit it (CFR duplication for a skipped/late frame).
enum Msg {
    Frame { slot: usize, buf: Arc<wgpu::Buffer>, dup: u32 },
}

/// The encode/write worker. Owns ffmpeg's stdin and does **all** the per-frame CPU work off
/// the render thread: the per-pixel HDR PQ/Rec.2020 conversion and the blocking pipe write.
/// Returns when the channel closes (all senders dropped), which EOFs ffmpeg via `stdin`.
///
/// Every message owns a ring slot, so this loop **must keep draining even after ffmpeg dies**
/// — bailing out early would strand the in-flight slots, the ring would drain away, and
/// `capture` would silently stop recording while the session still looked alive.
///
/// It reports `(slot, shortfall)` back: `shortfall` is how many of the frame's CFR
/// duplicates never reached ffmpeg, so the caller's CFR clock can re-emit them instead of
/// counting phantom frames (which would end the file short and drift it out of sync).
#[allow(clippy::too_many_arguments)]
fn encode_worker(
    rx: Receiver<Msg>,
    free_tx: Sender<(usize, u32)>,
    mut stdin: ChildStdin,
    w: u32,
    h: u32,
    padded_bpr: u32,
    fmt: RecordFormat,
    wide: bool,
) {
    let mut scratch: Vec<u8> = Vec::new();
    // Once ffmpeg is gone we stop encoding/writing but keep draining (see above).
    let mut dead = false;
    while let Ok(Msg::Frame { slot, buf, dup }) = rx.recv() {
        let mut encoded = false;
        if !dead {
            if let Ok(view) = buf.get_mapped_range(..) {
                encode_rows(
                    &view,
                    &mut scratch,
                    w,
                    h,
                    padded_bpr,
                    fmt,
                    wide,
                    REF_WHITE_NITS,
                    DEFAULT_MASTER_NITS,
                );
                encoded = true;
            }
        }
        // The map succeeded (that's why this frame is here), so unmap regardless — the slot
        // is unusable until we do.
        buf.unmap();

        let mut wrote = 0u32;
        if encoded {
            for _ in 0..dup {
                if stdin.write_all(&scratch).is_err() {
                    dead = true; // ffmpeg went away — stop writing, but keep draining
                    break;
                }
                wrote += 1;
            }
        }
        let _ = free_tx.send((slot, dup.saturating_sub(wrote)));
    }
}

pub struct Recorder {
    child: Child,
    /// Final muxed file (video + audio). The worker's ffmpeg encodes video to `video_tmp`;
    /// `finish` muxes it with the captured audio into `out_path`, or promotes it there.
    out_path: PathBuf,
    video_tmp: PathBuf,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    fmt: RecordFormat,
    wide: bool,
    padded_bpr: u32,
    /// #430 Tier 2 audio: the plugin's post-synth stereo stream, drained per frame into
    /// `audio_buf` (interleaved), muxed in at stop. Inert if no plugin is feeding the ring.
    /// Sample rate / channels are read from `audio` fresh at mux, not cached (the ring can be
    /// recreated at a new rate by a plugin re-init mid-take).
    audio: AudioRingReader,
    audio_buf: Vec<f32>,
    /// Readback ring: each frame's GPU copy lands in one of these and is mapped
    /// **asynchronously**, so the render thread never waits on the GPU.
    slots: Vec<Arc<wgpu::Buffer>>,
    /// Slots idle and safe to copy into.
    free: Vec<usize>,
    /// Slots the worker has finished with, as `(slot, shortfall)` — `shortfall` being CFR
    /// duplicates that never reached ffmpeg, which `capture` refunds to the CFR clock.
    free_rx: Receiver<(usize, u32)>,
    /// Cloned into each map callback so a *failed* map still returns its slot to the ring
    /// (and refunds its frames).
    free_tx: Sender<(usize, u32)>,
    /// To the encode worker. `finish` takes it, closing the channel so the worker drains.
    tx: Option<Sender<Msg>>,
    worker: Option<std::thread::JoinHandle<()>>,
    start: Instant,
    frames_emitted: u64,
    /// Fixed-timestep / offline capture: capture every frame 1:1 (no CFR resampling),
    /// block for a slot rather than drop, and no audio. See `start`.
    perfect: bool,
    /// Encoded file rate. The render loop still free-runs at vsync; the CFR pacer decides
    /// which rendered frames become file frames (at 24 fps that's roughly one in every
    /// two-and-a-half — the same sampling a film camera does).
    fps: Fps,
    /// **Warmed but not yet emitting** (#430 chunk mode). ffmpeg is spawned and the ring is
    /// allocated, but frames are discarded until `open_gate`. This is the "anticipation" that
    /// lets chunk 1's *first* frame be the downbeat frame: the several-millisecond fork/exec
    /// of ffmpeg happens before the boundary, not on it.
    gated: bool,
    /// Exact-cut chunk length in frames (`None` = run until told to stop). Set from
    /// [`chunk_frames`] so consecutive chunks butt-join on the timeline with no drift.
    target_frames: Option<u64>,
}

/// Everything that varies per take. Grouped into a struct because chunk mode adds several
/// knobs and a nine-argument `start` is unreadable.
#[derive(Clone, Debug)]
pub struct TakeOpts {
    /// Encoded file rate.
    pub fps: Fps,
    /// Fixed-timestep offline capture (Shift+R) — see [`Recorder::start`]. Mutually
    /// exclusive with chunk mode, which must ride the host's real-time transport.
    pub perfect: bool,
    /// Start **warmed but gated**: ffmpeg is spawned now, frames are discarded until
    /// [`Recorder::open_gate`] fires at the phrase boundary.
    pub gated: bool,
    /// Exact number of frames to emit before the take reports itself complete
    /// (`None` = run until stopped). Chunk mode sets this from [`chunk_frames`].
    pub target_frames: Option<u64>,
    /// Filename stem, minus extension (`None` = a millisecond timestamp). Chunk mode puts
    /// the absolute bar position here so a clip's place on the timeline is legible in the
    /// Finder without opening it.
    pub label: Option<String>,
}

impl Default for TakeOpts {
    fn default() -> TakeOpts {
        TakeOpts { fps: DEFAULT_FPS, perfect: false, gated: false, target_frames: None, label: None }
    }
}

impl Recorder {
    /// Spawn ffmpeg and allocate the readback buffer. `size`/`format` are the production
    /// texture's; `hdr`/`wide` are the visual's current EDR state.
    ///
    /// `opts.perfect` = **fixed-timestep / offline capture**: the caller drives the animation
    /// clock at exactly `1/fps` per frame (not wall-clock), so motion is perfectly even and
    /// readback speed can't jitter it. Here that means: capture **every** frame exactly once
    /// (no CFR wall-clock resampling, no drop/dup), **block** for a ring slot if the encoder
    /// falls behind (we're not real-time-bound), and **no audio** (audio is real-time; this
    /// mode may run faster or slower than wall time).
    pub fn start(
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        hdr: bool,
        wide: bool,
        opts: TakeOpts,
    ) -> std::io::Result<Recorder> {
        let TakeOpts { fps, perfect, gated, target_frames, label } = opts;
        let (w, h) = (size.0.max(2), size.1.max(2));
        let fmt = if hdr && format == wgpu::TextureFormat::Rgba16Float {
            RecordFormat::HdrPq
        } else {
            RecordFormat::Sdr
        };
        let bpp = bytes_per_pixel(format);
        let padded_bpr = padded_bytes_per_row(w, bpp);

        // Millisecond stamp, and never reuse an existing name: ffmpeg runs with `-y`, so a
        // whole-second stamp let two takes started in the same second silently overwrite
        // each other. A take is expensive to re-shoot — never clobber one.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dir = output_dir();
        // Chunk mode supplies its own stem (bpm + absolute bar), so a folder of takes sorts
        // and reads as a shot list. Everything else falls back to the timestamp.
        let stem_base = label.unwrap_or_else(|| format!("Organon-{ts}"));
        let mut out_path = dir.join(format!("{stem_base}.{}", fmt.ext()));
        let mut take = 2;
        while out_path.exists() {
            out_path = dir.join(format!("{stem_base}-{take}.{}", fmt.ext()));
            take += 1;
        }
        // The worker encodes video here; finish() muxes it with the captured audio into
        // out_path (or renames it there if there was no audio). Hidden dotfile, same
        // extension so ffmpeg picks the right muxer.
        let stem = out_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let video_tmp = dir.join(format!(".{stem}.videotmp.{}", fmt.ext()));

        // Audio: open the plugin's post-synth ring. The anchor (`reset_to_now`) is set at
        // the very end of `start`, co-located with the video CFR clock, so the ffmpeg-spawn /
        // buffer-alloc / thread-spawn latency below doesn't bake in an audio-leads-video
        // offset (Vercel review). The sample rate is read fresh at mux (it can change if the
        // plugin re-initializes). Inert (video-only) if no plugin is feeding the ring.
        let mut audio = AudioRingReader::open();

        // Build the arg list, substituting the SDR pix_fmt placeholder now that we know
        // the texture byte order. Video is encoded to the temp path.
        let sdr_pf = sdr_pix_fmt(format);
        let args: Vec<String> = ffmpeg_args(w, h, fps, fmt, &video_tmp)
            .into_iter()
            .map(|s| if s == "PIXFMT" { sdr_pf.to_string() } else { s })
            .collect();

        let bin = ffmpeg_bin();
        let mut child = Command::new(&bin)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit()) // surface ffmpeg's own logs to the terminal
            .spawn()
            .map_err(|e| {
                eprintln!(
                    "Recorder: failed to launch ffmpeg ('{bin}'): {e}. \
                     Install ffmpeg or set $ORGANON_FFMPEG."
                );
                e
            })?;
        let stdin = child.stdin.take();

        let slots: Vec<Arc<wgpu::Buffer>> = (0..RING)
            .map(|_| {
                Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("recorder-readback"),
                    size: padded_bpr as u64 * h as u64,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }))
            })
            .collect();

        // The encode worker owns stdin: every per-pixel conversion + pipe write happens
        // there, never on the render thread.
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        let (free_tx, free_rx) = std::sync::mpsc::channel::<(usize, u32)>();
        let worker = stdin.map(|stdin| {
            let free_tx = free_tx.clone();
            std::thread::Builder::new()
                .name("organon-recorder".into())
                .spawn(move || encode_worker(rx, free_tx, stdin, w, h, padded_bpr, fmt, wide))
                .expect("spawn recorder encode worker")
        });

        eprintln!(
            "Recorder: ● REC {}×{} {} {} {} → {}",
            w,
            h,
            match fmt {
                RecordFormat::Sdr => "SDR/H.264",
                RecordFormat::HdrPq => "HDR10/HEVC PQ",
            },
            fps.label(),
            if perfect {
                "[perfect / fixed-timestep, video-only]".to_string()
            } else if gated {
                match target_frames {
                    Some(n) => format!("[chunk, {n} frames, waiting for the downbeat]"),
                    None => "[waiting for the downbeat]".to_string(),
                }
            } else {
                "[real-time + audio]".to_string()
            },
            out_path.display()
        );

        // Real-time mode: anchor the audio ring at the same instant the video CFR clock
        // starts (below) so the two share a zero point. Perfect mode captures no audio.
        // A gated take anchors both later, in `open_gate`, at the phrase boundary.
        if !perfect && !gated {
            audio.reset_to_now();
        }
        Ok(Recorder {
            child,
            out_path,
            video_tmp,
            size: (w, h),
            format,
            fmt,
            wide,
            padded_bpr,
            audio,
            audio_buf: Vec::new(),
            slots,
            free: (0..RING).collect(),
            free_rx,
            free_tx,
            tx: Some(tx),
            worker,
            start: Instant::now(),
            frames_emitted: 0,
            perfect,
            fps,
            gated,
            target_frames,
        })
    }

    /// Open a gated take's shutter (#430 chunk mode). Called on the phrase boundary: the
    /// encoder is already warm, so this just re-anchors both clocks — the video CFR zero
    /// point and the audio ring cursor — to *now*, and lets frames through.
    ///
    /// Anchoring both here (rather than at `start`) is what keeps A/V in sync: the ffmpeg
    /// fork/exec and buffer allocation happened before the boundary, and neither shows up
    /// as an audio-leads-video offset in the file.
    pub fn open_gate(&mut self) {
        if !self.gated {
            return;
        }
        self.gated = false;
        self.start = Instant::now();
        self.frames_emitted = 0;
        if !self.perfect {
            self.audio.reset_to_now();
            self.audio_buf.clear();
        }
    }

    /// Warmed but not yet emitting — waiting for its phrase boundary.
    pub fn is_gated(&self) -> bool {
        self.gated
    }

    /// File frames written so far (a gated take reports 0).
    pub fn frames_emitted(&self) -> u64 {
        self.frames_emitted
    }

    /// True once an exact-cut take has emitted its full frame count. Chunk mode polls this
    /// to roll to the next clip; a take with no `target_frames` never reports complete.
    pub fn target_reached(&self) -> bool {
        matches!(self.target_frames, Some(n) if self.frames_emitted >= n)
    }

    /// True while the production texture still matches what we opened ffmpeg for. A
    /// mid-recording aspect/HDR change invalidates the recorder (caller should stop).
    /// `wide` is part of the identity, not just size/format: toggling Rec.2020 wide gamut
    /// mid-take changes the primaries the composite *produces*, while this recorder keeps
    /// the `wide` it opened with for both its CPU matrix and ffmpeg's stream tags — so the
    /// rest of the file would be encoded with the wrong matrix. The texture format doesn't
    /// change with it (an HDR toggle does, and `format` catches that), so check it here.
    pub fn matches(&self, size: (u32, u32), format: wgpu::TextureFormat, wide: bool) -> bool {
        self.size == (size.0.max(2), size.1.max(2)) && self.format == format && self.wide == wide
    }

    // What the take latched at `start`, so a mismatch can name itself instead of being inferred
    // (#582 — the auto-stop that killed takes after 2–3 frames).
    pub fn take_size(&self) -> (u32, u32) {
        self.size
    }
    pub fn take_format(&self) -> wgpu::TextureFormat {
        self.format
    }
    pub fn take_wide(&self) -> bool {
        self.wide
    }

    // Used by the Tier 3 editor card (reveal-in-Finder) + status line.
    #[allow(dead_code)]
    pub fn out_path(&self) -> &Path {
        &self.out_path
    }

    /// Read `tex` back and feed the encoder, CFR-paced against the wall clock.
    ///
    /// **Never blocks on the GPU.** The copy goes into a free ring slot and is mapped
    /// asynchronously; a later non-blocking `poll` fires the callback, which hands the
    /// frame to the encode worker (which owns the per-pixel conversion + the pipe write).
    /// The render thread's entire cost here is a texture copy, a submit, and a few channel
    /// ops. If every slot is still in flight the frame is skipped and the CFR clock
    /// duplicates it later — a hitch never changes playback speed *or* the live frame rate.
    /// `ideal` = the **beat-driven** frame index this take owes right now (see
    /// [`ideal_frame`]), used by chunk mode so the file's clock is the song's clock.
    /// `None` paces against the wall clock, the plain-take behaviour.
    pub fn capture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tex: &wgpu::Texture,
        ideal: Option<f64>,
    ) {
        // Warmed but not yet rolling: discard everything until the phrase boundary opens the
        // gate. Draining audio here too would bank pre-roll samples that the reset in
        // `open_gate` then throws away — cheaper to simply not.
        if self.gated {
            return;
        }
        // Exact cut: once the chunk's frame quota is met, emit nothing more. The caller rolls
        // to the next chunk; a frame past the quota would make this clip longer than its
        // phrase and push every later cut off the grid.
        if self.target_reached() {
            return;
        }
        // Real-time mode pulls audio every render frame (independent of the video CFR
        // pacing below) so the stream stays continuous even on skipped frames. Perfect
        // (offline) mode captures no audio.
        if !self.perfect {
            self.audio.drain(&mut self.audio_buf);
        }

        // Progress in-flight maps (non-blocking) and reclaim slots the worker has finished.
        let _ = device.poll(wgpu::PollType::Poll);
        while let Ok((slot, shortfall)) = self.free_rx.try_recv() {
            self.free.push(slot);
            // Refund frames that never reached ffmpeg (a dead encoder or a failed map).
            // Counting them would leave the CFR clock believing it was further ahead than it
            // is, ending the file short and drifting it against wall-clock duration.
            self.frames_emitted = self.frames_emitted.saturating_sub(shortfall as u64);
        }

        let (w, h) = self.size;
        // Perfect mode: exactly one frame per render frame, no wall-clock resampling. The
        // caller drives the animation at a fixed 1/FPS step, so 1:1 gives perfectly even
        // motion. Real-time mode: CFR-pace against the wall clock.
        let mut n = if self.perfect {
            1
        } else {
            // Chunk mode paces off musical time (`ideal`), plain takes off the wall clock.
            let n = match ideal {
                Some(ideal) => frames_due_at(ideal, self.frames_emitted),
                None => frames_due(self.start.elapsed().as_secs_f64(), self.fps.value(), self.frames_emitted),
            };
            if n == 0 {
                return; // ahead of the CFR clock — drop this render frame, skip the readback
            }
            n
        };
        // Never overshoot an exact-cut quota, even when catching up from a hitch: the
        // duplicate frames a catch-up emits must stop at the chunk boundary.
        if let Some(target) = self.target_frames {
            n = n.min(target.saturating_sub(self.frames_emitted) as u32);
            if n == 0 {
                return;
            }
        }
        let slot = match self.free.pop() {
            Some(s) => s,
            // Perfect mode never drops: block until the worker frees a slot. Poll(Wait) fires
            // any pending map callback (feeding the worker), then recv blocks for the worker
            // to return one. Bounded — all slots are in flight, and even a dead encoder keeps
            // draining + freeing.
            None if self.perfect => {
                let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                match self.free_rx.recv() {
                    Ok((s, _shortfall)) => s,
                    Err(_) => return, // worker gone entirely
                }
            }
            None => return, // real-time: ring saturated — let the CFR clock duplicate later
        };

        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("recorder-copy") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.slots[slot],
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);

        // Map ASYNCHRONOUSLY. The callback fires on a later `poll` (above) and hands the
        // mapped buffer straight to the worker — we never wait for the GPU here.
        let buf = self.slots[slot].clone();
        let tx = self.tx.clone();
        let free_tx = self.free_tx.clone();
        self.slots[slot].map_async(wgpu::MapMode::Read, .., move |res| {
            // Either way, this slot MUST come back to the ring and its frames MUST be
            // refunded — a stranded slot stays mapped and busy, and after RING of them the
            // ring is empty and `capture` silently stops recording mid-session.
            let handed_off = match res {
                Ok(()) => match &tx {
                    // Only the worker can unmap this: it reads the mapped range first.
                    Some(tx) => tx.send(Msg::Frame { slot, buf: buf.clone(), dup: n }).is_ok(),
                    None => false, // ffmpeg gave us no stdin, so there is no worker
                },
                Err(_) => false,
            };
            if !handed_off {
                // Nobody took it (map failed, worker gone, or never spawned): unmap it
                // ourselves if the map had succeeded, then give the slot + frames back.
                if res.is_ok() {
                    buf.unmap();
                }
                let _ = free_tx.send((slot, n));
            }
        });
        self.frames_emitted += n as u64;
    }

    /// Close the take and hand its **blocking tail** to a background thread.
    ///
    /// Only the `device.poll` stays on the render thread (cheap, and it needs `&Device`):
    /// any still-in-flight map callbacks must fire first, because each holds a `Sender`
    /// clone that would otherwise keep the worker's channel open forever. Everything after
    /// that — draining the encode worker, waiting on ffmpeg, writing the WAV, the second
    /// ffmpeg mux pass, the rename — takes **seconds**, and runs on the returned thread.
    ///
    /// This is what makes chunk mode possible: a take now ends without stalling the render
    /// loop, so rolling from one clip to the next costs no dropped frames. Keep the handle
    /// and join it before exit, or the process can die mid-mux and leave a `.videotmp` file.
    pub fn finish_async(self, device: &wgpu::Device) -> std::thread::JoinHandle<()> {
        self.close(device, false)
    }

    /// Close the take and **throw the file away**. Used when chunk mode is disarmed
    /// mid-phrase: that clip is a partial, and a short file sitting among exactly-cut ones
    /// is worse than no file. Still drains ffmpeg properly rather than orphaning the child.
    pub fn discard(self, device: &wgpu::Device) -> std::thread::JoinHandle<()> {
        self.close(device, true)
    }

    fn close(mut self, device: &wgpu::Device, discard: bool) -> std::thread::JoinHandle<()> {
        let frames = self.frames_emitted;
        let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        drop(self.tx.take()); // last sender → worker drains, then EOFs ffmpeg

        // Read the audio rate/channels *now*, on this thread: the reader is an mmap handle we
        // don't want to move, and the ring may have been recreated at a new rate mid-take.
        let sample_rate = self.audio.sample_rate();
        let channels = self.audio.channels().clamp(1, 2) as u16;
        let fin = Finalizer {
            child: self.child,
            out_path: std::mem::take(&mut self.out_path),
            video_tmp: std::mem::take(&mut self.video_tmp),
            audio_buf: std::mem::take(&mut self.audio_buf),
            sample_rate,
            channels,
            worker: self.worker.take(),
            frames,
            discard,
        };
        std::thread::Builder::new()
            .name("organon-recorder-finalize".into())
            .spawn(move || fin.run())
            .expect("spawn recorder finalizer")
    }

    /// Blocking [`finish_async`] — for shutdown paths that must not outlive the process.
    pub fn finish(self, device: &wgpu::Device) {
        let _ = self.finish_async(device).join();
    }
}

/// The slow tail of a take, run off the render thread by [`Recorder::finish_async`]: drain
/// the encode worker, wait for ffmpeg to flush the video, then mux the captured audio in
/// (a second ffmpeg pass, video copied) or promote the temp video to its final name.
///
/// Deliberately holds no `wgpu` types — the readback ring is dropped on the render thread
/// where it was created, and this carries only plain owned data across the thread boundary.
struct Finalizer {
    child: Child,
    out_path: PathBuf,
    video_tmp: PathBuf,
    audio_buf: Vec<f32>,
    sample_rate: u32,
    channels: u16,
    worker: Option<std::thread::JoinHandle<()>>,
    frames: u64,
    /// Drain ffmpeg properly, then delete rather than keep (a disarmed partial chunk).
    discard: bool,
}

impl Finalizer {
    fn run(mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if self.discard {
            // Still wait: killing the child here would leave a zombie and a locked temp file.
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.video_tmp);
            eprintln!("Recorder: ✕ discarded partial take ({} frames)", self.frames);
            return;
        }
        match self.child.wait() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("Recorder: video ffmpeg exited with {s} — {}", self.video_tmp.display());
                return;
            }
            Err(e) => {
                eprintln!("Recorder: waiting on video ffmpeg failed: {e}");
                return;
            }
        }

        if !self.audio_buf.is_empty() && self.sample_rate > 0 {
            let audio_tmp = self.video_tmp.with_extension("wav");
            let wav = build_wav_f32(&self.audio_buf, self.sample_rate, self.channels);
            if std::fs::write(&audio_tmp, &wav).is_ok() {
                let args = ffmpeg_mux_args(&self.video_tmp, &audio_tmp, &self.out_path);
                let ok = Command::new(ffmpeg_bin())
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                let _ = std::fs::remove_file(&audio_tmp);
                if ok {
                    let _ = std::fs::remove_file(&self.video_tmp);
                    let secs = self.audio_buf.len() as f64 / (self.sample_rate as f64 * 2.0);
                    eprintln!(
                        "Recorder: ■ saved {} ({} frames, {:.1}s audio @ {} Hz)",
                        self.out_path.display(),
                        self.frames,
                        secs,
                        self.sample_rate
                    );
                    return;
                }
                eprintln!("Recorder: audio mux failed — keeping video-only {}", self.video_tmp.display());
            }
        }

        match std::fs::rename(&self.video_tmp, &self.out_path) {
            Ok(()) => {
                eprintln!("Recorder: ■ saved {} ({} frames, video only)", self.out_path.display(), self.frames)
            }
            Err(e) => eprintln!(
                "Recorder: saved {} ({} frames); could not rename to {}: {e}",
                self.video_tmp.display(),
                self.frames,
                self.out_path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bpp_and_row_padding() {
        assert_eq!(bytes_per_pixel(wgpu::TextureFormat::Rgba16Float), 8);
        assert_eq!(bytes_per_pixel(wgpu::TextureFormat::Bgra8UnormSrgb), 4);
        // 1920×SDR: 1920*4 = 7680, already a multiple of 256.
        assert_eq!(padded_bytes_per_row(1920, 4), 7680);
        // 100×SDR: 400 → rounds up to 512.
        assert_eq!(padded_bytes_per_row(100, 4), 512);
        // 1080×HDR: 1080*8 = 8640 → next multiple of 256 is 8704.
        assert_eq!(padded_bytes_per_row(1080, 8), 8704);
    }

    #[test]
    fn cfr_pacing() {
        // At t=0 we owe the first frame.
        assert_eq!(frames_due(0.0, 60.0, 0), 1);
        // One frame-time later, having emitted 1, we owe exactly 1 more.
        assert_eq!(frames_due(1.0 / 60.0, 60.0, 1), 1);
        // Rendering faster than 60fps → drop this frame to hold CFR.
        assert_eq!(frames_due(0.001, 60.0, 1), 0);
        // A long stall → duplicate to catch up, but capped.
        assert_eq!(frames_due(10.0, 60.0, 1), 4);
    }

    #[test]
    fn bar_stop() {
        // 8 bars of 4/4 = 32 beats.
        assert!(!record_done(10.0, 41.0, 8, 4.0)); // 31 beats in
        assert!(record_done(10.0, 42.0, 8, 4.0)); // 32 beats in
        // 0 bars = free-run: never auto-done.
        assert!(!record_done(0.0, 1000.0, 0, 4.0));
        // Honours a non-4 meter.
        assert!(record_done(0.0, 6.0, 2, 3.0)); // 2 bars × 3 beats = 6
    }

    #[test]
    fn pq_curve_anchors() {
        assert!(pq_encode(0.0).abs() < 1e-6);
        assert!((pq_encode(10_000.0) - 1.0).abs() < 1e-4);
        // 100 nits sits near the middle of the PQ range (~0.508, ITU reference).
        let mid = pq_encode(100.0);
        assert!((0.49..0.52).contains(&mid), "pq(100) = {mid}");
        // Monotonic.
        assert!(pq_encode(203.0) > pq_encode(100.0));
    }

    #[test]
    fn rec2020_preserves_white() {
        let [r, g, b] = rec709_to_2020([1.0, 1.0, 1.0]);
        assert!((r - 1.0).abs() < 1e-3 && (g - 1.0).abs() < 1e-3 && (b - 1.0).abs() < 1e-3);
    }

    #[test]
    fn hdr_pixel_encodes_opaque_and_monotonic() {
        let dim = linear_edr_to_pq_u16([0.2, 0.2, 0.2], REF_WHITE_NITS, DEFAULT_MASTER_NITS, false);
        let bright = linear_edr_to_pq_u16([2.0, 2.0, 2.0], REF_WHITE_NITS, DEFAULT_MASTER_NITS, false);
        assert!(bright[0] > dim[0], "brighter input must give a higher PQ code");
        // Master-nits clamp: a huge value saturates at the ceiling, not past it.
        let huge = linear_edr_to_pq_u16([100.0, 100.0, 100.0], REF_WHITE_NITS, DEFAULT_MASTER_NITS, false);
        assert_eq!(huge[0], linear_edr_to_pq_u16([100.0, 100.0, 100.0], REF_WHITE_NITS, DEFAULT_MASTER_NITS, false)[0]);
    }

    #[test]
    fn encode_rows_sdr_strips_padding() {
        // 2×2 SDR, padded row = 512 (from 8 real bytes).
        let w = 2u32;
        let h = 2u32;
        let padded = padded_bytes_per_row(w, 4); // 512
        let mut src = vec![0u8; (padded * h) as usize];
        // Row 0 real pixels, row 1 real pixels; padding is 0xFF to prove it's dropped.
        for b in src.iter_mut() {
            *b = 0xFF;
        }
        for i in 0..8 {
            src[i] = i as u8; // row 0 real bytes 0..8
            src[padded as usize + i] = (100 + i) as u8; // row 1 real bytes
        }
        let mut dst = Vec::new();
        encode_rows(&src, &mut dst, w, h, padded, RecordFormat::Sdr, false, REF_WHITE_NITS, DEFAULT_MASTER_NITS);
        assert_eq!(dst.len(), (w * h * 4) as usize);
        assert_eq!(&dst[0..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(&dst[8..16], &[100, 101, 102, 103, 104, 105, 106, 107]);
    }

    #[test]
    fn encode_rows_hdr_size_and_alpha() {
        let w = 2u32;
        let h = 1u32;
        let padded = padded_bytes_per_row(w, 8); // 256
        let mut src = vec![0u8; (padded * h) as usize];
        // One mid-grey pixel, one bright pixel (f16 linear).
        let put = |src: &mut [u8], off: usize, v: [f32; 4]| {
            for (c, val) in v.iter().enumerate() {
                let bytes = half::f16::from_f32(*val).to_le_bytes();
                src[off + c * 2] = bytes[0];
                src[off + c * 2 + 1] = bytes[1];
            }
        };
        put(&mut src, 0, [0.5, 0.5, 0.5, 1.0]);
        put(&mut src, 8, [4.0, 4.0, 4.0, 1.0]);
        let mut dst = Vec::new();
        encode_rows(&src, &mut dst, w, h, padded, RecordFormat::HdrPq, false, REF_WHITE_NITS, DEFAULT_MASTER_NITS);
        assert_eq!(dst.len(), (w * h * 8) as usize); // rgba64le
        // Alpha (last channel) of pixel 0 is opaque.
        assert_eq!(&dst[6..8], &u16::MAX.to_le_bytes());
        // Brighter pixel's red PQ code > dimmer pixel's red PQ code.
        let r0 = u16::from_le_bytes([dst[0], dst[1]]);
        let r1 = u16::from_le_bytes([dst[8], dst[9]]);
        assert!(r1 > r0);
    }

    // The probe list is per-OS (#658 Tier 1), so its assertions are too — this one is the
    // pre-existing macOS/Linux test, unchanged apart from the `cfg` that keeps it off the
    // Windows list it was never about.
    #[cfg(not(windows))]
    #[test]
    fn ffmpeg_fallbacks_cover_homebrew_and_are_absolute() {
        // The visual is normally spawned by the plugin, which inherits a macOS GUI PATH
        // without Homebrew's prefix — these absolute probes are what make a plain
        // `brew install ffmpeg` reachable from a plugin-launched visual.
        assert!(FFMPEG_FALLBACKS.contains(&"/opt/homebrew/bin/ffmpeg"), "Homebrew arm64 prefix");
        assert!(FFMPEG_FALLBACKS.contains(&"/usr/local/bin/ffmpeg"), "Homebrew x86 / manual");
        assert!(FFMPEG_FALLBACKS.iter().all(|p| p.starts_with('/')), "probes must be absolute");
    }

    #[cfg(windows)]
    #[test]
    fn windows_ffmpeg_fallbacks_are_env_rooted_exe_paths() {
        assert!(!FFMPEG_FALLBACKS.is_empty(), "Windows keeps a short list, not an empty one");
        for p in FFMPEG_FALLBACKS {
            // Rooted in an environment variable, never a literal drive letter: Windows is not
            // guaranteed to be on C:, and `Program Files` is localized on some SKUs.
            assert!(p.starts_with('%'), "{p} must resolve from an environment variable");
            // `.exists()` is the gate, and `…\ffmpeg` never exists — the suffix is mandatory.
            assert!(p.ends_with(".exe"), "{p} must name the .exe");
        }
        // winget's shim dir is the entry that earns the list: its `Path` entry does not reach
        // a DAW that was already running when ffmpeg was installed.
        assert!(FFMPEG_FALLBACKS.iter().any(|p| p.contains("WinGet")), "winget links dir");
    }

    #[test]
    fn env_placeholders_expand_or_skip() {
        // A fixed environment, so the Windows probe list is testable from macOS and Linux.
        let env = |k: &str| match k {
            "LOCALAPPDATA" => Some(r"C:\Users\j\AppData\Local".to_string()),
            "SystemDrive" => Some("C:".to_string()),
            "BLANK" => Some(String::new()),
            _ => None,
        };
        assert_eq!(
            expand_env_placeholders(r"%LOCALAPPDATA%\Microsoft\WinGet\Links\ffmpeg.exe", env).as_deref(),
            Some(r"C:\Users\j\AppData\Local\Microsoft\WinGet\Links\ffmpeg.exe")
        );
        assert_eq!(
            expand_env_placeholders(r"%SystemDrive%\ffmpeg\bin\ffmpeg.exe", env).as_deref(),
            Some(r"C:\ffmpeg\bin\ffmpeg.exe")
        );
        // An unset variable skips the candidate — probing `%ProgramFiles%\…` literally would
        // always miss, quietly, which is the bug this whole path exists to avoid.
        assert_eq!(expand_env_placeholders(r"%ProgramFiles%\ffmpeg\bin\ffmpeg.exe", env), None);
        // Set-but-empty is treated as unset: `""` would leave a plausible-looking `\ffmpeg\…`.
        assert_eq!(expand_env_placeholders(r"%BLANK%\ffmpeg.exe", env), None);
        // THE macOS invariant: an entry with no placeholders comes back byte-identical, so the
        // shared probe loop cannot alter what the Homebrew/MacPorts paths resolve to.
        for p in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/opt/local/bin/ffmpeg"] {
            assert_eq!(expand_env_placeholders(p, env).as_deref(), Some(p));
        }
        // Stray percents follow cmd's rules rather than eating the rest of the path.
        assert_eq!(expand_env_placeholders("/tmp/50%", env).as_deref(), Some("/tmp/50%"));
        assert_eq!(expand_env_placeholders("a%%b", env).as_deref(), Some("a%b"));
    }

    #[test]
    fn bundled_ffmpeg_name_carries_the_platform_exe_suffix() {
        // The bundled-copy probe gates on `.exists()`, so this suffix is the difference
        // between that branch working on Windows and never firing at all (#658 Tier 1).
        #[cfg(windows)]
        assert_eq!(bundled_ffmpeg_name(), "ffmpeg.exe");
        #[cfg(not(windows))]
        assert_eq!(bundled_ffmpeg_name(), "ffmpeg"); // EXE_SUFFIX is "" — unchanged on macOS
    }

    #[test]
    fn output_base_prefers_the_os_videos_folder() {
        // macOS: dirs' videos dir IS `$HOME/Movies`, so this is byte-for-byte today's path.
        assert_eq!(
            output_base(Some(PathBuf::from("/Users/j/Movies")), Some(PathBuf::from("/Users/j"))),
            PathBuf::from("/Users/j/Movies").join("Organon")
        );
        // Windows: whatever the Known Folder actually resolved to, redirection included.
        assert_eq!(
            output_base(Some(PathBuf::from(r"D:\OneDrive\Videos")), None),
            PathBuf::from(r"D:\OneDrive\Videos").join("Organon")
        );
        // Linux with no xdg-user-dirs: name the conventional folder rather than the CWD.
        assert_eq!(
            output_base(None, Some(PathBuf::from("/home/j"))),
            PathBuf::from("/home/j/Videos").join("Organon")
        );
        // Neither: the pre-existing last resort, unchanged.
        assert_eq!(output_base(None, None), PathBuf::from("."));
    }

    #[test]
    fn ffmpeg_args_shapes() {
        let out = PathBuf::from("/tmp/x.mov");
        let hdr = ffmpeg_args(1920, 1080, DEFAULT_FPS, RecordFormat::HdrPq, &out);
        assert!(hdr.contains(&"hevc_videotoolbox".to_string()));
        assert!(hdr.contains(&"smpte2084".to_string()));
        assert!(hdr.contains(&"rgba64le".to_string()));
        assert!(hdr.contains(&"1920x1080".to_string()));
        assert!(hdr.contains(&"p010le".to_string()));
        assert_eq!(hdr.last().unwrap(), "/tmp/x.mov");

        let out2 = PathBuf::from("/tmp/x.mp4");
        let sdr = ffmpeg_args(1080, 1920, DEFAULT_FPS, RecordFormat::Sdr, &out2);
        assert!(sdr.contains(&"h264_videotoolbox".to_string()));
        assert!(sdr.contains(&"bt709".to_string()));
        assert!(sdr.contains(&"PIXFMT".to_string())); // placeholder the caller fills in
        assert!(sdr.contains(&"1080x1920".to_string()));
    }

    #[test]
    fn record_headroom_matches_master() {
        // The composite target and the recorder's nits clamp must agree.
        assert!((record_headroom() - DEFAULT_MASTER_NITS / REF_WHITE_NITS).abs() < 1e-6);
        assert!(record_headroom() > 1.0, "mastering peak is above SDR white");
    }

    #[test]
    fn wav_header_is_valid_float32() {
        // 2 stereo frames = 4 samples.
        let wav = build_wav_f32(&[0.0, 1.0, -1.0, 0.5], 48_000, 2);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 3); // IEEE float
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2); // channels
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 48_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 32); // bits
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 16);
        assert_eq!(wav.len(), 44 + 16);
        assert_eq!(f32::from_le_bytes([wav[44], wav[45], wav[46], wav[47]]), 0.0);
        assert_eq!(f32::from_le_bytes([wav[48], wav[49], wav[50], wav[51]]), 1.0);
    }

    // --- Phrase-chunk maths -------------------------------------------------
    // The music-video case that motivated chunk mode: an 8-beat phrase at 172 BPM cut into
    // a 24 fps timeline. 8 beats = 2.790698 s = 66.9767 frames — deliberately NOT an integer,
    // which is the whole difficulty.

    const BPM_172: f64 = 172.0;
    const PHRASE_8: f64 = 8.0;
    const FPS_24: f64 = 24.0;

    #[test]
    fn phrase_length_is_not_a_whole_number_of_frames() {
        let l = phrase_frames(PHRASE_8, BPM_172, FPS_24);
        assert!((l - 66.976_744).abs() < 1e-5, "8 beats @172 @24fps = {l} frames");
        // At 60 fps the same phrase is 167.44 frames — also fractional.
        assert!((phrase_frames(PHRASE_8, BPM_172, 60.0) - 167.441_86).abs() < 1e-4);
    }

    #[test]
    fn chunk_lengths_bracket_the_true_length() {
        // Every chunk is one of the two integers bracketing 66.977 — never anything else.
        for k in 0..200u64 {
            let n = chunk_frames(k, PHRASE_8, BPM_172, FPS_24);
            assert!(n == 66 || n == 67, "chunk {k} was {n} frames");
        }
        // And the short ones do occur — it isn't just 67 forever (which would drift long).
        let shorts = (0..200u64).filter(|k| chunk_frames(*k, PHRASE_8, BPM_172, FPS_24) == 66).count();
        assert!(shorts > 0, "expected some 66-frame chunks to hold the average");
    }

    #[test]
    fn boundaries_never_accumulate_error() {
        // THE invariant. Absolute rounding keeps every cut within half a frame of true
        // musical position forever; accumulating `+L` instead would walk the clips off the
        // grid. 500 chunks is ~23 minutes at this tempo — far past any real take.
        let l = phrase_frames(PHRASE_8, BPM_172, FPS_24);
        for k in 0..500u64 {
            let actual = chunk_start_frame(k, PHRASE_8, BPM_172, FPS_24) as f64;
            let ideal = k as f64 * l;
            assert!((actual - ideal).abs() <= 0.5, "chunk {k} drifted {} frames", actual - ideal);
        }
        // Sanity: 43 chunks of 8 beats at 172 BPM is exactly 120 s = exactly 2880 frames @24.
        assert_eq!(chunk_start_frame(43, PHRASE_8, BPM_172, FPS_24), 2880);
    }

    #[test]
    fn chunk_frames_sum_to_the_boundary() {
        // The per-chunk quotas must tile the timeline with no gaps or overlaps, or clips
        // butt-joined in an NLE would slide.
        let mut total = 0u64;
        for k in 0..97u64 {
            total += chunk_frames(k, PHRASE_8, BPM_172, FPS_24);
        }
        assert_eq!(total, chunk_start_frame(97, PHRASE_8, BPM_172, FPS_24));
    }

    #[test]
    fn beat_driven_pacing() {
        // One 8-beat phrase of musical time owes ~67 frames.
        let ideal = ideal_frame(PHRASE_8, BPM_172, FPS_24);
        assert!((ideal - 66.976_744).abs() < 1e-5);
        // At the very start we owe frame #1.
        assert_eq!(frames_due_at(0.0, 0), 1);
        // Having emitted 10 but standing at ideal frame 12.4, we owe 3 (12.4 → 13 target).
        assert_eq!(frames_due_at(12.4, 10), 3);
        // Ahead of the clock → drop this render frame.
        assert_eq!(frames_due_at(3.0, 9), 0);
        // A long stall is capped, same as the wall-clock pacer.
        assert_eq!(frames_due_at(1000.0, 1), 4);
        // A stopped/zero tempo can't divide by zero.
        assert_eq!(ideal_frame(4.0, 0.0, FPS_24), 0.0);
    }

    #[test]
    fn next_boundary_lands_on_the_grid() {
        // From mid-phrase, the next 8-beat boundary of an absolute grid.
        assert!((next_boundary(3.2, 8.0, 0.0) - 8.0).abs() < 1e-9);
        assert!((next_boundary(8.1, 8.0, 0.0) - 16.0).abs() < 1e-9);
        // Strictly after: standing exactly ON a boundary yields the NEXT one, so arming at
        // a downbeat can't produce a zero-length first chunk.
        assert!((next_boundary(16.0, 8.0, 0.0) - 24.0).abs() < 1e-9);
        // An offset grid (a track with a pickup bar).
        assert!((next_boundary(5.0, 8.0, 2.0) - 10.0).abs() < 1e-9);
        // Before the offset, the grid still resolves forward.
        assert!(next_boundary(0.0, 8.0, 2.0) > 0.0);
        // The host wraps pos_beats mod 1024; 1024 is a multiple of 8, so the grid is
        // continuous across the wrap (a phrase that doesn't divide 1024 would hiccup once).
        assert!((next_boundary(1023.5, 8.0, 0.0) - 1024.0).abs() < 1e-9);
    }

    #[test]
    fn fps_rationals_stay_exact() {
        // 23.976 must reach ffmpeg as 24000/1001, not a rounded 24 — an NLE sequence at
        // 23.976 would otherwise drift a frame against the file every ~42 seconds.
        let ntsc = Fps::new(24000, 1001);
        assert_eq!(ntsc.rate_arg(), "24000/1001");
        assert!((ntsc.value() - 23.976_023).abs() < 1e-5);
        assert_eq!(Fps::new(24, 1).rate_arg(), "24");
        assert_eq!(Fps::new(24, 1).label(), "24 fps");
        assert_eq!(DEFAULT_FPS, Fps::new(60, 1));
        // The cycle covers every choice and returns to the start.
        let mut f = FPS_CHOICES[0];
        for _ in 0..FPS_CHOICES.len() {
            f = next_fps(f);
        }
        assert_eq!(f, FPS_CHOICES[0]);
        // An unrecognised rate restarts the cycle rather than sticking.
        assert_eq!(next_fps(Fps::new(48, 1)), FPS_CHOICES[0]);
    }

    #[test]
    fn ffmpeg_args_carry_the_fractional_rate() {
        let a = ffmpeg_args(1920, 1080, Fps::new(24000, 1001), RecordFormat::Sdr, &PathBuf::from("/tmp/x.mp4"));
        assert!(a.contains(&"24000/1001".to_string()));
    }

    #[test]
    fn mux_args_shape() {
        let a = ffmpeg_mux_args(
            &PathBuf::from("/tmp/v.mov"),
            &PathBuf::from("/tmp/a.wav"),
            &PathBuf::from("/tmp/final.mov"),
        );
        assert!(a.contains(&"copy".to_string())); // video copied, not re-encoded
        assert!(a.contains(&"aac".to_string()));
        assert!(a.contains(&"-shortest".to_string()));
        assert_eq!(a.last().unwrap(), "/tmp/final.mov");
    }
}
