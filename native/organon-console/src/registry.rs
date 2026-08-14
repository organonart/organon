//! # The console's command registry — one table, four front doors
//!
//! A *console command* is a verb the console can perform on itself: what sits behind the
//! glyphs, where the viewer stands, whether the portal is open. There are now **four ways to
//! say one**, and they must be four renderings of one vocabulary rather than four
//! vocabularies that resemble each other:
//!
//! | Front door | Who is talking | How it arrives |
//! |---|---|---|
//! | `organon console background slate` | a terminal, a script, a harness with no other route in | clap → `cli::ConsoleOp` → the sidecar |
//! | `mcp__organon__console_background` | an agent, on its own initiative | MCP tool → `ConsoleDispatch` → the sidecar |
//! | `/background slate` | **a human, in the composer** | this module → `ConsoleDispatch` → the sidecar |
//! | a pie-menu wedge (not built) | a human, with a pointer | this module's hierarchy → the same call |
//!
//! All four converge on `Shell::apply_console(&ConsoleOp)`, which is what makes multiple
//! entry points safe. What this module adds is that the **vocabulary** converges too: the
//! slash surface is *generated* from the same `Vec<CommandSpec>` the MCP schemas are
//! generated from, so a verb cannot exist for an agent and not for the person sitting in
//! front of the console.
//!
//! ## Why a human's command must not cost inference
//!
//! Measured on 2026-08-13: James typed `organon console posture desktop` into a conversation
//! tab. The text went to the agent as a message; the agent ran inference to work out what it
//! meant; it made a tool-search call to *find* the tool; it called the tool; and the console
//! then asked James to approve his own command. About **13 seconds and a chunk of context for
//! a command he already knew he wanted.**
//!
//! Nothing there was a bug. It is what the console's older architecture forced: it composited
//! *around* a harness it did not own (Claude Code in a PTY, Pi in WSL) and had no way to hear
//! a human's intent except through that harness. The conversation front-end ended that — the
//! console owns the composer now — and this module is that assumption finally being revisited.
//!
//! 🚨 **A command typed by a person is not a request to an agent, and must not be priced like
//! one.** [`Registry::resolve`] runs before a byte reaches the child process: no message, no
//! inference, no tool search, no approval card, no tokens. The approval model is untouched and
//! still correct, because it answers a different question — *may this agent act on my behalf* —
//! and a human's own keystroke was never that question.
//!
//! ⚠️ **It is still audited.** The console lane does not apply anything here: it hands the
//! validated call to the same [`crate::mcp::ToolDispatch`] the agent's tools use, which writes
//! the console's own sidecar, which `Shell::drain_console` drains next frame through the real
//! [`crate::command::CommandService`] — leaving a `CommandRun` record either way. What the
//! slash lane removes is the inference and the card, not the discipline.
//!
//! ## The shape, and how a pie menu consumes it
//!
//! An [`Entry`] is a **group**, a **verb**, and its **arguments** — never a flat string. The
//! dotted catalog name is split once, here, so nothing downstream ever splits it again:
//! `console.background` is group `console`, verb `background`; `console.camera.read` is group
//! `console`, verb `camera.read` (everything after the *first* dot, so the split is total).
//!
//! A radial menu is then three reads and no new table:
//!
//! 1. [`Registry::groups`] → the wedges of the root ring (`console`, `view`).
//! 2. [`Registry::verbs_in`] → the wedges of the second ring, each labelled with
//!    [`Entry::verb`] and described by [`Entry::doc`].
//! 3. [`Entry::args`] → the third ring. An argument whose [`crate::command::ArgKind`] is
//!    `Choice` *is* a ring of wedges, one per option, already validated. `Float` carries its
//!    own band, so it is a dial rather than a list. `Int`/`Text` have no closed value space
//!    and are the one case that needs a typed field.
//!
//! A wedge press then builds the same `(name, args)` pair [`Registry::resolve`] builds from a
//! typed line and hands it to the same dispatch. **The menu is a second renderer of this
//! table, never a second table** — which is the whole reason the hierarchy is carried
//! explicitly rather than left implicit in a dotted string.
//!
//! ## Two lanes, because two different things answer
//!
//! [`Lane::Console`] verbs are handed down by `console_main` and act on the console. [`Lane::View`]
//! verbs are answered inside the conversation view itself and never leave it — `/surface`
//! summons an artifact into *this* transcript, and `/help` is a reading of this table. They
//! share the registry because a human types them in the same box and a menu should show them
//! in the same ring; they are marked because the code that runs them is not the same code.

use serde_json::{json, Map, Value};

use crate::command::{ArgKind, ArgSpec, CommandSpec};

/// Which machinery answers a verb. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Handed down by `console_main`: dispatched onto the console's audited sidecar, exactly
    /// as the CLI's line and the agent's tool call are.
    Console,
    /// Answered by the conversation view itself. Nothing crosses a process or a channel.
    View,
}

/// `/surface` — the view-lane verb that was the console's *only* local command before this
/// module existed. Its spelling and its behaviour are unchanged.
pub const VERB_SURFACE: &str = "view.surface";
/// `/help` — the reading of this table. Generated by [`Registry::help_lines`], never written
/// out, because a hand-written command list is a second table with no test holding it to the
/// first.
pub const VERB_HELP: &str = "view.help";

/// One verb, as a hierarchy rather than as a dotted string.
///
/// ⚠️ **[`CommandSpec::target`] is deliberately not carried.** That field names which
/// `CommandTarget` a dispatch lands on — machinery, not vocabulary — and a view-lane verb has
/// no target at all. Carrying it would force `/help` to claim a target it does not have, and
/// a menu would then be reading a field that means nothing to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    name: String,
    doc: String,
    args: Vec<ArgSpec>,
    lane: Lane,
}

impl Entry {
    /// A console verb, from the same [`CommandSpec`] the MCP schema is generated from.
    pub fn from_spec(spec: &CommandSpec, lane: Lane) -> Self {
        Entry {
            name: spec.name.clone(),
            doc: spec.doc.clone(),
            args: spec.args.clone(),
            lane,
        }
    }

    /// The catalog name — `console.background`. What the dispatch is called with, so a slash
    /// command and a tool call reach the same arm of the same `match`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The first ring: everything before the **first** dot.
    pub fn group(&self) -> &str {
        self.name.split_once('.').map(|(g, _)| g).unwrap_or(&self.name)
    }

    /// The second ring, and the word a human types after the slash: everything after the
    /// first dot.
    ///
    /// Dots inside it survive — `console.camera.read` is verb `camera.read`, typed
    /// `/camera.read`. Splitting on the *last* dot instead would make its group
    /// `console.camera`, inventing a ring with one wedge in it; splitting on the first keeps
    /// the group the product's own namespace and leaves the remainder whole.
    pub fn verb(&self) -> &str {
        self.name.split_once('.').map(|(_, v)| v).unwrap_or(&self.name)
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// The third ring. A `Choice` argument is a ring of wedges; a `Float` is a dial with its
    /// band already stated; `Int`/`Text` need a field.
    pub fn args(&self) -> &[ArgSpec] {
        &self.args
    }

    pub fn lane(&self) -> Lane {
        self.lane
    }

    /// `/background` — the verb alone, for a message that has already said what went wrong
    /// with the rest of the line.
    fn usage_head(&self) -> String {
        format!("/{}", self.verb())
    }

    /// `/camera [reset] [yaw <yaw>] [pitch <pitch>] [distance <distance>]` — the whole
    /// grammar of this verb, derived rather than written.
    ///
    /// Required arguments are positional, in declared order; optional ones are keyword-tagged,
    /// and an optional `Bool` is a bare flag. That is not a new grammar: it is exactly what
    /// `cli::CameraFraming::to_words` already puts on the sidecar, so `/camera reset distance
    /// 40` and the tail of the line `organon console camera --reset --distance 40` writes are
    /// the same words in the same order.
    pub fn usage(&self) -> String {
        let mut out = format!("/{}", self.verb());
        for arg in &self.args {
            if arg.required {
                out.push_str(&format!(" <{}>", arg.name));
            } else if matches!(arg.kind, ArgKind::Bool) {
                out.push_str(&format!(" [{}]", arg.name));
            } else {
                out.push_str(&format!(" [{} <{}>]", arg.name, arg.name));
            }
        }
        out
    }
}

/// What a composer line turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// Not a command. Send it to the agent unchanged — this is the answer for every ordinary
    /// sentence, including one that *mentions* a command.
    Message,
    /// A `//`-escaped line: send the text this carries (one slash removed) to the agent.
    Escaped(String),
    /// A command, validated against its own schema and ready to run.
    Run {
        lane: Lane,
        /// The catalog name, not the typed word — `console.background`.
        name: String,
        /// The dispatch arguments, in the shape the spec declares.
        args: Value,
    },
    /// Refused, carrying the sentence to show. **The composer must not be cleared**: see
    /// [`Registry::resolve`].
    Refused(String),
}

/// Every verb this console answers, in one table.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: Vec<Entry>,
    collisions: Vec<String>,
}

impl Registry {
    /// Build the registry from the console's own catalog — the same `Vec<CommandSpec>` handed
    /// to the MCP server — plus the view-lane verbs this crate owns.
    ///
    /// ⚠️ **The slash namespace is flat while the registry is not**, because a human types one
    /// word and a menu draws two rings. So two groups can collide on a verb, the first wins,
    /// and the loser is *reported* rather than silently unreachable — [`Registry::collisions`],
    /// the discipline `mcp::McpServer::name_collisions` already runs one layer over. The real
    /// table is pinned collision-free by a test in `console_main`, which is the only place that
    /// can see it.
    pub fn new(console: &[CommandSpec]) -> Self {
        let mut entries: Vec<Entry> =
            console.iter().map(|s| Entry::from_spec(s, Lane::Console)).collect();
        entries.extend(view_entries());
        let mut collisions = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for entry in &entries {
            if seen.contains(&entry.verb()) {
                collisions.push(entry.name.clone());
            } else {
                seen.push(entry.verb());
            }
        }
        Registry { entries, collisions }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Catalog names that are **not reachable** as a slash command because an earlier entry
    /// already holds the verb word. Empty is the normal state.
    pub fn collisions(&self) -> &[String] {
        &self.collisions
    }

    /// The root ring: distinct groups, in table order.
    pub fn groups(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for entry in &self.entries {
            if !out.contains(&entry.group()) {
                out.push(entry.group());
            }
        }
        out
    }

    /// The second ring: every verb in one group, in table order.
    pub fn verbs_in(&self, group: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.group() == group).collect()
    }

    /// The entry a typed word names — the first one holding it, see [`Registry::new`].
    pub fn entry(&self, verb: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.verb() == verb)
    }

    /// Every typeable word, in table order. The list a refusal carries.
    pub fn verbs(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for entry in &self.entries {
            if !out.contains(&entry.verb()) {
                out.push(entry.verb());
            }
        }
        out
    }

    /// What a composer line is.
    ///
    /// 🚨 **The two mistakes here are wildly asymmetric, and the asymmetry is what the rules
    /// are shaped around.** Failing to recognise a command sends a slash-word to the agent,
    /// which is merely odd. Over-recognising one *swallows a real message* — and the old
    /// composer cleared either way, so a human watched their sentence vanish into nothing.
    /// That is why the predecessor of this function (`conversation_view::local_command`)
    /// matched `/surface` exactly and forwarded everything else, including `/surfaces`.
    ///
    /// Exact matching is no longer the right instrument, because forwarding an unknown slash
    /// command to an agent is its own silent failure — the console knows the verb does not
    /// exist and says nothing. **[`Resolved::Refused`] is what makes both properties hold at
    /// once**: it names what would have worked, and the caller does *not* clear the composer,
    /// so nothing is ever swallowed. A refusal is recoverable; a swallow is not.
    ///
    /// The rules, in order:
    ///
    /// 1. A line that does not begin with `/` is a message. This alone is what keeps a
    ///    *mention* — "what does `/surface` do?", "use `/theme` for that" — reaching the agent:
    ///    a sentence about a command has words in front of it.
    /// 2. `//` is the escape: the rest is sent to the agent with one slash removed, which is
    ///    the answer to "how do I send a line that really does start with a slash".
    /// 3. A bare `/` names no verb, so there is nothing to refuse. It is a message, exactly as
    ///    it was before this module.
    /// 4. Otherwise the first word must be a verb in this table, and the rest must satisfy that
    ///    verb's own schema. Anything else is a refusal naming the alternatives.
    pub fn resolve(&self, line: &str) -> Resolved {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix('/') else {
            return Resolved::Message;
        };
        if let Some(escaped) = rest.strip_prefix('/') {
            return Resolved::Escaped(format!("/{escaped}"));
        }
        let mut words = rest.split_whitespace();
        let Some(verb) = words.next() else {
            return Resolved::Message;
        };
        let Some(entry) = self.entry(verb) else {
            return Resolved::Refused(format!(
                "`/{verb}` is not a command — known commands: {}. `/help` describes them",
                self.verbs().iter().map(|v| format!("/{v}")).collect::<Vec<_>>().join(", ")
            ));
        };
        match parse_args(entry, words) {
            Ok(args) => Resolved::Run { lane: entry.lane, name: entry.name.clone(), args },
            Err(message) => Resolved::Refused(message),
        }
    }

    /// `/help`'s body — the whole table, read out.
    ///
    /// Generated, so a verb added to the console's catalog is documented in the composer with
    /// no edit here. That is the same reason `doc/reference/` is generated and the same reason
    /// the MCP schemas are: a hand-written second list is a table with nothing holding it to
    /// the first.
    pub fn help_lines(&self) -> Vec<String> {
        let mut out = vec![
            "type / then a verb — a command you type runs here at once: no message to the \
             agent, no inference, no approval"
                .to_string(),
        ];
        for group in self.groups() {
            out.push(group.to_string());
            for entry in self.verbs_in(group) {
                out.push(format!("  {}  — {}", entry.usage(), entry.doc()));
                for arg in entry.args() {
                    match &arg.kind {
                        ArgKind::Choice(options) => {
                            out.push(format!("      {}: {}", arg.name, options.join(" | ")));
                        }
                        ArgKind::Float { min, max } => {
                            out.push(format!("      {}: {min} … {max}", arg.name));
                        }
                        ArgKind::Int | ArgKind::Bool | ArgKind::Text => {}
                    }
                }
            }
        }
        out.push(
            "//text sends a line beginning with a slash to the agent unchanged".to_string(),
        );
        out
    }
}

/// The verbs the conversation view answers itself.
///
/// `/surface` keeps the exact spelling and behaviour it had as the console's single local
/// command; `/help` is new and is the discoverability half of this change — a person must be
/// able to find out what exists without leaving the window.
fn view_entries() -> Vec<Entry> {
    vec![
        Entry {
            name: VERB_SURFACE.into(),
            doc: "Put a rendered surface in this conversation, with the panel that drives it \
                  directly beneath"
                .into(),
            args: Vec::new(),
            lane: Lane::View,
        },
        Entry {
            name: VERB_HELP.into(),
            doc: "List every command this console answers".into(),
            args: Vec::new(),
            lane: Lane::View,
        },
    ]
}

/// The words after the verb, as the dispatch arguments the spec declares.
///
/// Required arguments are positional in declared order; optional ones are keyword-tagged and
/// an optional `Bool` is a bare flag. An **absent** optional argument is simply not in the
/// object — `command::validate_args` reads a missing key and a `null` the same way, and the
/// console's `op_from` maps both back to `None`.
///
/// ⚠️ **Every value is checked here even though the dispatch will check it again**, and that
/// is not redundancy for its own sake: this is the only gate with a human in front of it, so
/// it is the only one whose message can name the alternatives *while the words are still in
/// the composer to be edited*. By the time `validate_args` sees them the line has been sent.
fn parse_args<'a>(entry: &Entry, words: impl Iterator<Item = &'a str>) -> Result<Value, String> {
    let words: Vec<&str> = words.collect();
    let mut out = Map::new();
    let mut it = words.iter().copied();

    for arg in entry.args.iter().filter(|a| a.required) {
        let Some(word) = it.next() else {
            return Err(format!(
                "`{}` needs `{}` — usage: {}",
                entry.usage_head(),
                arg.name,
                entry.usage()
            ));
        };
        out.insert(arg.name.clone(), coerce(entry, arg, word)?);
    }

    let optional: Vec<&ArgSpec> = entry.args.iter().filter(|a| !a.required).collect();
    // `loop` + an explicit `next` rather than `while let`: the body pulls a *second* word for
    // any non-flag argument, so the iterator has to stay in hand.
    loop {
        let Some(word) = it.next() else { break };
        if optional.is_empty() {
            return Err(if entry.args.is_empty() {
                format!("`{}` takes no arguments — got `{word}`", entry.usage_head())
            } else {
                format!("`{}` got more words than it takes — usage: {}", entry.usage_head(), entry.usage())
            });
        }
        let Some(arg) = optional.iter().find(|a| a.name == word) else {
            return Err(format!(
                "`{}` has no `{word}` — usage: {}",
                entry.usage_head(),
                entry.usage()
            ));
        };
        if out.contains_key(&arg.name) {
            // Last-wins would be a guess between "a caller building a line badly" and "two
            // intents concatenated", and the console's own sidecar parser refuses a repeated
            // camera axis for exactly that reason. One rule, both ends of the lane.
            return Err(format!("`{}`: `{word}` was given twice", entry.usage_head()));
        }
        if matches!(arg.kind, ArgKind::Bool) {
            out.insert(arg.name.clone(), Value::Bool(true));
            continue;
        }
        let Some(value) = it.next() else {
            return Err(format!("`{}`: `{word}` needs a value", entry.usage_head()));
        };
        out.insert(arg.name.clone(), coerce(entry, arg, value)?);
    }
    Ok(Value::Object(out))
}

/// One word, as the type its slot declares — refused, never approximated, with the message a
/// person can act on.
fn coerce(entry: &Entry, arg: &ArgSpec, word: &str) -> Result<Value, String> {
    let head = entry.usage_head();
    match &arg.kind {
        ArgKind::Choice(options) => {
            if options.iter().any(|o| o == word) {
                Ok(Value::String(word.to_string()))
            } else {
                Err(format!(
                    "`{head}`: `{word}` is not one of {}",
                    options.join(" | ")
                ))
            }
        }
        ArgKind::Text => Ok(Value::String(word.to_string())),
        ArgKind::Int => word
            .parse::<i64>()
            .map(|n| json!(n))
            .map_err(|_| format!("`{head}`: `{}` wants a whole number, got `{word}`", arg.name)),
        // Not-a-number and infinity both parse as `f64` and would then travel as `null`
        // through serde_json, so the finiteness check is the one that keeps a typo out of a
        // view matrix rather than a nicety.
        ArgKind::Float { min, max } => match word.parse::<f64>() {
            Ok(v) if v.is_finite() && (*min..=*max).contains(&v) => Ok(json!(v)),
            Ok(v) if v.is_finite() => {
                Err(format!("`{head}`: `{}` must be {min} … {max}, got {v}", arg.name))
            }
            _ => Err(format!("`{head}`: `{}` wants a number, got `{word}`", arg.name)),
        },
        ArgKind::Bool => match word {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("`{head}`: `{}` wants true or false, got `{word}`", arg.name)),
        },
    }
}

/// What the composer says back after a command ran.
///
/// Pure, and here rather than in the view, so the sentence a human reads is pinned by a test
/// instead of being verified by reading it. The console lane answers *accepted*, not
/// *applied* — the op lands on the next frame — so the receipt deliberately does not claim
/// the backdrop has changed.
pub fn receipt(typed: &str, result: &Result<Value, String>) -> String {
    match result {
        Err(message) => message.clone(),
        Ok(Value::Null) => format!("✓ {typed}"),
        Ok(value) => format!("✓ {typed} — {value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::TargetKind;

    /// A stand-in for the console's real catalog. It cannot be the real one — the material and
    /// rig tables live in the root crate, which this one cannot see — so it copies the
    /// *shapes* that matter: a required `Choice`, two required `Int`s beside a `Choice`, and
    /// the all-optional camera with a `Bool` flag and three banded `Float`s.
    fn console() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "console.background".into(),
                doc: "What sits behind the glyphs".into(),
                target: TargetKind::Viewport,
                args: vec![ArgSpec {
                    name: "name".into(),
                    kind: ArgKind::Choice(vec!["graphite".into(), "slate".into()]),
                    required: true,
                }],
            },
            CommandSpec {
                name: "console.patch".into(),
                doc: "Claim a rectangle".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec { name: "up".into(), kind: ArgKind::Int, required: true },
                    ArgSpec { name: "rows".into(), kind: ArgKind::Int, required: true },
                    ArgSpec {
                        name: "kind".into(),
                        kind: ArgKind::Choice(vec!["scene".into(), "panel".into()]),
                        required: true,
                    },
                ],
            },
            CommandSpec {
                name: "console.camera".into(),
                doc: "Where the viewer stands".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec { name: "reset".into(), kind: ArgKind::Bool, required: false },
                    ArgSpec {
                        name: "yaw".into(),
                        kind: ArgKind::Float { min: -180.0, max: 180.0 },
                        required: false,
                    },
                    ArgSpec {
                        name: "distance".into(),
                        kind: ArgKind::Float { min: 5.0, max: 4000.0 },
                        required: false,
                    },
                ],
            },
            CommandSpec {
                name: "console.camera.read".into(),
                doc: "Where the viewer stands right now".into(),
                target: TargetKind::Viewport,
                args: Vec::new(),
            },
        ]
    }

    fn registry() -> Registry {
        Registry::new(&console())
    }

    fn run(line: &str) -> Resolved {
        registry().resolve(line)
    }

    /// **The property the whole change is for**: everything in the table is typeable, and the
    /// word to type is derived rather than listed. A verb an agent can call and a human cannot
    /// is the two-vocabularies failure this module exists to prevent.
    #[test]
    fn every_entry_is_reachable_as_a_slash_command() {
        let reg = registry();
        assert!(reg.collisions().is_empty(), "the fixture table has no shadowed verb");
        for entry in reg.entries() {
            let found = reg.entry(entry.verb()).expect("its own verb resolves");
            assert_eq!(found.name(), entry.name());
            // …and the group/verb split is total: rejoining them is the catalog name.
            assert_eq!(format!("{}.{}", entry.group(), entry.verb()), entry.name());
        }
        assert_eq!(
            reg.verbs(),
            ["background", "patch", "camera", "camera.read", "surface", "help"]
        );
    }

    /// The hierarchy a pie menu reads: two rings from the table, no third table.
    #[test]
    fn the_registry_is_a_hierarchy_a_menu_can_walk() {
        let reg = registry();
        assert_eq!(reg.groups(), ["console", "view"]);
        let console: Vec<&str> = reg.verbs_in("console").iter().map(|e| e.verb()).collect();
        assert_eq!(console, ["background", "patch", "camera", "camera.read"]);
        let view: Vec<&str> = reg.verbs_in("view").iter().map(|e| e.verb()).collect();
        assert_eq!(view, ["surface", "help"]);
        assert!(reg.verbs_in("nothing").is_empty());

        // The third ring is the argument's own value space — already closed, already
        // validated, nothing for a menu to invent.
        let background = reg.entry("background").unwrap();
        let ArgKind::Choice(options) = &background.args()[0].kind else {
            panic!("a background is chosen from a table")
        };
        assert_eq!(options, &["graphite".to_string(), "slate".to_string()]);
        // A dial, not a list: the band travels with the argument.
        let camera = reg.entry("camera").unwrap();
        assert!(matches!(camera.args()[1].kind, ArgKind::Float { min, max } if min == -180.0 && max == 180.0));
    }

    /// 🚨 **The successor to `only_an_exact_slash_surface_is_a_local_command`, and it keeps
    /// that test's real point.**
    ///
    /// The old rule was exact-match-or-forward, and its stated reason was that
    /// over-recognition swallows a real message while the composer clears either way. That
    /// reason survives; the instrument does not, because forwarding `/surfaces` to an agent is
    /// its own silent failure. **A refusal keeps the words in the composer**, so nothing is
    /// swallowed — which is a stronger version of the property the old test was defending, not
    /// a relaxation of it.
    ///
    /// What must still hold exactly: a line that merely *mentions* a command reaches the agent.
    #[test]
    fn a_mention_is_not_a_command() {
        for message in [
            "what does /surface do?",
            "use /help if you get stuck",
            "the /background verb takes a material",
            "surface",
            "",
            "   ",
            // A bare slash names no verb, so there is nothing to refuse — unchanged from the
            // predecessor, which also let it through.
            "/",
            "/ ",
        ] {
            assert_eq!(run(message), Resolved::Message, "{message:?} belongs to the agent");
        }
    }

    /// The escape, which is the honest answer to "then how do I send a line that starts with a
    /// slash". One slash comes off; nothing else is touched.
    #[test]
    fn a_double_slash_sends_the_line_to_the_agent() {
        assert_eq!(run("//surface"), Resolved::Escaped("/surface".into()));
        assert_eq!(run("//not-a-verb at all"), Resolved::Escaped("/not-a-verb at all".into()));
        assert_eq!(run("  //x  "), Resolved::Escaped("/x".into()), "trimmed like a send");
    }

    /// An unknown command is refused **naming the known set** — the `kind::UnknownKind` rule,
    /// one layer out: an error that only says "no" leaves the caller with nowhere to go.
    #[test]
    fn an_unknown_command_is_refused_naming_the_known_set() {
        let Resolved::Refused(sentence) = run("/surfaces") else {
            panic!("an unknown slash command is refused, never forwarded as chat")
        };
        assert!(sentence.contains("`/surfaces`"), "it quotes back what was typed: {sentence}");
        for verb in registry().verbs() {
            assert!(sentence.contains(&format!("/{verb}")), "the refusal names /{verb}");
        }
        assert!(sentence.contains("/help"), "and says where the detail is");

        // The retired command, and a plain typo. Both refused, neither swallowed.
        assert!(matches!(run("/panel"), Resolved::Refused(_)));
        assert!(matches!(run("/backgroud slate"), Resolved::Refused(_)));
    }

    /// The grammar, verb by verb — required positional, optional keyword, `Bool` as a flag.
    #[test]
    fn a_command_parses_into_the_arguments_its_spec_declares() {
        assert_eq!(
            run("/background graphite"),
            Resolved::Run {
                lane: Lane::Console,
                name: "console.background".into(),
                args: json!({ "name": "graphite" }),
            }
        );
        assert_eq!(
            run("/patch 12 8 panel"),
            Resolved::Run {
                lane: Lane::Console,
                name: "console.patch".into(),
                args: json!({ "up": 12, "rows": 8, "kind": "panel" }),
            }
        );
        // An absent optional axis is absent from the object, which is what `validate_args`
        // and `op_from` both read as "not given".
        assert_eq!(
            run("/camera distance 40"),
            Resolved::Run {
                lane: Lane::Console,
                name: "console.camera".into(),
                args: json!({ "distance": 40.0 }),
            }
        );
        assert_eq!(
            run("/camera reset yaw -30 distance 40"),
            Resolved::Run {
                lane: Lane::Console,
                name: "console.camera".into(),
                args: json!({ "reset": true, "yaw": -30.0, "distance": 40.0 }),
            }
        );
        assert_eq!(
            run("/camera.read"),
            Resolved::Run {
                lane: Lane::Console,
                name: "console.camera.read".into(),
                args: json!({}),
            }
        );
        assert_eq!(
            run("/surface"),
            Resolved::Run { lane: Lane::View, name: VERB_SURFACE.into(), args: json!({}) },
            "the one command that existed before this module, unchanged"
        );
        assert_eq!(
            run(" /surface \n"),
            Resolved::Run { lane: Lane::View, name: VERB_SURFACE.into(), args: json!({}) },
            "trimmed like a send"
        );
    }

    /// Every way a line can be wrong, and the message it earns. Each of these used to be
    /// "send it to the agent and hope"; none of them clears the composer.
    #[test]
    fn a_malformed_command_is_refused_with_its_own_usage() {
        let cases = [
            ("/background", "needs `name`"),
            ("/background chartreuse", "is not one of"),
            ("/background graphite slate", "more words than it takes"),
            ("/surface slate", "takes no arguments"),
            ("/patch 12 8", "needs `kind`"),
            ("/patch twelve 8 scene", "wants a whole number"),
            ("/camera sideways 3", "has no `sideways`"),
            ("/camera distance", "needs a value"),
            ("/camera distance 9000", "must be 5 … 4000"),
            ("/camera distance forty", "wants a number"),
            ("/camera distance NaN", "wants a number"),
            ("/camera distance inf", "wants a number"),
            ("/camera yaw 1 yaw 2", "was given twice"),
        ];
        for (line, fragment) in cases {
            let Resolved::Refused(sentence) = run(line) else {
                panic!("{line:?} is malformed and must be refused")
            };
            assert!(
                sentence.contains(fragment),
                "{line:?} should say {fragment:?}, said: {sentence}"
            );
        }
    }

    /// `/help` is the table read out, so a verb added to the catalog documents itself.
    #[test]
    fn help_is_generated_from_the_table() {
        let reg = registry();
        let lines = reg.help_lines().join("\n");
        for entry in reg.entries() {
            assert!(lines.contains(&entry.usage()), "help offers {}", entry.usage());
            assert!(lines.contains(entry.doc()), "help explains {}", entry.verb());
        }
        assert!(lines.contains("graphite | slate"), "a Choice lists its ring");
        assert!(lines.contains("5 … 4000"), "a Float states its band");
        assert!(lines.contains("no inference"), "and says why a typed command is free");
        assert!(lines.contains("//text"), "and how to reach the agent with a slash");
    }

    /// Usage strings are derived, and the camera's is word-for-word the sidecar's own tail —
    /// which is what makes `/camera reset distance 40` and `camera reset distance 40` one
    /// spelling rather than two.
    #[test]
    fn usage_is_derived_from_the_schema() {
        let reg = registry();
        assert_eq!(reg.entry("background").unwrap().usage(), "/background <name>");
        assert_eq!(reg.entry("patch").unwrap().usage(), "/patch <up> <rows> <kind>");
        assert_eq!(
            reg.entry("camera").unwrap().usage(),
            "/camera [reset] [yaw <yaw>] [distance <distance>]"
        );
        assert_eq!(reg.entry("camera.read").unwrap().usage(), "/camera.read");
    }

    /// A verb word held twice is reported, never silently unreachable.
    #[test]
    fn a_shadowed_verb_is_reported() {
        let mut specs = console();
        specs.push(CommandSpec {
            name: "elsewhere.background".into(),
            doc: "a second claimant".into(),
            target: TargetKind::Viewport,
            args: Vec::new(),
        });
        let reg = Registry::new(&specs);
        assert_eq!(reg.collisions(), ["elsewhere.background"]);
        assert_eq!(
            reg.entry("background").unwrap().name(),
            "console.background",
            "the first claimant keeps the word"
        );
    }

    /// The receipt says accepted, never applied — the op lands on the next frame.
    #[test]
    fn a_receipt_reports_what_actually_happened() {
        assert_eq!(
            receipt("/background slate", &Ok(json!({ "accepted": "background slate" }))),
            "✓ /background slate — {\"accepted\":\"background slate\"}"
        );
        assert_eq!(receipt("/surface", &Ok(Value::Null)), "✓ /surface");
        assert_eq!(
            receipt("/camera distance 9", &Err("out of range".to_string())),
            "out of range",
            "a failure is reported as itself, never dressed as a success"
        );
    }
}
