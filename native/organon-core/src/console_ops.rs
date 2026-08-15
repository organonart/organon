//! The **console command lane** — the CLI's write path to Organon Console, and the
//! sidecar wire format both ends speak.
//!
//! organon#49 Tier 5a. Lifted out of `cli.rs`, whole and unchanged, for the reason
//! [`kind`](crate::kind) is already here and stated in its own header: the two ends of this
//! channel live in **different crates**. `bin/ctl.rs` writes it from the root crate;
//! `console_main.rs` reads it, and Tier 5c moves that reader into `organon-console`. This is
//! the only crate both can see.
//!
//! 📌 **It was never plugin code.** All of `cli.rs`'s nih-plug coupling is two `use` lines
//! inside one function — `docs_files`, the `organon docs` generator — and none of it is here.
//! What kept this section upstream was its address, not a dependency.
//!
//! ⚠️ **This is a wire format between two processes**, so the rules in the section comment
//! below are load-bearing, and one of them bears repeating: **versioning is the verb.** A
//! reader that meets a verb it does not know skips the line rather than failing, which is what
//! lets a newer CLI talk to an older console. Adding a verb is how this format changes.

use crate::ipc;
use crate::camera;
use crate::kind::Kind;

// ---------------------------------------------------------------------------
// The console lane (#4 Tier 2) — the CLI's write path to the **console**.
//
// This is a third transport, not a third verb on an existing one, because it has
// a third destination (brief R3). `CliOp` lines on `ipc::cli_cmd_path()` are
// drained by the **World**, inside `World::frame_body`; the eyes channel is
// answered by the **visual**. A backdrop or a lighting rig is neither: it is
// `Console` state, owned by `console_main.rs`, and nothing in the World can reach it.
// Routing a console verb over `cli.txt` would put it in a queue read by a process
// that cannot act on it — green, silent, and wrong. So: its own sidecar,
// [`console_cmd_path`], drained in the console's frame path.
//
// The discipline is copied from the command channel deliberately: append-only
// UTF-8, one op per line, created on first write, reader self-detects growth by
// file length, and the CLI is never an IPC writer. Plain text, verb first — no
// JSON, matching [`CliOp`]'s simplicity.
//
// **Versioning is the verb.** There is no version token and there should not be
// one: a reader that meets a verb it does not know skips the line
// ([`parse_console_op`] returns `None`), so a newer CLI talking to an older
// console degrades to "that op did nothing" instead of to a parse error that
// poisons the rest of the drain. Adding a verb is how this format changes.
// ---------------------------------------------------------------------------

/// One console command, as it crosses the CLI→console sidecar.
///
/// The payload is an unvalidated name on purpose. `bin/ctl.rs` rejects an unknown one
/// at the clap boundary — before a line is ever written — so validation lives where the
/// error message can be good ("did you mean"), and the drain resolves what it is given.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleOp {
    /// What sits behind the glyphs: a substrate material, or a backdrop *source*
    /// (`world` / `off` / `substrate` — `console_main.rs`'s `BackdropSource` value space).
    Background(String),
    /// The substrate's lighting rig.
    Rig(String),
    /// The **palette the console paints with** — one of `organon_console::theme::Theme::NAMES`.
    ///
    /// 📌 **Live, and that is the whole reason it is a verb on this lane rather than only a
    /// preference.** Four palettes compared by relaunching four times is not a comparison:
    /// the thing being judged is a wash of colour across a window full of real text, and the
    /// judgement is made by looking back and forth. A relaunch destroys the "back".
    ///
    /// The payload is unvalidated here for [`ConsoleOp::Background`]'s reason — a palette
    /// *name* is only meaningful to the console's own table, and this crate has no business
    /// holding a second copy of it. `bin/ctl.rs` restricts it at the clap boundary from
    /// `Theme::NAMES` itself, and the console refuses an unknown name again on arrival.
    Theme(String),
    /// **How the console holds itself** — `terminal`, `desktop`, or a bare `0`–`1` scalar on
    /// the axis between them (`organon_console::posture::Posture`).
    ///
    /// ⚠️ **This SNAPS, and the snap is deliberate rather than a stage on the way to a
    /// tween.** The posture axis exists so a later tier *can* animate it, but a layout change
    /// costs one frame to re-wrap (~7.6 ms at 400 transcript elements —
    /// `doc/console_rewrap_measurement.md`), and a single such frame is not something a person
    /// perceives as a jump. What they would perceive is a half-second of text re-flowing under
    /// their eyes, which is the tween's problem to earn, not this verb's to approximate.
    ///
    /// A `String` rather than a parsed scalar because the value space is a word **or** a
    /// number, and `Posture::resolve` — which owns both spellings and the refusal that names
    /// them — lives in the console's crate. Parsing here would put half of that knowledge on
    /// the wire side of the lane.
    Posture(String),
    /// Reserve a run of blank rows in the console's transcript (Console Spike Tier 5) —
    /// a hole that stays put as the transcript scrolls, for a GPU-rendered panel to be
    /// painted into later. The payload is the row count, validated against
    /// [`MAX_BLOCK_ROWS`] at the clap boundary before a line is ever written.
    Block(u16),
    /// **Claim** a rectangle the writer already left in its own output (Console Spike
    /// Tier 5) — `up` lines above the cursor, `rows` tall.
    ///
    /// 🚨 **This, not [`ConsoleOp::Block`], is the correct shape, and the difference is not
    /// a refinement — it is the whole mechanism.** `Block` has the console *write* rows into
    /// the transcript at the cursor. But the cursor is, by definition, the live input point:
    /// the place a shell's prompt is waiting and a keystroke will land. Injecting there puts
    /// the hole **between the prompt and the typing**, which no terminal does, and which is
    /// worst precisely when the shell is idle — measured on 2026-08-11, prompt stranded above
    /// an eight-row hole with the cursor below it.
    ///
    /// So the console never writes. The program leaves the gap as part of its own output —
    /// ordinary blank lines through the ordinary PTY, which the shell, ConPTY and the console
    /// all agree exist — and then says where it is. The console only *records*.
    ///
    /// `kind` is what the console should draw in it — see [`Kind`], the vocabulary both
    /// front-ends resolve from.
    Patch { up: u16, rows: u16, kind: Kind },
    /// Open or close **the portal** — a screen-anchored, live, orbitable window onto the
    /// Organon world, floating over the transcript while the transcript scrolls past under it.
    ///
    /// 📌 **It carries no position, and that absence is the point.** Every other verb on this
    /// lane that puts something in the page has to say *where* in a character grid — and
    /// [`ConsoleOp::Patch`]'s doc records what that costs: the anchor is resolved at drain
    /// time, out of band with the PTY byte stream, so it is only correct while the child is
    /// idle and anything printed in between moves it. A **screen**-anchored rectangle resolves
    /// against the window instead, so the hardest-won caveat of the patch verbs simply does not
    /// arise here.
    Portal(PortalCmd),
    /// Move **the viewer's viewpoint** — the yaw, pitch and distance a drag and a wheel over
    /// the portal already write.
    ///
    /// ⚠️ **There are two cameras and this is only one of them.** `cam_path` / `cam_speed` /
    /// `cam_kick` / `cam_damping` (the `organon set` lane, on the *world*) are the auto-orbit:
    /// part of the composition, travelling in `Shared`, saved in a preset. This is where the
    /// *viewer stands* to watch that composition — host state that dies with the window and is
    /// in no preset. They compose rather than compete: the finalization adds the auto-orbit's
    /// offset to this base, so a shot framed here still spins if `cam_path` says to.
    ///
    /// 📌 **Absolute, never relative, and that is forced.** This lane is fire-and-forget with
    /// no return path (see [`console_cmd_path`]), so a caller can never read back where the
    /// camera is and therefore can never compute a delta. Absolute is the only shape that
    /// survives the transport.
    Camera(CameraFraming),
}

/// Where the viewer stands, as a partial update — `None` on an axis means "leave it".
///
/// A struct of options rather than one axis per command because framing a shot is **one
/// intent**: an agent that wants to be closer *and* a little above says so once and the
/// viewpoint moves once. Three commands would move the camera through two intermediate
/// framings nobody asked to see, and on a live portal those are frames a person watches.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraFraming {
    /// Return to the framing the window opened with, **before** the three below are applied —
    /// so `--reset --distance 40` means "back to the default view, then pull in", which is the
    /// recovery an agent that has lost the camera actually wants.
    pub reset: bool,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub distance: Option<f32>,
}

/// The camera words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`, by the console's command schema and by
/// [`CameraFraming::from_words`] — [`PORTAL_WORDS`]' arrangement, for its reason.
pub const CAMERA_WORDS: &[&str] = &["reset", "yaw", "pitch", "distance"];

impl CameraFraming {
    /// Does this ask for anything at all? A framing that names no axis and does not reset is
    /// not a command with defaults — it is a line that says nothing, and the lane's standing
    /// rule for that is to skip it.
    pub fn is_empty(self) -> bool {
        !self.reset && self.yaw.is_none() && self.pitch.is_none() && self.distance.is_none()
    }

    /// Is every stated axis finite and inside the band the command lane declares?
    ///
    /// ⚠️ **Refused, not clamped, and the asymmetry with `World` is deliberate.**
    /// `World::apply_camera_input` clamps, because a hand physically cannot ask for more than
    /// the band — a wheel that reaches the ceiling simply stops. A *typed* `--distance 9000`
    /// is not a gesture that overshot, it is a number that means something else (degrees for
    /// radians, metres for world units), and silently answering 4000 would let the mistake
    /// look like it worked. The clamp stays as the belt; this is the brace.
    pub fn in_range(self) -> bool {
        let ok = |v: Option<f32>, lo: f32, hi: f32| {
            v.is_none_or(|x| x.is_finite() && (lo..=hi).contains(&x))
        };
        ok(self.yaw, -crate::camera::YAW_LIMIT, crate::camera::YAW_LIMIT)
            && ok(self.pitch, -crate::camera::PITCH_LIMIT, crate::camera::PITCH_LIMIT)
            && ok(
                self.distance,
                crate::camera::DISTANCE_MIN,
                crate::camera::DISTANCE_MAX,
            )
    }

    /// The words this framing travels as, in [`CAMERA_WORDS`] order.
    ///
    /// Keyword-tagged rather than positional — unlike `patch`'s three counts — because the
    /// axes are **optional and independent**. A positional form would need a placeholder for
    /// "not this one", and a placeholder in a value slot is a value waiting to be misread.
    pub fn to_words(self) -> String {
        let mut out = String::new();
        if self.reset {
            out.push_str(" reset");
        }
        for (word, value) in [("yaw", self.yaw), ("pitch", self.pitch), ("distance", self.distance)]
        {
            if let Some(v) = value {
                out.push_str(&format!(" {word} {v}"));
            }
        }
        out
    }

    /// Parse the words after `camera`. `None` for an unknown word, a missing or unparseable
    /// number, a repeated axis, or a framing that asks for nothing — every one of which the
    /// drain treats exactly as it treats an unknown verb, by skipping the line.
    ///
    /// 📌 A **repeated** axis is malformed rather than last-wins. `camera distance 40 distance
    /// 4` is either a caller building a line badly or two intents concatenated; guessing which
    /// would move a camera somebody is looking at.
    pub fn from_words<'a>(words: impl Iterator<Item = &'a str>) -> Option<Self> {
        let mut out = Self::default();
        let mut it = words;
        while let Some(word) = it.next() {
            match word {
                "reset" => {
                    if out.reset {
                        return None;
                    }
                    out.reset = true;
                }
                "yaw" | "pitch" | "distance" => {
                    let v: f32 = it.next()?.parse().ok()?;
                    let slot = match word {
                        "yaw" => &mut out.yaw,
                        "pitch" => &mut out.pitch,
                        _ => &mut out.distance,
                    };
                    if slot.is_some() {
                        return None;
                    }
                    *slot = Some(v);
                }
                _ => return None,
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// What `organon console portal` does. See [`ConsoleOp::Portal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalCmd {
    Open,
    Close,
    /// One word for both directions — what a key binding or an alias wants.
    Toggle,
}

/// The portal words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`'s possible-values parser, by the console's command schema
/// and by [`PortalCmd::from_word`] — `organon_core::kind::KIND_WORDS`' arrangement exactly,
/// for the reason recorded there: a second hand-maintained copy is how a CLI comes to accept
/// a word nothing can act on.
pub const PORTAL_WORDS: &[&str] = &["open", "close", "toggle"];

impl PortalCmd {
    /// The word this command travels as, on the wire and in `--help`.
    pub fn as_word(self) -> &'static str {
        match self {
            PortalCmd::Open => "open",
            PortalCmd::Close => "close",
            PortalCmd::Toggle => "toggle",
        }
    }

    /// The command a word names, or `None` — which the sidecar reader treats as a line to
    /// skip, exactly as it treats an unknown verb.
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "open" => Some(PortalCmd::Open),
            "close" => Some(PortalCmd::Close),
            "toggle" => Some(PortalCmd::Toggle),
            _ => None,
        }
    }
}

/// What a claimed rectangle **shows** when the claim does not say.
///
/// 🚨 **This default belongs to this lane, not to the vocabulary.** [`Kind`] itself has no
/// `Default`, deliberately: a sidecar line with no third word was written before kinds
/// existed and *meant* the rendered scene, which is a wire-compatibility fact about
/// `patch <up> <rows> [kind]` and about nothing else. The conversation front-end has no such
/// history and must not inherit an answer to a question it never asked — see
/// `organon_core::kind`'s module doc.
///
/// An **unknown** kind is a different thing entirely and gets the opposite treatment
/// ([`parse_console_op`]): a newer CLI naming a kind this build cannot draw skips the line
/// rather than falling back to here, because guessing would paint the wrong object into a
/// rectangle someone else's output is holding open.
pub const PATCH_DEFAULT_KIND: Kind = Kind::Scene;

/// The tallest block `organon console block` will open.
///
/// A ceiling rather than an adjective: the pane is a few tens of rows on any real display,
/// so a block is a hole in a screenful — not a way to push the whole scrollback out of the
/// buffer one command at a time. 200 leaves room for a wall-sized pane and still costs less
/// than 2 % of the 10 000-line scrollback.
pub const MAX_BLOCK_ROWS: u16 = 200;

/// `$TMPDIR/<namespace>-console.txt` — the console command channel.
///
/// Namespace-derived through [`ipc::ns_file`] like every other IPC file, which is the one
/// cross-product invariant: it is what lets a console and a plain Organon run at once
/// without either steering the other. A hard-coded `$TMPDIR` path would silently break
/// that co-existence.
///
/// It lives here rather than beside `cli_cmd_path` in `organon-core`'s `ipc.rs` because
/// `ns_file` is **public**: this lane is between the CLI and the console, and adding a
/// sidecar needs no edit to the host-free spine. If a third party ever has to build this
/// path too, moving it into `ipc.rs` is the moment — not before.
pub fn console_cmd_path() -> std::path::PathBuf {
    ipc::ns_file("console.txt")
}

/// Serialize one console op to a single sidecar line (newline-free).
pub fn console_op_to_line(op: &ConsoleOp) -> String {
    match op {
        ConsoleOp::Background(name) => format!("background {name}"),
        ConsoleOp::Rig(name) => format!("rig {name}"),
        ConsoleOp::Theme(name) => format!("theme {name}"),
        ConsoleOp::Posture(word) => format!("posture {word}"),
        ConsoleOp::Block(rows) => format!("block {rows}"),
        ConsoleOp::Patch { up, rows, kind } => {
            format!("patch {up} {rows} {}", kind.as_word())
        }
        ConsoleOp::Portal(cmd) => format!("portal {}", cmd.as_word()),
        ConsoleOp::Camera(f) => format!("camera{}", f.to_words()),
    }
}

/// Parse one sidecar line. `None` for a malformed line **or an unknown verb** — the
/// drain skips those rather than failing, which is what makes the format
/// forward-compatible (see the section comment above).
///
/// Paired with [`console_op_to_line`] so both ends of the lane — this CLI writing and
/// the console reading — speak one vocabulary from one place. That pairing is the whole
/// reason these are here and not inlined at either end.
pub fn parse_console_op(line: &str) -> Option<ConsoleOp> {
    let mut it = line.trim().split_whitespace();
    match it.next()? {
        "background" => Some(ConsoleOp::Background(it.next()?.to_string())),
        "rig" => Some(ConsoleOp::Rig(it.next()?.to_string())),
        // Unvalidated for the same reason the two above are: a palette name resolves against
        // `Theme::NAMES` and a posture word against `Posture::resolve`, both of which live in
        // the console's own crate. Refusing here would need a second copy of each table, and
        // this lane's whole forward-compatibility story is that a name this build cannot use
        // is the *console's* to refuse out loud, not the wire's to drop in silence.
        "theme" => Some(ConsoleOp::Theme(it.next()?.to_string())),
        "posture" => Some(ConsoleOp::Posture(it.next()?.to_string())),
        // A row count that does not parse — or does not fit — is a malformed line, and a
        // malformed line is skipped exactly like an unknown verb. The `Background`/`Rig`/
        // `Theme`/`Posture` arms take their payload unvalidated because a *name* is only
        // meaningful to the console's own tables; a count is meaningful right here, and `u16`
        // is the type the whole lane carries.
        "block" => it.next()?.parse::<u16>().ok().map(ConsoleOp::Block),
        // Two counts, both required: a claim with a missing half is malformed, not a claim
        // with a default. Defaulting `up` would silently anchor the rectangle at the cursor —
        // exactly the placement this verb exists to avoid.
        "patch" => {
            let up = it.next()?.parse::<u16>().ok()?;
            let rows = it.next()?.parse::<u16>().ok()?;
            // The kind is the one field on this lane with a default, and only because it
            // arrived after the verb did: a line with no third word was written when a claim
            // had nothing to choose between, and [`PATCH_DEFAULT_KIND`] is what it meant. An
            // *unknown* third word is a different thing entirely — a newer CLI naming a kind
            // this build cannot draw — and the lane's standing rule for that is to skip the
            // line rather than guess, since guessing would paint the wrong object in a
            // rectangle someone else's output is holding open. `from_word` rather than
            // `resolve` for exactly that: there is no human on this end to read a refusal.
            let kind = match it.next() {
                None => PATCH_DEFAULT_KIND,
                Some(word) => Kind::from_word(word)?,
            };
            Some(ConsoleOp::Patch { up, rows, kind })
        }
        // ⚠️ **No default, deliberately — unlike `patch`'s `kind`.** That field defaults
        // because a line written before the field existed *meant* something specific, and
        // `Scene` is what it meant. There has never been a `portal` line without a word, so a
        // bare `portal` is not an older spelling of anything: it is malformed, and the lane's
        // standing rule for malformed is to skip rather than guess. Guessing would toggle a
        // window on and off from a typo.
        "portal" => PortalCmd::from_word(it.next()?).map(ConsoleOp::Portal),
        // Keyword-tagged and variable-length, unlike every other verb on this lane — see
        // [`CameraFraming::to_words`] on why the axes cannot be positional. A bare `camera`
        // yields `None` for the same reason a bare `portal` does: it is not an older spelling
        // of anything, so guessing would move a camera somebody is looking at.
        "camera" => CameraFraming::from_words(it).map(ConsoleOp::Camera),
        _ => None,
    }
}

/// Append console ops to the console command channel (one line each), creating the file
/// if it is not there yet.
///
/// Fire-and-forget, exactly like [`append_ops`]: the console drains them on its next
/// frame, and ops written while no console is running are never read — deliberately not
/// replayed at its next start.
pub fn append_console_ops(ops: &[ConsoleOp]) -> std::io::Result<()> {
    use std::io::Write;
    let body: String = ops
        .iter()
        .map(|o| format!("{}\n", console_op_to_line(o)))
        .collect();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_cmd_path())?;
    f.write_all(body.as_bytes())
}


#[cfg(test)]
mod tests {
    use super::*;

        // -----------------------------------------------------------------------
        // The console lane (#4 Tier 2)
        // -----------------------------------------------------------------------

        /// The format/parse pair is the shared vocabulary of two processes, so the round trip
        /// is the contract, not a nicety: the CLI writes with `console_op_to_line` and the
        /// console reads with `parse_console_op`, and if they ever disagree the failure is a
        /// silently ignored command rather than an error anybody sees.
        #[test]
        fn console_ops_round_trip_through_the_wire_format() {
            for op in [
                ConsoleOp::Background("graphite".into()),
                ConsoleOp::Background("paper".into()),
                ConsoleOp::Background("slate".into()),
                ConsoleOp::Background("metal".into()),
                // The three backdrop *sources* ride the same verb as the materials.
                ConsoleOp::Background("world".into()),
                ConsoleOp::Background("off".into()),
                ConsoleOp::Background("substrate".into()),
                ConsoleOp::Rig("studio".into()),
                ConsoleOp::Rig("daylight".into()),
                // #38: the console's own dressing rather than the substrate's. Every palette
                // rides the round trip for `Patch`'s reason — a name that survived one direction
                // only would be a verb that reports `queued` and repaints nothing.
                ConsoleOp::Theme("organon".into()),
                ConsoleOp::Theme("light".into()),
                ConsoleOp::Theme("dark".into()),
                ConsoleOp::Theme("chocolate".into()),
                ConsoleOp::Posture("terminal".into()),
                ConsoleOp::Posture("desktop".into()),
                // The scalar spelling, which is the one that could have been lost: it is the
                // only payload on this lane that is a number carried as a word.
                ConsoleOp::Posture("0.5".into()),
                // Tier 5: the payload is a count, not a name — the first op on this lane whose
                // argument is not a word.
                ConsoleOp::Block(1),
                ConsoleOp::Block(12),
                ConsoleOp::Block(MAX_BLOCK_ROWS),
                // …and the claim, whose payload is two counts and a kind. Every kind rides the
                // round trip, because the kind is the field that decides what gets *painted* and
                // a spelling that survived one direction only would paint the wrong object.
                ConsoleOp::Patch { up: 0, rows: 1, kind: Kind::Scene },
                ConsoleOp::Patch { up: 12, rows: 12, kind: Kind::Scene },
                ConsoleOp::Patch { up: 12, rows: 12, kind: Kind::Panel },
                ConsoleOp::Patch {
                    up: MAX_BLOCK_ROWS,
                    rows: MAX_BLOCK_ROWS,
                    kind: Kind::Panel,
                },
            ] {
                let line = console_op_to_line(&op);
                assert!(!line.contains('\n'), "a line must be one line: {line:?}");
                assert_eq!(parse_console_op(&line), Some(op.clone()), "line was {line:?}");
                // The drain reads whole lines from a file; trailing whitespace is not a
                // different command.
                assert_eq!(parse_console_op(&format!("  {line}  ")), Some(op));
            }

            // The exact bytes on the wire, pinned — the console's drain is written against
            // these, and "the round trip works" would still hold if both ends changed together
            // while an already-running console spoke the old spelling.
            assert_eq!(
                console_op_to_line(&ConsoleOp::Background("slate".into())),
                "background slate"
            );
            assert_eq!(console_op_to_line(&ConsoleOp::Rig("studio".into())), "rig studio");
            assert_eq!(console_op_to_line(&ConsoleOp::Theme("light".into())), "theme light");
            assert_eq!(
                console_op_to_line(&ConsoleOp::Posture("desktop".into())),
                "posture desktop"
            );
            assert_eq!(console_op_to_line(&ConsoleOp::Block(12)), "block 12");
            assert_eq!(
                console_op_to_line(&ConsoleOp::Patch { up: 12, rows: 12, kind: Kind::Panel }),
                "patch 12 12 panel"
            );
        }

        /// **This lane's two facts about the kind**, now that the vocabulary itself lives in
        /// `organon_core::kind` and is round-tripped by its own tests.
        ///
        /// The first is that the CLI word for the engine-drawn kind is `scene` and has not moved.
        /// It is in `--help`, in the `organon-cli` skill and on this sidecar wire, so it is public
        /// surface: renaming an internal variant is free, renaming a word someone types is not.
        ///
        /// The second is the default, pinned here rather than on [`Kind`] because it is a
        /// wire-compatibility rule belonging to `patch` alone — what a claim written before there
        /// was a choice resolves to, so changing it silently repaints every such claim.
        #[test]
        fn the_cli_word_for_a_scene_is_still_scene_and_a_kindless_claim_means_it() {
            assert_eq!(Kind::Scene.as_word(), "scene", "public CLI surface — do not rename");
            assert_eq!(Kind::from_word("scene"), Some(Kind::Scene));
            assert_eq!(PATCH_DEFAULT_KIND, Kind::Scene, "a claim with no kind is a scene");
        }

        /// A claim's kind: absent means `scene` (the shape the verb shipped with), unknown means
        /// **skip the line**. The two are deliberately not the same answer — see `parse_console_op`.
        #[test]
        fn a_claim_with_no_kind_is_a_scene_and_an_unknown_kind_is_skipped() {
            assert_eq!(
                parse_console_op("patch 12 12"),
                Some(ConsoleOp::Patch { up: 12, rows: 12, kind: Kind::Scene }),
                "a line from before the kind existed still claims a rectangle"
            );
            assert_eq!(
                parse_console_op("patch 0 8 panel"),
                Some(ConsoleOp::Patch { up: 0, rows: 8, kind: Kind::Panel })
            );
            assert_eq!(parse_console_op("patch 12 12 hologram"), None, "a kind we cannot draw");
            assert_eq!(parse_console_op("patch 12"), None, "a claim with a missing half");
            assert_eq!(parse_console_op("patch"), None);
            assert_eq!(parse_console_op("patch up rows"), None);
            assert_eq!(parse_console_op("patch -1 12"), None, "negative is not a u16");
        }

        /// An unknown verb parses to `None` so the drain SKIPS it. That is the whole
        /// forward-compatibility story of this format: a newer CLI writing a verb an older
        /// console has never heard of must degrade to "that op did nothing", never to a parse
        /// failure that poisons the rest of the file.
        #[test]
        fn an_unknown_console_verb_is_skipped_not_fatal() {
            assert_eq!(parse_console_op("scrim 0.5"), None); // a plausible future verb
            assert_eq!(parse_console_op("frobnicate x"), None);
            assert_eq!(parse_console_op(""), None);
            assert_eq!(parse_console_op("   "), None);
            // A known verb with no argument is malformed, not a different command.
            assert_eq!(parse_console_op("background"), None);
            assert_eq!(parse_console_op("rig"), None);
            assert_eq!(parse_console_op("theme"), None);
            assert_eq!(parse_console_op("posture"), None);
            // ⚠️ An unknown palette or posture is NOT skipped here, and that is the one place
            // this lane deliberately parses something it cannot use. `theme phosphor` is a
            // well-formed line naming a palette this build has no way to paint, and the console
            // is the only end that can say so — dropping it at the wire would make a typo
            // indistinguishable from a console that was not running.
            assert_eq!(
                parse_console_op("theme phosphor"),
                Some(ConsoleOp::Theme("phosphor".into())),
                "the console refuses this out loud; the wire must not swallow it"
            );
            assert_eq!(
                parse_console_op("posture sideways"),
                Some(ConsoleOp::Posture("sideways".into()))
            );
            // `CliOp`'s vocabulary is a DIFFERENT lane's — a `cli.txt` line landing here (a
            // mis-wired drain) must be skipped rather than half-understood.
            assert_eq!(parse_console_op("set metallic 0.9"), None);
            assert_eq!(parse_console_op("gen 3"), None);
        }

        /// **`block` is the first console verb whose argument is a number**, so the malformed
        /// cases are a different set from a name's: a count that is not a count, or one this lane
        /// cannot carry. All of them are skipped rather than clamped — a clamp would open a block
        /// of a size nobody asked for, which is worse than opening none.
        #[test]
        fn a_block_row_count_that_is_not_a_count_is_skipped() {
            assert_eq!(parse_console_op("block 8"), Some(ConsoleOp::Block(8)));
            assert_eq!(parse_console_op("block 0"), Some(ConsoleOp::Block(0)));
            assert_eq!(parse_console_op("block"), None, "no argument at all");
            assert_eq!(parse_console_op("block rows"), None);
            assert_eq!(parse_console_op("block -1"), None, "negative is not a u16");
            assert_eq!(parse_console_op("block 8.5"), None, "fractional rows do not exist");
            assert_eq!(parse_console_op("block 70000"), None, "past u16 entirely");
            // Trailing junk after the count is ignored, exactly as a name's is — the wire form
            // is `<verb> <word>` and the drain reads whole lines.
            assert_eq!(parse_console_op("block 8 rows"), Some(ConsoleOp::Block(8)));
        }

        /// **The camera lane's whole wire form, both directions.** It is the first verb here whose
        /// arguments are keyword-tagged and variable-length, so the round trip is the only thing
        /// that keeps the writer and the reader agreeing about a shape neither can see the other
        /// half of.
        #[test]
        fn a_camera_framing_survives_the_sidecar_round_trip_in_every_combination() {
            let cases = [
                CameraFraming { reset: true, ..Default::default() },
                CameraFraming { yaw: Some(0.8), ..Default::default() },
                CameraFraming { pitch: Some(-0.25), ..Default::default() },
                CameraFraming { distance: Some(40.0), ..Default::default() },
                CameraFraming { reset: true, distance: Some(40.0), ..Default::default() },
                CameraFraming { reset: false, yaw: Some(-1.5), pitch: Some(0.5), distance: Some(12.5) },
                CameraFraming { reset: true, yaw: Some(0.0), pitch: Some(0.0), distance: Some(0.1) },
            ];
            for f in cases {
                let op = ConsoleOp::Camera(f);
                let line = console_op_to_line(&op);
                assert!(line.starts_with("camera "), "the verb leads the line: {line:?}");
                assert_eq!(parse_console_op(&line), Some(op), "round trip of {line:?}");
            }
        }

        /// The malformed set, which for this verb is larger than for any other on the lane — and
        /// every member of it is **skipped**, never guessed at. A camera command that half-parsed
        /// would move a viewpoint somebody is looking at to a place nobody asked for.
        #[test]
        fn a_camera_line_that_says_nothing_definite_is_skipped() {
            assert_eq!(parse_console_op("camera"), None, "no axis at all — not a default framing");
            assert_eq!(parse_console_op("camera reset"), Some(ConsoleOp::Camera(
                CameraFraming { reset: true, ..Default::default() }
            )));
            assert_eq!(parse_console_op("camera yaw"), None, "an axis with no number");
            assert_eq!(parse_console_op("camera yaw nine"), None, "a number that is not one");
            assert_eq!(parse_console_op("camera roll 0.2"), None, "an axis this build has not got");
            assert_eq!(parse_console_op("camera yaw 0.1 yaw 0.2"), None, "a repeated axis");
            assert_eq!(parse_console_op("camera reset reset"), None, "a repeated flag");
            assert_eq!(parse_console_op("camera yaw 0.1 junk"), None, "trailing junk is not ignored");
            // ⚠️ Unlike `block 8 rows`, trailing words are NOT tolerated here: on a keyword-tagged
            // line a stray word is indistinguishable from an axis the writer meant and this build
            // does not have, and quietly applying the half it understood is the wrong half.
            assert_eq!(
                parse_console_op("camera distance 40"),
                Some(ConsoleOp::Camera(CameraFraming { distance: Some(40.0), ..Default::default() }))
            );
        }

        /// The band is the one the **hand** is held to, read from `scene_input` rather than
        /// restated — so a limit that moves for a drag moves for a typed command in the same
        /// commit. Out of range is refused, not clamped; see [`CameraFraming::in_range`].
        #[test]
        fn a_framing_is_in_range_exactly_when_every_stated_axis_is_inside_the_hands_own_band() {
            use crate::camera as si;
            assert!(CameraFraming { reset: true, ..Default::default() }.in_range(), "reset alone");
            assert!(CameraFraming {
                yaw: Some(si::YAW_LIMIT),
                pitch: Some(-si::PITCH_LIMIT),
                distance: Some(si::DISTANCE_MAX),
                reset: false,
            }
            .in_range(), "the edges are inside");
            assert!(!CameraFraming { yaw: Some(si::YAW_LIMIT + 0.1), ..Default::default() }.in_range());
            assert!(!CameraFraming { pitch: Some(2.0), ..Default::default() }.in_range());
            assert!(!CameraFraming { distance: Some(0.0), ..Default::default() }.in_range(),
                "zero distance is inside no band — the viewpoint would be at the pivot");
            assert!(!CameraFraming { distance: Some(9000.0), ..Default::default() }.in_range());
            assert!(!CameraFraming { yaw: Some(f32::NAN), ..Default::default() }.in_range(),
                "a NaN passes every comparison and poisons the view matrix");
            assert!(!CameraFraming { distance: Some(f32::INFINITY), ..Default::default() }.in_range());
            // The defaults `--reset` restores must themselves be reachable, or reset would be the
            // one framing the command lane refuses.
            assert!(CameraFraming {
                reset: true,
                yaw: Some(si::DEFAULT_YAW),
                pitch: Some(si::DEFAULT_PITCH),
                distance: Some(si::DEFAULT_DISTANCE),
            }
            .in_range());
        }

        /// The word table `--help`, the console's schema and the parser are all built from must
        /// name every axis the parser accepts, and nothing it does not — the drift guard
        /// `organon_core::kind::KIND_WORDS` and [`PORTAL_WORDS`] carry, for a verb whose words are
        /// not an enum and so cannot be checked by exhaustiveness.
        #[test]
        fn every_camera_word_is_one_the_parser_takes_and_the_table_has_no_others() {
            for word in CAMERA_WORDS {
                let line = if *word == "reset" {
                    format!("camera {word}")
                } else {
                    format!("camera {word} 0.5")
                };
                assert!(
                    parse_console_op(&line).is_some(),
                    "`{word}` is in the table `--help` is built from but the parser refuses it"
                );
            }
            assert_eq!(CAMERA_WORDS.len(), 4, "reset plus the three axes — no more, no fewer");
        }

        /// The console sidecar is namespace-derived like every other IPC file. This is the
        /// invariant that lets a console and a plain Organon run side by side; a hard-coded
        /// `$TMPDIR` name would break co-existence silently, which is precisely the failure
        /// `ns_file` exists to prevent.
        #[test]
        fn the_console_channel_is_namespaced() {
            let p = console_cmd_path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(name, format!("{}-console.txt", ipc::namespace()));
            // Same directory as its sibling channels — compared against one of them rather
            // than against `temp_dir()` so the assertion cannot drift from `ns_file`.
            assert_eq!(p.parent(), ipc::cli_cmd_path().parent());
            // …and it is its OWN file: routing console ops onto the World's lane is the exact
            // mistake this channel exists to avoid (brief R3).
            assert_ne!(console_cmd_path(), ipc::cli_cmd_path());
            assert_ne!(console_cmd_path(), ipc::eyes_cmd_path());
        }
}
