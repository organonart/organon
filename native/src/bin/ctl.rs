//! `organon` — the Organon command surface (#452 Tiers 1–2).
//!
//! A plain local CLI so external agents (Bianca) and humans can read Organon's
//! live state and drive its controls without MCP, sockets, or the editor:
//! - reads decode the `Shared` IPC mmap directly (frame-fresh);
//! - writes append `CliOp` lines to the command sidecar, drained each frame by
//!   the visual into the #317 Performer's override lane (last-touched-wins).
//!
//! This binary owns the **clap** argument surface — per-subcommand `--help`,
//! "did you mean" suggestions, `--version`, and `organon completions <shell>`
//! for bash/zsh/fish tab completion (param ids complete as values). All the
//! actual logic lives in `organic_math_native::cli` (pure, unit-tested); this
//! file maps commands to I/O and exit codes:
//!   0 = ok · 2 = bad usage (clap) · 3 = read command with no live Organon.

use clap::{CommandFactory, Parser, Subcommand};
use organic_math_native::{agent, cli, ipc};

/// Possible-values parser over the Tier-1 actuatable param ids — powers both
/// validation ("did you mean") and shell completion of `<ID>` arguments.
fn param_ids() -> clap::builder::PossibleValuesParser {
    clap::builder::PossibleValuesParser::new(agent::ACTUATABLE_IDS.iter().copied())
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

/// Map the clap surface onto the library's command model (validation included).
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
        Cmd::Completions { .. } | Cmd::Snap { .. } | Cmd::Record { .. } | Cmd::Docs { .. } => {
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
