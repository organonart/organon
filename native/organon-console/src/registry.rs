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
//! ## Candidates — the same table, read forwards from a half-typed line
//!
//! [`Registry::candidates`] answers *"what could this line become next"* for any prefix:
//! after `/`, after `/th`, after `/theme `, after `/theme ch`. It returns
//! [`Candidate`]s — a label, its doc, **the whole line accepting it would produce**, and
//! whether that line is a complete command — never a rendered row and never egui.
//!
//! 🚨 **Three surfaces draw that list and there is one generator.** The popup above the
//! composer is the first; the pie menu is the second, and its three rings are exactly
//! [`Registry::groups`] → [`Registry::verbs_in`] → an argument's `Choice`; `/help` is the
//! third. A renderer that had to build its own list would be a second vocabulary — the
//! failure this whole module exists to prevent, reached from the other end.
//!
//! ## Two lanes, because two different things answer
//!
//! [`Lane::Console`] verbs are handed down by `console_main` and act on the console. [`Lane::View`]
//! verbs are answered inside the conversation view itself and never leave it — `/surface`
//! summons an artifact into *this* transcript, and `/help` is a reading of this table. They
//! share the registry because a human types them in the same box and a menu should show them
//! in the same ring; they are marked because the code that runs them is not the same code.

use organon_core::panels;
use organon_core::tabs::UiTab;
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

/// `console.theme` — the palette verb, and the one console-lane name this crate has to
/// **recognise** rather than merely forward.
///
/// ⚠️ **It is spelled here and imported by `console_main`, not spelled in both.** Almost all of
/// `/theme`'s work is console-lane and dispatches like any other verb; two of its argument
/// values (`edit`, `adjust` — [`crate::theme_edit::EDIT_WORDS`]) instead open a surface in
/// *this* transcript, so the conversation view has to be able to tell that this particular verb
/// arrived. A second spelling of the string on this side is a comparison that silently stops
/// matching the day somebody renames the verb, and the symptom would be `/theme edit` quietly
/// dispatching to the console as an unknown palette name.
pub const VERB_THEME: &str = "console.theme";

/// The argument name `console.theme` carries its value in. Same rule as [`VERB_THEME`]: read
/// out of the dispatch payload on this side, declared by `console_main`'s spec on the other.
pub const THEME_ARG: &str = "name";

/// `/organon` — put one of Organon's own editor panels into this conversation.
///
/// A view-lane verb, like [`VERB_SURFACE`]: it acts on *this* transcript and never leaves the
/// crate. Two required arguments, and they are the two rings — `<tab> <panel>` — mirroring
/// Organon's own hierarchy rather than inventing a command tree beside it.
pub const VERB_ORGANON: &str = "view.organon";
/// The first ring's argument name: one of [`organon_core::tabs::UiTab::ALL`].
pub const ORGANON_TAB_ARG: &str = "tab";
/// The second ring's argument name: a [`organon_core::panels::Panel::slug`] on that tab.
pub const ORGANON_PANEL_ARG: &str = "panel";

/// One verb, as a hierarchy rather than as a dotted string.
///
/// ⚠️ **[`CommandSpec::target`] is deliberately not carried.** That field names which
/// `CommandTarget` a dispatch lands on — machinery, not vocabulary — and a view-lane verb has
/// no target at all. Carrying it would force `/help` to claim a target it does not have, and
/// a menu would then be reading a field that means nothing to it.
#[derive(Debug, Clone)]
pub struct Entry {
    name: String,
    doc: String,
    args: Vec<ArgSpec>,
    lane: Lane,
    narrow: Option<NarrowFn>,
}

/// ⚠️ **Hand-written because [`Entry::narrow`] must not be compared**, and `derive` would have
/// compared it: rustc warns that function-pointer equality is not meaningful (addresses are not
/// guaranteed unique, so two distinct narrowings can compare equal and one function can compare
/// unequal to itself across codegen units). Ignoring it is also the right *meaning* — an entry
/// is its vocabulary, and the hook is how a ring is drawn, not what the verb is. Every field a
/// caller can observe through the public accessors is compared.
impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.doc == other.doc
            && self.args == other.args
            && self.lane == other.lane
    }
}

/// An argument whose options depend on what an **earlier** positional word said.
///
/// 🚨 **Why this is a hook on the entry and not a new [`ArgKind`].** `/organon look` and
/// `/organon motion` must offer different second rings, which a `Choice(Vec<String>)` cannot
/// express — its list is fixed when the table is built. The obvious fix is a dependent variant
/// of `ArgKind`, and it was rejected on measurement: that enum is matched exhaustively at
/// **~30 sites** across `command.rs`, `mcp.rs`, `conversation_view.rs` and this file, so a new
/// arm is a change to the MCP schema generator, the dispatch validator and three renderers —
/// for one verb. A hook touches the candidate walk and nothing else.
///
/// ⚠️ **It narrows the ring; it does not validate the line.** The declared `ArgKind::Choice` for
/// a dependent argument is the **union** across every parent value, so `/organon motion surface`
/// passes the schema — `surface` really is a panel slug, just not one of Motion's. The pair is
/// checked where the pair is understood: the view lane answers it and names the mismatch. That
/// is the honest split (a command schema has one value list per argument and no notion of a
/// pair), and it is pinned by `an_unknown_pair_is_refused_by_the_view_lane` in
/// `conversation_view`.
///
/// The signature is a plain `fn` pointer rather than a boxed closure so that [`Entry`] stays
/// `Clone + Debug + PartialEq` — the registry is copied into every pane, and a trait object
/// would cost all three derives for no gain: every narrowing this console has is a pure
/// function of a static table.
///
/// `positional` holds the words that filled the **required** arguments so far, in declared
/// order; `arg` is the name of the one being offered. `None` means "no opinion", and the
/// argument's own [`ArgKind`] answers as usual. Each option is `(label, doc)` — the doc is what
/// lets `/organon look` show `surface` beside the panel's real heading.
pub type NarrowFn = fn(arg: &str, positional: &[&str]) -> Option<Vec<(String, String)>>;

impl Entry {
    /// A console verb, from the same [`CommandSpec`] the MCP schema is generated from.
    pub fn from_spec(spec: &CommandSpec, lane: Lane) -> Self {
        Entry {
            name: spec.name.clone(),
            doc: spec.doc.clone(),
            args: spec.args.clone(),
            lane,
            // A `CommandSpec` is the vocabulary the MCP schema is generated from and carries no
            // hook: a console verb's value space is closed at build time. See [`NarrowFn`].
            narrow: None,
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
            narrow: None,
        },
        Entry {
            name: VERB_HELP.into(),
            doc: "List every command this console answers".into(),
            args: Vec::new(),
            lane: Lane::View,
            narrow: None,
        },
        Entry {
            name: VERB_ORGANON.into(),
            doc: "Put one of Organon's own editor panels here — its tabs, then that tab's panels"
                .into(),
            args: vec![
                ArgSpec {
                    name: ORGANON_TAB_ARG.into(),
                    kind: ArgKind::Choice(UiTab::ALL.iter().map(|t| t.word()).collect()),
                    required: true,
                },
                ArgSpec {
                    name: ORGANON_PANEL_ARG.into(),
                    // The union across tabs — see [`NarrowFn`] for why the declared list is
                    // wider than any one ring, and where the pair is actually checked.
                    kind: ArgKind::Choice(
                        panels::slugs().into_iter().map(str::to_string).collect(),
                    ),
                    required: true,
                },
            ],
            lane: Lane::View,
            narrow: Some(organon_options),
        },
    ]
}

/// `/organon`'s two rings, read straight off Organon's own taxonomy.
///
/// 🚨 **Nothing here is a list this crate wrote.** The tabs are
/// [`organon_core::tabs::UiTab::ALL`] — which that module calls "the single source of truth the
/// editor's tab bar iterates" — and the panels are [`organon_core::panels::in_tab`], which the
/// editor itself reads its card headings out of. A hand-written copy of either would stop
/// matching Organon the moment a panel was added, and the ring would be confidently wrong.
///
/// ⚠️ **A tab with no panels in the table answers `Some(vec![])`, not `None`.** `None` would
/// fall through to the declared union and offer *every* slug on a tab that has none of them.
/// The empty ring is the true answer, and [`Palette::hint`] has nothing to add to it — the
/// conversation view says which tabs are joined.
fn organon_options(arg: &str, positional: &[&str]) -> Option<Vec<(String, String)>> {
    match arg {
        // The tab ring is exactly the declared `Choice`, so there is nothing to narrow. Saying
        // so explicitly beats falling through the `_` arm: a future third argument would then
        // be silently un-narrowed rather than obviously unhandled.
        ORGANON_TAB_ARG => None,
        ORGANON_PANEL_ARG => {
            let tab = UiTab::from_word(positional.first().copied().unwrap_or_default())?;
            Some(
                panels::in_tab(tab)
                    .into_iter()
                    .map(|p| {
                        let doc = match p.status {
                            panels::Status::Live => p.title.to_string(),
                            // Said in the ring, before the choice is made rather than after —
                            // see `panels::Status`.
                            panels::Status::Declared => {
                                format!("{} — not transplanted yet", p.title)
                            }
                        };
                        (p.slug.to_string(), doc)
                    })
                    .collect(),
            )
        }
        _ => None,
    }
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

/// What the composer says back after a command ran, as a value rather than a sentence.
///
/// ⚠️ **`ok` is not decoration.** A surface that shows a receipt has to be able to give a
/// refusal more weight than a success — a confirmation nobody reads costs nothing, a refusal
/// nobody reads costs the whole command — and it cannot do that from a string it would have
/// to parse a tick out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Whether the call was accepted. **Accepted, not applied**: the console lane's op lands
    /// on the next frame, so this deliberately does not claim the backdrop has changed.
    pub ok: bool,
    /// What to say, without any marker — the marker belongs to whatever draws it, and the
    /// two surfaces that draw one do not have the same fonts available.
    pub text: String,
}

/// The structured half of [`receipt`], and the one the panel above the composer reads.
pub fn receipt_of(typed: &str, result: &Result<Value, String>) -> Receipt {
    match result {
        Err(message) => Receipt { ok: false, text: message.clone() },
        Ok(Value::Null) => Receipt { ok: true, text: typed.to_string() },
        Ok(value) => Receipt { ok: true, text: format!("{typed} — {value}") },
    }
}

/// What the pane's **log** says back after a command ran.
///
/// Pure, and here rather than in the view, so the sentence a human reads is pinned by a test
/// instead of being verified by reading it. Formatted from [`receipt_of`] rather than beside
/// it, so the line in the log and the band above the composer cannot come to disagree about
/// what happened.
pub fn receipt(typed: &str, result: &Result<Value, String>) -> String {
    let receipt = receipt_of(typed, result);
    if receipt.ok {
        format!("✓ {}", receipt.text)
    } else {
        receipt.text
    }
}

// ---------------------------------------------------------------------------
// Candidates — what a half-typed line could become next
// ---------------------------------------------------------------------------

/// What kind of word a [`Candidate`] is, so a renderer can place it without re-deriving
/// where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// A verb — the word straight after the slash. `group` is [`Entry::group`], which is the
    /// pie menu's root ring and the heading [`Registry::help_lines`] already prints.
    Verb { group: String, lane: Lane },
    /// The name of an optional argument, typed as a keyword before its value.
    Keyword,
    /// One option of an argument whose value space is closed — a `Choice`'s own table, or a
    /// `Bool`'s two words.
    Value,
}

/// One thing a person could type next, as a value rather than as a rendered row.
///
/// 🚨 **Three surfaces draw this list and there is one generator**: the popup above the
/// composer, the pie menu (`groups` → `verbs_in` → an argument's `Choice`), and `/help`. A
/// renderer that needed its own generator would be a second vocabulary, which is the exact
/// failure this module exists to prevent — so nothing here is a formatted line, and nothing
/// here imports egui.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The word itself — `theme`, `chocolate`, `distance`.
    pub label: String,
    /// One line about it, taken from the table rather than written here. Empty when the
    /// label is its own description: a `Choice` option stands for nothing but itself.
    pub doc: String,
    /// 🚨 **The whole composer line this candidate would produce**, not the fragment it adds.
    /// Accepting is therefore `line = candidate.completion`, and asking
    /// [`Registry::candidates`] again with it yields the next ring — which is the whole of
    /// what a renderer has to implement to walk the hierarchy.
    pub completion: String,
    pub kind: CandidateKind,
    /// Accepting this leaves a line that is a **complete, valid command** — verb resolved and
    /// every required argument satisfied. Derived by asking [`Registry::resolve`] about
    /// [`Candidate::completion`], so it cannot drift from what Enter would actually do.
    pub completes: bool,
}

/// Which word of the line is being narrowed.
///
/// ⚠️ The `ArgSpec` is carried whole rather than reduced to a list of options, because the
/// arguments with *no* closed value space are precisely the ones a renderer has to treat
/// differently — `Float` is a dial with its band already stated, `Int` and `Text` need a
/// field. [`Palette::hint`] is the sentence; this is the fact.
#[derive(Debug, Clone, PartialEq)]
pub enum Slot {
    /// The line is naming a verb.
    Verb,
    /// The verb is settled and the next word is this argument's value.
    Value { verb: String, arg: ArgSpec },
    /// Every required argument is satisfied. The next word, if there is one, names one of the
    /// verb's remaining optional arguments.
    Keyword { verb: String },
}

/// Every continuation of one partial line, with what the line is currently asking for.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub slot: Slot,
    /// The partial word being narrowed. Empty when the line ends in whitespace, which is what
    /// makes `/theme` and `/theme ` different questions.
    pub typed: String,
    /// In table order, narrowed by [`Palette::typed`]. May be empty — a `Float` argument has
    /// no options to offer and a mistyped verb has none left.
    pub candidates: Vec<Candidate>,
    /// The line **as it stands** already resolves to a command, so Enter would run it.
    pub runnable: bool,
}

impl Palette {
    /// Nothing to show: no continuations, and nothing to say about the slot either.
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty() && self.hint().is_none()
    }

    /// What to type when there is nothing to choose from — the one case a list cannot answer.
    /// `None` whenever [`Palette::candidates`] is the whole truth.
    pub fn hint(&self) -> Option<String> {
        let Slot::Value { arg, .. } = &self.slot else { return None };
        match &arg.kind {
            ArgKind::Choice(_) | ArgKind::Bool => None,
            kind => Some(format!("{}: {}", arg.name, value_space(kind))),
        }
    }

    /// The verb this line has settled on, if it has settled on one.
    pub fn verb(&self) -> Option<&str> {
        match &self.slot {
            Slot::Verb => None,
            Slot::Value { verb, .. } | Slot::Keyword { verb } => Some(verb),
        }
    }

    /// 🚨 **The guard on running a command nobody pressed Enter for.**
    ///
    /// James asked for this — *"it will just execute the thing as soon as it knows what we
    /// want"* — and the thing that makes it safe rather than terrifying is that "knows what we
    /// want" is a *provable* state, not a guess: exactly one continuation is left **and** that
    /// continuation completes the command. `/s` leaves only `surface`, which takes no
    /// arguments, so there is nothing else the line could have meant. `/t` leaves only
    /// `theme`, which still needs a value — so it does **not** fire, because firing there
    /// would run a command while the hand is still typing its argument.
    ///
    /// ⚠️ **Off unless `enabled`.** The caller owns that switch; this owns the rule.
    pub fn autorun(&self, enabled: bool) -> Option<&Candidate> {
        if !enabled {
            return None;
        }
        match &self.candidates[..] {
            [only] if only.completes => Some(only),
            _ => None,
        }
    }
}

/// An argument's value space, in words. ASCII on purpose — see the glyph allowlist in
/// `conversation_view`'s tests; a range written with `…` would ship as a box.
fn value_space(kind: &ArgKind) -> String {
    match kind {
        ArgKind::Choice(options) => options.join(" | "),
        ArgKind::Float { min, max } => format!("a number from {min} to {max}"),
        ArgKind::Int => "a whole number".to_string(),
        ArgKind::Bool => "true or false".to_string(),
        ArgKind::Text => "any word".to_string(),
    }
}

/// Case-insensitive **prefix**, which is the whole matching rule. See
/// [`Registry::candidates`] for why it is not a subsequence.
fn narrows(word: &str, typed: &str) -> bool {
    word.to_ascii_lowercase().starts_with(&typed.to_ascii_lowercase())
}

impl Registry {
    /// Every continuation of a half-typed composer line, or `None` if the line is not a
    /// command line at all.
    ///
    /// 🚨 **`None` is the load-bearing answer, because the composer is also where a human
    /// talks to the agent.** A panel that opened while prose was being typed would be
    /// intolerable, so the test is the same one [`Registry::resolve`] applies in its rules 1
    /// and 2 and no other: the line must begin with `/`, and `//` is an escape that means the
    /// line is a message. Everything else — a sentence *mentioning* a command, a line with a
    /// word in front of the slash — answers `None` and the panel is not drawn.
    ///
    /// ⚠️ A bare `/` answers `Some` with the whole table even though `resolve` calls it a
    /// message. Those are not in conflict: showing the choices is what `/` is *for*, and
    /// nothing is run until the line is a command.
    ///
    /// # Prefix, not fuzzy
    ///
    /// Matching is a case-insensitive **prefix**, in table order. A subsequence match
    /// (`/pst` → `posture`) is faster on a long list and this list is nine verbs long, so the
    /// speed is not on offer; what it would buy instead is the ability for a line that reads
    /// like a typo to match a distant verb — and with [`Palette::autorun`] available, a
    /// surprising match becomes a surprising *action*. Prefix is also what makes "press
    /// another key and it narrows" literally true, which is the property being copied from
    /// which-key. Fuzzy is not reachable and is not built; [`narrows`] is the one function
    /// that would have to change.
    ///
    /// # Where the line's words go
    ///
    /// The grammar is [`parse_args`]' exactly, read tolerantly: required arguments positional
    /// in declared order, then optional ones keyword-tagged, with an optional `Bool` a bare
    /// flag. A word that names no argument stops the walk with no candidates rather than
    /// guessing — `resolve` will refuse that line, and offering a continuation for a line
    /// that cannot run would be inventing one.
    pub fn candidates(&self, line: &str) -> Option<Palette> {
        let head = line.trim_start();
        let rest = head.strip_prefix('/')?;
        if rest.starts_with('/') {
            return None;
        }
        // Trailing whitespace is what separates "still typing this word" from "on to the
        // next one", and `split_whitespace` throws it away — so it is read first.
        let open = rest.is_empty() || rest.ends_with(char::is_whitespace);
        let mut words: Vec<&str> = rest.split_whitespace().collect();
        let typed = if open { "" } else { words.pop().unwrap_or("") };
        // Everything before the partial word, so a completion is built by appending rather
        // than by re-rendering a line the human already has in front of them.
        let stem = &head[..head.len() - typed.len()];
        let runnable = matches!(self.resolve(line), Resolved::Run { .. });

        let Some(verb_word) = words.first() else {
            let candidates = self
                .verbs()
                .into_iter()
                .filter(|verb| narrows(verb, typed))
                .filter_map(|verb| self.entry(verb))
                .map(|entry| self.verb_candidate(entry, stem))
                .collect();
            return Some(Palette { slot: Slot::Verb, typed: typed.to_string(), candidates, runnable });
        };
        let Some(entry) = self.entry(verb_word) else {
            // A verb this table does not have. `resolve` names the known set when Enter is
            // pressed; there is nothing here to complete.
            return Some(Palette {
                slot: Slot::Verb,
                typed: typed.to_string(),
                candidates: Vec::new(),
                runnable,
            });
        };

        let required: Vec<&ArgSpec> = entry.args().iter().filter(|a| a.required).collect();
        let optional: Vec<&ArgSpec> = entry.args().iter().filter(|a| !a.required).collect();
        let mut filled = 0usize;
        let mut used: Vec<&str> = Vec::new();
        let mut awaiting: Option<&ArgSpec> = None;
        // The *values* of the required arguments, not just how many are in — an argument whose
        // ring depends on an earlier one needs the word, and until [`NarrowFn`] there was
        // nothing that did, so only the count was kept.
        let mut positional: Vec<&str> = Vec::new();
        for word in &words[1..] {
            if filled < required.len() {
                filled += 1;
                positional.push(word);
                continue;
            }
            if let Some(arg) = awaiting.take() {
                used.push(arg.name.as_str());
                continue;
            }
            match optional.iter().copied().find(|a| a.name == *word) {
                Some(arg) if matches!(arg.kind, ArgKind::Bool) => used.push(arg.name.as_str()),
                Some(arg) => awaiting = Some(arg),
                None => {
                    return Some(Palette {
                        slot: Slot::Keyword { verb: entry.verb().to_string() },
                        typed: typed.to_string(),
                        candidates: Vec::new(),
                        runnable,
                    })
                }
            }
        }

        let verb = entry.verb().to_string();
        let (slot, candidates) = if filled < required.len() {
            let arg = required[filled];
            let more = filled + 1 < required.len() || !optional.is_empty();
            let candidates = self.value_candidates(entry, arg, &positional, typed, stem, more);
            (Slot::Value { verb, arg: arg.clone() }, candidates)
        } else if let Some(arg) = awaiting {
            let more = used.len() + 1 < optional.len();
            let candidates = self.value_candidates(entry, arg, &positional, typed, stem, more);
            (Slot::Value { verb, arg: arg.clone() }, candidates)
        } else {
            let candidates = optional
                .iter()
                .copied()
                .filter(|arg| !used.contains(&arg.name.as_str()))
                .filter(|arg| narrows(&arg.name, typed))
                .map(|arg| self.keyword_candidate(arg, stem))
                .collect();
            (Slot::Keyword { verb }, candidates)
        };
        Some(Palette { slot, typed: typed.to_string(), candidates, runnable })
    }

    /// A verb, as the word plus the line typing it would produce. The trailing space is what
    /// opens the next ring the instant the verb is accepted — and it is absent for a verb
    /// that takes nothing, because a line ending in a space it will never fill reads as
    /// unfinished when it is not.
    fn verb_candidate(&self, entry: &Entry, stem: &str) -> Candidate {
        let tail = if entry.args().is_empty() { "" } else { " " };
        let completion = format!("{stem}{}{tail}", entry.verb());
        Candidate {
            label: entry.verb().to_string(),
            doc: entry.doc().to_string(),
            completes: matches!(self.resolve(&completion), Resolved::Run { .. }),
            completion,
            kind: CandidateKind::Verb {
                group: entry.group().to_string(),
                lane: entry.lane(),
            },
        }
    }

    /// One optional argument's name. Always trailing-spaced: a value-taking keyword needs its
    /// value next and a flag may be followed by another keyword.
    fn keyword_candidate(&self, arg: &ArgSpec, stem: &str) -> Candidate {
        let completion = format!("{stem}{} ", arg.name);
        Candidate {
            label: arg.name.clone(),
            doc: value_space(&arg.kind),
            completes: matches!(self.resolve(&completion), Resolved::Run { .. }),
            completion,
            kind: CandidateKind::Keyword,
        }
    }

    /// The options of an argument whose value space is closed. Empty for `Float`, `Int` and
    /// `Text`, which is not a gap — [`Palette::hint`] answers those, and a renderer with a
    /// dial reads the `ArgKind` off the slot.
    fn value_candidates(
        &self,
        entry: &Entry,
        arg: &ArgSpec,
        positional: &[&str],
        typed: &str,
        stem: &str,
        more: bool,
    ) -> Vec<Candidate> {
        // The entry's own hook first, and its `Some` wins outright — including `Some(vec![])`,
        // which is the true answer for a tab with no panels and must not fall through to the
        // declared union. See [`NarrowFn`].
        let options: Vec<(String, String)> = match entry.narrow.and_then(|f| f(&arg.name, positional))
        {
            Some(narrowed) => narrowed,
            None => match &arg.kind {
                ArgKind::Choice(options) => {
                    options.iter().map(|o| (o.clone(), String::new())).collect()
                }
                ArgKind::Bool => {
                    vec![("true".to_string(), String::new()), ("false".to_string(), String::new())]
                }
                ArgKind::Float { .. } | ArgKind::Int | ArgKind::Text => Vec::new(),
            },
        };
        options
            .into_iter()
            .filter(|(option, _)| narrows(option, typed))
            .map(|(option, doc)| {
                let completion = format!("{stem}{option}{}", if more { " " } else { "" });
                Candidate {
                    label: option,
                    doc,
                    completes: matches!(self.resolve(&completion), Resolved::Run { .. }),
                    completion,
                    kind: CandidateKind::Value,
                }
            })
            .collect()
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
            ["background", "patch", "camera", "camera.read", "surface", "help", "organon"]
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
        assert_eq!(view, ["surface", "help", "organon"]);
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

    // -----------------------------------------------------------------------
    // Candidates
    // -----------------------------------------------------------------------

    fn labels(line: &str) -> Vec<String> {
        registry()
            .candidates(line)
            .map(|p| p.candidates.iter().map(|c| c.label.clone()).collect())
            .unwrap_or_default()
    }

    fn palette(line: &str) -> Palette {
        registry().candidates(line).unwrap_or_else(|| panic!("{line:?} is a command line"))
    }

    /// 🚨 **The rule the composer's other job depends on: a panel must not open over prose.**
    ///
    /// The test is `resolve`'s own — begins with `/`, and `//` escapes — so the two cannot
    /// come apart. Every line here is one a human types *at an agent*, and none of them may
    /// produce a candidate list.
    #[test]
    fn a_line_that_is_not_a_command_offers_nothing() {
        for message in [
            "what does /surface do?",
            "use /help if you get stuck",
            "the /background verb takes a material",
            "surface",
            "",
            "   ",
            // The escape is a message by construction, so the panel stays shut for it too —
            // and it is the answer to "how do I type a line that really starts with a slash".
            "//surface",
            "//",
        ] {
            assert!(
                registry().candidates(message).is_none(),
                "{message:?} belongs to the agent, so nothing may pop up over it"
            );
        }
    }

    /// `/` offers **every** verb — the "show me my choices" keystroke, and the reason the
    /// panel opens for a line `resolve` still calls a message.
    #[test]
    fn a_bare_slash_offers_the_whole_table() {
        let bare = palette("/");
        assert_eq!(bare.slot, Slot::Verb);
        assert_eq!(
            bare.candidates.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            registry().verbs(),
            "in table order, and nothing left out"
        );
        assert!(!bare.runnable, "a bare slash names no verb, so Enter runs nothing");
        // Each carries its doc and its group, which is what the pie menu's first two rings
        // are built from.
        let background = &bare.candidates[0];
        assert_eq!(background.doc, "What sits behind the glyphs");
        assert_eq!(
            background.kind,
            CandidateKind::Verb { group: "console".into(), lane: Lane::Console }
        );
        assert_eq!(
            bare.candidates.iter().find(|c| c.label == "surface").unwrap().kind,
            CandidateKind::Verb { group: "view".into(), lane: Lane::View },
            "the view lane is in the same list, marked"
        );
    }

    /// A prefix narrows to exactly the right set, and the next letter narrows again.
    #[test]
    fn a_prefix_narrows_to_exactly_the_verbs_that_start_with_it() {
        assert_eq!(labels("/c"), ["camera", "camera.read"]);
        assert_eq!(labels("/camera."), ["camera.read"], "the dot is part of the verb");
        assert_eq!(labels("/b"), ["background"]);
        assert_eq!(labels("/s"), ["surface"]);
        assert_eq!(labels("/BA"), ["background"], "matching is case-insensitive");
        assert!(labels("/z").is_empty(), "and a letter no verb starts with offers nothing");
        // ⚠️ Prefix, NOT subsequence: `pth` is inside `patch` in order and must not match it.
        assert!(labels("/pth").is_empty(), "a subsequence is not a prefix");
    }

    /// Accepting a candidate is `line = completion` — and asking again with it yields the
    /// next ring. **That loop is the whole of what a renderer implements.**
    #[test]
    fn a_completion_is_the_whole_line_and_asking_again_walks_the_hierarchy() {
        let [surface] = &palette("/s").candidates[..] else { panic!("one verb starts with s") };
        assert_eq!(surface.completion, "/surface", "no argument, so no trailing space");
        assert!(surface.completes, "and the line it produces is a whole command");

        let [background] = &palette("/b").candidates[..] else { panic!("one verb starts with b") };
        assert_eq!(background.completion, "/background ", "an argument follows, so a space");
        assert!(!background.completes, "…and the line it produces is NOT yet a command");

        // Ask again with what accepting produced: the argument's own value space, closed and
        // already validated, with no second table anywhere.
        let values = palette(&background.completion);
        assert_eq!(values.candidates.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), [
            "graphite", "slate"
        ]);
        assert!(matches!(&values.slot, Slot::Value { verb, arg } if verb == "background" && arg.name == "name"));
        let slate = values.candidates.iter().find(|c| c.label == "slate").unwrap();
        assert_eq!(slate.completion, "/background slate");
        assert!(slate.completes, "which is a command, so Enter would run it");
        assert!(matches!(registry().resolve(&slate.completion), Resolved::Run { .. }));
    }

    /// Values complete like verbs do — the half that makes the surface feel finished, and it
    /// is free, because a `Choice` **is** the list.
    #[test]
    fn a_value_completes_from_the_arguments_own_table() {
        assert_eq!(labels("/background g"), ["graphite"]);
        assert_eq!(labels("/background "), ["graphite", "slate"]);
        assert_eq!(labels("/patch 12 8 s"), ["scene"], "the third required argument, in order");
        assert!(labels("/background chartreuse").is_empty(), "a value no table has");
        // A required `Bool` would be a two-word ring; the fixture has none, so the closed
        // spaces exercised here are `Choice`'s.
        assert!(palette("/background g").verb() == Some("background"));
    }

    /// An argument with no closed value space says what to type instead of listing nothing.
    /// ⚠️ The band comes off `ArgKind::Float` rather than being restated.
    #[test]
    fn an_open_argument_offers_a_hint_instead_of_a_list() {
        let rows = palette("/patch ");
        assert!(rows.candidates.is_empty(), "an Int has no options to offer");
        assert_eq!(rows.hint().as_deref(), Some("up: a whole number"));
        assert!(!rows.is_empty(), "…but the panel still has something to say");

        let yaw = palette("/camera yaw ");
        assert_eq!(yaw.hint().as_deref(), Some("yaw: a number from -180 to 180"));
        let Slot::Value { arg, .. } = &yaw.slot else { panic!("a value slot") };
        assert!(
            matches!(arg.kind, ArgKind::Float { min, max } if min == -180.0 && max == 180.0),
            "and the band itself is on the slot, for a renderer that draws a dial"
        );
        assert!(!yaw.hint().unwrap().chars().any(|c| !c.is_ascii()), "ASCII, per the glyph rule");
    }

    /// The optional half of the grammar: keyword names are candidates, a flag is a bare word,
    /// and one already given is not offered twice.
    #[test]
    fn optional_arguments_are_offered_by_name_and_only_once() {
        assert_eq!(labels("/camera "), ["reset", "yaw", "distance"]);
        assert_eq!(labels("/camera d"), ["distance"]);
        assert_eq!(
            labels("/camera reset "),
            ["yaw", "distance"],
            "a flag consumes one word and the flag itself is not offered again"
        );
        assert_eq!(labels("/camera yaw 30 "), ["reset", "distance"], "…nor is a filled axis");
        let flag = palette("/camera ").candidates[0].clone();
        assert_eq!(flag.completion, "/camera reset ");
        assert!(flag.completes, "a flag alone is already a whole camera command");
        assert_eq!(flag.kind, CandidateKind::Keyword);
        assert_eq!(
            palette("/camera ").candidates[1].doc,
            "a number from -180 to 180",
            "a keyword's doc is its value space, derived from the schema"
        );
        // A word naming no argument stops the walk rather than guessing: `resolve` refuses
        // that line, and completing a line that cannot run would be inventing a grammar.
        assert!(labels("/camera sideways ").is_empty());
    }

    /// `runnable` is `resolve`'s own answer about the line as it stands, which is what a
    /// surface needs to know whether Enter would do anything.
    #[test]
    fn runnable_is_what_enter_would_actually_do() {
        assert!(!palette("/theme").runnable, "the fixture has no theme; an unknown verb");
        assert!(!palette("/background").runnable, "a required argument is missing");
        assert!(palette("/background slate").runnable);
        assert!(palette("/camera").runnable, "every axis optional, so the bare verb runs");
        assert!(!palette("/camera distance").runnable, "…but a keyword without its value does not");
    }

    /// 🚨 **The auto-execute guard, and the case it must refuse.**
    ///
    /// Firing on `/s` is what was asked for: one continuation left, and it needs nothing
    /// else. Firing on `/b` would run `background` with no material — a command executing
    /// while the hand is still typing its argument, which is the failure this rule exists to
    /// prevent. Off unless the caller says otherwise, either way.
    #[test]
    fn auto_execute_fires_only_on_a_complete_command() {
        let unique_and_complete = palette("/s");
        assert_eq!(
            unique_and_complete.autorun(true).map(|c| c.completion.as_str()),
            Some("/surface"),
            "one candidate, and it completes: this is the case James asked for"
        );
        assert_eq!(
            unique_and_complete.autorun(false),
            None,
            "…and it is off unless it is switched on"
        );

        assert_eq!(
            palette("/b").autorun(true),
            None,
            "unique, but `background` still needs a material — it must NOT fire"
        );
        assert_eq!(palette("/c").autorun(true), None, "two candidates is not knowing what we want");
        assert_eq!(palette("/z").autorun(true), None, "and none is not either");
        // The value ring fires the same way, which is what makes `/theme ch` finish itself.
        assert_eq!(
            palette("/background g").autorun(true).map(|c| c.completion.as_str()),
            Some("/background graphite")
        );
        assert_eq!(
            palette("/patch ").autorun(true),
            None,
            "an argument with no closed value space can never satisfy the guard"
        );
    }

    /// Whitespace decides which question is being asked, and `split_whitespace` throws it
    /// away — so it is read before the split. Both spellings must work from a real composer,
    /// which may hold leading spaces.
    #[test]
    fn a_trailing_space_moves_the_line_to_the_next_word() {
        assert_eq!(labels("/background"), ["background"], "still naming the verb");
        assert_eq!(labels("/background "), ["graphite", "slate"], "…now naming its value");
        // A leading space is trimmed the way a send trims it, and the completion comes back
        // without it rather than preserving something the line will not keep.
        assert_eq!(palette("  /b").candidates[0].completion, "/background ");
    }

    /// The receipt's two halves are one value: the log's sentence is formatted from the
    /// structure the panel reads, so they cannot come to disagree.
    #[test]
    fn a_receipt_is_a_value_before_it_is_a_sentence() {
        assert_eq!(
            receipt_of("/surface", &Ok(Value::Null)),
            Receipt { ok: true, text: "/surface".into() }
        );
        assert_eq!(
            receipt_of("/camera distance 9", &Err("out of range".to_string())),
            Receipt { ok: false, text: "out of range".into() },
            "a refusal carries no marker of its own — the surface decides how loud it is"
        );
        for (typed, result) in [
            ("/surface", Ok(Value::Null)),
            ("/background slate", Ok(json!({ "accepted": "background slate" }))),
            ("/camera distance 9", Err("out of range".to_string())),
        ] {
            let structured = receipt_of(typed, &result);
            let sentence = receipt(typed, &result);
            assert!(
                sentence.ends_with(&structured.text),
                "the log's line is the structured text, marked: {sentence}"
            );
        }
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

    // -----------------------------------------------------------------------
    // `/organon` — the two rings, and the narrowing that makes them two
    // -----------------------------------------------------------------------

    /// The first ring is Organon's tab bar, in Organon's own order — James's eight words.
    /// Derived from `UiTab::ALL`, never listed here, so a tab added to the editor appears in
    /// the ring with no edit to this crate.
    #[test]
    fn the_first_ring_is_organons_tab_bar() {
        assert_eq!(
            labels("/organon "),
            UiTab::ALL.iter().map(|t| t.word()).collect::<Vec<_>>()
        );
        assert_eq!(
            labels("/organon "),
            ["generator", "motion", "environment", "look", "synth", "audio", "settings", "mind"]
        );
    }

    /// The second ring changes with the first — the whole point of [`NarrowFn`]. `look` offers
    /// the Look tab's panels; a tab whose cards are not joined to the table yet offers nothing
    /// at all rather than the declared union.
    #[test]
    fn the_second_ring_depends_on_the_first() {
        let look = labels("/organon look ");
        assert_eq!(look.len(), 25, "every card the editor draws on the Look tab");
        assert_eq!(look[0], "surface");
        assert!(look.contains(&"bloom".to_string()));
        assert!(
            labels("/organon motion ").is_empty(),
            "an unjoined tab offers nothing, not every slug in the union"
        );
    }

    /// A candidate carries the panel's real heading, and says out loud when choosing it would
    /// open nothing — before the choice, not after.
    #[test]
    fn a_declared_panel_says_so_in_the_ring() {
        let reg = registry();
        let ring = reg.candidates("/organon look ").unwrap().candidates;
        let surface = ring.iter().find(|c| c.label == "surface").unwrap();
        assert_eq!(surface.doc, "Surface — not transplanted yet");
        let bloom = ring.iter().find(|c| c.label == "bloom").unwrap();
        assert_eq!(bloom.doc, "Bloom — not transplanted yet");
    }

    /// James's own example: the `l` completes to `look` on its own, and the ring under it
    /// becomes the Look tab's panels. Both halves are the machinery that already shipped —
    /// [`Palette::autorun`] and [`Candidate::completion`] being the whole line — so this pins
    /// the composition rather than a new mechanism.
    #[test]
    fn typing_l_settles_on_look_and_opens_its_panels() {
        let reg = registry();
        let palette = reg.candidates("/organon l").unwrap();
        assert_eq!(palette.candidates.len(), 1, "only `look` begins with an l");
        let only = &palette.candidates[0];
        assert_eq!(only.label, "look");
        assert_eq!(only.completion, "/organon look ");
        assert!(!only.completes, "a tab alone is not a command — the panel is still needed");
        // …and asking again with that line is the next ring. One loop, no new machinery.
        assert_eq!(labels(&only.completion).len(), 25);
    }

    /// `su` leaves exactly one panel, and accepting it completes the command — which is what
    /// makes `/organon look su` fire without Enter under [`Palette::autorun`]. This is the
    /// property `panels::no_slug_is_a_prefix_of_another` exists to protect.
    #[test]
    fn a_lone_panel_completes_the_whole_command() {
        let reg = registry();
        let palette = reg.candidates("/organon look su").unwrap();
        assert_eq!(palette.candidates.len(), 1);
        let only = &palette.candidates[0];
        assert_eq!(only.completion, "/organon look surface");
        assert!(only.completes, "both rings are filled, so Enter would run it");
        assert_eq!(palette.autorun(true).map(|c| c.label.as_str()), Some("surface"));
    }

    /// ⚠️ The union hole, stated as a test rather than left to be discovered: the declared
    /// `Choice` is every slug on every tab, so a wrong pair passes the schema. See
    /// [`NarrowFn`] — the view lane is what refuses it, and it names the mismatch.
    #[test]
    fn a_wrong_pair_passes_the_schema_and_is_refused_later() {
        let reg = registry();
        assert!(
            matches!(reg.resolve("/organon motion surface"), Resolved::Run { .. }),
            "`surface` is a real slug, so the schema cannot see the mismatch"
        );
        // A word that is no panel at all *is* caught here, because that the schema can see.
        assert!(matches!(reg.resolve("/organon look nonesuch"), Resolved::Refused(_)));
    }

    /// The narrowing is scoped to the one verb that declares it. Every other entry's rings are
    /// exactly what its `ArgKind` says, unchanged by this addition.
    #[test]
    fn narrowing_does_not_leak_into_other_verbs() {
        let reg = registry();
        assert_eq!(labels("/background "), ["graphite", "slate"]);
        assert!(reg.entry("background").is_some_and(|e| e.narrow.is_none()));
    }
}
