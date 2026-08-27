//! **One launched module, from the console's side** — T5, and the tier where a rectangle stops
//! being a sentence about a module and becomes a picture drawn by one.
//!
//! `doc/organon_module_viewport.md` §5 is the lifecycle and §4.6 is the vacancy rule. The three
//! files this sits between:
//!
//! | | Answers | |
//! |---|---|---|
//! | [`crate::module`] (T3a) | *may it run?* | `modules.json`, grants, the commit |
//! | [`crate::module_work`] (T3b) | *are there bytes to run?* | `git`, `cargo`, and the launch seam |
//! | **this file** (T5) | *is it running, and what is on the screen?* | a process, a channel, a verdict |
//! | `organon_module` | *how do the two talk?* | the ring, the input, the presence policy |
//!
//! # 🚨 Two things this file must never do
//!
//! **It must never learn what the module is.** No `ascent`, no game, no content. A producer
//! name, a rectangle, a size, a texture and a channel — §4.6's *never* list, and every type in
//! this file is one of those five things.
//!
//! **It must never paint the last good frame.** That is not enforced here; it is enforced
//! *upstream* of here, and this file is written so it cannot be re-decided. `ModuleChannel::poll`
//! makes [`organon_module::Poll::Frame`] unreachable once the producer is judged stalled, lost or
//! gone, and `FrameTexture::apply` takes a whole `Poll` — never a frame — so a caller that
//! forgets gets its picture dropped anyway. What this file adds is the one verdict neither of
//! them can reach: the **process handle**.
//!
//! # The verdict the channel cannot give, and why it outranks the channel's
//!
//! `organon_module`'s `presence.rs` writes down its own honest limit: *"hung and
//! exited-without-a-farewell are not distinguishable from inside a shared mapping, and the thing
//! that genuinely knows is the process handle, which is T3b's launcher's."* That handle is
//! [`ModuleProcess`], and [`ModuleHost::observe`] asks it **first**.
//!
//! 📌 The ordering is not a preference. A producer that dies without a farewell leaves a mapping
//! whose counters simply stop, so the channel reaches `Stalled` after a second and `Lost` after
//! five — both of which say *"it stopped responding"* and invite a restart, which is right but
//! late and vague. The process handle knows within one frame and knows the exit code. Reporting
//! the channel's guess while holding a certainty would be choosing the weaker of two answers.
//!
//! ⚠️ **The reverse also holds and is the reason [`Notice::Exited`] is not simply
//! `Presence::Gone`.** A process that is *alive* tells you nothing about whether it is drawing —
//! that is exactly the failure `Presence::NotProducing` exists for — so a host that trusted the
//! handle alone would call a wedged producer healthy. Both are needed, in this order.
//!
//! # What is deliberately NOT here
//!
//! **Interaction.** §5.3's click latch, `Lifecycle::Running`, the way out — none of it. A module
//! arrives [`Lifecycle::Attached`] and this file has no method that changes it, which is
//! `CLAUDE.md` invariant 4 made structural rather than remembered: there is nothing to call.
//! `ModuleChannel::set_lifecycle` exists one crate down and no line here reaches it.
//!
//! **A claim on `engine_plan`.** A hosted producer renders no `World`, touches no `frame_index`
//! and shares no TAA jitter phase, so `the_engine_is_asked_for_at_most_one_frame` does not widen
//! and `region_showing_world` goes on asking for `Content::ThreeD(Producer::Organon)` precisely.
//! Nothing in this file is an input to that arbiter.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use organon_module::{
    channel_file_name, FrameCapacity, Lifecycle, ModuleChannel, PixelFormat, Poll, Presence,
    CHANNEL_ENV,
};

use crate::module::{ApprovedModule, ModuleState, RESTART_VERB};
use crate::module_work::{module_binary, ModuleProcess, WorkFault, Workshop};

/// The largest frame a hosted producer's channel can carry, and therefore the mapping's size.
///
/// 🚨 **The capacity is fixed once, at creation, and a resize does not change it** — that is the
/// contract's first sentence about who owns the size, and the whole reason a size change is not
/// an allocation on the frame path. So this number is chosen for the *largest rectangle a region
/// could ever be*, not for the one it is.
///
/// 2560×1440: three slots of `2560 × 1440 × 4` is about 44 MiB of *sparse* file-backed mapping —
/// pages are committed as they are written, so a module drawing into a 640×360 region pays for
/// 640×360. ⚠️ A console asking for more than this is **clamped, never refused**
/// (`ModuleChannel::ask_size`): the right answer to a region that grew past the plan is a
/// slightly small picture scaled up, not a black rectangle.
pub const MAX_FRAME: (u32, u32) = (2560, 1440);

/// The pixel format the channel declares.
///
/// ⚠️ **Matched to what the console samples**, and it is not a free choice: the format is in the
/// header, a producer builds its pipeline against it, and a mismatch is
/// `RefusalReason::FormatUnsupported` — a refusal a restart cannot fix. `Rgba8UnormSrgb` is what
/// `upload_exhibit` and `make_surface_texture` already use, so a module's picture and a
/// photograph and the engine's own render are linearized the same number of times.
pub const FRAME_FORMAT: PixelFormat = PixelFormat::Rgba8UnormSrgb;

// ---------------------------------------------------------------------------------------
// What the rectangle says — the four rows, from the three parties that can answer
// ---------------------------------------------------------------------------------------

/// **Why this rectangle is a sentence rather than a picture.** §4.6's four rows, and every one of
/// them comes from whichever party actually knows.
///
/// 🚨 **There is no `sentence()` written in this file**, deliberately: each arm delegates to the
/// author of its own fact. The registry's two rows are `ModuleState`'s words, the producer's are
/// `organon_module::Presence`'s, and only the two the *launcher* alone can reach are phrased
/// here. A `Notice` that reworded any of them would be a second copy of a sentence, which is what
/// `module.rs`'s verb constants exist to make impossible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Rows 1 and 2 — nothing is running and nothing could be. See [`ModuleState`].
    Registry(ModuleState),
    /// Rows 3 and 4 — a producer is attached and the channel has a verdict about it.
    Producer { producer: String, presence: Presence },
    /// 🚨 **Row 4, said with certainty instead of after a timeout.** The process ended and this
    /// console watched it happen, so there is no silence to interpret.
    ///
    /// ⚠️ **Not folded into `Presence::Gone`.** That means *"the producer said goodbye"* — a
    /// courteous exit, whose sentence invites starting it again. This is *"it ended"*, which
    /// covers a panic, a `std::process::exit`, and a kill from outside; the exit code is the only
    /// thing that separates them and it is the one thing a person can act on.
    Exited { producer: String, status: Option<i32> },
    /// The launch itself did not happen. Not one of §4.6's four rows because §4.6 was written
    /// before anything could launch — it is *approved, built, and the binary is not there*, which
    /// `is_built()` cannot see because it reads the record and not the disk.
    Launch { producer: String, why: String },
}

impl Notice {
    /// The sentence the rectangle carries.
    pub fn sentence(&self) -> String {
        match self {
            Notice::Registry(state) => state.sentence(),
            Notice::Producer { producer, presence } => presence.sentence(producer, RESTART_VERB),
            Notice::Exited { producer, status: Some(code) } => format!(
                "{producer} ended with exit code {code}. `{RESTART_VERB}` to start it again"
            ),
            Notice::Exited { producer, status: None } => format!(
                "{producer} was ended from outside. `{RESTART_VERB}` to start it again"
            ),
            Notice::Launch { producer, why } => {
                format!("{producer} would not start — {why}")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// One host
// ---------------------------------------------------------------------------------------

/// **One producer, launched and watched.**
///
/// Created by [`ModuleHost::launch`], driven once per console frame by [`ModuleHost::observe`],
/// and ended by dropping it — see [`Drop`], which is where the process is not leaked.
pub struct ModuleHost {
    producer: String,
    /// Where the channel file is, kept so [`Drop`] can take it off the disk.
    channel_path: PathBuf,
    channel: ModuleChannel,
    child: Box<dyn ModuleProcess>,
    /// 🚨 **Remembered rather than re-asked.** `ModuleProcess::exited` is already sticky for its
    /// own reason (`try_wait` reports once); this is the second half — once the console has said
    /// *"it ended"* it must go on saying so, because a rectangle that reverted to *"is running"*
    /// after reporting a death is the stale picture arriving through the sentence instead of
    /// through the texture.
    exited: Option<Option<i32>>,
    launched_at: Instant,
    /// Only for the diagnostic line. The console's judgement about the producer comes from the
    /// channel, never from this.
    frames_seen: u64,
}

/// ⚠️ **Hand-written, because a derive is impossible and a missing `Debug` is a real cost.**
/// Neither `ModuleChannel` nor `Box<dyn ModuleProcess>` is `Debug`, so the derive cannot exist —
/// and without one, `Result<ModuleHost, WorkFault>::unwrap_err()` does not compile, which is
/// exactly the call every refusal test makes. It prints the identity and the counters, never the
/// channel: `FrameView`'s rule, whose derive turned one failed expectation into 14 MiB of bytes.
impl std::fmt::Debug for ModuleHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleHost")
            .field("producer", &self.producer)
            .field("channel", &self.channel_path)
            .field("exited", &self.exited)
            .field("frames_seen", &self.frames_seen)
            .field("uptime_s", &self.launched_at.elapsed().as_secs_f32())
            .finish()
    }
}

impl ModuleHost {
    /// **Create the channel, then launch the binary into it.**
    ///
    /// 🚨 **That order is the whole of the startup handshake and it cannot be the other way
    /// round.** `ModuleChannel::create` writes the header, seeds the two producer-owned words
    /// and lays down the slots; a producer that opened the file first would find a size it
    /// cannot believe and refuse with `WireFault`. `create`'s own docs say it expects the
    /// producer not to exist yet — this is the call site that makes that true.
    ///
    /// ⚠️ **`spawn` and not `run`, and it returns in microseconds.** This is on the frame
    /// thread, which every other verb in `module_work` is forbidden from touching — the
    /// difference is that a clone is a network and a build is a compiler, while this is one
    /// `CreateProcess`. Nothing is waited for and nothing is collected.
    pub fn launch(
        shop: &dyn Workshop,
        store_root: &Path,
        channel_dir: &Path,
        module: &ApprovedModule,
        instance: u32,
        namespace: &str,
    ) -> Result<ModuleHost, WorkFault> {
        let producer = module.producer.clone();
        let binary = module_binary(store_root, &producer)?;
        // ⚠️ Checked before the channel is made rather than after, so a missing binary does not
        // leave a 44 MiB file behind for a process that will never open it.
        if !shop.file_exists(&binary) {
            return Err(WorkFault::LaunchFailed {
                path: binary.display().to_string(),
                why: format!(
                    "there is no such file — `{}` produces it",
                    crate::module::BUILD_VERB
                ),
            });
        }
        let channel_path = channel_dir.join(channel_file_name(&producer, instance));
        let capacity = FrameCapacity::new(MAX_FRAME.0, MAX_FRAME.1, FRAME_FORMAT);
        let channel = ModuleChannel::create(&channel_path, capacity).map_err(|e| {
            WorkFault::ChannelFailed { producer: producer.clone(), why: e.to_string() }
        })?;

        // The working directory is the module's own checkout. A module that reads a file beside
        // its binary — a shader, a level, a font — is doing the ordinary thing, and inheriting
        // the console's cwd would make that work only when the console happened to be launched
        // from the right place.
        let cwd = crate::module_work::checkout_dir(store_root, &producer)?;
        // 🚨 **The second handoff, and it is the one that makes a typed command reach a running
        // module.** `organon-module`'s input ring carries four verbs and refuses a generic
        // message on purpose, so `console setting` writes a file instead — and a module can only
        // watch a file it has been told the path of. Derived here, like the channel path and the
        // binary, and never read from anything a module wrote.
        //
        // ⚠️ The file need not exist. "Nothing has been set" is the arrival state, and a module
        // that treated an absent file as a fault would refuse to draw before anybody had chosen
        // anything.
        let settings = crate::module::ModuleRegistry::settings_path(store_root, &producer)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let env = [
            (CHANNEL_ENV, channel_path.display().to_string()),
            (crate::module::SETTINGS_ENV, settings.clone()),
            // 📌 The same injection `term.rs` already makes into every child the console spawns,
            // for the same reason: a module process coexisting with an Organon session must not
            // address the default namespace. Following the existing convention rather than
            // inventing a second one is the point.
            ("ORGANON_IPC_NS", namespace.to_string()),
        ];
        let child = shop.spawn(&binary, &cwd, &env)?;
        eprintln!(
            "[module] launched `{producer}` from {} — channel {}, capacity {}x{}",
            binary.display(),
            channel_path.display(),
            MAX_FRAME.0,
            MAX_FRAME.1
        );
        Ok(ModuleHost {
            producer,
            channel_path,
            channel,
            child,
            exited: None,
            launched_at: Instant::now(),
            frames_seen: 0,
        })
    }

    pub fn producer(&self) -> &str {
        &self.producer
    }

    /// How long this host has existed. For the diagnostic line, never for a verdict — the
    /// timeout that turns *starting* into *lost* is `Timings::start_within`, and it belongs to
    /// the channel because it is a policy about a producer rather than about a process.
    pub fn uptime(&self) -> std::time::Duration {
        self.launched_at.elapsed()
    }

    pub fn frames_seen(&self) -> u64 {
        self.frames_seen
    }

    /// Ask the producer to draw at `w` × `h`. Level-triggered — see `ModuleChannel::ask_size`.
    pub fn ask_size(&mut self, w: u32, h: u32) -> bool {
        if self.channel.ask_size(w, h) {
            eprintln!("[module] `{}` asked for {w}x{h}", self.producer);
            true
        } else {
            false
        }
    }

    /// The OS id of the process behind this host, or `None` once it has ended.
    ///
    /// See [`crate::module_work::ModuleProcess::pid`] for what it may and may not be used for.
    pub fn pid(&self) -> Option<u32> {
        self.child.pid()
    }

    /// **Put one input event on this producer's wire.**
    ///
    /// Answers whether it landed. `false` covers both refusals and a full ring, and the caller
    /// wants neither: input is a stream of transitions, so the recovery from losing one is
    /// [`organon_module::InputEvent::ReleaseAll`], which the latch already sends on every exit.
    ///
    /// 🚨 **A reserved key cannot get through here even if the console asks.**
    /// `ModuleChannel::send` is `input::push`, which tests `is_deliverable` before it touches
    /// the ring — so this method is safe to call with anything, and §5.3's promise does not
    /// depend on the call site remembering. `module_input::translate` drops them too; that is
    /// the cheap guard, this is the structural one.
    ///
    /// ⚠️ **Silent on refusal, deliberately.** A module being flown generates events at the
    /// frame rate, so an `eprintln!` per refusal — the pattern [`ModuleHost::ask_size`] uses for
    /// a once-per-resize event — would be a log line per frame per held key. The console's own
    /// counters are where a persistent refusal shows up.
    pub fn send_input(&mut self, ev: organon_module::InputEvent) -> bool {
        self.channel.send(ev).is_ok()
    }

    /// **One console frame.** Bump the console's own liveness counter, ask the process handle,
    /// then ask the channel.
    ///
    /// The [`Observation`] borrows this host for as long as the frame it belongs to — the pixels
    /// live in the channel's staging buffer, which is reused, so a caller that kept one across
    /// frames would be looking at the next frame's bytes. The borrow is what makes that
    /// impossible rather than a rule.
    /// ⚠️ **The borrow checker will push the next person to invert the order below, and the
    /// inverted version is wrong.**
    ///
    /// `ModuleChannel::poll` holds `&mut self.channel` for the whole life of the `Poll` it
    /// returns, because the pixels live in the channel's staging buffer. So the shortest path to
    /// "also read the exit status" is to ask the handle *after* the poll, where nothing is
    /// borrowed and it compiles first try. That reads as the natural arrangement and it silently
    /// reverses the one rule this function exists to enforce: a producer that died leaves a
    /// mapping that looks healthy for a second, so a channel-first `observe` reports `Live` about
    /// a process this console watched exit.
    ///
    /// 📌 The order is therefore not a style preference and not an accident of how it was
    /// written — it is the whole content of the function, and the language is pushing against it.
    /// ✏️ Noticed by a reviewer rather than by me: I wrote the ordering deliberately and
    /// documented *why* it is right, and never saw that the code around it recommends the
    /// opposite.
    pub fn observe(&mut self, now: Instant) -> Observation<'_> {
        self.channel.heartbeat();
        // 🚨 The handle first — see this module's header on why the order is not a preference.
        if self.exited.is_none() {
            if let Some(status) = self.child.exited() {
                self.exited = Some(status);
                eprintln!(
                    "[module] `{}` ended after {:.1} s and {} frame(s) — exit {}",
                    self.producer,
                    self.launched_at.elapsed().as_secs_f32(),
                    self.frames_seen,
                    status.map_or("(from outside)".to_string(), |c| c.to_string())
                );
            }
        }
        if let Some(status) = self.exited {
            return Observation {
                poll: Poll::Gone,
                notice: Some(Notice::Exited { producer: self.producer.clone(), status }),
            };
        }
        let producer = self.producer.clone();
        let poll = self.channel.poll(now);
        if matches!(poll, Poll::Frame(_)) {
            self.frames_seen += 1;
        }
        // ⚠️ `Holding` is NOT a notice: it means the producer is fine and there is nothing new,
        // which is the ordinary case for a paused module the console polls faster than it draws.
        // Turning it into a sentence would replace a good picture with prose sixty times a
        // second.
        let notice = match poll.presence() {
            Presence::Live => None,
            presence => Some(Notice::Producer { producer, presence }),
        };
        Observation { poll, notice }
    }

    /// Ask the producer to exit, politely. The kill is [`Drop`]'s.
    pub fn request_close(&mut self) {
        self.channel.request_close();
    }

    /// How many frames were thrown away because the producer wrote a slot this console was
    /// holding, and how many slot headers could not be believed. Both are **producer bugs** and
    /// both are zero against one that honours the protocol — surfaced so they are diagnosable
    /// rather than presenting as an occasional strange frame.
    pub fn faults(&self) -> (u64, u64) {
        (self.channel.torn_reads(), self.channel.corrupt_reads())
    }

    /// The staging buffer is allocated once, at `create`, and never again. T0's finding is that
    /// a per-frame allocation costs double and reads as a verdict on the mechanism, so the
    /// absence of one is asserted rather than commented.
    pub fn staging_allocations(&self) -> u64 {
        self.channel.staging_allocations()
    }

    /// The lifecycle this host is asking for. **Always [`Lifecycle::Attached`] in this tier** —
    /// there is no method that changes it. See this module's header.
    pub fn lifecycle(&self) -> Lifecycle {
        self.channel.lifecycle()
    }
}

impl Drop for ModuleHost {
    /// 🚨 **Ask, then kill, then take the file off the disk.**
    ///
    /// A console that only ever asked would leak a process every time a module ignored the
    /// request; a console that only ever killed would deny a well-behaved module the chance to
    /// shut down. So both, in that order — and the request costs one store to a mapped word, so
    /// there is nothing to wait for and nothing is waited for. A module that wanted to exit
    /// cleanly and is killed a microsecond later has still lost, which is the honest limit of
    /// doing this in a `Drop`: it is a *courtesy*, and the kill is the mechanism.
    ///
    /// ⚠️ **The channel file is removed.** It is `$TMPDIR`-scoped and namespaced, so leaving one
    /// behind is not a correctness problem — but it is 44 MiB of sparse file per launch, and a
    /// console restarted a dozen times over an afternoon would leave a dozen. A failure to
    /// remove it is ignored: the process may still hold the mapping on Windows, and a console
    /// that refused to close a viewport because a temporary file was busy would be trading a
    /// real capability for a tidy directory.
    fn drop(&mut self) {
        self.channel.request_close();
        self.child.kill();
        let _ = std::fs::remove_file(&self.channel_path);
        eprintln!(
            "[module] released `{}` after {:.1} s and {} frame(s)",
            self.producer,
            self.launched_at.elapsed().as_secs_f32(),
            self.frames_seen
        );
    }
}

/// What one console frame learned about one producer.
///
/// 🚨 **Both halves, always, and they are not alternatives.** `poll` is what the *texture* obeys
/// — `FrameTexture::apply` takes it whole, which is what makes "never the last good frame"
/// unforgettable — and `notice` is what the *rectangle* says when there is no picture. A caller
/// that had to choose between them could choose to keep a texture it should have dropped.
pub struct Observation<'a> {
    pub poll: Poll<'a>,
    /// `None` when the producer is live and drawing. See [`ModuleHost::observe`] on why
    /// `Holding` is not a notice.
    pub notice: Option<Notice>,
}

// ---------------------------------------------------------------------------------------
// Every host the console is holding
// ---------------------------------------------------------------------------------------

/// **The producers this console currently has running**, keyed by name.
///
/// A `BTreeMap` rather than a `HashMap` so the diagnostic ordering is stable — this is a handful
/// of entries and never a hot lookup.
#[derive(Default)]
pub struct ModuleHosts {
    hosts: BTreeMap<String, ModuleHost>,
    /// 🚨 **A launch that failed is remembered, and this is not a cache — it is the thing that
    /// stops the console retrying sixty times a second.** A missing binary would otherwise be a
    /// `CreateProcess` per frame, each writing a line to stderr; the rectangle would say the
    /// right thing while the console spent itself on it. Cleared by
    /// [`ModuleHosts::forget_failure`], which is what a restart is.
    failures: BTreeMap<String, Notice>,
    /// The instance number the next launch of each producer gets, so two channels are never one
    /// file. See `channel_file_name`'s `instance`.
    ///
    /// ⚠️ It counts *launches*, not live hosts: a producer that died and was restarted must not
    /// reuse the file its predecessor may still hold open on Windows.
    next_instance: BTreeMap<String, u32>,
}

impl ModuleHosts {
    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    pub fn get_mut(&mut self, producer: &str) -> Option<&mut ModuleHost> {
        self.hosts.get_mut(producer)
    }

    pub fn producers(&self) -> Vec<&str> {
        self.hosts.keys().map(String::as_str).collect()
    }

    /// **Make sure `producer` is running, or say why it is not.**
    ///
    /// The whole decision, in the order the answers become available:
    ///
    /// 1. A host already exists → nothing to do.
    /// 2. A previous launch failed → the remembered refusal, without trying again.
    /// 3. The registry stands in the way → [`ModuleState`]'s sentence, and nothing is launched.
    /// 4. Otherwise launch it.
    ///
    /// 📌 **Step 3 is `ModuleRegistry::vacancy` and not a second reading of the registry.** Its
    /// `None` is the working case — *"the picture is what the rectangle says"* read the right way
    /// round — so `is_built()` and the two refusal rows are consulted in exactly one place.
    pub fn ensure(
        &mut self,
        shop: &dyn Workshop,
        registry: &crate::module::ModuleRegistry,
        store_root: &Path,
        channel_dir: &Path,
        producer: &str,
        namespace: &str,
    ) -> Option<Notice> {
        if self.hosts.contains_key(producer) {
            return None;
        }
        if let Some(remembered) = self.failures.get(producer) {
            return Some(remembered.clone());
        }
        if let Some(state) = registry.vacancy(producer) {
            return Some(Notice::Registry(state));
        }
        // `vacancy` returning `None` means the registry holds it and it is built, so this is the
        // one arm where `get` cannot be empty — but it is answered rather than unwrapped, because
        // an `unreachable!` in a frame is a crash where a sentence would do.
        let Some(module) = registry.get(producer) else {
            return Some(Notice::Registry(ModuleState::NotApproved {
                producer: producer.to_string(),
            }));
        };
        let instance = self.next_instance.entry(producer.to_string()).or_insert(0);
        let this = *instance;
        *instance += 1;
        match ModuleHost::launch(shop, store_root, channel_dir, module, this, namespace) {
            Ok(host) => {
                self.hosts.insert(producer.to_string(), host);
                None
            }
            Err(fault) => {
                let notice =
                    Notice::Launch { producer: producer.to_string(), why: fault.sentence() };
                eprintln!("organon-console: {}", notice.sentence());
                self.failures.insert(producer.to_string(), notice.clone());
                Some(notice)
            }
        }
    }

    /// Stop hosting every producer not in `wanted`, and say why.
    ///
    /// 🚨 **This is the release site, and it is total by asking about the state rather than by
    /// being called from each verb.** A region cleared, a region retargeted, a layout replaced
    /// and a tab closed are four routes to the same fact — *nothing is asking for this producer*
    /// — and `render_viewport`'s single-release-site argument is the precedent: releasing at each
    /// verb is how one route comes to be missed.
    pub fn retain(&mut self, wanted: &[&str]) {
        let going: Vec<String> = self
            .hosts
            .keys()
            .filter(|k| !wanted.contains(&k.as_str()))
            .cloned()
            .collect();
        for name in going {
            // `Drop` asks, kills, removes the file and logs.
            self.hosts.remove(&name);
        }
        // A remembered failure for a producer nobody is asking about is not worth keeping: the
        // next time a region names it is the next time it deserves a fresh attempt.
        self.failures.retain(|k, _| wanted.contains(&k.as_str()));
    }

    /// **Restart one producer** — `console module restart`.
    ///
    /// Drops the host (which asks, kills and removes the channel) and forgets any remembered
    /// launch failure, so the next [`ModuleHosts::ensure`] launches again. Returns whether
    /// anything was actually being held, so the caller can refuse by name rather than reporting
    /// a restart that did not happen.
    ///
    /// ⚠️ **It does not launch.** Restarting is *stop, and let the next frame start it* — the
    /// frame is already asking `ensure` for every producer a region names, so a launch here would
    /// be a second launch site, and two launch sites is how a producer comes to be started twice
    /// against one channel. `ProducerChannel::open`'s docs name that exact hazard as the one
    /// thing its side of the rule cannot prevent.
    pub fn restart(&mut self, producer: &str) -> bool {
        let had_host = self.hosts.remove(producer).is_some();
        let had_failure = self.failures.remove(producer).is_some();
        had_host || had_failure
    }

    /// Forget a remembered launch refusal, so the next frame tries again. What a successful
    /// `console module build` should do to a producer whose binary was missing.
    pub fn forget_failure(&mut self, producer: &str) {
        self.failures.remove(producer);
    }
}

#[cfg(test)]
#[path = "module_host_tests.rs"]
mod tests;
