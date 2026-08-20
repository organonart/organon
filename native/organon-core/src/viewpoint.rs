//! Where the instrument ends — the viewpoint's band and its origin, and nothing else.
//!
//! ⚠️ **Not `camera`, on purpose.** `organon_console::camera` already exists and is a different
//! subject entirely — *who owns the viewpoint*, the hand or the agent — and it reads these
//! constants. Two `camera` modules in one workspace, one reading the other, is a trap; this one
//! is named for the word the surrounding prose already used.
//!
//! organon#49 Tier 5a. Four constants, lifted out of `scene_input` for the reason its own
//! [`PITCH_LIMIT`] doc already gave in capitals: **one number, four readers**. Those readers
//! used to be four call sites in one crate. They are now spread across three —
//! `World::apply_camera_input` and the camera finalization in `organon-world`,
//! [`CameraFraming::in_range`](crate::console_ops::CameraFraming::in_range) here, and
//! `console_main`'s `console.camera` schema, heading for `organon-console` in T5c.
//!
//! A constant with readers in three crates has to live in the crate all three can see, or it
//! becomes two constants — and the failure mode the original doc names is exactly what that
//! looks like: *"an agent comes to be refused a value the hand can reach, or granted one it
//! cannot — and either reads as the camera being broken rather than as two constants
//! disagreeing."*
//!
//! 📌 `scene_input` **re-exports all four**, so every `scene_input::PITCH_LIMIT` path in the
//! tree resolves unchanged. This is a move, not a fork; there is still one number.
//!
//! ⚠️ **The three `DEFAULT_*` came too, and they are not limits.** They are where the viewpoint
//! starts and what `organon console camera --reset` returns it to — so the console lane reads
//! them for the same reason it reads the bounds, and splitting the pair across two crates would
//! leave `--reset` resolving its target one crate away from the band it has to land inside.

/// How far the viewpoint may tip, in radians — straight down to straight up with a little held
/// back, so the orbit basis never degenerates against `Vec3::Y`.
///
/// 🚨 **One number, four readers.** `World::apply_camera_input` clamps to it, the camera
/// finalization clamps the auto-orbit's *sum* to it, `cli`'s `console camera` validates against
/// it, and `console_main`'s `console.camera` schema declares it as its `ArgKind::Float` range. A
/// second copy is how an agent comes to be refused a value the hand can reach, or granted one it
/// cannot — and either reads as the camera being broken rather than as two constants disagreeing.
pub const PITCH_LIMIT: f32 = 1.5;

/// The closest the viewpoint may sit to the pivot. Near zero rather than at it, so you can zoom
/// all the way *through* the centre and come out the other side with geometry still visible.
pub const DISTANCE_MIN: f32 = 0.1;

/// The furthest the viewpoint may sit from the pivot. See [`PITCH_LIMIT`] on the one-number rule.
pub const DISTANCE_MAX: f32 = 4000.0;

/// How far yaw may be *asked* for, in radians — one full turn either way.
///
/// ⚠️ **Unlike the two above, this is not a clamp anywhere.** Yaw is an angle: the trigonometry
/// wraps, so every value is meaningful and `World` stores whatever it is given. This is the bound
/// the **command lane** declares, and it exists so the schema can state a range at all. ±2π covers
/// every distinct viewpoint twice over; a request outside it is a unit mistake (degrees for
/// radians is the likely one) and is better refused with a record than silently wrapped.
pub const YAW_LIMIT: f32 = std::f32::consts::TAU;

/// Where the viewpoint starts, and what `organon console camera --reset` returns it to.
///
/// 📌 These are `World::new`'s own initial values, named rather than repeated — which is what
/// makes "reset" provably *the framing the window opened with* instead of three numbers that were
/// true on the day someone copied them.
pub const DEFAULT_YAW: f32 = 0.7;

/// See [`DEFAULT_YAW`].
pub const DEFAULT_PITCH: f32 = 0.45;

/// See [`DEFAULT_YAW`].
pub const DEFAULT_DISTANCE: f32 = 520.0;
