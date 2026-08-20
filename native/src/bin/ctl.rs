//! `organon` — the Organon command surface (#452 Tiers 1–2).
//!
//! A plain local CLI so external agents (Bianca) and humans can read Organon's
//! live state and drive its controls without MCP, sockets, or the editor:
//! - reads decode the `Shared` IPC mmap directly (frame-fresh);
//! - writes append `CliOp` lines to the command sidecar, drained each frame by
//!   the visual into the #317 Performer's override lane (last-touched-wins);
//! - `console` verbs (#4 Tier 2) append `ConsoleOp` lines to a **separate**
//!   sidecar, drained by the console. Different destination, different channel —
//!   see `cli.rs`'s console-lane comment for why routing them over `cli.txt`
//!   would be silently wrong.
//!
//! This binary owns the **clap** argument surface — per-subcommand `--help`,
//! "did you mean" suggestions, `--version`, and `organon completions <shell>`
//! for bash/zsh/fish tab completion (param ids complete as values). All the
//! actual logic lives in `organic_math_native::cli` (pure, unit-tested); this
//! file maps commands to I/O and exit codes:
//!   0 = ok · 2 = bad usage (clap) · 3 = read command with no live Organon.

use clap::{CommandFactory, Parser, Subcommand};
use organic_math_native::{agent, cli, ipc, scene_input};
use organon_core::kind;
use organon_console::{panel_stack, posture, region, screen, theme};

/// Possible-values parser over the Tier-1 actuatable param ids — powers both
/// validation ("did you mean") and shell completion of `<ID>` arguments.
fn param_ids() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(agent::ACTUATABLE_IDS.iter().copied())
}

// ---------------------------------------------------------------------------
// `organon console` — the console's own vocabulary (#4 Tier 2).
//
// 🚨 **INTEGRATOR: the two lists below are COPIES and must be bound to their
// source in the change that merges Tier 2's leaves.** The materials and the rigs
// are canonically owned by Leaf A's new `substrate_materials` module
// (`MATERIAL_NAMES` / `RIG_NAMES`), which was being written concurrently with
// this file and so could not be imported yet. `substrate_camera` and
// `substrate_scene` are ungated `pub mod`s in `lib.rs`, so `substrate_materials`
// will be reachable from this bin the moment it lands. Add, in `tests` below:
//
//     assert_eq!(CONSOLE_MATERIALS, substrate_materials::MATERIAL_NAMES);
//     assert_eq!(CONSOLE_RIGS,      substrate_materials::RIG_NAMES);
//
// An equality test, not a re-import, so the failure names the drift instead of
// hiding it: `--help` and the renderer must agree, and if they do not, the CLI
// accepts a name nothing can draw. This is exactly the failure `agent::id_range`
// already demonstrated by hand-maintaining a second copy of `params.rs`'s ranges
// (drifted on 9 of 45 ids — brief R6). Two copies for the length of one tier is
// a declared debt; two copies with no test is how it becomes permanent.
// ---------------------------------------------------------------------------

/// The substrate **materials** — Leaf A's to own; see the block comment above.
const CONSOLE_MATERIALS: &[&str] = &["graphite", "paper", "slate", "metal"];

/// The backdrop **sources**, which are not materials and not Leaf A's. These come from
/// `console_main.rs`'s `BackdropSource` value space — `world` keeps the live `organon
/// set`/`generator`/`recipe` response behind the glyphs, `off` is a flat fill, and
/// `substrate` selects the lit plane without saying which material. One verb covers both
/// because from the outside there is one question: what is behind the text?
const CONSOLE_SOURCES: &[&str] = &["world", "off", "substrate"];

/// The lighting rigs — Leaf A's to own; see the block comment above.
const CONSOLE_RIGS: &[&str] = &["studio", "daylight"];

/// Possible-values parser for `console background <NAME>`: materials, then sources.
fn background_names() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(
        CONSOLE_MATERIALS.iter().chain(CONSOLE_SOURCES.iter()).copied(),
    )
}

/// Possible-values parser for `console rig <NAME>`.
fn rig_names() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(CONSOLE_RIGS.iter().copied())
}

/// Possible-values parser for `console theme <NAME>`.
///
/// **Built from `Theme::NAMES` itself**, on [`patch_kinds`]' rule rather than the two
/// hand-copied lists above — the palettes live in `organon-console`, which this binary already
/// depends on, so there is no reason to restate them and every reason not to: a fifth palette
/// must appear in `--help` and in tab completion the moment it appears in the table, and a
/// copy is how a CLI comes to offer a colour nothing can paint.
fn theme_names() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(theme::Theme::NAMES.iter().copied())
}

/// Possible-values parser for `console screen <STATE>`. Built from
/// `organon_console::screen::SCREEN_WORDS`, on [`patch_kinds`]' rule and for its reason.
///
/// 📌 **Unlike `console posture`, this one CAN be a `PossibleValuesParser`, and the contrast
/// is the design showing through the CLI.** A posture's value space is two words *or* a
/// scalar, which clap cannot state — so its gate has to move to `run_console` and its words do
/// not tab-complete. A screen state has three words and nothing between them, because a window
/// either covers the display or it does not. So the check lands at the clap boundary, where the
/// error is best, and the words complete for free.
fn screen_words() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(screen::SCREEN_WORDS.iter().copied())
}

/// Possible-values parsers for `console viewport <REGION> <CONTENT>`. Built from
/// `organon_console::region`'s own two tables, on [`patch_kinds`]' rule and for its reason.
///
/// 📌 Both are closed lists with nothing between the words, so — like `screen` and unlike
/// `posture` — the check lands at the clap boundary where the error is best, and both rings tab
/// complete for free.
fn region_words() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(region::REGION_WORDS.iter().copied())
}

/// See [`region_words`]. ⚠️ This list carries `off`, which is **not** a content kind — it
/// empties the region. `region::CONTENT_WORDS` is the one table that says so, and this reads it
/// rather than restating it, so a kind added there reaches `--help` and completion together.
fn content_words() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(region::CONTENT_WORDS.iter().copied())
}

/// Possible-values parsers for `console stack <ACTION> <PANEL>`. Built from
/// `organon_console::panel_stack`'s own two tables, on [`patch_kinds`]' rule and for its reason.
fn stack_actions() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(panel_stack::STACK_ACTIONS.iter().copied())
}

/// See [`stack_actions`]. ⚠️ This list carries `all`, which is **not** a panel — it names the
/// whole column, exactly as `off` in `content_words` is not a content kind. `panel_stack`'s
/// `panel_words` is the one place the slugs and that word are joined, so `--help`, the schema
/// and completion cannot come to offer different sets.
fn stack_panels() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(panel_stack::panel_words())
}

/// Possible-values parser for `console patch --kind <KIND>`.
///
/// Unlike the two lists above this one is **built from the shared table**
/// (`organon_core::kind::KIND_WORDS`) rather than restated here, so it cannot drift: the
/// kinds are `Kind`'s value space, and since #48 T1 that value space is the *console's*, not
/// the patch lane's — the conversation front-end resolves from the same one.
fn patch_kinds() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(kind::KIND_WORDS.iter().copied())
}

/// Possible-values parser for `console portal <STATE>`. Built from `cli::PORTAL_WORDS`, on
/// [`patch_kinds`]' rule and for its reason.
fn portal_words() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(cli::PORTAL_WORDS.iter().copied())
}

#[derive(Parser)]
#[command(
    name = "organon",
    version,
    about = "The Organon command surface — read the live state, drive the controls (#452)",
    after_help = "The visual must be running for reads to be live and for writes to take \
                  effect; commands written while it is down are deliberately NOT replayed \
                  at its next start.\n\nTab completion: `organon completions zsh` (see its \
                  --help for install lines). Writes go through the Performer's override \
                  lane: your sliders always win by touching (last-touched-wins), and \
                  `organon release` lets go of everything."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Live status: generator / surface / material, tempo, transport
    Status {
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// The whole vocabulary: every settable param (id, kind, range, current) +
    /// the generator / surface / material lists. Works with Organon offline.
    Catalog {
        /// Emit JSON instead of text (each entry carries its `desc`)
        #[arg(long)]
        json: bool,
        /// Inline every description — the full operating manual (text mode)
        #[arg(long, alias = "manual")]
        verbose: bool,
    },
    /// Describe one param / generator / surface / material / recipe: what it does, plus
    /// (for a param) its kind, range, and current value. e.g. `organon describe metallic`,
    /// `organon describe dna`, `organon describe glass`, `organon describe helix`.
    Describe {
        /// A param id, a generator / surface / material name or ordinal, or a recipe name
        query: String,
    },
    /// List the built-in recipe library — named starting-points you can apply with one
    /// command (no saved presets needed). e.g. `organon recipes`
    Recipes {
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Apply a recipe by name (selects its generator/surface/material + sets its key params
    /// through the override lane) — a launch pad you then tweak. e.g. `organon recipe helix`
    Recipe {
        /// The recipe name — see `organon recipes`
        which: String,
        /// Print what it would do without applying it
        #[arg(long)]
        dry_run: bool,
    },
    /// Read one param's current value (or every param with --all)
    Get {
        /// Param id (tab-completes; omit when using --all)
        #[arg(value_parser = param_ids())]
        id: Option<String>,
        /// Read every actuatable param
        #[arg(long, conflicts_with = "id")]
        all: bool,
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Stream reads: one JSON line per tick (the agent feedback loop)
    Watch {
        /// Tick interval in milliseconds (min 16)
        #[arg(long, default_value_t = 250)]
        ms: u64,
        /// Comma-separated param ids (default: every actuatable param)
        #[arg(long, value_delimiter = ',', value_parser = param_ids())]
        fields: Vec<String>,
    },
    /// Queue absolute param sets (raw units), e.g. `organon set metallic 0.9 glow 1.5`
    ///
    /// ⚠️ **`allow_negative_numbers` is load-bearing, not tidiness.** Without it clap reads
    /// the leading `-` of a value as a flag and rejects the whole command
    /// (`error: unexpected argument '-3' found`), which put **every negatively-ranged param
    /// out of reach through the documented syntax**: `exposure`, `elevation`, `azimuth`, and
    /// the six `rot_mod_*` / `trans_mod_*` axes — nine of them. The first public-repo trial
    /// hit it immediately (`organon set exposure -3.0`) and had to fall back to the `--`
    /// separator, which nothing documented. Setting a param to a negative number is the most
    /// ordinary thing this subcommand does; it must not need an escape hatch.
    #[command(allow_negative_numbers = true)]
    Set {
        /// Alternating <ID> <VALUE> pairs
        #[arg(required = true, num_args = 1.., value_names = ["ID VALUE"])]
        pairs: Vec<String>,
    },
    /// Queue a full phrase-plan JSON (the #317 debug-executor format:
    /// {"name":"…","moves":[{"op":"set_param","id":"…","value":…},
    /// {"op":"ramp","id":"…","to":…,"bars":…}]})
    Do {
        /// The plan JSON (validated + normalized before queueing)
        plan: String,
    },
    /// Release one agent hold (values stay put), or everything when no id given
    Release {
        /// Param id to release (tab-completes; omit to release all)
        #[arg(value_parser = param_ids())]
        id: Option<String>,
    },
    /// Switch the generator by name, unambiguous substring, or ordinal
    #[command(alias = "gen")]
    Generator {
        /// e.g. `dna`, `"strange attractor"`, `8` — `organon catalog` lists all
        which: String,
    },
    /// Switch the surface mode by name, unambiguous substring, or ordinal
    #[command(alias = "surf")]
    Surface {
        /// e.g. `"swept tubes"`, `plexus`, `2`
        which: String,
    },
    /// Switch the material by name, unambiguous substring, or ordinal
    #[command(alias = "mat")]
    Material {
        /// e.g. `chrome`, `glass`, `velvet`, `0`
        which: String,
    },
    /// Read one frame back to a PNG (#452 Tier 3 — "the eyes"). Requires a running
    /// visual window; prints the written path. Use it to close the see→act→see loop:
    /// `organon set …` then `organon snap` then judge the image.
    Snap {
        /// Output path (default: ./organon-snap-<unix-ms>.png). Made absolute — the
        /// visual runs from a different working directory.
        #[arg(short, long, value_name = "FILE")]
        out: Option<std::path::PathBuf>,
    },
    /// Drive the in-app video recorder (#452 Tier 3). Prints the output file path.
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },
    /// Drive the console itself — the surface behind the glyphs and the shape of the
    /// transcript, not the world in front of it. e.g. `organon console background slate`,
    /// `organon console rig daylight`, `organon console block 12`
    Console {
        #[command(subcommand)]
        action: ConsoleAction,
    },
    /// Print a shell completion script (bash / zsh / fish / …)
    #[command(after_help = "install:\n  zsh:  organon completions zsh > \
                            /usr/local/share/zsh/site-functions/_organon  (then restart zsh)\n  \
                            bash: organon completions bash > \
                            /usr/local/etc/bash_completion.d/organon\n  \
                            fish: organon completions fish > \
                            ~/.config/fish/completions/organon.fish\n\
                            or eval on the fly:  source <(organon completions zsh)")]
    Completions {
        /// The shell to generate for
        shell: clap_complete::Shell,
    },
    /// Regenerate the Markdown reference under `doc/reference/` from the descriptions
    /// compiled into the binary. Works with Organon offline — it reads no live state.
    #[command(after_help = "The checked-in pages are pinned by a test \
                            (`generated_reference_is_current`), so run this after changing \
                            any description in `agent.rs` or `recipe.rs` and commit the \
                            result alongside the code change.")]
    Docs {
        /// Where to write (default: `doc/reference` beside the `native/` directory)
        #[arg(short, long, value_name = "DIR")]
        out: Option<std::path::PathBuf>,
        /// Report drift and exit non-zero instead of writing anything
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum RecordAction {
    /// Start recording; auto-stop after N bars (0 = free-run until `record stop`)
    Start {
        /// Beat-synced length in bars (0 = manual stop only)
        #[arg(long, default_value_t = 0)]
        bars: u32,
    },
    /// Stop the active recording and finalize the file
    Stop,
}

/// `organon console …` — the console's chrome, kept in a namespace of its own rather than
/// flattened into the verbs above.
///
/// The split is not cosmetic. Every other verb here addresses **the world**: what Organon
/// renders, reachable in any edition, answered by the visual or by `Shared`. These address
/// **the console** — a different process's own state, on a different channel, meaningful
/// only when a console is running. A `background` at the top level would sit beside
/// `material` and read like a sibling of it, which is the one thing it is not.
#[derive(Subcommand)]
enum ConsoleAction {
    /// Set what sits behind the glyphs — a substrate material, or a backdrop source
    #[command(after_help = "Materials dress the lit plane; `world`, `off` and `substrate` \
                            choose what is drawn at all. `world` keeps the live response \
                            to `organon set` / `generator` / `recipe` behind the text.")]
    Background {
        /// The material or source (see the value list above)
        #[arg(value_parser = background_names())]
        name: String,
    },
    /// Set the substrate's lighting rig
    Rig {
        /// The rig (see the value list above)
        #[arg(value_parser = rig_names())]
        name: String,
    },
    /// Set the console's colour palette — every colour it paints, at once
    #[command(after_help = "The change is LIVE and it STICKS: the window repaints on its \
                            next frame, and the choice is written to preferences.json so the \
                            next console opens on it. That is the point of a verb rather \
                            than a launch variable — four palettes compared by relaunching \
                            four times is not a comparison.\n\n`organon` is the phosphor \
                            green the console has always had, and is the default for anyone \
                            who has never chosen. `light` is the one to try if the room \
                            is bright; `dark` and `chocolate` are two other rooms.\n\n\
                            ORGANON_SHELL_THEME overrides a stored choice for ONE launch and \
                            says so on the console's own stderr when it does. It never \
                            writes: unset it and your stored palette is back.")]
    Theme {
        /// The palette (see the value list above)
        #[arg(value_parser = theme_names())]
        name: String,
    },
    /// Set how the console holds itself — terminal-tight or desktop-open
    #[command(after_help = "`terminal` is the console exactly as it has always drawn. \
                            `desktop` insets the conversation behind a 90-point left \
                            gutter, opens the padding out, and rules each card down the \
                            left instead of boxing it. A bare number from 0 to 1 stands \
                            anywhere between them — the axis is a scalar and every form \
                            token lerps along it, so `0.5` is a real console and not a \
                            rounding of one of the ends.\n\n\
                            ⚠️ This SNAPS. There is no animation: the layout is the next \
                            frame's, which costs one re-wrap of the transcript. A smooth \
                            tween is a later tier and is a bigger question than it looks \
                            (doc/console_rewrap_measurement.md).\n\n\
                            ⚠️ It is NOT remembered. A palette is what the console is made \
                            of; a posture is a view you take to look at something, and this \
                            one has never been drawn on a real screen. Closing the console \
                            puts it back at `terminal`.")]
    Posture {
        /// terminal, desktop, or a number from 0 to 1
        ///
        /// ⚠️ **No `value_parser` here, unlike every other named argument on this
        /// subcommand, and it is not an omission.** The value space is two words *or* a
        /// scalar, and `PossibleValuesParser` cannot express that — it would have to refuse
        /// `0.5`, which the type represents and `Form::at` draws. So the gate moves one step
        /// later, into `run_console`, where `Posture::resolve` owns both spellings and the
        /// refusal that names them. The cost is real and worth stating: `<POSTURE>` does not
        /// tab-complete the two words, which is why `--help` lists them twice over.
        word: String,
    },
    /// Fill the display with the console's window, or give it its edges back
    #[command(after_help = "`full` is borderless full screen on the display the window is \
                            already on; `windowed` is the ordinary window it opened as; \
                            `toggle` is whichever it is not.\n\n\
                            ⚠️ This is NOT a posture, and the distinction is load-bearing \
                            rather than pedantic. A posture is a set of form tokens — margins, \
                            corners, padding, line height — and full screen changes none of \
                            them. The two are orthogonal: full screen at `terminal` posture and \
                            full screen at `desktop` posture are both real consoles, and every \
                            combination of the two verbs means exactly what it says.\n\n\
                            📌 F11 flips it from inside the window, at any time, in any tab. \
                            That is the way out, and it is a real one: no title bar means no \
                            close button, and the function keys are the one band the terminal \
                            has never sent to a child — so unlike Escape, claiming it takes \
                            nothing away from `vim`.\n\n\
                            ⚠️ It is NOT remembered, on `posture`'s rule: the console opens \
                            windowed however you left it.")]
    Screen {
        /// full, windowed, or toggle
        #[arg(value_parser = screen_words())]
        state: String,
    },
    /// Divide the pane into regions and say what each one holds
    #[command(after_help = "The window's ONE pane becomes up to four, and each region holds one \
                            kind of thing. `full` is the whole pane — what the console opens \
                            holding — then `left`/`right`/`top`/`bottom` are halves and \
                            `topleft`/`topright`/`bottomleft`/`bottomright` are quarters. A \
                            region never splits again: to get quarters, name the quarters.\n\n\
                            Assigning a region that CONTAINS one already held, or is contained \
                            by one, displaces it and says so — that is how `/viewport left \
                            agent` works from a console holding `full`. An assignment that only \
                            PARTLY overlaps (`top` while `left` is held) is refused by name: \
                            neither contains the other, so there is nothing unambiguous to take \
                            away.\n\n`off` empties a region. It is refused on a region that \
                            already holds nothing, and refused on the last region holding an \
                            `agent` — a console with nothing to talk to has no obvious way \
                            back, since the verb that would fix it is typed at an agent.\n\n\
                            `3d` is a LIVE 3D VIEWPORT — drag inside it to orbit, wheel to \
                            zoom, and `organon set`/`generator`/`recipe` typed at a prompt drive \
                            what it shows. ⚠️ Only ONE region may hold it, because its producer \
                            is Organon and Organon draws at most one frame per console frame; a \
                            second is refused by name. ⚠️ An open portal TAKES that frame — the \
                            region then says so and `console portal close` gives it back.\n\n\
                            `panel` is a SCROLLING STACK of Organon's own editor panels — \
                            `console stack add <panel>` fills it, and the region's size has \
                            nothing to do with how many panels are in it. There is ONE stack \
                            and every `panel` region is a view of it. `media` is not in the \
                            vocabulary yet. ⚠️ Only one region can show the live tab — \
                            a second `agent` region says so rather than drawing it twice.\n\n\
                            📌 Orthogonal to `posture` and `screen` both, and it is NOT \
                            remembered: the console opens undivided however you left it.")]
    Viewport {
        /// Which part of the pane (see the value list above)
        #[arg(value_parser = region_words())]
        region: String,
        /// What it holds: agent, 3d, panel, or off to empty it
        #[arg(value_parser = content_words())]
        content: String,
    },
    /// Put one of Organon's editor panels in a region's scrolling stack, or take one out
    #[command(after_help = "A region assigned `panel` holds a SCROLLING COLUMN of Organon's own \
                            editor panels, and this is what fills it. Two commands, not one: \
                            `console viewport left panel` says a region is for panels, then \
                            `console stack add surface` says which panel. That split is why \
                            neither sentence ever needs a third word.\n\n\
                            📌 The region's size is independent of how many panels are in it — \
                            a top-left corner scrolls twenty exactly as a full-height column \
                            does.\n\n\
                            🚨 There is ONE stack, console-wide, and every region holding \
                            `panel` is a view of it. Two such regions are two views of one \
                            instrument (they scroll independently, and hold the same panels), \
                            which is the same rule that gives the console one parameter mirror \
                            rather than one per card.\n\n\
                            `remove` takes out the LAST copy of a panel — the one you just \
                            added. `remove all` empties the column; `all` is a word for the \
                            whole column and not a panel, so `add all` is refused by name.\n\n\
                            ⚠️ Only Look ▸ Surface has real controls. The other twenty-four \
                            panels are named, addressable, and open a line saying they have \
                            not been transplanted into the console yet — which is honest, and \
                            visible in the column beside one that has been.\n\n\
                            ⚠️ Refused if NO region holds `panel`: a column nothing is showing \
                            is a command that appears to work and changes no pixel. It is also \
                            NOT remembered — a console opens with an empty stack however you \
                            left it.")]
    Stack {
        /// add or remove
        #[arg(value_parser = stack_actions())]
        action: String,
        /// Which panel — a Look-tab slug, or `all` with `remove` to empty the column
        #[arg(value_parser = stack_panels())]
        panel: String,
    },
    /// Reserve a run of blank rows in the transcript — a hole that scrolls with the text
    #[command(after_help = "The rows are opened in the ACTIVE tab, just below the cursor, and \
                            the next prompt lands underneath them. They are ordinary \
                            scrollback rows: they age, scroll and evict like any other. \
                            Nothing is painted into them yet — this reserves the space.")]
    Block {
        /// How many rows to open
        #[arg(value_parser = clap::value_parser!(u16).range(1..=cli::MAX_BLOCK_ROWS as i64))]
        rows: u16,
    },
    /// Claim a rectangle you already left in your own output — the console only records it
    #[command(after_help = "Print your text with a gap in it — ordinary blank lines, ordinary \
                            stdout — then say where the gap is. `--up` counts back from the \
                            line you are on now. The console writes NOTHING: it records the \
                            rectangle and paints it. This is the correct verb; `block` has \
                            the console open the rows itself, which lands them between a \
                            prompt and the typing and is wrong wherever anything is waiting \
                            for input.\n\n`--kind` says what sort of thing belongs in the \
                            rectangle — a name the console resolves, never a command and \
                            never a path. It defaults to the kind the verb shipped with, so \
                            a claim written without one is unchanged.")]
    Patch {
        /// How many lines above the current line the rectangle's first row sits
        #[arg(long, value_parser = clap::value_parser!(u16).range(0..=cli::MAX_BLOCK_ROWS as i64))]
        up: u16,
        /// How many rows tall
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=cli::MAX_BLOCK_ROWS as i64))]
        rows: u16,
        /// What the console draws in it (see the value list above)
        #[arg(long, default_value = "scene", value_parser = patch_kinds())]
        kind: String,
    },
    /// Open or close the portal — a live window onto the world, floating over the transcript
    #[command(after_help = "The portal holds its place on SCREEN: the transcript scrolls past \
                            underneath it and it does not scroll away. It shows the world, so \
                            `organon set` / `generator` / `recipe` typed in this same console \
                            drive what is inside it. Drag it to orbit, wheel over it to zoom — \
                            neither moves the text behind it.\n\nIt occludes the rows it \
                            floats over, which is what floating means; `close` gives them \
                            back. While it is open the backdrop does not paint, so the engine \
                            is asked for one frame per frame and no more.")]
    Portal {
        /// open, close, or toggle
        #[arg(value_parser = portal_words())]
        state: String,
    },
    /// Move the viewer's viewpoint on the portal — yaw, pitch and distance
    #[command(after_help = "This is where you STAND, not what the world does. `organon set \
                            cam_path 1` spins the composition; this walks around it. The two \
                            compose — a shot framed here still spins if `cam_path` says to.\n\n\
                            All three are absolute, in the units the drag and the wheel \
                            already write: yaw and pitch in RADIANS (yaw ±6.283 = one turn \
                            either way, pitch ±1.5 ≈ straight down to straight up), distance \
                            in world units (0.1–4000; the default view sits at 520). A value \
                            outside its band is refused rather than clamped — it is far more \
                            often a unit mistake than an overshoot.\n\n`--reset` returns to \
                            the framing the window opened with, and is applied BEFORE the \
                            others, so `--reset --distance 40` means \"default view, then \
                            pull in\".\n\n🚨 The hand always wins: while you are dragging or \
                            wheeling the portal — and for two seconds after — a camera \
                            command is dropped, and the console says so on its own stderr.")]
    Camera {
        /// Back to the framing the window opened with (applied first)
        #[arg(long)]
        reset: bool,
        /// Absolute yaw, in radians (±6.283)
        #[arg(long, allow_negative_numbers = true)]
        yaw: Option<f32>,
        /// Absolute pitch, in radians (±1.5)
        #[arg(long, allow_negative_numbers = true)]
        pitch: Option<f32>,
        /// Absolute distance from the pivot, in world units (0.1–4000)
        #[arg(long)]
        distance: Option<f32>,
    },
}

/// Map the clap surface onto the library's command model (validation included).
///
/// ⚠️ **`Console` is deliberately absent from this mapping** — see the `unreachable!` arm.
fn to_ctl(cmd: Cmd) -> Result<cli::CtlCmd, String> {
    Ok(match cmd {
        Cmd::Status { json } => cli::CtlCmd::Status { json },
        Cmd::Catalog { json, verbose } => cli::CtlCmd::Catalog { json, verbose },
        Cmd::Describe { query } => cli::CtlCmd::Describe { query },
        Cmd::Recipes { json } => cli::CtlCmd::Recipes { json },
        Cmd::Recipe { which, dry_run } => cli::CtlCmd::Recipe { name: which, dry_run },
        Cmd::Get { id, all, json } => match (id, all) {
            (Some(id), false) => cli::CtlCmd::Get { id: Some(id), json },
            (None, true) => cli::CtlCmd::Get { id: None, json },
            _ => return Err("get wants exactly one <ID>, or --all".to_string()),
        },
        Cmd::Watch { ms, fields } => {
            cli::validate_fields(&fields)?;
            cli::CtlCmd::Watch { ms: ms.max(16), fields }
        }
        Cmd::Set { pairs } => cli::CtlCmd::Set { pairs: cli::pairs_from(&pairs)? },
        Cmd::Do { plan } => cli::CtlCmd::Do { json: cli::normalize_plan(&plan)? },
        Cmd::Release { id } => cli::CtlCmd::Release { id },
        Cmd::Generator { which } => cli::CtlCmd::Generator { which },
        Cmd::Surface { which } => cli::CtlCmd::Surface { which },
        Cmd::Material { which } => cli::CtlCmd::Material { which },
        // `Console` sits here rather than gaining an arm above because it never becomes a
        // `CtlCmd`. `CtlCmd` is the *world's* command model — `ops_for` turns it into
        // `CliOp`s bound for `cli.txt`, which the World drains (brief R3). A console verb
        // has a different reader and a different channel, so giving it a `CtlCmd` would
        // mean inventing a variant that `ops_for` must then remember to return `None` for
        // — a silent-failure shape, one forgotten match arm from writing a backdrop
        // command into a queue nothing that can act on it will ever read.
        Cmd::Completions { .. }
        | Cmd::Snap { .. }
        | Cmd::Record { .. }
        | Cmd::Docs { .. }
        | Cmd::Console { .. } => {
            unreachable!("handled before mapping")
        }
    })
}

/// A unique, single-token request id for one eyes command. Runtime `SystemTime` +
/// pid is plenty here (one process, one request), and it needs no shared counter.
fn make_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// Append one line to a channel file (creating it), for the eyes request channel.
fn append_line(path: std::path::PathBuf, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(format!("{line}\n").as_bytes())
}

/// Resolve a path to absolute (against the CLI's cwd) WITHOUT requiring it to exist —
/// the visual runs from a different working directory, so a relative path would land
/// in the wrong place. Does not canonicalize (the file isn't written yet).
fn absolutize(p: &std::path::Path) -> std::path::PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Write (or, with `check`, verify) the generated Markdown reference.
///
/// The default destination is the repo checkout this binary was built from, which is the
/// only case that can be inferred — an installed `organon` on someone else's machine has
/// no reference tree to update, so it is asked for `--out` rather than silently writing
/// somewhere surprising. Exit codes: 0 ok · 1 an I/O failure · 2 no destination · 3 drift
/// found under `--check`.
fn write_docs(out: Option<std::path::PathBuf>, check: bool) {
    let dir = match out {
        Some(d) => absolutize(&d),
        None => {
            let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|r| r.join(cli::DOCS_DIR));
            match d.filter(|d| d.is_dir()) {
                Some(d) => d,
                None => {
                    eprintln!(
                        "organon: no default docs directory here — pass --out <DIR>.\n\
                         (The default is the `{}` of the checkout this binary was built \
                         from, which only exists on a development machine.)",
                        cli::DOCS_DIR
                    );
                    std::process::exit(2);
                }
            }
        }
    };

    let mut stale = Vec::new();
    for (name, want) in cli::docs_files() {
        let path = dir.join(name);
        // Content, not bytes: a Windows checkout is CRLF (see `cli::docs_match`). A
        // byte compare here would report every page stale and rewrite all of them on
        // every run, on the one platform least able to tell that was wrong.
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref().is_some_and(|c| cli::docs_match(c, &want)) {
            continue;
        }
        if check {
            stale.push(path.display().to_string());
            continue;
        }
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("organon: cannot create {}: {e}", parent.display());
                std::process::exit(1);
            }
        }
        if let Err(e) = std::fs::write(&path, &want) {
            eprintln!("organon: cannot write {}: {e}", path.display());
            std::process::exit(1);
        }
        println!("wrote {}", path.display());
    }

    if check {
        if stale.is_empty() {
            println!("doc/reference is current");
        } else {
            eprintln!("organon: stale reference pages (run `organon docs` and commit):");
            for p in &stale {
                eprintln!("  {p}");
            }
            std::process::exit(3);
        }
    }
}

/// Issue one eyes request (`snap`/`record`) and block for the reply, polling the reply
/// channel for our nonce. Exit codes: 0 ok · 1 the visual reported an error · 3 no reply
/// within `timeout` (the visual isn't running, or is wedged).
fn run_eyes(req: cli::EyesReq, timeout: std::time::Duration) -> ! {
    let nonce = make_nonce();
    if let Err(e) = append_line(ipc::eyes_cmd_path(), &req.to_line(&nonce)) {
        eprintln!("organon: cannot write the eyes command channel: {e}");
        std::process::exit(3);
    }
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let body = std::fs::read_to_string(ipc::eyes_reply_path()).unwrap_or_default();
        if let Some(res) = cli::find_eyes_reply(&body, &nonce) {
            match res {
                Ok(path) => {
                    println!("{path}");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("organon: {e}");
                    std::process::exit(1);
                }
            }
        }
        if start.elapsed() >= timeout {
            eprintln!(
                "organon: no response from the visual within {:.0}s — is Organon's visual \
                 window open? (reads are live only while it runs)",
                timeout.as_secs_f32()
            );
            std::process::exit(3);
        }
    }
}

/// Queue one console op on the console sidecar and exit (#4 Tier 2). Fire-and-forget:
/// the console drains `cli::console_cmd_path()` on its next frame.
///
/// Exit codes: **0** on a successful append · **1** on an I/O failure. A bad name never
/// reaches here — clap rejects it at parse time with the usual **2**, with "did you mean"
/// for free, which is why the name is a plain `String` by the time we see it.
///
/// **The op-path's `is_live` warning is deliberately absent here, and that is not an
/// oversight.** Below, a queued `CliOp` prints "no live Organon snapshot detected" when
/// `ipc::Reader::open().is_live()` is false. That heuristic is about the **World** lane
/// (brief R3): it probes the `Shared` mmap's `seq` counter for *motion*, so what it really
/// measures is redraw cadence, not existence. The console publishes `Shared` every redraw
/// (`console_main.rs`), so a console idling between repaints can read "dead" while it is
/// plainly alive and about to drain this very line. Printing "your command was dropped" at
/// the moment it is being honoured is worse than saying nothing. The probe also costs up
/// to ~150 ms per invocation, which a console verb — the one people will hold a key down
/// on — should not pay.
fn run_console(action: ConsoleAction) -> ! {
    let op = match action {
        ConsoleAction::Background { name } => cli::ConsoleOp::Background(name),
        ConsoleAction::Rig { name } => cli::ConsoleOp::Rig(name),
        // clap has already restricted this to `Theme::NAMES`, so it travels as typed; the
        // console resolves it again on arrival, which is the gate that matters for a line
        // written straight onto the sidecar by hand.
        ConsoleAction::Theme { name } => cli::ConsoleOp::Theme(name),
        // 🚨 **The only console argument validated here rather than by clap**, because its
        // value space is two words *or* a number and `PossibleValuesParser` cannot say that.
        // Resolving now — and throwing the resolved value away — is deliberate: it is the
        // difference between a human seeing `sideways is not a posture` here, and seeing
        // `queued: posture sideways` followed by silence from a window they may not be
        // looking at. The word travels rather than the scalar so the console's own refusal
        // can quote what was typed.
        ConsoleAction::Posture { word } => match posture::Posture::resolve(&word) {
            Ok(_) => cli::ConsoleOp::Posture(word),
            Err(e) => {
                eprintln!("organon: {e}");
                std::process::exit(2);
            }
        },
        // clap has already restricted this to `SCREEN_WORDS`, so it travels as typed — the
        // `Theme` arm's arrangement, not the `Posture` arm's, because this value space is a
        // closed list and `PossibleValuesParser` can say so. The console resolves it again on
        // arrival, which is the gate for a line written straight onto the sidecar by hand.
        ConsoleAction::Screen { state } => cli::ConsoleOp::Screen(state),
        // Both words travel as typed, the `Screen` arm's arrangement: clap has already
        // restricted each to its own closed table, and the console resolves them again on
        // arrival — which is the gate that matters for a line written straight onto the
        // sidecar by hand. Nothing is checked *between* them here on purpose: whether this
        // region may hold this content depends on what the console is holding right now, which
        // is state only the console has.
        ConsoleAction::Viewport { region, content } => {
            cli::ConsoleOp::Viewport { region, content }
        }
        // The `Viewport` arm's arrangement exactly, and its reason in full: clap has restricted
        // each word to its own closed table, the console resolves them again on arrival — which
        // is the gate that matters for a line written straight onto the sidecar by hand — and
        // nothing is checked *between* them here, because whether the column can honour this
        // depends on what the console is holding right now.
        ConsoleAction::Stack { action, panel } => cli::ConsoleOp::Stack { action, panel },
        ConsoleAction::Block { rows } => cli::ConsoleOp::Block(rows),
        // clap has already restricted `kind` to `kind::KIND_WORDS`, so `from_word` cannot miss
        // here; the fallback rather than an `expect` because it is not a guess — it is the
        // same default a kindless sidecar line resolves to, so both spellings of "no kind
        // stated" land on one answer. It names `cli::PATCH_DEFAULT_KIND` explicitly because
        // that default is this lane's, not the vocabulary's: `Kind` has no `Default` to reach
        // for, which is what stops the patch wire's history leaking into the other front-end.
        ConsoleAction::Patch { up, rows, kind } => cli::ConsoleOp::Patch {
            up,
            rows,
            kind: kind::Kind::from_word(&kind).unwrap_or(cli::PATCH_DEFAULT_KIND),
        },
        // 🚨 No fallback twin of the line above, and the asymmetry is the point: a patch has a
        // default because a kindless line is an older spelling of `scene`, whereas there is
        // no state a portal command silently means. clap has already
        // restricted the word to `cli::PORTAL_WORDS`, so this cannot miss — and if it somehow
        // did, refusing beats toggling a window off because of a typo.
        ConsoleAction::Portal { state } => match cli::PortalCmd::from_word(&state) {
            Some(cmd) => cli::ConsoleOp::Portal(cmd),
            None => {
                eprintln!(
                    "organon: `{state}` is not a portal state — expected one of {:?}",
                    cli::PORTAL_WORDS
                );
                std::process::exit(2);
            }
        },
        // 🚨 **Both checks are here, at the clap boundary, and neither could be there.** clap
        // can require *an* argument but not "at least one of four", and `value_parser!(f32)`
        // carries no range (`.range()` is the integer parser's). So this is where a human gets
        // the good error, before a byte is written — the console's own schema is the second
        // gate, for a line hand-written straight onto the sidecar, and it reports through a
        // dispatch record instead of a terminal.
        ConsoleAction::Camera { reset, yaw, pitch, distance } => {
            let framing = cli::CameraFraming { reset, yaw, pitch, distance };
            if framing.is_empty() {
                eprintln!(
                    "organon: `console camera` needs at least one of {:?} — it moves the \
                     viewpoint, so a command that names no axis would be a no-op",
                    cli::CAMERA_WORDS
                );
                std::process::exit(2);
            }
            if !framing.in_range() {
                eprintln!(
                    "organon: a camera axis is out of range — yaw ±{:.3} rad, pitch ±{:.1} \
                     rad, distance {}–{} world units. Refused rather than clamped: a value \
                     this far out is usually a unit mistake, and a silent clamp would let it \
                     look like it worked.",
                    scene_input::YAW_LIMIT,
                    scene_input::PITCH_LIMIT,
                    scene_input::DISTANCE_MIN,
                    scene_input::DISTANCE_MAX,
                );
                std::process::exit(2);
            }
            cli::ConsoleOp::Camera(framing)
        }
    };
    if let Err(e) = cli::append_console_ops(std::slice::from_ref(&op)) {
        eprintln!(
            "organon: cannot write the console command channel ({}): {e}",
            cli::console_cmd_path().display()
        );
        std::process::exit(1);
    }
    println!("queued: {}", cli::console_op_to_line(&op));
    std::process::exit(0);
}

fn main() {
    let parsed = Cli::parse(); // --help/-V/usage errors exit here (code 2)

    if let Cmd::Completions { shell } = parsed.cmd {
        clap_complete::generate(shell, &mut Cli::command(), "organon", &mut std::io::stdout());
        return;
    }

    if let Cmd::Docs { out, check } = parsed.cmd {
        write_docs(out, check);
        return;
    }

    // #4 Tier 2: the console lane. It branches HERE, beside the other early exits, rather
    // than falling through to `to_ctl` — a console verb never becomes a `CtlCmd` because it
    // addresses the console's own state on its own sidecar, not the World's on `cli.txt`
    // (brief R3). Branching before the mapping is what keeps that structural instead of
    // conventional.
    if let Cmd::Console { action } = parsed.cmd {
        run_console(action);
    }

    // #452 Tier 3 ("the eyes"): snap/record ride a request+reply channel (the visual does
    // the GPU work and hands a path back), not the fire-and-forget CliOp lane.
    match parsed.cmd {
        Cmd::Snap { out } => {
            let path = out.unwrap_or_else(|| {
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                std::path::PathBuf::from(format!("organon-snap-{ms}.png"))
            });
            let path = absolutize(&path).display().to_string();
            run_eyes(cli::EyesReq::Snap { path }, std::time::Duration::from_secs(10));
        }
        Cmd::Record { action } => {
            let (req, timeout) = match action {
                RecordAction::Start { bars } => (
                    cli::EyesReq::RecordStart { bars },
                    std::time::Duration::from_secs(10),
                ),
                // `finish` blocks on the ffmpeg flush + optional audio mux — allow longer.
                RecordAction::Stop => {
                    (cli::EyesReq::RecordStop, std::time::Duration::from_secs(60))
                }
            };
            run_eyes(req, timeout);
        }
        _ => {}
    }

    let cmd = match to_ctl(parsed.cmd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("organon: {e}");
            std::process::exit(2);
        }
    };

    // Write commands: queue ops on the command channel and report. A missing
    // live snapshot is only a warning — but note the visual's seed rule means
    // commands written while it is down are dropped at its next start.
    match cli::ops_for(&cmd) {
        Err(e) => {
            eprintln!("organon: {e}");
            std::process::exit(2);
        }
        Ok(Some(ops)) => {
            if let Err(e) = cli::append_ops(&ops) {
                eprintln!("organon: cannot write command channel: {e}");
                std::process::exit(1);
            }
            for op in &ops {
                println!("queued: {}", op.to_line());
            }
            if !ipc::Reader::open().is_live() {
                eprintln!(
                    "organon: warning — no live Organon snapshot detected; queued \
                     commands are NOT replayed when the visual starts later"
                );
            }
            return;
        }
        Ok(None) => {}
    }

    // Read commands.
    let reader = ipc::Reader::open();
    let live = reader.is_live();
    let require_live = || {
        if !live {
            eprintln!(
                "organon: no live Organon snapshot (visual/plugin not running, or a \
                 layout-version mismatch — rebuild/redeploy so both sides match)"
            );
            std::process::exit(3);
        }
    };

    match cmd {
        cli::CtlCmd::Catalog { json, verbose } => {
            // The vocabulary works offline; current values appear when live.
            let s = live.then(|| reader.read());
            if json {
                println!("{}", cli::catalog_json(s.as_ref()));
            } else {
                print!("{}", cli::catalog_text(s.as_ref(), verbose));
            }
        }
        cli::CtlCmd::Describe { query } => {
            // Description prose works offline; the current value appears when live.
            let s = live.then(|| reader.read());
            match cli::describe_text(s.as_ref(), &query) {
                Ok(out) => print!("{out}"),
                Err(e) => {
                    eprintln!("organon: {e}");
                    std::process::exit(2);
                }
            }
        }
        cli::CtlCmd::Status { json } => {
            require_live();
            let s = reader.read();
            if json {
                println!("{}", cli::status_json(&s));
            } else {
                print!("{}", cli::status_text(&s));
            }
        }
        cli::CtlCmd::Get { id, json } => {
            require_live();
            let s = reader.read();
            match cli::get_output(&s, id.as_deref(), json) {
                Ok(out) => print!("{}{}", out, if out.ends_with('\n') { "" } else { "\n" }),
                Err(e) => {
                    eprintln!("organon: {e}");
                    std::process::exit(2);
                }
            }
        }
        cli::CtlCmd::Watch { ms, fields } => {
            require_live();
            // One JSON line per tick until interrupted (Bianca's feedback loop).
            // Liveness follows the STREAM (review finding): if `seq` freezes for
            // ~2s the writer is gone — end the stream with exit 3 instead of
            // emitting stale state forever (an agent sees the stream end, not a
            // silent flatline).
            let stale_ticks = (2000 / ms.max(16)).max(2);
            let mut last_seq: Option<u32> = None;
            let mut frozen: u64 = 0;
            loop {
                let s = reader.read();
                if last_seq == Some(s.seq) {
                    frozen += 1;
                    if frozen >= stale_ticks {
                        eprintln!(
                            "organon: snapshot went stale (writer stopped) — ending watch"
                        );
                        std::process::exit(3);
                    }
                } else {
                    frozen = 0;
                    last_seq = Some(s.seq);
                }
                println!("{}", cli::watch_line(&s, &fields));
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        cli::CtlCmd::Recipes { json } => {
            // The recipe library is built-in knowledge — works fully offline.
            print!("{}", cli::recipes_text(json));
        }
        cli::CtlCmd::Recipe { name, dry_run } => {
            let ops = match cli::recipe_ops(&name) {
                Ok(ops) => ops,
                Err(e) => {
                    eprintln!("organon: {e}");
                    std::process::exit(2);
                }
            };
            if dry_run {
                // Show exactly what it would do; change nothing.
                print!("{}", cli::recipe_detail(&name).unwrap_or_default());
                return;
            }
            if let Err(e) = cli::append_ops(&ops) {
                eprintln!("organon: cannot write command channel: {e}");
                std::process::exit(1);
            }
            println!("applied recipe '{name}' ({} ops)", ops.len());
            if !ipc::Reader::open().is_live() {
                eprintln!(
                    "organon: warning — no live Organon snapshot detected; queued commands are \
                     NOT replayed when the visual starts later"
                );
            }
        }
        // Write commands were fully handled above.
        _ => unreachable!("write command fell through ops_for"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("organon").chain(args.iter().copied()))
    }

    /// Nine catalogued params have negative ranges — `exposure`, `elevation`, `azimuth` and
    /// the six `rot_mod_*` / `trans_mod_*` axes. Every one of them was unreachable through
    /// the documented `organon set <id> <value>` syntax until `allow_negative_numbers`,
    /// because clap read `-3.0` as an unknown flag and refused the command outright.
    ///
    /// This pins the plain form. The `--` separator still works and is still correct; it is
    /// no longer the *only* thing that works.
    #[test]
    fn set_accepts_negative_values_without_a_dash_dash() {
        let c = parse(&["set", "exposure", "-3.0"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Set { pairs: vec![("exposure".into(), -3.0)] }
        );

        // Mixed signs in one command, and a bare `-4` without a decimal point.
        let c = parse(&["set", "exposure", "-4", "glow", "1.5", "elevation", "-0.25"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Set {
                pairs: vec![
                    ("exposure".into(), -4.0),
                    ("glow".into(), 1.5),
                    ("elevation".into(), -0.25),
                ]
            }
        );

        // The escape hatch the trial had to use keeps working — it is documented in SKILL.md
        // and in shell history everywhere; breaking it to fix the plain form would be a trade.
        let c = parse(&["set", "--", "exposure", "-3.0"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Set { pairs: vec![("exposure".into(), -3.0)] }
        );
    }

    #[test]
    fn clap_surface_is_wellformed_and_maps_to_ctl() {
        // clap's own self-check (catches conflicting flags/ids at test time).
        Cli::command().debug_assert();

        let c = parse(&["get", "metallic", "--json"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Get { id: Some("metallic".into()), json: true }
        );
        let c = parse(&["get", "--all"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Get { id: None, json: false });
        let c = parse(&["set", "metallic", "0.9", "glow", "1.5"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Set { pairs: vec![("metallic".into(), 0.9), ("glow".into(), 1.5)] }
        );
        let c = parse(&["watch", "--ms", "100", "--fields", "glow,metallic"]).unwrap();
        assert_eq!(
            to_ctl(c.cmd).unwrap(),
            cli::CtlCmd::Watch { ms: 100, fields: vec!["glow".into(), "metallic".into()] }
        );
        let c = parse(&["gen", "dna"]).unwrap(); // alias
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Generator { which: "dna".into() });
        let c = parse(&["describe", "metallic"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Describe { query: "metallic".into() });
        let c = parse(&["recipes"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Recipes { json: false });
        let c = parse(&["recipe", "helix"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Recipe { name: "helix".into(), dry_run: false });
        let c = parse(&["recipe", "helix", "--dry-run"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Recipe { name: "helix".into(), dry_run: true });
        let c = parse(&["catalog", "--verbose"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Catalog { json: false, verbose: true });
        let c = parse(&["catalog", "--manual"]).unwrap(); // alias
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Catalog { json: false, verbose: true });
        let c = parse(&["release"]).unwrap();
        assert_eq!(to_ctl(c.cmd).unwrap(), cli::CtlCmd::Release { id: None });

        // clap rejects: unknown param ids on completing args, unknown commands,
        // ids alongside --all.
        assert!(parse(&["get", "nonsense"]).is_err());
        assert!(parse(&["get", "metallic", "--all"]).is_err());
        assert!(parse(&["release", "nonsense"]).is_err());
        assert!(parse(&["watch", "--fields", "nonsense"]).is_err());
        assert!(parse(&["frobnicate"]).is_err());
        // Mapping rejects: bad pairs / a bare `get`.
        assert!(to_ctl(parse(&["set", "metallic"]).unwrap().cmd).is_err());
        assert!(to_ctl(parse(&["get"]).unwrap().cmd).is_err());
    }

    /// `organon console <verb> <NAME>` parses, and every name in the two vocabularies is
    /// accepted while anything else is refused by clap (exit 2, "did you mean" for free).
    ///
    /// This is the whole validation story for the console lane: nothing downstream checks
    /// the name again, because clap is the only place that can produce a good error for it.
    #[test]
    fn console_subcommand_validates_its_vocabularies() {
        for name in CONSOLE_MATERIALS.iter().chain(CONSOLE_SOURCES.iter()).copied() {
            let c = parse(&["console", "background", name]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Background { name: got } } => {
                    assert_eq!(got, name)
                }
                _ => panic!("`console background {name}` parsed as something else"),
            }
        }
        for name in CONSOLE_RIGS.iter().copied() {
            let c = parse(&["console", "rig", name]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Rig { name: got } } => {
                    assert_eq!(got, name)
                }
                _ => panic!("`console rig {name}` parsed as something else"),
            }
        }

        // clap rejects unknown names, a missing name, and an unknown console verb.
        assert!(parse(&["console", "background", "nonsense"]).is_err());
        assert!(parse(&["console", "rig", "nonsense"]).is_err());
        // The two vocabularies are separate: a rig is not a background and vice versa.
        assert!(parse(&["console", "background", "studio"]).is_err());
        assert!(parse(&["console", "rig", "slate"]).is_err());
        assert!(parse(&["console", "background"]).is_err());
        assert!(parse(&["console"]).is_err());
        assert!(parse(&["console", "frobnicate", "x"]).is_err());
    }

    /// CONTRACT: **every palette in `Theme::NAMES` is typeable, and nothing else is.**
    ///
    /// The list is read from the table rather than restated, unlike `CONSOLE_MATERIALS` and
    /// `CONSOLE_RIGS` above — so this test does not pin the four names, it pins that
    /// `--help`'s value list and the console's own resolver are the *same* list. A fifth
    /// palette therefore arrives in the CLI for free, and a palette removed from the table
    /// stops being typeable in the same commit.
    #[test]
    fn console_theme_offers_exactly_the_palettes_that_exist() {
        for name in theme::Theme::NAMES {
            let c = parse(&["console", "theme", name]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Theme { name: got } } => {
                    assert_eq!(got, name)
                }
                _ => panic!("`console theme {name}` parsed as something else"),
            }
            // …and the console can actually paint it. This is the assertion that would have
            // caught a name in the table with no `by_name` arm behind it.
            assert!(theme::Theme::by_name(name).is_some(), "`{name}` is offered but unpaintable");
        }
        assert!(parse(&["console", "theme", "phosphor"]).is_err(), "not a palette this build has");
        assert!(parse(&["console", "theme", "Light"]).is_err(), "case is not folded anywhere");
        assert!(parse(&["console", "theme"]).is_err(), "a palette verb with no palette");
    }

    /// CONTRACT: **`console posture` accepts a word or a scalar, and clap accepts BOTH
    /// spellings through to `run_console`.**
    ///
    /// ⚠️ The refusal for this one verb happens *after* clap — the value space is two words
    /// or a number and `PossibleValuesParser` cannot say that — so `parse` succeeding on
    /// `sideways` is the correct behaviour here rather than a hole. What closes it is
    /// `Posture::resolve` in `run_console`, and this pins that the two ends of that
    /// arrangement agree about which strings are good.
    #[test]
    fn console_posture_takes_a_word_or_a_scalar_and_refuses_after_clap() {
        for word in posture::POSTURE_WORDS.iter().copied().chain(["0", "0.5", "1", "1.0"]) {
            let c = parse(&["console", "posture", word]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Posture { word: got } } => {
                    assert_eq!(got, word)
                }
                _ => panic!("`console posture {word}` parsed as something else"),
            }
            assert!(posture::Posture::resolve(word).is_ok(), "`{word}` must resolve");
        }
        // clap lets these through; `run_console` is what exits 2 on them. `90` is the case
        // worth naming: degrees where the axis wanted a fraction, which a clamp would have
        // answered with `desktop` and called a success.
        for bad in ["sideways", "1.5", "90", "nan", "inf"] {
            assert!(parse(&["console", "posture", bad]).is_ok(), "clap does not gate `{bad}`");
            assert!(posture::Posture::resolve(bad).is_err(), "`{bad}` must be refused");
        }
        // A leading `-` is clap's business, not the resolver's: `-0.1` never reaches
        // `Posture::resolve` because clap reads it as an unknown flag first. Refused either
        // way, which is all that matters — but by a different gate, so it is asserted apart.
        assert!(posture::Posture::resolve("-0.1").is_err());
        assert!(parse(&["console", "posture"]).is_err(), "a posture verb with no posture");
    }

    /// CONTRACT: **`console screen` is gated by clap itself, which is the contrast the verb
    /// above exists to be read against.** A posture's value space is two words *or* a scalar,
    /// so `PossibleValuesParser` cannot state it and the gate has to move to `run_console`. A
    /// screen state is three words and nothing between them — a window either covers the
    /// display or it does not — so the whole value space is expressible, the check happens
    /// before a byte is written, and the words tab-complete.
    ///
    /// ⚠️ The refusals below are asserted at the **clap** boundary, not the resolver's. That is
    /// the point: `screen sideways` must never reach the sidecar at all, whereas
    /// `posture sideways` does reach `run_console` and is stopped there.
    #[test]
    fn console_screen_is_a_closed_word_list_clap_can_state() {
        for word in screen::SCREEN_WORDS.iter().copied() {
            let c = parse(&["console", "screen", word]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Screen { state: got } } => {
                    assert_eq!(got, word)
                }
                _ => panic!("`console screen {word}` parsed as something else"),
            }
            assert!(screen::ScreenCmd::resolve(word).is_ok(), "`{word}` must resolve");
        }
        // Refused by clap, unlike the posture verb's equivalents. `fullscreen` is the case
        // worth naming: it is the word somebody reaches for first, and the console reserves
        // that phrase for a portal state, so it must fail loudly rather than near-match.
        for bad in ["fullscreen", "on", "off", "Full", "0", "sideways"] {
            assert!(parse(&["console", "screen", bad]).is_err(), "clap must gate `{bad}`");
            assert!(screen::ScreenCmd::resolve(bad).is_err(), "`{bad}` must be refused");
        }
        assert!(parse(&["console", "screen"]).is_err(), "a screen verb with no state");
    }

    /// **`console block` is bounded at the clap boundary, which is the only place a row count
    /// can produce a good error.** The sidecar skips a malformed line in silence by design, so
    /// a count that slipped past here would become a command that vanishes — or, without the
    /// upper bound, a single word that pushes a fifth of the scrollback out of the buffer.
    #[test]
    fn console_block_takes_a_bounded_row_count() {
        for rows in [1u16, 12, cli::MAX_BLOCK_ROWS] {
            let c = parse(&["console", "block", &rows.to_string()]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Block { rows: got } } => {
                    assert_eq!(got, rows)
                }
                _ => panic!("`console block {rows}` parsed as something else"),
            }
        }
        assert!(parse(&["console", "block", "0"]).is_err(), "a block of nothing is a typo");
        assert!(parse(&["console", "block", &(cli::MAX_BLOCK_ROWS + 1).to_string()]).is_err());
        assert!(parse(&["console", "block", "-3"]).is_err());
        assert!(parse(&["console", "block", "12.5"]).is_err());
        assert!(parse(&["console", "block", "lots"]).is_err());
        assert!(parse(&["console", "block"]).is_err(), "the count is not optional");
    }

    /// **`console patch` names a kind, and the kind is optional in exactly one direction.**
    /// Omitting it must keep meaning what the verb meant before kinds existed — the arm that
    /// is already verified on screen — while a word the console cannot resolve has to fail
    /// *here*, at the clap boundary, since the sidecar's own answer to an unknown kind is to
    /// skip the line in silence.
    #[test]
    fn console_patch_names_a_kind_and_defaults_to_the_one_it_shipped_with() {
        let c = parse(&["console", "patch", "--up", "12", "--rows", "12"]).unwrap();
        match c.cmd {
            Cmd::Console { action: ConsoleAction::Patch { up, rows, kind } } => {
                assert_eq!((up, rows), (12, 12));
                assert_eq!(kind::Kind::from_word(&kind), Some(cli::PATCH_DEFAULT_KIND));
            }
            _ => panic!("`console patch` parsed as something else"),
        }
        for word in kind::KIND_WORDS {
            let c =
                parse(&["console", "patch", "--up", "0", "--rows", "8", "--kind", word]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Patch { kind, .. } } => {
                    assert_eq!(kind, *word)
                }
                _ => panic!("`console patch --kind {word}` parsed as something else"),
            }
        }
        assert!(
            parse(&["console", "patch", "--up", "0", "--rows", "8", "--kind", "hologram"])
                .is_err(),
            "a kind nothing can draw must fail where the error can be good"
        );
        // The two counts stay required and stay bounded — a kind does not soften them.
        assert!(parse(&["console", "patch", "--rows", "8"]).is_err());
        assert!(parse(&["console", "patch", "--up", "0"]).is_err());
        assert!(parse(&["console", "patch", "--up", "0", "--rows", "0"]).is_err());
        assert!(parse(&[
            "console",
            "patch",
            "--up",
            "0",
            "--rows",
            &(cli::MAX_BLOCK_ROWS + 1).to_string()
        ])
        .is_err());
    }

    /// **`console portal` takes a state, and that state is not optional.** Every word in
    /// `cli::PORTAL_WORDS` reaches an op and round-trips through the sidecar's own line form;
    /// anything else fails here, at the clap boundary, because the sidecar's answer to a word
    /// it cannot resolve is to skip the line in silence — and a portal command that vanishes
    /// looks exactly like a portal that failed to render.
    ///
    /// ⚠️ The bare `console portal` case is the one worth pinning: unlike `patch --kind` there
    /// is no state it silently means, so it must be an error rather than a default.
    #[test]
    fn console_portal_takes_a_state_and_has_no_default_one() {
        for word in cli::PORTAL_WORDS {
            let c = parse(&["console", "portal", word]).unwrap();
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Portal { state } } => {
                    assert_eq!(&state, word);
                    let cmd = cli::PortalCmd::from_word(&state)
                        .unwrap_or_else(|| panic!("`{word}` is in the table but resolves to nothing"));
                    assert_eq!(
                        cli::parse_console_op(&cli::console_op_to_line(&cli::ConsoleOp::Portal(
                            cmd
                        ))),
                        Some(cli::ConsoleOp::Portal(cmd)),
                        "`portal {word}` must survive the sidecar round trip"
                    );
                }
                _ => panic!("`console portal {word}` parsed as something else"),
            }
        }
        assert!(parse(&["console", "portal"]).is_err(), "there is no default state");
        assert!(parse(&["console", "portal", "ajar"]).is_err(), "an unknown state");
        assert!(parse(&["console", "portal", "open", "close"]).is_err(), "one state, not two");
    }

    /// **`console viewport` takes two words and neither is optional.** Every region word crosses
    /// with every content word, reaches an op, and round-trips through the sidecar's own line
    /// form — the whole cross product rather than a sample, because the cost of a pair that
    /// survives one direction only is a command the console skips in silence, which looks
    /// exactly like a split that failed to draw.
    ///
    /// ⚠️ **The bare and half-written cases are what this pins hardest.** `console viewport
    /// left` looks like a command; there is no content it silently means, so it must fail here
    /// where the error can name what is missing — not on the wire, where the answer is to drop
    /// the line.
    #[test]
    fn console_viewport_takes_a_region_and_a_content_and_defaults_neither() {
        for r in region::REGION_WORDS {
            for w in region::CONTENT_WORDS {
                let c = parse(&["console", "viewport", r, w]).unwrap();
                match c.cmd {
                    Cmd::Console { action: ConsoleAction::Viewport { region, content } } => {
                        assert_eq!(&region, r);
                        assert_eq!(&content, w);
                        let op = cli::ConsoleOp::Viewport { region, content };
                        assert_eq!(
                            cli::parse_console_op(&cli::console_op_to_line(&op)),
                            Some(op),
                            "`viewport {r} {w}` must survive the sidecar round trip"
                        );
                    }
                    _ => panic!("`console viewport {r} {w}` parsed as something else"),
                }
            }
            assert!(parse(&["console", "viewport", r]).is_err(), "`{r}` alone is half a command");
        }
        assert!(parse(&["console", "viewport"]).is_err(), "neither word has a default");
        assert!(parse(&["console", "viewport", "middle", "agent"]).is_err(), "no such region");
        // ✏️ **This line used to read `"3d"`.** Tier 2b put that word in the vocabulary, so the
        // assertion moved to `media` — the kind that is still absent (§1.13's placement question
        // owns it). It is the CLI-side twin of the same edit in `console_main.rs`, and the pair is
        // why the word list is worth exercising from both ends: this is the leg the four-leg bar
        // only *type-checks*, and a `possible_values` table that has grown a word its negative
        // assertion still denies compiles perfectly and fails only when somebody runs it.
        assert!(parse(&["console", "viewport", "left", "media"]).is_err(), "not in the vocabulary");
        assert!(parse(&["console", "viewport", "left", "agent", "right"]).is_err(), "one pair");
        // 🚨 The clap gate is the *word* tables and nothing more — whether a region MAY hold a
        // content depends on what the console is holding right now, which this process cannot
        // see. `left off` is a legal line and a refusal waiting to happen at the other end.
        assert!(parse(&["console", "viewport", "left", "off"]).is_ok());
    }

    /// **`console stack` takes two words and neither is optional** — `viewport`'s test one
    /// module over, and the whole cross product for its reason: a pair that survives one
    /// direction only is a command the console skips in silence.
    ///
    /// ⚠️ **`add all` is legal HERE and refused at the other end**, which is the same shape as
    /// `viewport left off`: clap's gate is the *word* tables and nothing more, and whether the
    /// column can honour a command depends on state this process cannot see. Pinning it as
    /// `is_ok()` is what stops somebody "fixing" it into the CLI, where the refusal would then
    /// exist in two places and could disagree.
    #[test]
    fn console_stack_takes_an_action_and_a_panel_and_defaults_neither() {
        for a in panel_stack::STACK_ACTIONS {
            for p in panel_stack::panel_words() {
                let c = parse(&["console", "stack", a, p]).unwrap();
                match c.cmd {
                    Cmd::Console { action: ConsoleAction::Stack { action, panel } } => {
                        assert_eq!(&action, a);
                        assert_eq!(panel, p);
                        let op = cli::ConsoleOp::Stack { action, panel };
                        assert_eq!(
                            cli::parse_console_op(&cli::console_op_to_line(&op)),
                            Some(op),
                            "`stack {a} {p}` must survive the sidecar round trip"
                        );
                    }
                    _ => panic!("`console stack {a} {p}` parsed as something else"),
                }
            }
            assert!(parse(&["console", "stack", a]).is_err(), "`{a}` alone is half a command");
        }
        assert!(parse(&["console", "stack"]).is_err(), "neither word has a default");
        assert!(parse(&["console", "stack", "shuffle", "surface"]).is_err(), "no such action");
        assert!(parse(&["console", "stack", "add", "nonesuch"]).is_err(), "no such panel");
        // `clear` was never an action word — the emptying word rides the panel ring. Pinned
        // because it is the obvious guess and the CLI must not half-accept it.
        assert!(parse(&["console", "stack", "clear"]).is_err(), "clear is not an action");
        assert!(parse(&["console", "stack", "add", "surface", "bloom"]).is_err(), "one pair");
    }

    /// **`console camera` takes any subset of four flags and round-trips through the sidecar.**
    /// Negative angles are the case worth pinning at the clap boundary: without
    /// `allow_negative_numbers` clap reads `-0.4` as an unknown short flag, and the failure is
    /// a usage error for a value that is not only legal but *half of the range*.
    #[test]
    fn console_camera_takes_any_subset_and_survives_the_round_trip() {
        let cases: &[(&[&str], cli::CameraFraming)] = &[
            (&["--reset"], cli::CameraFraming { reset: true, ..Default::default() }),
            (&["--yaw", "0.8"], cli::CameraFraming { yaw: Some(0.8), ..Default::default() }),
            (
                &["--pitch", "-0.4"],
                cli::CameraFraming { pitch: Some(-0.4), ..Default::default() },
            ),
            (
                &["--distance", "40"],
                cli::CameraFraming { distance: Some(40.0), ..Default::default() },
            ),
            (
                &["--reset", "--distance", "40"],
                cli::CameraFraming { reset: true, distance: Some(40.0), ..Default::default() },
            ),
            (
                &["--yaw", "-1.2", "--pitch", "0.3", "--distance", "12.5"],
                cli::CameraFraming {
                    reset: false,
                    yaw: Some(-1.2),
                    pitch: Some(0.3),
                    distance: Some(12.5),
                },
            ),
        ];
        for (argv, want) in cases {
            let mut full = vec!["console", "camera"];
            full.extend_from_slice(argv);
            let c = parse(&full).unwrap_or_else(|e| panic!("`{argv:?}` should parse: {e}"));
            match c.cmd {
                Cmd::Console { action: ConsoleAction::Camera { reset, yaw, pitch, distance } } => {
                    let got = cli::CameraFraming { reset, yaw, pitch, distance };
                    assert_eq!(got, *want, "{argv:?}");
                    assert!(got.in_range(), "{argv:?} is inside the hand's own band");
                    let op = cli::ConsoleOp::Camera(got);
                    assert_eq!(
                        cli::parse_console_op(&cli::console_op_to_line(&op)),
                        Some(op),
                        "{argv:?} must survive the sidecar round trip"
                    );
                }
                _ => panic!("`console camera {argv:?}` parsed as something else"),
            }
        }
    }

    /// ⚠️ **The two checks clap cannot make, pinned as the library predicates `run_console`
    /// calls.** clap can require *an* argument but not "at least one of four", and
    /// `value_parser!(f32)` carries no range — `.range()` belongs to the integer parser. So
    /// `console camera` with no flags PARSES, and it must not then queue a line that moves
    /// nothing; and an out-of-band value parses too, and must be refused rather than clamped.
    /// Both are `run_console`'s exit-2 arms, and both are these predicates.
    #[test]
    fn a_camera_command_with_no_axis_or_an_out_of_band_one_is_caught_after_clap() {
        let bare = parse(&["console", "camera"]).expect("clap itself accepts it");
        match bare.cmd {
            Cmd::Console { action: ConsoleAction::Camera { reset, yaw, pitch, distance } } => {
                assert!(
                    cli::CameraFraming { reset, yaw, pitch, distance }.is_empty(),
                    "…and `is_empty` is what catches it, before a byte is written"
                );
            }
            _ => panic!("`console camera` parsed as something else"),
        }
        let far = parse(&["console", "camera", "--distance", "9000"]).expect("clap accepts it");
        match far.cmd {
            Cmd::Console { action: ConsoleAction::Camera { reset, yaw, pitch, distance } } => {
                let f = cli::CameraFraming { reset, yaw, pitch, distance };
                assert!(!f.is_empty(), "it does name an axis");
                assert!(!f.in_range(), "…and `in_range` is what refuses it");
            }
            _ => panic!("`console camera --distance 9000` parsed as something else"),
        }
    }

    /// **The drift guard the block comment at the top of this file asks for** (#4 Tier 2,
    /// closed by the integrator). `--help`'s value lists and the renderer's tables are now
    /// one table: a material added to `substrate_materials` reaches this CLI's completion and
    /// its "did you mean" with no hand edit, and a material *removed* from there fails here
    /// rather than leaving clap accepting a name nothing can draw.
    ///
    /// An equality test rather than a re-import, deliberately: clap wants `&'static str`
    /// possible values and the failure should *name* the drift. This is the fix for exactly
    /// the failure `agent::id_range` demonstrated by hand-maintaining a second copy of
    /// `params.rs`'s ranges — drifted on 9 of 45 ids (brief R6).
    ///
    /// ⚠️ **`CONSOLE_SOURCES` is pinned by a literal, not bound.** `world`/`off`/`substrate`
    /// are `BackdropSource`'s value space, and `BackdropSource` lives in `src/console_main.rs`
    /// — another `[[bin]]`, which no `bin` can import. The other half of this literal is
    /// `BACKDROP_SOURCE_WORDS` there, asserted against `console_source` by
    /// `every_source_word_resolves_and_a_typed_name_is_stricter_than_the_env_var`. Two
    /// alarms, one wire missing; the fix is a `pub const` in `cli.rs` beside
    /// `parse_console_op` (already the declared home of "both ends speak one vocabulary from
    /// one place"), and it is in CONSOLE_ARCHITECTURE.md's honesty ledger.
    #[test]
    fn the_console_vocabularies_are_bound_to_the_tables_that_draw_them() {
        use organic_math_native::substrate_materials;
        assert_eq!(
            CONSOLE_MATERIALS,
            &substrate_materials::MATERIAL_NAMES[..],
            "clap offers a material list the renderer does not have"
        );
        assert_eq!(
            CONSOLE_RIGS,
            &substrate_materials::RIG_NAMES[..],
            "clap offers a rig list the renderer does not have"
        );
        assert_eq!(
            CONSOLE_SOURCES,
            &["world", "off", "substrate"][..],
            "the other half of this literal is BACKDROP_SOURCE_WORDS in src/console_main.rs"
        );
        // The two vocabularies must stay disjoint, or `background studio` would parse.
        for r in CONSOLE_RIGS {
            assert!(!CONSOLE_MATERIALS.contains(r), "`{r}` is in both vocabularies");
            assert!(!CONSOLE_SOURCES.contains(r), "`{r}` is in both vocabularies");
        }
    }

    /// The console lane must not leak into the World's. Two ends of the same claim: a
    /// console verb never reaches `to_ctl` (it branches in `main` first, so reaching the
    /// mapping is the `unreachable!`), and the op it does produce parses back through the
    /// console vocabulary — not `CliOp`'s.
    #[test]
    fn console_ops_ride_their_own_lane() {
        let c = parse(&["console", "background", "slate"]).unwrap();
        let Cmd::Console { action: ConsoleAction::Background { name } } = c.cmd else {
            panic!("expected a console background");
        };
        let op = cli::ConsoleOp::Background(name);
        let line = cli::console_op_to_line(&op);
        assert_eq!(line, "background slate");
        assert_eq!(cli::parse_console_op(&line), Some(op));
        // The World's parser must NOT understand it — if it did, a mis-wired drain would
        // half-apply console commands instead of visibly ignoring them.
        assert_eq!(agent::CliOp::parse(&line), None);
    }

    #[test]
    fn completions_generate_for_zsh() {
        let mut out = Vec::new();
        clap_complete::generate(
            clap_complete::Shell::Zsh,
            &mut Cli::command(),
            "organon",
            &mut out,
        );
        let script = String::from_utf8(out).unwrap();
        assert!(script.contains("#compdef organon"));
        // Param ids ride the completion script as possible values.
        assert!(script.contains("metallic"));
    }
}
