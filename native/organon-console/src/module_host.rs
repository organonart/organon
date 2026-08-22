//! **The console's half of the hosted-module contract — map, launch, feed, and the sentence.**
//!
//! `CONSOLE_ARCHITECTURE.md` §1.14:
//!
//! > **A producer yields a texture the console can sample, at a size the console asks for.**
//!
//! `doc/organon_module_viewport.md` §9's **T3b's launcher and T5's lifecycle**. Everything the
//! *protocol* needs already exists in [`organon_module`]; this file is the party that creates
//! the channel, starts the process on the other end of it, tells that process where the channel
//! is, and turns whatever comes back into one sentence a rectangle can carry.
//!
//! 🚨 **wgpu-free on purpose, exactly like the rest of this crate.** The texture is
//! `organon_module::FrameTexture`'s and lives with the window, in `console_main.rs`. What is
//! here is every decision that can be made without a device — which is nearly all of them — so
//! the path composition, the binary name, the environment, the lifecycle and the four §4.6
//! sentences are headless tests rather than things only a running console can exercise.
//!
//! # 🚨 How the producer learns where the channel is
//!
//! **The console hands it the FULL ABSOLUTE PATH, in [`CHANNEL_ENV`], and the producer composes
//! nothing.** The alternative — handing over `$ORGANON_IPC_NS` and letting the producer rebuild
//! the path — was rejected, and the reason is structural rather than a preference:
//!
//! * The path is `ipc::ns_file(channel_file_name(producer, instance))`. The second half is in
//!   `organon-module` and a module has it. **The first half is `organon-core`'s, and
//!   `organon-module`'s manifest forbids depending on that crate** — in as many words:
//!   *"resolving a path is the console's job, done once at the call site, and taking a dependency
//!   on the engine's spine to get one `PathBuf` would put `glam`, `half`, `bytemuck`, `serde` and
//!   `serde_json` into a game's build for a string."*
//! * So handing over the namespace would oblige a module in **another repository** to
//!   re-implement a path rule owned by a crate it may not link. Two implementations of one rule
//!   in two trees, and the symptom of drift is a channel that opens nothing with no error
//!   anywhere to say why — the module would be looking in the right directory for a file with
//!   almost the right name.
//! * `channel.rs` already settled the half of this that was its own: *"This crate names the file
//!   and the console places it."* Handing over the placed result is that sentence carried one
//!   step further rather than a second idea.
//! * And it survives a channel that one day is not under `$TMPDIR` at all.
//!
//! 📌 **[`NAMESPACE_ENV`] is set on the child as well, and it is NOT how the channel is found.**
//! It is set for the reason `term.rs` already sets it on every tab the console spawns: an
//! `organon` CLI — or anything else Organon-shaped — run *by the module* must address this
//! console's session rather than the default namespace. Naming both variables and saying which
//! one is the address is deliberate; a producer that reached for the namespace in order to
//! rebuild the path would be doing the exact thing the bullets above rule out.
//!
//! ⚠️ **An environment variable rather than an argv argument, and this is the deciding argument
//! rather than a taste.** §9's T1 requires the module's own `main.rs` to keep its window, its
//! pump and a working launch script — so a module keeps its own command line, written by
//! somebody else. An argument appended by the console can collide with an argument parser in
//! another repository; an environment variable cannot collide with anything. It is also not
//! visible to every other user on the machine in a process listing, which argv is.
//!
//! # 🚨 What is NOT decided here, and must not migrate in
//!
//! * **What the module is.** §4.6's *never*: no module-shaped verb, no knowledge of what the
//!   picture contains. This file knows a name, a path, a size and an exit status.
//! * **Whether a module may run.** [`crate::module`] owns that, and this file reads its answer.
//! * **What a module's binary is called.** It is *derived* from the producer name — see
//!   [`module_binary`]. A manifest cannot name a program to run, which is
//!   [`crate::module_work::Tool`]'s rule arriving at the one place it would otherwise be
//!   quietly given up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::Instant;

use organon_module::{
    channel_file_name, FrameCapacity, InputEvent, Lifecycle, ModuleChannel, PixelFormat, Poll,
    Presence, PushFault,
};

use crate::module::{check_producer_name, ModuleFault, ModuleState, BUILD_VERB, RESTART_VERB};
use crate::module_work::{artifact_dir, WorkFault};

/// **The address.** The absolute path of the frame channel, in the child's environment.
///
/// See this module's header for why it is the path rather than the namespace, and why it is an
/// environment variable rather than an argument.
pub const CHANNEL_ENV: &str = "ORGANON_MODULE_CHANNEL";

/// The namespace, set on the child for `term.rs`'s reason and **not** as a way to find the
/// channel. The same spelling `organon-core::ipc` reads, because it is the same variable.
pub const NAMESPACE_ENV: &str = "ORGANON_IPC_NS";

/// Override the frame capacity, as `<width>x<height>`. Unset is [`DEFAULT_CAPACITY`].
pub const CAPACITY_ENV: &str = "ORGANON_MODULE_CAPACITY";

/// The largest picture a channel is built to carry, and therefore the size of the mapping.
///
/// 🚨 **Capacity is chosen once, at creation, and a bigger request is CLAMPED rather than
/// refused** — `ModuleChannel::ask_size`'s rule, and the right one: a console whose region grew
/// past what it planned for should show a slightly small picture now, not a black rectangle.
///
/// 2560×1440 because that is the largest size
/// `doc/measurements/module-frame-boundary-2026-08-21.md` actually measured (1.41–1.55 ms round
/// trip, 8.4–9.3 % of a 60 Hz frame) — so the default is the biggest picture there is evidence
/// for rather than the biggest one somebody might want.
///
/// ⚠️ **The cost is address space rather than resident memory**, which is what makes the generous
/// answer the cheap one: three slots of 2560×1440×4 is ~44 MB of mapped file, and a channel
/// running at region size touches the first few hundred kilobytes of each slot and nothing else.
/// [`CAPACITY_ENV`] moves it without a rebuild for a display this does not fit.
pub const DEFAULT_CAPACITY: (u32, u32) = (2560, 1440);

/// The wire format the console asks for.
///
/// `Rgba8UnormSrgb` is `console_main.rs`'s `BACKDROP_FORMAT` — the same bytes the engine's own
/// render is stored in, so a module's picture and Organon's arrive at egui having been
/// linearized the same number of times. ⚠️ **The view is a different question and it has bitten
/// this console before**: see `organon_module::gpu::linear_view_format`.
pub const WIRE_FORMAT: PixelFormat = PixelFormat::Rgba8UnormSrgb;

// ---------------------------------------------------------------------------------------
// Where the channel is, and what the binary is called
// ---------------------------------------------------------------------------------------

/// `$TMPDIR/<namespace>-module-<producer>-<instance>.frames`.
///
/// 🚨 **The one composition site**, and deliberately the only place in the tree where
/// `organon-core`'s `ns_file` and `organon-module`'s `channel_file_name` meet. The producer never
/// performs this arithmetic; it is handed the answer (see the module header).
///
/// ⚠️ **The producer name is checked before it becomes part of a file name**, on
/// [`crate::module_work::checkout_dir`]'s precedent and for a sharper version of its reason: a
/// name containing a separator would compose `…/module-../x-0.frames` and place the mapping
/// outside `$TMPDIR` entirely. [`crate::module::check_producer_name`] refuses that, and every
/// route a name reaches here by has already run it — `Producer::stored` does, `set_module` does.
/// This is the third, because a path component built from someone else's string is exactly the
/// kind of check that is correct until a fourth caller arrives.
pub fn channel_path(producer: &str, instance: u32) -> Result<PathBuf, ModuleFault> {
    check_producer_name(producer)?;
    Ok(organon_core::ipc::ns_file(&channel_file_name(producer, instance)))
}

/// `<checkout>/target/release/<producer>[.exe]`.
///
/// 🚨 **Derived, never read out of the manifest**, and this is where that rule earns its keep
/// rather than merely being consistent. [`crate::module_work::Tool`] exists so *"no string out of
/// a manifest — or out of `modules.json`, which is a file a person can edit — can become the
/// first argument to `Command::new`"*, and a `binary = "…"` key would hand that back in one line.
/// [`crate::module::check_producer_name`] has already refused every string that could name
/// something other than one file in one directory.
///
/// 📌 It is also [`crate::module_work::artifact_dir`]'s own argument one level down: *"a stored
/// path is a second statement of where the build went, and the two can come apart"*. A module
/// whose cargo target is not named after its producer therefore does not launch, and says so by
/// name — which is a legible refusal rather than a wrong program.
pub fn module_binary(artifacts: &Path, producer: &str) -> PathBuf {
    artifacts.join(producer).with_extension(std::env::consts::EXE_EXTENSION)
}

/// The capacity a channel is created at — [`CAPACITY_ENV`], else [`DEFAULT_CAPACITY`].
///
/// ⚠️ **A malformed value falls back and says so**, rather than refusing to host anything. An
/// unparseable capacity is a typo in a launch shim; refusing the module over it would turn a
/// cosmetic mistake into a console that cannot show a picture — the trade `theme::select` already
/// made for the palette, for the same reason, and the note is returned rather than swallowed for
/// the same reason again.
pub fn capacity_from(raw: Option<&str>) -> (FrameCapacity, Option<String>) {
    let (w, h) = DEFAULT_CAPACITY;
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return (FrameCapacity::new(w, h, WIRE_FORMAT), None);
    };
    let parsed = raw
        .split_once(['x', 'X'])
        .and_then(|(a, b)| Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?)))
        .filter(|(a, b)| *a > 0 && *b > 0);
    match parsed {
        Some((cw, ch)) => (FrameCapacity::new(cw, ch, WIRE_FORMAT), None),
        None => (
            FrameCapacity::new(w, h, WIRE_FORMAT),
            Some(format!(
                "{CAPACITY_ENV} is `{raw}`, which is not `<width>x<height>` — using the default \
                 {w}x{h}"
            )),
        ),
    }
}

// ---------------------------------------------------------------------------------------
// The launch, as data
// ---------------------------------------------------------------------------------------

/// **Everything a launch is, before anything is spawned.**
///
/// 🚨 A value rather than a `Command`, for [`crate::module_work::Workshop`]'s reason one door
/// along: a `Command` is a thing you can only check by running it, and the interesting questions
/// here — *which program*, *which path is it told*, *does the namespace reach it* — are all
/// answerable about a struct in a headless test. [`LaunchPlan::spawn`] is the only line that
/// touches the operating system, and it contains no decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub producer: String,
    /// The binary. [`module_binary`]'s answer — never a string out of a file.
    pub program: PathBuf,
    /// What [`CHANNEL_ENV`] will be set to.
    pub channel: PathBuf,
    /// The working directory: the module's own checkout, so a module that reads a file beside
    /// itself finds the tree it was built from rather than wherever the console was started.
    pub cwd: PathBuf,
    /// The environment additions, sorted by name so a test reads as a table and a diff of two
    /// plans is a diff of the environment rather than of a hash order.
    pub env: BTreeMap<String, String>,
}

/// Why a module could not be launched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchFault {
    /// Something about the name or the store. Carries the other module's own sentence.
    Work(String),
    /// The producer is not approved, or is approved and not built. ⚠️ Not a fault in the sense
    /// the others are — it is §4.6's first two rows, which the rectangle already knows how to
    /// say. It arrives here because *"do not launch"* and *"say why there is no picture"* are
    /// the same answer.
    Vacant(ModuleState),
    /// The build record names bytes, and the binary those bytes were supposed to produce is not
    /// on the disk. **The commonest real failure**, and the one whose default message would be
    /// least useful: a module whose cargo target is not named after its producer builds
    /// perfectly and lands somewhere this cannot find.
    NoBinary { producer: String, looked_at: PathBuf },
    /// The channel file could not be created.
    NoChannel { producer: String, why: String },
    /// The process would not start.
    NoProcess { producer: String, program: PathBuf, why: String },
}

impl LaunchFault {
    pub fn sentence(&self) -> String {
        match self {
            LaunchFault::Work(s) => s.clone(),
            LaunchFault::Vacant(state) => state.sentence(),
            LaunchFault::NoBinary { producer, looked_at } => format!(
                "`{producer}` is built, and there is no program called `{producer}` in {} — a \
                 module's binary is named after its producer, so its cargo target must be too. \
                 `{BUILD_VERB}` builds it again",
                looked_at.display(),
            ),
            LaunchFault::NoChannel { producer, why } => format!(
                "`{producer}` could not be given a frame channel: {why} — nothing was started"
            ),
            LaunchFault::NoProcess { producer, program, why } => format!(
                "`{producer}` would not start: {why} ({}). `{RESTART_VERB}` tries again",
                program.display()
            ),
        }
    }
}

impl From<ModuleFault> for LaunchFault {
    fn from(f: ModuleFault) -> Self {
        LaunchFault::Work(f.sentence())
    }
}

impl From<WorkFault> for LaunchFault {
    fn from(f: WorkFault) -> Self {
        LaunchFault::Work(f.sentence())
    }
}

/// Work out what launching `producer` would be, without doing any of it.
///
/// `namespace` is passed rather than read so this stays headless — `console_main.rs` hands it
/// `organon_core::ipc::namespace()`, which is the same value [`channel_path`] composed the path
/// from, resolved once per process.
///
/// ⚠️ **`exists` is a parameter, so the one filesystem question this asks is the caller's.** A
/// test can answer it either way and read the refusal; the console passes `Path::is_file`.
pub fn plan_launch(
    store_root: &Path,
    producer: &str,
    instance: u32,
    namespace: &str,
    registry: &crate::module::ModuleRegistry,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<LaunchPlan, LaunchFault> {
    check_producer_name(producer)?;
    // 🚨 The trust gate, and it is `module.rs`'s answer rather than a second reading of the
    // registry: nothing launches for a producer that is not approved AND built.
    if let Some(state) = registry.vacancy(producer) {
        return Err(LaunchFault::Vacant(state));
    }
    let artifacts = artifact_dir(store_root, producer)?;
    let program = module_binary(&artifacts, producer);
    if !exists(&program) {
        return Err(LaunchFault::NoBinary {
            producer: producer.to_string(),
            looked_at: artifacts,
        });
    }
    let channel = channel_path(producer, instance)?;
    let mut env = BTreeMap::new();
    env.insert(CHANNEL_ENV.to_string(), channel.display().to_string());
    env.insert(NAMESPACE_ENV.to_string(), namespace.to_string());
    Ok(LaunchPlan {
        producer: producer.to_string(),
        program,
        channel,
        cwd: crate::module_work::checkout_dir(store_root, producer)?,
        env,
    })
}

impl LaunchPlan {
    /// Start it. **The only function in this file that touches the operating system**, and it
    /// makes no decisions — everything it uses was decided by [`plan_launch`].
    ///
    /// ⚠️ **stdout and stderr are inherited on purpose.** A module's own diagnostics are the
    /// first thing anybody debugging one will want, and a console that swallowed them would be
    /// asking a person to reproduce the failure outside the console in order to see it. The cost
    /// is a chatty module interleaving with `organon-console:` lines, which is legible; the
    /// alternative is a silent process nobody can question.
    pub fn spawn(&self) -> Result<Child, LaunchFault> {
        let mut cmd = Command::new(&self.program);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd.current_dir(&self.cwd);
        cmd.spawn().map_err(|e| LaunchFault::NoProcess {
            producer: self.producer.clone(),
            program: self.program.clone(),
            why: e.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------------------
// What the rectangle says when it has no picture
// ---------------------------------------------------------------------------------------

/// **§4.6's four rows, as one total type.**
///
/// 🚨 **The console draws this exactly when it has no texture to sample**, which is what makes
/// *"never the last good frame"* a property of the drawing code rather than a rule at the call
/// site. `organon_module::gpu::FrameTexture::view` is `None` for every state §4.6 forbids
/// painting through, and its own doc says so: *"a call site that draws the sentence whenever this
/// is `None` has implemented the whole vacancy rule."* This is the other half of that sentence.
///
/// ⚠️ **It is total, including for a producer that is perfectly healthy.** [`Vacancy::Live`]
/// carrying `Presence::Live` is the one frame between *"the process is up"* and *"a picture has
/// arrived"*, and a rectangle needs something to say in it. A type with a hole for the working
/// case would push that decision back to the paint site, which is where it would be got wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vacancy {
    /// Rows 1–2. Nothing is running and [`crate::module`] knows what stands in the way.
    Idle(ModuleState),
    /// Rows 3–4 and *starting*. A process exists and the channel judged it.
    Live { producer: String, presence: Presence },
    /// 🚨 **Row 4, from the process handle rather than from the mapping.**
    ///
    /// `presence.rs` states the limit this answers: *"hung and exited-without-a-farewell are not
    /// distinguishable from inside a shared mapping, and the thing that genuinely knows is the
    /// process handle, which is T3b's launcher's."* This is that launcher and it holds it — so a
    /// module that died without saying goodbye is reported as **dead**, immediately and with its
    /// exit code, rather than as a silence that becomes `Lost` five seconds later.
    Exited { producer: String, code: Option<i32> },
    /// The launch itself did not happen.
    Failed(LaunchFault),
}

impl Vacancy {
    /// The sentence the rectangle carries.
    ///
    /// 📌 **Every verb in it comes from `module.rs`'s constants**, through
    /// `Presence::sentence`'s `restart_verb` parameter, so the words in a rectangle and the words
    /// a person types cannot drift apart.
    pub fn sentence(&self) -> String {
        match self {
            Vacancy::Idle(state) => state.sentence(),
            Vacancy::Live { producer, presence } => presence.sentence(producer, RESTART_VERB),
            Vacancy::Exited { producer, code: Some(code) } => format!(
                "{producer} exited with code {code}. `{RESTART_VERB}` starts it again"
            ),
            Vacancy::Exited { producer, code: None } => format!(
                "{producer} was killed before it said goodbye. `{RESTART_VERB}` starts it again"
            ),
            Vacancy::Failed(fault) => fault.sentence(),
        }
    }

    /// 🚨 **Whether the console may paint a picture in this state** — §4.6's *never the last good
    /// frame*, as a value the paint site can be tested against.
    ///
    /// ⚠️ **It is the second lock, not the first.** `FrameTexture::view()` already returns `None`
    /// for every forbidden state, so a correct paint site never has to ask this. It exists
    /// because that guarantee lives in another crate behind a feature flag, and a rule that is
    /// only true because of code you cannot see from here is a rule nothing local can prove. The
    /// two agreeing is asserted; the two disagreeing is what the test would catch.
    pub fn picture_may_be_shown(&self) -> bool {
        match self {
            Vacancy::Live { presence, .. } => presence.picture_may_be_shown(),
            Vacancy::Idle(_) | Vacancy::Exited { .. } | Vacancy::Failed(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------------------
// What a picture is allowed to be told
// ---------------------------------------------------------------------------------------

/// One frame of pointer state over a hosted rectangle.
///
/// 📌 **A struct rather than eight arguments**, on `presence::Observed`'s rule and for its reason:
/// the interesting cases here are *combinations* — leaving the rectangle with a button held,
/// losing focus mid-drag — and a positional call of eight values is a call nobody can check by
/// reading it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerFrame {
    /// Whether the pointer is inside the rectangle **this** frame.
    pub over: bool,
    /// ...and whether it was last frame. The transition is what carries [`InputEvent::ReleaseAll`].
    pub was_over: bool,
    /// Whether the console's window has keyboard focus.
    pub focused: bool,
    /// ...and whether it did last frame.
    pub was_focused: bool,
    pub dx: f32,
    pub dy: f32,
    /// Buttons that went down this frame, and buttons that came up — in
    /// `organon_module::MouseButton::ALL` order, which is the order the wire numbers them in.
    pub pressed: [bool; 5],
    pub released: [bool; 5],
}

/// **What the console forwards to a module that has not been clicked into.**
///
/// 🚨 **§5.3, and the restraint is the design.** *"Before the click it is a picture: no key claim,
/// no pointer capture, wheel scrolls the page."* So this forwards **motion and mouse buttons over
/// the rectangle, and nothing else** — no keys, no wheel, and no absolute position (the protocol
/// has no way to express one, which is `input.rs`'s deliberate refusal rather than an omission
/// here).
///
/// 📌 That is not a placeholder for the latch; it is what a *picture* may be told. The latch is
/// the step that adds the keyboard, and §5.3 is explicit that **the way out must be decided before
/// the way in is built** — a key the module is told it will never receive, which is
/// `input::RESERVED`'s job, and a decision that is James's rather than a session's. Until then a
/// module gets exactly the input a person could give a photograph, and the click that would latch
/// it is forwarded already, so the gesture is legible from the module's side before it means
/// anything on this one.
///
/// ⚠️ **The two `ReleaseAll` rules are the whole of the correctness here**, and both are about a
/// button that goes down inside the rectangle and comes up somewhere the console never sees:
/// leaving the rectangle, and losing focus. Without them a module holds a button for ever and
/// reads as wedged — which looks exactly like the failure §4.6's fourth row is about, arrived at
/// from the one direction that is the console's fault rather than the module's.
pub fn pointer_events(f: &PointerFrame) -> Vec<InputEvent> {
    use organon_module::{Button, MouseButton};

    // Focus loss outranks everything, and is sent once at the edge rather than every frame: a
    // console in the background would otherwise fill the ring with a verb that says "nothing".
    if f.was_focused && !f.focused {
        return vec![InputEvent::ReleaseAll];
    }
    if !f.focused {
        return Vec::new();
    }
    if !f.over {
        // Leaving with something held is the case this exists for.
        return if f.was_over { vec![InputEvent::ReleaseAll] } else { Vec::new() };
    }

    let mut out = Vec::new();
    // 🚨 **Motion before buttons.** A press is *at* a position, so a click delivered before the
    // motion that reached it is a click at where the pointer used to be. At 60 Hz that is a few
    // pixels; on a slow frame it is the width of the whole gesture.
    if f.dx != 0.0 || f.dy != 0.0 {
        out.push(InputEvent::Pointer { dx: f.dx, dy: f.dy });
    }
    for (i, b) in MouseButton::ALL.iter().enumerate() {
        if f.pressed.get(i).copied().unwrap_or(false) {
            out.push(InputEvent::Down(Button::Mouse(*b)));
        }
    }
    for (i, b) in MouseButton::ALL.iter().enumerate() {
        if f.released.get(i).copied().unwrap_or(false) {
            out.push(InputEvent::Up(Button::Mouse(*b)));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// One live module
// ---------------------------------------------------------------------------------------

/// **One producer, one process, one channel.**
///
/// 🚨 **The launcher holds the process handle, and that is not bookkeeping.** It is the one fact
/// the shared mapping cannot produce — see [`Vacancy::Exited`]. A caller asks the handle *first*,
/// so a crash is named the instant the operating system knows about it rather than after
/// `Timings::dead_after` has elapsed on a counter that stopped for a reason nobody can see.
pub struct Hosted {
    producer: String,
    instance: u32,
    path: PathBuf,
    channel: ModuleChannel,
    /// `None` once the process has been reaped — see [`Hosted::exited`].
    child: Option<Child>,
    exited: Option<ExitStatus>,
    /// The last size asked for, in **physical pixels**. Kept so a caller can ask every frame
    /// without the channel writing a control block every frame.
    asked: (u32, u32),
    lifecycle: Lifecycle,
}

impl Hosted {
    /// Create the channel, then start the process on the other end of it.
    ///
    /// 🚨 **The order is not negotiable and the contract depends on it.** `ModuleChannel::create`
    /// seeds two producer-owned words *"before the producer process exists, so there is no second
    /// party to race"* — its own comment — and a producer that opened the file first would map a
    /// header nobody had written. So: channel, then spawn, always.
    ///
    /// ⚠️ **The channel file is remade every time**, which is what makes a restart clean: a stale
    /// mapping from a previous run holds a `latest_slot` and a frame counter a new producer would
    /// inherit.
    pub fn start(
        plan: &LaunchPlan,
        capacity: FrameCapacity,
        instance: u32,
    ) -> Result<Hosted, LaunchFault> {
        let channel = ModuleChannel::create(&plan.channel, capacity).map_err(|e| {
            LaunchFault::NoChannel { producer: plan.producer.clone(), why: e.to_string() }
        })?;
        let child = plan.spawn()?;
        Ok(Hosted {
            producer: plan.producer.clone(),
            instance,
            path: plan.channel.clone(),
            channel,
            child: Some(child),
            exited: None,
            asked: (capacity.max_width, capacity.max_height),
            lifecycle: Lifecycle::Attached,
        })
    }

    pub fn producer(&self) -> &str {
        &self.producer
    }

    /// Which instance this is — [`channel_path`]'s second argument, kept so a log line can tell
    /// two hosts of one producer apart.
    pub fn instance(&self) -> u32 {
        self.instance
    }

    pub fn channel_path(&self) -> &Path {
        &self.path
    }

    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// The size last asked for, in physical pixels.
    pub fn asked(&self) -> (u32, u32) {
        self.asked
    }

    /// The staging-allocation counter, straight from the channel. **Always 1.**
    ///
    /// 🚨 T0's condition, exposed so the console can *assert* it rather than intend it: a fresh
    /// staging buffer per frame measures 2.72 ms at 1440p against 1.37 ms reused, and 3.3× of the
    /// gap is `memcpy` of identical bytes into a destination that has never been written.
    pub fn staging_allocations(&self) -> u64 {
        self.channel.staging_allocations()
    }

    /// How many frames were thrown away because a slot changed under the copy. Zero against a
    /// producer that honours the reader's hold.
    pub fn torn_reads(&self) -> u64 {
        self.channel.torn_reads()
    }

    /// How many slot headers could not be believed. Non-zero means the producer is writing
    /// outside the protocol.
    pub fn corrupt_reads(&self) -> u64 {
        self.channel.corrupt_reads()
    }

    /// Ask for a size, in physical pixels. Returns whether the request changed.
    ///
    /// ⚠️ **Level-triggered, and the early return is what makes it so.** The console measures its
    /// region every frame and would otherwise write a control block sixty times a second for a
    /// value that has not moved; the contract's own note says an oscillating console must not be
    /// able to enqueue a queue of reallocations.
    pub fn ask_size(&mut self, w: u32, h: u32) -> bool {
        if (w, h) == self.asked {
            return false;
        }
        self.asked = (w, h);
        self.channel.ask_size(w, h)
    }

    /// §5.1's two states. The console is the only party that can move this.
    pub fn set_lifecycle(&mut self, lifecycle: Lifecycle) {
        self.lifecycle = lifecycle;
        self.channel.set_lifecycle(lifecycle);
    }

    /// Bump the console's own liveness counter. Once per console frame.
    pub fn heartbeat(&mut self) {
        self.channel.heartbeat();
    }

    /// One input event. `PushFault::Refused` means the protocol does not carry it.
    pub fn send(&mut self, ev: InputEvent) -> Result<(), PushFault> {
        self.channel.send(ev)
    }

    /// 🚨 **Ask the process handle before the clock.**
    ///
    /// `try_wait` is non-blocking and answers *"has this process ended"* with an authority the
    /// mapping does not have. Once it says yes the answer is remembered — a reaped child cannot
    /// be waited on twice, and the verdict must not oscillate between *dead* and *silent*.
    ///
    /// `None` while it is running, or when the handle could not be asked — in which case the
    /// counters remain the only evidence, which is the pre-launcher situation rather than a new
    /// failure.
    pub fn exited(&mut self) -> Option<ExitStatus> {
        if let Some(status) = self.exited {
            return Some(status);
        }
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                self.exited = Some(status);
                self.child = None;
                Some(status)
            }
            Ok(None) => None,
            // ⚠️ Not treated as death. `try_wait` failing is the operating system declining to
            // answer, and reporting "exited" for it would be inventing a fact; the liveness
            // counter still works and reaches the same conclusion if the process really is gone.
            Err(_) => None,
        }
    }

    /// Take the newest complete frame, or find out why there is not one.
    ///
    /// ⚠️ **Borrows `&mut self` for the whole of the returned `Poll`**, which is the contract's
    /// shape rather than this file's: the frame's pixels are a view into the mapping. Call
    /// [`Hosted::exited`] before this, never during it.
    pub fn poll(&mut self, now: Instant) -> Poll<'_> {
        self.channel.poll(now)
    }

    /// The verdict, from a poll already taken and an exit already checked.
    ///
    /// 📌 **An associated function rather than a method, and the borrow is why**: the caller needs
    /// the frame *and* the sentence out of one poll, and a method taking `&self` alongside a live
    /// `Poll` would be two borrows of one mapping. Taking the two facts as values is what lets the
    /// ordering — handle first, clock second — be a thing a test states rather than a thing the
    /// borrow checker happened to allow.
    pub fn verdict(producer: &str, exited: Option<ExitStatus>, presence: Presence) -> Vacancy {
        match exited {
            Some(status) => {
                Vacancy::Exited { producer: producer.to_string(), code: status.code() }
            }
            None => Vacancy::Live { producer: producer.to_string(), presence },
        }
    }

    /// Ask the module to exit, and stop believing anything it says afterwards.
    ///
    /// ⚠️ **A request, and this console does not escalate it.** `request_close` sets a word the
    /// producer reads; whether it obeys is its own business. The kill would be this launcher's to
    /// perform and is deliberately not performed: a module part-way through writing a file is not
    /// something a console should terminate over a layout change. What *is* guaranteed is that
    /// nothing it draws afterwards reaches the screen, because the channel goes with the
    /// [`Hosted`].
    pub fn request_close(&mut self) {
        self.channel.request_close();
    }
}

impl Drop for Hosted {
    /// The channel file goes when the host does.
    ///
    /// ⚠️ **Best-effort and unconditional.** A leftover mapping in `$TMPDIR` is not a correctness
    /// problem — the next launch remakes it — but a console that accumulates one per region change
    /// over a long session is a console that fills a disk quietly. On Windows the unlink fails
    /// while the producer still holds the mapping, which is exactly the case where leaving it is
    /// right.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{ApprovedModule, BuildRecord, Granted, ModuleSource};
    use organon_module::RefusalReason;
    use std::collections::BTreeMap as Map;
    use std::time::Duration;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn registry_with(built: bool) -> crate::module::ModuleRegistry {
        let mut reg = crate::module::ModuleRegistry::default();
        let mut m = ApprovedModule {
            producer: "ascent".into(),
            name: "Ascent".into(),
            url: "https://example.invalid/ascent".into(),
            commit: HASH.into(),
            reference: None,
            granted: Granted::none(),
            built: None,
            extra: Map::new(),
        };
        if built {
            m.built = Some(BuildRecord {
                commit: HASH.into(),
                dirty: false,
                extra: Map::new(),
            });
        }
        reg.upsert(m).unwrap();
        reg
    }

    fn store() -> PathBuf {
        // Never touched — every test here answers `exists` itself.
        PathBuf::from(if cfg!(windows) { "C:\\store" } else { "/store" })
    }

    // -----------------------------------------------------------------------------------
    // The path convention
    // -----------------------------------------------------------------------------------

    /// 🚨 **CONTRACT: the channel is under `$TMPDIR`, is one file name, and is namespaced.**
    ///
    /// The producer never composes this — it is handed the whole path (see the module header) —
    /// so what this pins is that the console's half is `ns_file`'s answer rather than a second
    /// spelling of it. A test that rebuilt the string by hand would pass against any composition;
    /// this asks the two functions that own the two halves.
    #[test]
    fn a_channel_lives_beside_every_other_organon_mapping() {
        let p = channel_path("ascent", 0).unwrap();
        assert_eq!(p.parent().unwrap(), std::env::temp_dir());
        let name = p.file_name().unwrap().to_str().unwrap();
        assert!(
            name.ends_with(&channel_file_name("ascent", 0)),
            "{name} does not end with the contract's own file name"
        );
        assert!(
            name.starts_with(organon_core::ipc::namespace()),
            "{name} is not in this process's IPC namespace"
        );
    }

    /// ⚠️ **Mutation-tested.** Dropping `check_producer_name` from [`channel_path`] — the whole
    /// body then being one `Ok(ns_file(…))` — makes this fail at *"`..` must not become part of a
    /// mapping's file name"*, with the path it would have produced named in the message.
    #[test]
    fn a_producer_name_cannot_walk_out_of_tmpdir() {
        for bad in ["..", ".", "../evil", "a/b", "a\\b", "a:b", "", "organon"] {
            let got = channel_path(bad, 0);
            assert!(
                got.is_err(),
                "`{bad}` must not become part of a mapping's file name — it composed {:?}",
                got.ok()
            );
        }
        // And the separator really would have escaped, which is what makes the gate load-bearing
        // rather than tidy.
        assert!(channel_file_name("../evil", 0).contains('/'));
    }

    /// Two viewports holding one producer are two channels. `Producer::only_one_because` answers
    /// `None` for a hosted module — *"a separate process rendering into its own texture shares no
    /// `frame_index` and no jitter phase"* — so this is reachable rather than theoretical.
    #[test]
    fn two_instances_of_one_producer_are_two_files() {
        assert_ne!(channel_path("ascent", 0).unwrap(), channel_path("ascent", 1).unwrap());
    }

    // -----------------------------------------------------------------------------------
    // The launch plan
    // -----------------------------------------------------------------------------------

    /// 🚨 **CONTRACT: the child is told the channel by its FULL PATH, in [`CHANNEL_ENV`].**
    ///
    /// This is the one agreement a second repository is written against, so it is asserted as a
    /// table rather than inspected: the variable's spelling, that its value is the same path the
    /// console created, and that the namespace travels **beside** it rather than instead of it.
    ///
    /// ⚠️ **Mutation-tested.** Handing over the namespace alone — dropping the `CHANNEL_ENV`
    /// insert — fails at the first assertion; setting `CHANNEL_ENV` to the file *name* rather
    /// than the path fails at *"the child must be handed an absolute path"*.
    #[test]
    fn the_child_is_handed_the_path_and_the_namespace_beside_it() {
        let reg = registry_with(true);
        let plan =
            plan_launch(&store(), "ascent", 0, "organic-math", &reg, &|_| true).expect("plan");

        assert_eq!(plan.env.get(CHANNEL_ENV).map(String::as_str), Some(&*plan.channel.display().to_string()));
        assert_eq!(plan.env.get(NAMESPACE_ENV).map(String::as_str), Some("organic-math"));
        assert_eq!(plan.env.len(), 2, "nothing else is injected");
        assert!(
            Path::new(plan.env.get(CHANNEL_ENV).unwrap()).is_absolute(),
            "the child must be handed an absolute path, never a name to compose"
        );
        assert_eq!(plan.channel, channel_path("ascent", 0).unwrap());
        assert_eq!(CHANNEL_ENV, "ORGANON_MODULE_CHANNEL");
        assert_eq!(NAMESPACE_ENV, "ORGANON_IPC_NS");
    }

    /// 🚨 **CONTRACT: the program is derived from the producer name, and no file names it.**
    ///
    /// `module_work::Tool` exists so no string out of a manifest can become the first argument to
    /// `Command::new`; a binary name read from a manifest would hand that back. This pins the
    /// derivation and the platform suffix together, because getting the second wrong is a module
    /// that launches on one operating system and reports *not built* on the other.
    #[test]
    fn the_binary_is_the_producer_name_in_the_derived_artifact_dir() {
        let arts = artifact_dir(&store(), "ascent").unwrap();
        let bin = module_binary(&arts, "ascent");
        assert_eq!(bin.parent().unwrap(), arts);
        assert_eq!(bin.file_stem().unwrap(), "ascent");
        assert_eq!(
            bin.extension().map(|e| e.to_str().unwrap()),
            if cfg!(windows) { Some("exe") } else { None },
        );
        assert!(arts.ends_with(Path::new("target").join("release")));
    }

    /// 🚨 **CONTRACT: nothing launches for a producer that is not approved AND built** — and the
    /// refusal is `module.rs`'s own sentence, not a second one written here.
    ///
    /// ⚠️ **Mutation-tested.** Removing the `registry.vacancy` gate makes both halves fail at
    /// *"an unapproved producer must not reach a spawn"*: the plan comes back `Ok`, naming a
    /// program under a checkout nobody approved.
    #[test]
    fn an_unapproved_or_unbuilt_producer_never_reaches_a_program() {
        let empty = crate::module::ModuleRegistry::default();
        match plan_launch(&store(), "ascent", 0, "ns", &empty, &|_| true) {
            Err(LaunchFault::Vacant(ModuleState::NotApproved { producer })) => {
                assert_eq!(producer, "ascent")
            }
            other => panic!("an unapproved producer must not reach a spawn — got {other:?}"),
        }

        let approved = registry_with(false);
        match plan_launch(&store(), "ascent", 0, "ns", &approved, &|_| true) {
            Err(LaunchFault::Vacant(ModuleState::ApprovedNotBuilt { producer, .. })) => {
                assert_eq!(producer, "ascent")
            }
            other => panic!("an unbuilt producer must not reach a spawn — got {other:?}"),
        }
    }

    /// A build that produced no binary of that name is named, with the directory that was looked
    /// in — the commonest real failure and the one a default message would explain worst.
    #[test]
    fn a_built_module_with_no_binary_says_where_it_looked() {
        let reg = registry_with(true);
        match plan_launch(&store(), "ascent", 0, "ns", &reg, &|_| false) {
            Err(LaunchFault::NoBinary { producer, looked_at }) => {
                assert_eq!(producer, "ascent");
                assert_eq!(looked_at, artifact_dir(&store(), "ascent").unwrap());
                let s = LaunchFault::NoBinary { producer, looked_at }.sentence();
                assert!(s.contains("ascent"), "{s}");
                assert!(s.contains(BUILD_VERB), "the sentence must name the verb: {s}");
            }
            other => panic!("expected NoBinary, got {other:?}"),
        }
    }

    /// The working directory is the module's own checkout, so a module reading a file beside
    /// itself finds the tree it was built from — never wherever the console happened to start.
    #[test]
    fn a_module_runs_in_its_own_checkout() {
        let reg = registry_with(true);
        let plan = plan_launch(&store(), "ascent", 0, "ns", &reg, &|_| true).unwrap();
        assert_eq!(plan.cwd, crate::module_work::checkout_dir(&store(), "ascent").unwrap());
        assert!(plan.program.starts_with(&plan.cwd));
    }

    // -----------------------------------------------------------------------------------
    // Capacity
    // -----------------------------------------------------------------------------------

    #[test]
    fn capacity_falls_back_loudly_and_never_refuses_to_host() {
        let (default, note) = capacity_from(None);
        assert_eq!((default.max_width, default.max_height), DEFAULT_CAPACITY);
        assert!(note.is_none());

        let (set, note) = capacity_from(Some("1920x1080"));
        assert_eq!((set.max_width, set.max_height), (1920, 1080));
        assert!(note.is_none());
        assert_eq!(capacity_from(Some(" 1920 X 1080 ")).0, set, "spacing and case are the same ask");

        for bad in ["", "  ", "wide", "1920", "1920x", "0x1080", "1920x0", "-1x-1"] {
            let (fallback, note) = capacity_from(Some(bad));
            assert_eq!(
                (fallback.max_width, fallback.max_height),
                DEFAULT_CAPACITY,
                "`{bad}` must fall back rather than refuse"
            );
            // The empty cases are "unset", which is not a mistake and says nothing.
            if !bad.trim().is_empty() {
                let note = note.unwrap_or_else(|| panic!("`{bad}` fell back silently"));
                assert!(note.contains(CAPACITY_ENV), "{note}");
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // The four sentences
    // -----------------------------------------------------------------------------------

    fn every_vacancy() -> Vec<Vacancy> {
        let s = Duration::from_secs(3);
        vec![
            Vacancy::Idle(ModuleState::NotApproved { producer: "ascent".into() }),
            Vacancy::Idle(ModuleState::ApprovedNotBuilt {
                producer: "ascent".into(),
                commit: "0123456".into(),
            }),
            Vacancy::Live {
                producer: "ascent".into(),
                presence: Presence::Starting { elapsed: s },
            },
            Vacancy::Live { producer: "ascent".into(), presence: Presence::Live },
            Vacancy::Live { producer: "ascent".into(), presence: Presence::Stalled { silent: s } },
            Vacancy::Live {
                producer: "ascent".into(),
                presence: Presence::NotProducing { silent: s, reason: None },
            },
            Vacancy::Live {
                producer: "ascent".into(),
                presence: Presence::NotProducing {
                    silent: s,
                    reason: Some(RefusalReason::SizeUnsupported),
                },
            },
            Vacancy::Live { producer: "ascent".into(), presence: Presence::Lost { silent: s } },
            Vacancy::Live { producer: "ascent".into(), presence: Presence::Gone },
            Vacancy::Exited { producer: "ascent".into(), code: Some(101) },
            Vacancy::Exited { producer: "ascent".into(), code: None },
            Vacancy::Failed(LaunchFault::NoProcess {
                producer: "ascent".into(),
                program: PathBuf::from("ascent"),
                why: "permission denied".into(),
            }),
        ]
    }

    /// 🚨 **CONTRACT: every state has a sentence, it names the module, and it is one line.**
    ///
    /// §4.6: *"the console owns every one of the module's failure modes as a sentence in the
    /// rectangle"*, with *"its own line rather than a shared 'something went wrong'"*. So: no
    /// empty answer, the producer named in every one, and no newline — `paint_module_notice`
    /// clips to the region, and a sentence that wrapped itself would spill past a 320-point
    /// column.
    #[test]
    fn every_state_says_which_module_and_says_it_in_one_line() {
        let mut seen: Vec<String> = Vec::new();
        for v in every_vacancy() {
            let s = v.sentence();
            assert!(!s.is_empty(), "{v:?} has no sentence");
            assert!(s.contains("ascent"), "{v:?} does not name the module: {s}");
            assert!(!s.contains('\n'), "{v:?} is more than one line: {s}");
            seen.push(s);
        }
        // 📌 **Distinct, which is the half §4.6 is actually about.** Twelve states that all read
        // "ascent has a problem" would satisfy every assertion above and none of the requirement.
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "two states share a sentence: {seen:#?}");
    }

    /// 🚨 **CONTRACT: a state that forbids a picture names the way back.**
    ///
    /// A rectangle that says a module is broken and does not say what to type is a rectangle that
    /// tells you to go and read the source.
    #[test]
    fn every_broken_state_names_a_verb_a_person_can_type() {
        for v in every_vacancy() {
            if v.picture_may_be_shown() {
                continue;
            }
            let s = v.sentence();
            let names_a_verb = s.contains(RESTART_VERB)
                || s.contains(BUILD_VERB)
                || s.contains(crate::module::APPROVE_VERB);
            assert!(names_a_verb, "{v:?} says what is wrong and not what to do: {s}");
        }
    }

    /// 🚨 **CONTRACT: the picture is forbidden in exactly the states §4.6 forbids it in**, and
    /// this side agrees with the contract crate's side.
    ///
    /// ⚠️ **The two answers live in two crates and one is behind a feature flag**, so agreement
    /// cannot be a compile error. `FrameTexture::view()` returning `None` is the lock that
    /// actually holds; this asserts the local answer has not drifted from it.
    ///
    /// ⚠️ **Mutation-tested.** Adding `Vacancy::Exited` to the permitted set fails at *"a module
    /// that exited must never show its last frame"* — the single worst thing this design can
    /// paint, and the easiest to paint by accident because the texture is still valid.
    #[test]
    fn a_picture_is_permitted_only_while_starting_or_live() {
        for v in every_vacancy() {
            let expected = matches!(
                &v,
                Vacancy::Live { presence: Presence::Starting { .. } | Presence::Live, .. }
            );
            assert_eq!(v.picture_may_be_shown(), expected, "{v:?}");
            if let Vacancy::Live { presence, .. } = &v {
                assert_eq!(
                    v.picture_may_be_shown(),
                    presence.picture_may_be_shown(),
                    "this crate and the contract disagree about {presence:?}"
                );
            }
        }
        assert!(
            !Vacancy::Exited { producer: "ascent".into(), code: Some(1) }.picture_may_be_shown(),
            "a module that exited must never show its last frame"
        );
    }

    /// 🚨 **CONTRACT: the process handle outranks the clock.**
    ///
    /// `presence.rs` says the mapping cannot tell *hung* from *exited without a farewell*, and
    /// that the process handle is what genuinely knows. So a host that has reaped its child
    /// reports [`Vacancy::Exited`] **whatever** the counters say — including while they still
    /// read `Live`, which is precisely the window a crashed producer is in for the first second.
    ///
    /// ⚠️ **Mutation-tested.** Reversing the arms of [`Hosted::verdict`] — the presence winning —
    /// fails here at *"a reaped process is dead however healthy its last heartbeat looked"*.
    #[test]
    fn a_reaped_process_outranks_every_counter() {
        // A real exit status, taken from a real process, because `ExitStatus` cannot be built.
        let status = Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .args(if cfg!(windows) { ["/C", "exit 7"] } else { ["-c", "exit 7"] })
            .status()
            .expect("a shell");
        assert_eq!(status.code(), Some(7));

        for presence in [
            Presence::Live,
            Presence::Starting { elapsed: Duration::ZERO },
            Presence::Stalled { silent: Duration::from_secs(1) },
        ] {
            let v = Hosted::verdict("ascent", Some(status), presence);
            assert_eq!(
                v,
                Vacancy::Exited { producer: "ascent".into(), code: Some(7) },
                "a reaped process is dead however healthy its last heartbeat looked"
            );
            assert!(!v.picture_may_be_shown());
            assert!(v.sentence().contains("code 7"), "{}", v.sentence());
        }

        // And with no exit, the channel's verdict is the answer unchanged.
        assert_eq!(
            Hosted::verdict("ascent", None, Presence::Live),
            Vacancy::Live { producer: "ascent".into(), presence: Presence::Live }
        );
    }

    // -----------------------------------------------------------------------------------
    // A real channel, a real process, and the allocation counter
    // -----------------------------------------------------------------------------------

    // -----------------------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------------------

    /// 🚨 **CONTRACT: an unlatched module is told about motion and mouse buttons, and about
    /// nothing else.**
    ///
    /// §5.3's *"before the click it is a picture"*. There is no key here to assert the absence of,
    /// which is exactly the point: the protocol carries keys and this function never builds one,
    /// so the restraint lives in the one place a latch would later have to change.
    #[test]
    fn a_picture_is_told_about_the_pointer_and_nothing_else() {
        use organon_module::{Button, MouseButton};
        let mut f = PointerFrame {
            over: true,
            was_over: true,
            focused: true,
            was_focused: true,
            ..Default::default()
        };
        assert_eq!(pointer_events(&f), vec![], "a still pointer says nothing");

        f.dx = 3.0;
        f.dy = -2.0;
        f.pressed[0] = true;
        assert_eq!(
            pointer_events(&f),
            vec![
                InputEvent::Pointer { dx: 3.0, dy: -2.0 },
                InputEvent::Down(Button::Mouse(MouseButton::Left)),
            ],
            "motion comes before the press that happened at the end of it"
        );

        // Every event this builds is one the protocol will actually carry, and none is a key.
        for ev in pointer_events(&f) {
            assert!(ev.is_deliverable(), "{ev:?} would be refused at the encode site");
            assert!(
                !matches!(ev, InputEvent::Down(Button::Key(_)) | InputEvent::Up(Button::Key(_))),
                "a picture must claim no key: {ev:?}"
            );
        }
    }

    /// 🚨 **CONTRACT: a button held out of the rectangle, or out of focus, is released.**
    ///
    /// ⚠️ **Mutation-tested.** Dropping either `ReleaseAll` — the `was_over` arm or the focus edge
    /// — fails here. The failure it prevents is a module holding a mouse button for ever, which
    /// presents as a wedged viewport: §4.6's fourth row, reached from the one direction that is
    /// the console's fault rather than the module's.
    #[test]
    fn leaving_or_losing_focus_releases_everything_once() {
        let held = PointerFrame {
            over: true,
            was_over: true,
            focused: true,
            was_focused: true,
            ..Default::default()
        };

        let left = PointerFrame { over: false, ..held };
        assert_eq!(pointer_events(&left), vec![InputEvent::ReleaseAll]);
        // ...and only once. The next frame it is simply outside.
        let still_out = PointerFrame { over: false, was_over: false, ..held };
        assert_eq!(pointer_events(&still_out), vec![]);

        let blurred = PointerFrame { focused: false, ..held };
        assert_eq!(pointer_events(&blurred), vec![InputEvent::ReleaseAll]);
        let still_blurred = PointerFrame { focused: false, was_focused: false, ..held };
        assert_eq!(pointer_events(&still_blurred), vec![], "a background console sends nothing");

        // 🚨 Focus outranks the rectangle: losing focus while already outside still releases,
        // because the button that is held was pressed while inside.
        let blurred_outside = PointerFrame { focused: false, over: false, was_over: false, ..held };
        assert_eq!(pointer_events(&blurred_outside), vec![InputEvent::ReleaseAll]);
    }

    /// 🚨 **T0's condition, asserted rather than intended: the staging buffer is allocated ONCE,
    /// and stays once across a resize and across every poll.**
    ///
    /// `doc/measurements/module-frame-boundary-2026-08-21.md`: a fresh staging buffer plus a fresh
    /// destination per iteration costs **2.72 ms at 1440p against 1.37 ms reused**, and 3.3× of
    /// the gap is `memcpy` of *identical bytes* — first-touch page faulting on a destination
    /// nothing has written. A per-frame allocation therefore measures 2.7 ms, reads as a verdict
    /// on the whole mechanism, and buys `unsafe` interop to fix an allocator problem.
    ///
    /// ⚠️ **The resize is the half worth having**, and it is the half a naive implementation gets
    /// wrong: `ask_size` is the one call that could plausibly want to re-buffer, and the contract
    /// sizes the staging buffer at **capacity** precisely so it does not.
    ///
    /// This drives a real `ModuleChannel` with no producer at all — which is exactly the *launched,
    /// not yet producing* row, so it also proves that row is reachable and times out.
    #[test]
    fn the_staging_buffer_is_allocated_once_and_a_resize_does_not_move_it() {
        let path = std::env::temp_dir().join(format!(
            "organon-console-host-alloc-{}-{:?}.frames",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        let cap = FrameCapacity::new(320, 240, WIRE_FORMAT);
        let mut ch = ModuleChannel::create(&path, cap).expect("channel");
        assert_eq!(ch.staging_allocations(), 1);

        let start = Instant::now();
        for step in 0..600u32 {
            // A size that genuinely changes most steps, and returns to earlier values — the
            // oscillating console `ask_size`'s doc warns about.
            let w = 64 + (step % 7) * 32;
            let h = 64 + (step % 5) * 24;
            ch.ask_size(w, h);
            ch.heartbeat();
            let _ = ch.poll(start + std::time::Duration::from_millis(step as u64));
        }
        assert_eq!(
            ch.staging_allocations(),
            1,
            "600 polls and ~600 size changes allocated more than once"
        );
        assert!(ch.size_epoch() > 1, "the sizes really did change");
        assert_eq!(ch.torn_reads(), 0);
        assert_eq!(ch.corrupt_reads(), 0);
        drop(ch);
        let _ = std::fs::remove_file(&path);
    }

    /// 🚨 **§4.6 row 3 is reachable AND it times out — the state that "looks most like working
    /// software" if it does not.**
    ///
    /// A channel with no producer behind it is a module that was launched and has not spoken. The
    /// contract's default is ten seconds; this drives the clock rather than sleeping on it.
    ///
    /// ⚠️ **Mutation-tested.** Making `Presence::Starting` terminal — never reaching `Lost` —
    /// fails at *"starting must not sit for ever"*, which is the whole of what row 3 asks for.
    #[test]
    fn launched_and_never_producing_becomes_lost_rather_than_starting_for_ever() {
        let path = std::env::temp_dir()
            .join(format!("organon-console-host-start-{}.frames", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut ch = ModuleChannel::create(&path, FrameCapacity::new(64, 64, WIRE_FORMAT)).unwrap();
        let t0 = Instant::now();

        let early = Hosted::verdict("ascent", None, ch.poll(t0).presence());
        assert!(
            matches!(early, Vacancy::Live { presence: Presence::Starting { .. }, .. }),
            "a launched module with nothing behind it is starting — got {early:?}"
        );
        assert!(early.picture_may_be_shown(), "starting keeps whatever it had, which is nothing");
        assert!(early.sentence().contains("starting"), "{}", early.sentence());

        let late = Hosted::verdict(
            "ascent",
            None,
            ch.poll(t0 + Duration::from_secs(30)).presence(),
        );
        assert!(
            !late.picture_may_be_shown(),
            "starting must not sit for ever — after 30 s it is still {late:?}"
        );
        assert!(late.sentence().contains(RESTART_VERB), "{}", late.sentence());
        drop(ch);
        let _ = std::fs::remove_file(&path);
    }
}
