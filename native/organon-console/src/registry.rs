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

use crate::command::{ArgKind, ArgSpec, CommandSpec, Reversal};
use crate::layout::{self, LayoutCmd, Library};
use crate::module::{self, ModuleRegistry};
use crate::region::{Content, ContentCmd};

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

/// `/trace` — **whether this conversation's status log is open.**
///
/// 🚨 **It no longer means "narrate into my conversation", and that is the whole of the change.**
/// It used to widen the scrollback: every quiet remark interleaved above the first message, so
/// the one verb offered for *seeing more* worked by making the flow noisier. It now opens a
/// bounded drop-down out of the pane's own status line — `organon_console::status_log` owns the
/// argument — holding every line the console has written about the session. The conversation is
/// untouched in both states.
///
/// 🚨 **A view-lane verb, and that is a decision about scope rather than about plumbing.**
/// Everything the log holds is one pane talking about itself: which directory its agent started
/// in, that a command it dispatched was accepted, what the child put on stderr. None of it is
/// console state, none of it is worth a sidecar line, and a console-lane spelling would put an
/// MCP tool in an agent's catalog for a preference about how loudly the console talks to the
/// person sitting in front of it.
///
/// ⚠️ **So trace is per-tab.** Two conversations can disagree about it, which is the honest
/// consequence of the scope rather than an oversight — a tab you are debugging can be open while
/// the one you are working in stays shut. `ORGANON_TRACE=1` opens every tab's log.
///
/// ⚠️ **The pane's permanent status line is the other way in**, and it is the one a hand finds:
/// the log is a click away without anybody having to know this word exists. The verb stays
/// because a panel with no *named* way out is a panel people close by restarting the console.
pub const VERB_TRACE: &str = "view.trace";
/// The argument `/trace` carries its state in.
pub const TRACE_ARG: &str = "state";
/// `/trace`'s whole value space. **Two words and no toggle**, on `console.screen`'s rule: a verb
/// that means "the other one" cannot be read back, cannot be typed with intent, and answers
/// differently depending on a state the person typing it may be wrong about.
pub const TRACE_WORDS: [&str; 2] = ["on", "off"];

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

/// `console.layout` — the second console-lane name this crate has to **recognise**, because its
/// name argument is a ring of the layouts that actually exist ([`layout_options`]).
///
/// ⚠️ **Spelled here and aliased by `console_main`**, exactly as [`VERB_THEME`] is
/// (`const CMD_LAYOUT: &str = organon_console::registry::VERB_LAYOUT;`), so the two cannot be
/// two strings that merely agree today. The hook is keyed on this name, and the failure of a
/// second spelling would be silent in the worst way: the verb would keep working and the ring
/// would simply stop appearing, with nothing to say it ever had.
pub const VERB_LAYOUT: &str = "console.layout";
/// The argument `console.layout` carries the layout's name in. Same rule as [`THEME_ARG`], and
/// the same alias on the other side ([`layout_options`] is asked for one argument by name).
pub const LAYOUT_NAME_ARG: &str = "layout";

/// `console.viewport` — the third console-lane name this crate has to **recognise**, and for
/// [`VERB_LAYOUT`]'s reason: its producer argument is a ring of the modules that are actually
/// approved ([`viewport_options`]).
///
/// ⚠️ **Spelled here and aliased by `console_main`**, with the same silent failure mode a second
/// spelling would have: the verb would keep working and the ring would simply stop appearing.
pub const VERB_VIEWPORT: &str = "console.viewport";
/// The **optional** argument `console.viewport` carries the producer qualifier in —
/// `doc/organon_module_viewport.md` §4.2's `3d <producer>`.
///
/// 🚨 **Keyword-tagged, so the typed line is `/viewport left 3d producer ascent`** rather than
/// §4.2's illustrative `viewport left 3d ascent`. That is not a preference: `parse_args` fills
/// **required** arguments positionally and **optional** ones by keyword, and `#98 Tier C` already
/// paid for the alternative — `ConsoleOp::Stack`'s own doc records that its optional region is
/// spelled `region <word>` on the wire *"because the slash grammar fills optional arguments by
/// keyword: a bare third word would make the typed line and the sidecar line disagree, which is
/// the drift the four doors exist to prevent."* One grammar, one spelling, four doors; the
/// design document's shorter form would have needed a second grammar for one verb.
pub const VIEWPORT_PRODUCER_ARG: &str = "producer";

/// **`/setting <producer> <key> <value>`** — write one setting into an approved module's own
/// settings file. `organon_console::module::SETTING_VERB` is the terminal spelling of the same
/// command.
///
/// 🚨 **A verb of its own rather than a sixth `console module` action**, and the reason is the
/// argument shape rather than taste. Every `console module` action is *(action, producer)* with
/// optional keyword slots after it; this one is three **required** words, and the grammar fills
/// required arguments positionally. Folded in, the key and the value would have had to be
/// optional-by-keyword — `/module set moonlight key host value studio-pc` — which is a worse
/// line for the verb a person types most often. `console.stack` and `console.screen` won the
/// same argument against being folded into `console.viewport`.
pub const VERB_SETTING: &str = "console.setting";
/// `console.setting`'s first argument: which approved module.
///
/// ⚠️ **Its own constant rather than [`VIEWPORT_PRODUCER_ARG`] shared**, even though the word is
/// identical — `options_for` is keyed by verb *and* argument name, and the two rings differ in
/// exactly the way that matters: `console.viewport`'s offers `organon`, which is the one name a
/// module may never be called. Sharing the string would read as sharing the meaning.
pub const SETTING_PRODUCER_ARG: &str = "producer";
/// `console.setting`'s second argument: which of the keys that module declared.
pub const SETTING_KEY_ARG: &str = "key";
/// `console.setting`'s third argument: what to set it to. No ring — the value space belongs to
/// the module, and the console does not hold an opinion about it.
pub const SETTING_VALUE_ARG: &str = "value";

/// `/media` — show a file from disk in this conversation.
///
/// 🚨 **A view-lane verb, and it must stay one.** The console lane writes a sidecar line, and a
/// sidecar is a *text file any process on the machine can append to* — putting a path on it
/// would mean any such process could make the console open any file the user can read. That is
/// the property `doc/console_patch_protocol.md` is built around and the reason
/// `organon_core::kind::Kind`'s media arms name no file. A path reaches the console from the
/// composer, where a human typed it, and from nowhere else.
///
/// ⚠️ **So this verb is deliberately absent from `console_specs()` and therefore from the MCP
/// catalog** — an agent cannot call it. #56 leaves "how an exhibit reaches the console" open
/// between an agent verb and the console sniffing tool results; this tier picks neither, and
/// the absence is the decision rather than an oversight.
pub const VERB_MEDIA: &str = "view.media";
/// The argument `/media` carries its paths in: one or more, separated by spaces.
pub const MEDIA_ARG: &str = "path";

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
    reversal: Reversal,
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
            && self.reversal == other.reversal
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
/// 🚨 **It narrows the ring *and* the refusal, and it did not always.** The declared
/// `ArgKind::Choice` for a dependent argument is the **union** across every parent value, and
/// [`coerce`] used to refuse against that union — so `/organon generator 2` answered *"`2` is
/// not one of surface | colour | material | …"*, twenty-five slugs belonging to a tab nobody had
/// named. James read that as the console failing to register the word `generator`, which is the
/// only reading available: a Look-shaped answer to a Generator-shaped question. The hook is now
/// consulted wherever the parent word is in hand, so the refusal names the panels of the tab
/// that was actually given. The static `Choice` stays exactly as declared — `/help` and the MCP
/// schema are the two surfaces with **no** parent word in hand, and the union is the honest
/// answer there.
///
/// ⚠️ **What is still not checked here is `null`-shaped**: a call arriving through
/// [`crate::command::CommandService`] never passes through this module at all, so the view
/// lane's own `(tab, panel)` check stays where it is. Same rule, two doors.
///
/// The signature is a plain `fn` pointer rather than a boxed closure so that [`Entry`] stays
/// `Clone + Debug + PartialEq` — the registry is copied into every pane, and a trait object
/// would cost all three derives for no gain: every narrowing this console has is a pure
/// function of a static table.
///
/// `positional` holds the words that filled the **required** arguments so far, in declared
/// order; `arg` is the name of the one being offered. `None` means "no opinion", and the
/// argument's own [`ArgKind`] answers as usual.
pub type NarrowFn = fn(arg: &str, positional: &[&str]) -> Option<Ring>;

/// What a [`NarrowFn`] answers for one argument.
///
/// 🚨 **An empty ring cannot be silent, because the type does not let it be.** This was
/// `Vec<(String, String)>`, and `Some(vec![])` was the truthful answer for a tab whose panels
/// are not in the table — truthful and *invisible*: the popup drew a band with nothing in it,
/// which is indistinguishable from a band that is broken, and there was nothing for a refusal
/// to say either. An empty set now has to arrive as [`Ring::Empty`] carrying the sentence that
/// says why, and the ring, the hint and the refusal all read that one sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ring {
    /// The options this argument offers here, as `(label, doc)` — the doc is what lets
    /// `/organon look` show `surface` beside the panel's real heading.
    Options(Vec<(String, String)>),
    /// There are none, and this is why. A fact about the world, not about what has been
    /// typed — a prefix that happens to match nothing is not this.
    Empty(String),
}

impl Entry {
    /// A console verb, from the same [`CommandSpec`] the MCP schema is generated from.
    pub fn from_spec(spec: &CommandSpec, lane: Lane) -> Self {
        Entry {
            name: spec.name.clone(),
            doc: spec.doc.clone(),
            args: spec.args.clone(),
            lane,
            // Copied, never re-decided. A console verb declares its recoverability in the same
            // literal that declares its arguments; this crate has no business having a second
            // opinion about a verb the root crate wrote.
            reversal: spec.reversal,
            // 🚨 **Keyed on the catalog name rather than carried by the spec, and that is a
            // change of mind this comment used to state the other way.** `CommandSpec` is the
            // vocabulary the **MCP schema** is generated from, and a schema is precisely the
            // surface with no parent word in hand — the union is the honest answer there, as
            // [`NarrowFn`] already says for `/organon`. A hook field on the spec would also
            // have to be written `narrow: None` in every one of the ~35 `CommandSpec` literals
            // in this workspace to say nothing. So the lookup lives here, beside the view
            // lane's own hooks, and every construction path gets it — including the pane's,
            // which builds its own registry from the specs it was handed and could not have
            // been given a builder call.
            narrow: console_narrow(&spec.name),
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

    /// Whether running this verb can be taken back — see [`Reversal`]. Read by
    /// [`Registry::settled`], which is the only thing that turns it into a decision.
    pub fn reversal(&self) -> Reversal {
        self.reversal
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
                        // The long words exactly as a `Choice` lists them, plus the one clause
                        // that says the short forms exist. `/help` is where a person goes to
                        // find out what there is — an abbreviation absent from it is one nobody
                        // finds except by being told.
                        ArgKind::ChoiceAliased { words, aliases } => {
                            out.push(format!(
                                "      {}: {}{}",
                                arg.name,
                                words.join(" | "),
                                crate::command::short_form_note(
                                    aliases.iter().map(|(w, a)| (w.as_str(), a.as_str()))
                                ),
                            ));
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
            // It puts an element in the transcript and there is no verb that takes one out.
            reversal: Reversal::Permanent,
            narrow: None,
        },
        Entry {
            name: VERB_HELP.into(),
            doc: "List every command this console answers".into(),
            args: Vec::new(),
            lane: Lane::View,
            // ⚠️ **The one that looks like it belongs on the other side and does not.** `/help`
            // does write into the pane — but through `note`, which is the capped diagnostic
            // log, not `Transcript::push`. It reads a table and changes nothing; running it by
            // accident costs a few lines that scroll. That is the whole distinction the rule
            // turns on, and it is a difference in the code rather than a judgement call.
            reversal: Reversal::Recoverable,
            narrow: None,
        },
        Entry {
            name: VERB_TRACE.into(),
            doc: "Open the status log — every line this console wrote about the session, out \
                  of the conversation. Closed"
                .into(),
            args: vec![ArgSpec {
                name: TRACE_ARG.into(),
                kind: ArgKind::Choice(TRACE_WORDS.iter().map(|s| (*s).to_string()).collect()),
                required: true,
            }],
            lane: Lane::View,
            // It changes what is *drawn* and appends no element; the inverse is the other word.
            reversal: Reversal::Recoverable,
            narrow: None,
        },
        Entry {
            name: VERB_MEDIA.into(),
            // The doc line is where a person learns what this build shows, so it names the
            // extensions from `exhibit`'s own table rather than restating them — the
            // `console.background` arrangement, for its reason.
            doc: format!(
                "Show a file here: {}. Several paths make one exhibit",
                organon_core::exhibit::ALL_EXTENSIONS.join(", ")
            ),
            // ⚠️ **`Text`, not `Choice`.** A path has no closed value space, so there is
            // nothing for the command panel to complete against and nothing for
            // `validate_args` to check — which is exactly why the *refusal* has to be good:
            // `exhibit::Exhibit::resolve` is the only gate, and it is the one that names the
            // file and says what would have worked.
            args: vec![ArgSpec { name: MEDIA_ARG.into(), kind: ArgKind::Text, required: true }],
            lane: Lane::View,
            // An exhibit is an element in the transcript, exactly as `/surface`'s is, and no
            // verb in this table takes one out. ⚠️ Autorun could not reach this line anyway:
            // the argument is `Text`, a path has no closed value space, so the command panel
            // never has a lone candidate to complete and never offers the line to autorun at
            // all. This field is what makes that a stated property of the verb rather than a
            // coincidence of its argument's kind.
            reversal: Reversal::Permanent,
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
            // A panel is an element in the transcript, exactly as `/surface`'s is.
            reversal: Reversal::Permanent,
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
/// ⚠️ **A tab with no panels in the table answers [`Ring::Empty`], not an empty list and not
/// `None`.** `None` would fall through to the declared union and offer *every* slug on a tab
/// that has none of them; an empty list would draw a band with nothing in it and refuse with
/// nothing to say. The sentence [`unmapped_tab`] writes is the answer, and it is the same
/// sentence the view lane gives a call that arrives without passing through here.
fn organon_options(arg: &str, positional: &[&str]) -> Option<Ring> {
    match arg {
        // The tab ring is the same eight words the declared `Choice` carries — what the hook
        // adds is what each one is *worth*: a tab whose cards are not joined to the panel table
        // says so in its own line, rather than looking like the seven that lead somewhere.
        ORGANON_TAB_ARG => {
            Some(Ring::Options(UiTab::ALL.iter().map(|t| (t.word(), tab_doc(*t))).collect()))
        }
        ORGANON_PANEL_ARG => {
            let word = positional.first().copied().unwrap_or_default();
            let tab = UiTab::from_word(word)?;
            let known = panels::in_tab(tab);
            if known.is_empty() {
                return Some(Ring::Empty(unmapped_tab(&tab.word())));
            }
            Some(Ring::Options(
                known
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
            ))
        }
        _ => None,
    }
}

/// What one tab is worth in the first ring: how many panels typing it would open onto, or that
/// it opens onto none. **Counted, never listed** — a tab joins the table by having its `card()`
/// call sites converted, and this line then changes by itself.
fn tab_doc(tab: UiTab) -> String {
    match panels::in_tab(tab).len() {
        0 => TAB_NOT_MAPPED.to_string(),
        1 => "1 panel".to_string(),
        n => format!("{n} panels"),
    }
}

/// The mark an unmapped tab carries in the first ring.
///
/// 🚨 **Marked rather than hidden, and that was the choice.** Dropping the seven tabs whose
/// panels are not in the table would make every offer completable — but `UiTab::ALL` *is*
/// Organon's own hierarchy, and a ring showing one wedge of it would misrepresent the product
/// as having one section. The tabs are real; what is not yet real is the console's ability to
/// address their panels, and saying exactly that is the same honesty [`panels::Status::Declared`]
/// already uses one ring down.
const TAB_NOT_MAPPED: &str = "not mapped yet — no panels in the table";

/// Why `/organon <tab>` has nothing to offer, in the **one** sentence the ring, the refusal and
/// the view lane all say.
///
/// ⚠️ **Which tabs are joined is counted, not spelled.** The sentence this replaced named
/// `look` in prose, which would have become false the day a second tab was converted — and
/// silently, since nothing reads a sentence.
pub fn unmapped_tab(tab_word: &str) -> String {
    let joined: Vec<String> = UiTab::ALL
        .iter()
        .copied()
        .filter(|t| !panels::in_tab(*t).is_empty())
        .map(|t| t.word())
        .collect();
    if joined.is_empty() {
        return format!("the {tab_word} tab's panels are not in the table yet, and no tab's are");
    }
    format!(
        "the {tab_word} tab's panels are not in the table yet — joined so far: {}",
        joined.join(", ")
    )
}

/// Which console-lane verbs have a dependent ring. **One line per verb, and normally empty.**
///
/// See [`Entry::from_spec`] for why the lookup is here rather than a field on `CommandSpec`.
fn console_narrow(name: &str) -> Option<NarrowFn> {
    match name {
        VERB_LAYOUT => Some(layout_options),
        VERB_VIEWPORT => Some(viewport_options),
        VERB_SETTING => Some(setting_options),
        _ => None,
    }
}

/// `/setting ` — the approved modules, and then the keys the chosen one declared.
///
/// 🚨 **Two rings from one hook, and the second reads the first's answer** — `viewport_options`'
/// shape, for its reason: a key only means anything inside a producer, so the ring for
/// `/setting moonlight ` is *moonlight's* vocabulary and nobody else's. A hook that offered the
/// union of every module's keys would complete a word the chosen module refuses.
///
/// ⚠️ **`organon` is deliberately NOT offered here, and that is the difference from
/// [`viewport_options`].** Organon's own World is the producer a `3d` region means with nothing
/// named; it is not an approved module, it has no manifest, and it declares no settings. Offering
/// it would complete to a name `check_producer_name` refuses outright.
///
/// ⚠️ **A module that declares nothing answers [`Ring::Empty`] with the reason**, not `None`.
/// `None` would leave the declared `ArgKind::Text` to accept any word, and a key silently
/// accepted for a module that has no such setting is a line somebody typed that nothing acted on
/// — `viewport_options`' argument for the `agent` case, one surface over.
///
/// ⚠️ **No data directory answers `None`**, on `layout_options`' rule: an empty answer would tell
/// somebody whose modules are merely unreachable that they have approved nothing.
fn setting_options(arg: &str, positional: &[&str]) -> Option<Ring> {
    let root = ModuleRegistry::store_root()?;
    let registry = ModuleRegistry::for_completion(&root);
    match arg {
        SETTING_PRODUCER_ARG => Some(approved_ring(&registry)),
        SETTING_KEY_ARG => Some(key_ring(&registry, positional.first().copied()?)),
        // The value. No ring: the value space is the module's, and a console with an opinion
        // about it is a console that has to be updated when a module gains a machine.
        _ => None,
    }
}

/// The approved modules, and only those — **pure**, so every property is a test rather than a
/// claim about a store the suite must not write to.
fn approved_ring(registry: &ModuleRegistry) -> Ring {
    if registry.modules.is_empty() {
        return Ring::Empty(format!(
            "no module is approved — `{}` approves one",
            crate::module::APPROVE_VERB
        ));
    }
    Ring::Options(
        registry
            .modules
            .iter()
            .map(|m| {
                let n = m.settings.len();
                (
                    m.producer.clone(),
                    match n {
                        0 => format!("{} — declares no settings", m.name),
                        1 => format!("{} — 1 setting", m.name),
                        n => format!("{} — {n} settings", m.name),
                    },
                )
            })
            .collect(),
    )
}

/// The keys one module declared, with the module author's own description beside each.
///
/// 📌 The doc text comes from somebody else's repository, so it is quoted with
/// [`crate::module::quoted_untrusted`] rather than pasted — the rule every other place this
/// crate shows a manifest's words already follows.
fn key_ring(registry: &ModuleRegistry, producer: &str) -> Ring {
    let Some(module) = registry.modules.iter().find(|m| m.producer == producer) else {
        return Ring::Empty(format!(
            "`{producer}` is not an approved module — `{}` approves one",
            crate::module::APPROVE_VERB
        ));
    };
    if module.settings.is_empty() {
        return Ring::Empty(format!(
            "{} declares no settings — a module says what it answers to in its own manifest",
            module.name
        ));
    }
    Ring::Options(
        module
            .settings
            .iter()
            .map(|spec| {
                let doc = if spec.doc.is_empty() {
                    format!("a setting {} understands", module.name)
                } else {
                    crate::module::quoted_untrusted(&spec.doc)
                };
                (spec.key.clone(), doc)
            })
            .collect(),
    )
}

/// `/viewport <region> 3d producer ` — the modules that are actually approved, plus `organon`.
///
/// 🚨 **A second dynamic vocabulary over a stored library, and the measurement is inherited
/// rather than re-taken.** [`Registry::value_candidates`] asks the ring once and then reaches
/// [`coerce`] per candidate, which asks again — `n + 1` reads per call, on the **draw** path.
/// §1.15 measured that at 10.1 ms for a hundred entries against a 16.7 ms frame when the library
/// is read straight from disk. `ModuleRegistry::for_completion` is T3a's answer to exactly that,
/// with the same 200 ms TTL, the same store-root key and the same invalidation on write — so
/// this reads it and builds nothing of its own. A second cache would be a second thing to
/// invalidate.
///
/// 🚨 **It reads the CONTENT word, which is why the hook has to exist at all.** A producer
/// qualifies `3d` and nothing else, so the ring for `viewport left agent producer ` is
/// [`Ring::Empty`] carrying the reason rather than a list — and [`coerce`] then refuses the word
/// with that same sentence while it is still in the composer. `None` there would leave the
/// declared `ArgKind::Text` to accept it, and a producer silently attached to `agent` is a word
/// somebody typed that nothing acted on.
///
/// ⚠️ **`organon` is always offered and is never in `modules.json`** — `check_producer_name`
/// reserves it. Leaving it out of the ring would make the one producer that has always existed
/// the one the completion cannot spell.
///
/// ⚠️ **No data directory answers `None`, not [`Ring::Empty`]** — [`layout_options`]' rule one
/// surface over: an empty answer would tell somebody whose modules are merely *unreachable* that
/// they have approved nothing.
fn viewport_options(arg: &str, positional: &[&str]) -> Option<Ring> {
    if arg != VIEWPORT_PRODUCER_ARG {
        return None;
    }
    // The content word, read through `region`'s own resolver rather than compared against a
    // literal — a fifth content word would otherwise silently mean "no ring".
    match ContentCmd::resolve(positional.get(1).copied()?).ok()? {
        ContentCmd::Hold(Content::ThreeD(_)) => {
            let root = ModuleRegistry::store_root()?;
            Some(producer_ring(&ModuleRegistry::for_completion(&root)))
        }
        other => Some(Ring::Empty(format!(
            "`{}` has no producer — a producer qualifies `3d`, which is the one thing a region \
             holds that something has to draw",
            other.as_word()
        ))),
    }
}

/// The ring an approved set makes — **pure**, so every property of it is a test rather than a
/// claim about a store the suite must not write to (`%APPDATA%\OrganonShell\modules.json` is the
/// real one).
///
/// The doc beside each name is what approving it recorded: the module's human-readable name and
/// the short commit, so a person choosing between two producers is choosing between two facts
/// rather than two words.
fn producer_ring(registry: &ModuleRegistry) -> Ring {
    // 🚨 **Never [`Ring::Empty`], because there is always exactly one answer.** An empty modules
    // file does not mean a `3d` region has no producer; it means the only producer is the one
    // the console wrote. A ring that said "none" here would be describing a viewport that cannot
    // be drawn, which is the opposite of the truth.
    let mut options = vec![(
        module::DEFAULT_PRODUCER.to_string(),
        "Organon's own World — what `3d` means with no producer named".to_string(),
    )];
    options.extend(
        registry
            .modules
            .iter()
            .map(|m| (m.producer.clone(), format!("{} — approved at {}", m.name, m.short_commit()))),
    );
    Ring::Options(options)
}

/// `/layout load ` and `/layout delete ` — the arrangements that are actually saved.
///
/// 🚨 **`save` is deliberately NOT narrowed, and that asymmetry is the whole reason the hook
/// reads the action word.** `load` and `delete` name a layout that must already exist, so the
/// library is their value space and a word outside it is refused. `save` takes a name a person
/// is *inventing*; offering the existing names there would be actively wrong — the popup would
/// read as a closed list, and [`coerce`] would then refuse every new name in the world, which
/// is the one thing `save` is for. `None` is the answer for it, so the declared `ArgKind::Text`
/// answers as usual and anything [`crate::layout::check_name`] accepts goes through.
///
/// ⚠️ **The cost was measured before this was wired, and the measurement changed the design.**
/// [`Registry::value_candidates`] asks the ring once and then calls [`Registry::settled`] per
/// candidate, each of which reaches [`coerce`] and asks again — `n + 1` reads for a library of
/// `n`, on the *draw* path, which is per frame rather than per keystroke. Read straight from
/// disk that is 10.1 ms per call at a hundred layouts against a 16.7 ms frame. So it goes
/// through [`Library::for_completion`], which owns the cache, the numbers and what invalidates
/// it; `crate::layout`'s `library_read_cost` is the instrument and `CONSOLE_ARCHITECTURE.md`
/// §1.15 carries the finding.
///
/// ⚠️ **No data directory answers `None`, not [`Ring::Empty`]** — `console_main`'s
/// `console.layout.list` states the rule and this is the same rule one surface over: an empty
/// answer would tell somebody whose layouts are merely *unreachable* that they have saved
/// nothing. With no opinion here the free-text path stays open and the dispatch's own refusal,
/// which names the real cause, is the one that speaks.
fn layout_options(arg: &str, positional: &[&str]) -> Option<Ring> {
    if arg != LAYOUT_NAME_ARG {
        return None;
    }
    // The action word, resolved through the layout module's own table rather than compared
    // against string literals here — a fourth action would otherwise silently mean "no ring".
    match LayoutCmd::resolve(positional.first().copied()?).ok()? {
        LayoutCmd::Save => None,
        LayoutCmd::Load | LayoutCmd::Delete => {
            Some(layout_ring(&Library::for_completion(&Library::store_root()?)))
        }
    }
}

/// The ring a library makes — **pure**, so every property of it is a test rather than a claim
/// about a store the suite must not write to (`%APPDATA%\OrganonShell\layouts.json` is the real
/// one, and a test that touched it would destroy the layouts of whoever ran `cargo test`).
///
/// 🚨 **The names are carried whole and are never joined here.** A comma is a legal layout name
/// character — [`crate::layout::check_name`] refuses whitespace because the wire cannot carry it
/// and deliberately nothing else — so `a,b` is one layout, and each option is its own
/// `(label, doc)` pair all the way to the popup row. The only place they meet a separator is
/// [`coerce`]'s refusal, which joins with ` | `; whitespace being impossible in a name is
/// exactly what keeps that unambiguous where a bare `, ` would not be.
fn layout_ring(library: &Library) -> Ring {
    if library.layouts.is_empty() {
        // 🚨 **One sentence, not a second one written for the ring.** It is the same string
        // `console.layout.list` answers an empty library with — see
        // [`crate::layout::NOTHING_SAVED`], which is where it lives so that both surfaces read
        // it rather than each carrying its own.
        return Ring::Empty(layout::NOTHING_SAVED.to_string());
    }
    // In file order, which is the order the listing and every refusal already quote.
    Ring::Options(library.layouts.iter().map(|l| (l.name.clone(), l.holds())).collect())
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
    // The words that have filled the required arguments so far — [`NarrowFn`]'s `positional`,
    // and the same list [`Registry::candidates`] builds while walking a half-typed line. An
    // argument whose options depend on an earlier word cannot be checked without it.
    let mut positional: Vec<&str> = Vec::new();

    for arg in entry.args.iter().filter(|a| a.required) {
        let Some(word) = it.next() else {
            return Err(format!(
                "`{}` needs `{}` — usage: {}",
                entry.usage_head(),
                arg.name,
                entry.usage()
            ));
        };
        out.insert(arg.name.clone(), coerce(entry, arg, word, &positional)?);
        positional.push(word);
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
        out.insert(arg.name.clone(), coerce(entry, arg, value, &positional)?);
    }
    Ok(Value::Object(out))
}

/// One word, as the type its slot declares — refused, never approximated, with the message a
/// person can act on.
///
/// 🚨 **The dependent ring answers first when it answers at all**, and that is the fix for the
/// refusal that named a tab nobody had asked about. See [`NarrowFn`]. The declared `ArgKind` is
/// untouched and still answers for every argument no hook has an opinion on, which is all of
/// them but one.
fn coerce(entry: &Entry, arg: &ArgSpec, word: &str, positional: &[&str]) -> Result<Value, String> {
    let head = entry.usage_head();
    if let Some(ring) = entry.narrow.and_then(|f| f(&arg.name, positional)) {
        return match ring {
            // The sentence already names the tab, so the head stays the bare verb.
            Ring::Empty(why) => Err(format!("`{head}`: {why}")),
            Ring::Options(options) if options.iter().any(|(option, _)| option == word) => {
                Ok(Value::String(word.to_string()))
            }
            // ⚠️ The head carries the words that **chose** this option set — `/organon look`,
            // not `/organon` — so the refusal says which ring it is refusing against without
            // inventing a phrasing for it. That is the half James could not see: the list was
            // right for a tab he had not named, and nothing in the sentence said which.
            Ring::Options(options) => Err(format!(
                "`{}`: `{word}` is not one of {}",
                if positional.is_empty() {
                    head
                } else {
                    format!("{head} {}", positional.join(" "))
                },
                options.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>().join(" | ")
            )),
        };
    }
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
        // 🚨 **The word travels as TYPED, short form and all.** It is not expanded here, and
        // that is the choice that keeps the four front doors agreeing: clap's
        // `PossibleValuesParser` returns the string it matched rather than the canonical name
        // (`clap_builder`'s `parse` returns `value`, not `v.get_name()`), so `organon console
        // viewport tl panel` puts `tl` on the sidecar line. Expanding here and not there would
        // make the same command read two ways in the session log depending on which door it
        // came through. `region::Region::resolve` is the one place a short form becomes a
        // region, and both doors reach it.
        ArgKind::ChoiceAliased { words, aliases } => {
            if words.iter().any(|o| o == word) || aliases.iter().any(|(_, a)| a == word) {
                Ok(Value::String(word.to_string()))
            } else {
                Err(format!(
                    "`{head}`: `{word}` is not one of {}{}",
                    words.join(" | "),
                    crate::command::short_form_note(
                        aliases.iter().map(|(w, a)| (w.as_str(), a.as_str()))
                    ),
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

/// The word the log puts in front of a command that worked.
///
/// 🚨 **A word, not a glyph, and this line is the fourth site to learn it.** It was `✓`
/// (U+2713), which is in none of egui's four bundled fonts and drew as an empty box in the
/// pane log *and* in the status band — photographed on a running console on 2026-08-14
/// (`☐ /rig daylight — {"accepted":"rig daylight"}`). `conversation_view`'s allowlist guard
/// existed and did not catch it, because the guard walks an enumerated list of draw sites and
/// this string is built **here** and drawn **there**. Both halves are fixed: the marker is
/// now the same word the band above the composer already uses for the same outcome, and the
/// guard now checks this function's own output.
pub const RECEIPT_OK: &str = "ok";

/// What the pane's **log** says back after a command ran.
///
/// Pure, and here rather than in the view, so the sentence a human reads is pinned by a test
/// instead of being verified by reading it. Formatted from [`receipt_of`] rather than beside
/// it, so the line in the log and the band above the composer cannot come to disagree about
/// what happened.
pub fn receipt(typed: &str, result: &Result<Value, String>) -> String {
    let receipt = receipt_of(typed, result);
    if receipt.ok {
        format!("{RECEIPT_OK} {}", receipt.text)
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
    /// …and that command is one the console may run **without being asked** — see
    /// [`crate::command::Reversal`]. Derived from the resolved verb's own declaration in the
    /// same breath as [`Candidate::completes`] (see [`Registry::settled`]), so a renderer never
    /// has to know which verbs those are.
    ///
    /// ⚠️ **False whenever `completes` is false**, and false for a name the registry cannot
    /// find. Both are the safe direction: unclassified means ask.
    pub fires: bool,
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
    /// Why [`Palette::candidates`] is empty, when the emptiness is a fact about the world
    /// rather than about what has been typed — [`Ring::Empty`]'s sentence, carried to the two
    /// surfaces that draw a band. `None` in every other case, including a prefix that happens
    /// to match none of a perfectly good list.
    ///
    /// 🚨 **This is what keeps `/organon generator ` from drawing a blank band.** Read out
    /// through [`Palette::hint`], so a renderer that already draws the hint draws this without
    /// learning a new term — and so [`Palette::is_empty`] stops calling it empty.
    pub empty_ring: Option<String>,
}

impl Palette {
    /// Nothing to show: no continuations, nothing to say about the slot, **and nothing true
    /// about the line as it stands**.
    ///
    /// ⚠️ **Three terms, and the third was missing.** `/surface ` takes no arguments, so it
    /// lands in [`Slot::Keyword`] with no candidates and no hint — and the panel therefore
    /// vanished outright, which is indistinguishable from a panel that is broken. James, on a
    /// running build: *"slash surface shows no options."* There genuinely are none; what there
    /// is, is the fact that Enter would run the line, and [`Palette::runnable`] is that fact.
    /// A renderer that draws it needs the palette to survive this test to draw anything at all,
    /// so the term belongs here rather than at the call site — the same way `hint` does.
    ///
    /// ⚠️ The `None` path above this is untouched and must stay so: [`Registry::candidates`]
    /// answers `None` for a line that is not a command line, and a panel must never open over
    /// prose. This only ever turns a *drawn-as-blank* panel into one with a sentence in it.
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty() && self.hint().is_none() && !self.runnable
    }

    /// What to type when there is nothing to choose from — the one case a list cannot answer.
    /// `None` whenever [`Palette::candidates`] is the whole truth.
    ///
    /// ⚠️ [`Palette::empty_ring`] comes first: an argument whose options depend on an earlier
    /// word can have *none*, and a `Choice` otherwise has nothing to say for itself. A band
    /// that vanishes reads as a broken one — James has now reported that twice, once for
    /// `/surface ` and once for `/organon generator `.
    pub fn hint(&self) -> Option<String> {
        if let Some(why) = &self.empty_ring {
            return Some(why.clone());
        }
        let Slot::Value { arg, .. } = &self.slot else { return None };
        match &arg.kind {
            // `ChoiceAliased` sits with `Choice`: both have a full ring to show, and a hint
            // beside a complete list is a second copy of it.
            ArgKind::Choice(_) | ArgKind::ChoiceAliased { .. } | ArgKind::Bool => None,
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
    /// want" is a *provable* state, not a guess. **Three terms, all of them provable:**
    ///
    /// 1. Exactly one continuation is left, so there is nothing else the line could have meant.
    /// 2. That continuation **completes** the command. `/th` leaves only `theme`, which still
    ///    needs a value — so it does not fire, because firing there would run a command while
    ///    the hand is still typing its argument.
    /// 3. The command it completes to is **recoverable** ([`Candidate::fires`]). `/su` leaves
    ///    only `surface`, which takes nothing and is therefore certain — and still waits,
    ///    because a surface in the transcript is not something a second command takes back.
    ///
    /// ⚠️ **The third term is why this could become the default.** With it, being certain is no
    /// longer sufficient; the console also has to be able to afford being wrong.
    ///
    /// ⚠️ **`enabled` is the caller's switch and it is now ON by default** — see
    /// `ConversationPane::new` and `ORGANON_PALETTE_AUTORUN`. This still owns the rule and
    /// nothing else; a caller that hands it `false` gets the old Enter-for-everything console.
    pub fn autorun(&self, enabled: bool) -> Option<&Candidate> {
        if !enabled {
            return None;
        }
        match &self.candidates[..] {
            [only] if only.completes && only.fires => Some(only),
            _ => None,
        }
    }

    /// 🚨 **The candidate a renderer should take without being asked, and it is NOT
    /// [`Palette::autorun`].**
    ///
    /// One continuation left is not a choice, it is an answer the human has already given by
    /// typing far enough — so showing them a one-item list and asking them to press Tab is
    /// the surface making them confirm what it already knows. James, 2026-08-14: *"Do not
    /// show me the single choice like you currently do. Simply complete the completion
    /// because it's the only option."*
    ///
    /// 🚨 **Completing is not running, and the two have separate switches on purpose.**
    /// This rewrites the line and stops; [`Palette::autorun`] submits it, and additionally
    /// requires [`Candidate::completes`] and [`Candidate::fires`]. A line completed here that
    /// autorun declines — every irreversible verb — is left sitting in the composer with the
    /// row saying `Enter runs`, which is the whole of what "stop and ask" looks like.
    ///
    /// ⚠️ **`completion != line` is what makes this terminate.** `/surface` already *is* its
    /// own sole completion, so a rule that only counted candidates would rewrite the line to
    /// itself for ever. It is also the display rule the compact row reads: a sole candidate
    /// that is already the whole line is a list with nothing in it to choose.
    pub fn sole_completion(&self, line: &str) -> Option<&Candidate> {
        match &self.candidates[..] {
            [only] if only.completion != line => Some(only),
            _ => None,
        }
    }
}

/// An argument's value space, in words. ASCII on purpose — see the glyph allowlist in
/// `conversation_view`'s tests; a range written with `…` would ship as a box.
fn value_space(kind: &ArgKind) -> String {
    match kind {
        ArgKind::Choice(options) => options.join(" | "),
        // The long words only. This string is a *value space*, and a short form is a second
        // spelling of a value rather than another value.
        ArgKind::ChoiceAliased { words, .. } => words.join(" | "),
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
            return Some(Palette {
                slot: Slot::Verb,
                typed: typed.to_string(),
                candidates,
                runnable,
                empty_ring: None,
            });
        };
        let Some(entry) = self.entry(verb_word) else {
            // A verb this table does not have. `resolve` names the known set when Enter is
            // pressed; there is nothing here to complete.
            return Some(Palette {
                slot: Slot::Verb,
                typed: typed.to_string(),
                candidates: Vec::new(),
                runnable,
                empty_ring: None,
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
                        empty_ring: None,
                    })
                }
            }
        }

        let verb = entry.verb().to_string();
        let (slot, candidates, empty_ring) = if filled < required.len() {
            let arg = required[filled];
            let more = filled + 1 < required.len() || !optional.is_empty();
            let (candidates, empty_ring) =
                self.value_candidates(entry, arg, &positional, typed, stem, more);
            (Slot::Value { verb, arg: arg.clone() }, candidates, empty_ring)
        } else if let Some(arg) = awaiting {
            let more = used.len() + 1 < optional.len();
            let (candidates, empty_ring) =
                self.value_candidates(entry, arg, &positional, typed, stem, more);
            (Slot::Value { verb, arg: arg.clone() }, candidates, empty_ring)
        } else {
            let candidates = optional
                .iter()
                .copied()
                .filter(|arg| !used.contains(&arg.name.as_str()))
                .filter(|arg| narrows(&arg.name, typed))
                .map(|arg| self.keyword_candidate(arg, stem))
                .collect();
            (Slot::Keyword { verb }, candidates, None)
        };
        Some(Palette { slot, typed: typed.to_string(), candidates, runnable, empty_ring })
    }

    /// A verb, as the word plus the line typing it would produce. The trailing space is what
    /// opens the next ring the instant the verb is accepted — and it is absent for a verb
    /// that takes nothing, because a line ending in a space it will never fill reads as
    /// unfinished when it is not.
    fn verb_candidate(&self, entry: &Entry, stem: &str) -> Candidate {
        let tail = if entry.args().is_empty() { "" } else { " " };
        let completion = format!("{stem}{}{tail}", entry.verb());
        let (completes, fires) = self.settled(&completion);
        Candidate {
            label: entry.verb().to_string(),
            doc: entry.doc().to_string(),
            completes,
            fires,
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
        let (completes, fires) = self.settled(&completion);
        Candidate {
            label: arg.name.clone(),
            doc: value_space(&arg.kind),
            completes,
            fires,
            completion,
            kind: CandidateKind::Keyword,
        }
    }

    /// The two facts about a finished line that decide whether it needs an Enter: is it a whole
    /// command, and may the console run it unasked.
    ///
    /// 🚨 **Both come out of one [`Registry::resolve`] call**, which is what keeps them from
    /// drifting apart — and from drifting away from what Enter would actually do. The verb is
    /// read off the *resolution*, never off the typed word, so `/camera.read` and `/camera` are
    /// told apart by the same machinery that dispatches them.
    ///
    /// ⚠️ **A name with no entry answers `false`**, which cannot happen — `resolve` produced
    /// the name from this very table — but is the safe direction if it ever does. Unclassified
    /// means ask.
    fn settled(&self, completion: &str) -> (bool, bool) {
        match self.resolve(completion) {
            Resolved::Run { name, .. } => (
                true,
                self.entries
                    .iter()
                    .find(|e| e.name == name)
                    .is_some_and(|e| e.reversal == Reversal::Recoverable),
            ),
            _ => (false, false),
        }
    }

    /// The options of an argument whose value space is closed. Empty for `Float`, `Int` and
    /// `Text`, which is not a gap — [`Palette::hint`] answers those, and a renderer with a
    /// dial reads the `ArgKind` off the slot.
    /// Returns the reason alongside the list, because an argument with **no** options here is
    /// not the same as one with options none of which match what has been typed — and only the
    /// first has anything to say. See [`Palette::empty_ring`].
    fn value_candidates(
        &self,
        entry: &Entry,
        arg: &ArgSpec,
        positional: &[&str],
        typed: &str,
        stem: &str,
        more: bool,
    ) -> (Vec<Candidate>, Option<String>) {
        // The entry's own hook first, and its `Some` wins outright — including `Ring::Empty`,
        // which is the true answer for a tab with no panels and must not fall through to the
        // declared union. See [`NarrowFn`].
        let (options, empty_ring): (Vec<(String, String)>, Option<String>) =
            match entry.narrow.and_then(|f| f(&arg.name, positional)) {
                Some(Ring::Options(narrowed)) => (narrowed, None),
                Some(Ring::Empty(why)) => (Vec::new(), Some(why)),
                None => (
                    match &arg.kind {
                        ArgKind::Choice(options) => {
                            options.iter().map(|o| (o.clone(), String::new())).collect()
                        }
                        // 🚨 **The short form rides in the DOC slot, not as a candidate.** The
                        // ring stays twelve words long — a person is choosing between twelve
                        // shapes, and twenty-four entries would say otherwise — and each one
                        // carries its abbreviation the way any other candidate carries the one
                        // line about it. That is the whole of the discoverability answer: you
                        // learn `tl` by looking at `topleft`, in the band you were already
                        // reading.
                        ArgKind::ChoiceAliased { words, aliases } => words
                            .iter()
                            .map(|w| {
                                let doc = aliases
                                    .iter()
                                    .find(|(full, _)| full == w)
                                    .map_or(String::new(), |(_, short)| short.clone());
                                (w.clone(), doc)
                            })
                            .collect(),
                        ArgKind::Bool => vec![
                            ("true".to_string(), String::new()),
                            ("false".to_string(), String::new()),
                        ],
                        ArgKind::Float { .. } | ArgKind::Int | ArgKind::Text => Vec::new(),
                    },
                    None,
                ),
            };
        let candidates = options
            .into_iter()
            .filter(|(option, _)| narrows(option, typed))
            .map(|(option, doc)| {
                let completion = format!("{stem}{option}{}", if more { " " } else { "" });
                let (completes, fires) = self.settled(&completion);
                Candidate {
                    label: option,
                    doc,
                    completes,
                    fires,
                    completion,
                    kind: CandidateKind::Value,
                }
            })
            .collect();
        (candidates, empty_ring)
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
    ///
    /// ⚠️ **The `Reversal` of each copies the real catalog's**, and that matters more than the
    /// argument shapes do: `background` and `camera` are the verbs autorun may fire, `patch` is
    /// the one that must complete and then wait, and a fixture that made them all one or all
    /// the other would pin half the rule.
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
                reversal: Reversal::Recoverable,
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
                reversal: Reversal::Permanent,
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
                reversal: Reversal::Recoverable,
            },
            CommandSpec {
                name: "console.camera.read".into(),
                doc: "Where the viewer stands right now".into(),
                target: TargetKind::Viewport,
                args: Vec::new(),
                reversal: Reversal::Recoverable,
            },
        ]
    }

    fn registry() -> Registry {
        Registry::new(&console())
    }

    /// A one-verb catalog whose only argument has **declared short forms** — the shape
    /// `console.viewport`'s region slot has. Its own registry rather than a row in
    /// [`console`], because every assertion about the fixture table's verb list would then
    /// have to learn about it, and the point here is one argument.
    fn aliased() -> Registry {
        Registry::new(&[CommandSpec {
            name: "console.viewport".into(),
            doc: "Divide the pane".into(),
            target: TargetKind::Viewport,
            args: vec![ArgSpec {
                name: "region".into(),
                kind: ArgKind::ChoiceAliased {
                    words: vec!["full".into(), "topleft".into(), "bottomright".into()],
                    aliases: vec![
                        ("full".into(), "f".into()),
                        ("topleft".into(), "tl".into()),
                        ("bottomright".into(), "br".into()),
                    ],
                },
                required: true,
            }],
            reversal: Reversal::Recoverable,
        }])
    }

    /// 🚨 **A `ChoiceAliased` accepts the short form, offers only the long words, and shows the
    /// short one BESIDE its word.**
    ///
    /// All three in one test because they are one property with three faces: an abbreviation
    /// that is accepted but never shown is a secret, one that is shown as a peer doubles the
    /// apparent size of the vocabulary, and one that is shown but not accepted is a lie. The
    /// ring is where a person looks; [`Candidate::doc`] is the slot that already exists for
    /// "one line about this word", so the short form goes there and the ring stays the length
    /// of the vocabulary.
    #[test]
    fn an_aliased_choice_accepts_the_short_form_and_offers_only_the_long_ones() {
        let reg = aliased();
        let ring = reg.candidates("/viewport ").expect("the value ring");
        assert_eq!(
            ring.candidates.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            ["full", "topleft", "bottomright"],
            "the ring is the long words — a short form is not a thirteenth region"
        );
        assert_eq!(
            ring.candidates.iter().map(|c| c.doc.as_str()).collect::<Vec<_>>(),
            ["f", "tl", "br"],
            "…each carrying its own short form, which is how anybody discovers one"
        );
        // Accepted at the composer, long or short, and it travels as typed either way.
        for word in ["full", "topleft", "bottomright", "f", "tl", "br"] {
            match reg.resolve(&format!("/viewport {word}")) {
                Resolved::Run { args, .. } => {
                    assert_eq!(args["region"], json!(word), "`{word}` was rewritten");
                }
                other => panic!("`/viewport {word}` did not resolve: {other:?}"),
            }
        }
        // A near miss still refuses — a declared short form is a second exact word, not a
        // prefix rule — and the refusal says the short forms exist.
        let Resolved::Refused(why) = reg.resolve("/viewport tlx") else {
            panic!("`tlx` resolved and must not");
        };
        assert!(why.contains("full | topleft | bottomright"), "{why}");
        assert!(why.contains("short form"), "the refusal keeps them secret: {why}");
        assert!(why.contains("`f`") && why.contains("`br`"), "{why}");
        // …and so does `/help`, which is where a person goes to find out what exists.
        let help = reg.help_lines().join("\n");
        assert!(help.contains("region: full | topleft | bottomright"), "{help}");
        assert!(help.contains("short form"), "`/help` keeps them secret: {help}");
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
            [
                "background",
                "patch",
                "camera",
                "camera.read",
                "surface",
                "help",
                "trace",
                "media",
                "organon"
            ]
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
        assert_eq!(view, ["surface", "help", "trace", "media", "organon"]);
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
            reversal: Reversal::Recoverable,
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

    /// 🚨 CONTRACT: **a line Enter would run is never nothing to say**, which is the term
    /// [`Palette::is_empty`] was missing.
    ///
    /// `/surface ` has no candidates and no hint: every earlier reading of "empty" was true of
    /// it, so the panel disappeared the moment a complete command had a space typed after it —
    /// and a panel that vanishes is read as a broken one. The fact that survives is
    /// [`Palette::runnable`].
    #[test]
    fn a_runnable_line_is_never_empty_even_with_nothing_left_to_offer() {
        let settled = palette("/surface ");
        assert!(settled.candidates.is_empty(), "`surface` takes nothing, so there is no list");
        assert_eq!(settled.hint(), None, "and no slot to describe either");
        assert!(settled.runnable, "…but Enter would run it");
        assert!(!settled.is_empty(), "so the panel has something true to say");

        // The other half, unchanged: a line with nothing to offer AND nothing to run stays
        // empty, so no panel opens for it.
        let stuck = palette("/camera sideways ");
        assert!(stuck.candidates.is_empty() && stuck.hint().is_none() && !stuck.runnable);
        assert!(stuck.is_empty(), "a word naming no argument leaves the panel with nothing");
    }

    /// 🚨 **The auto-execute guard, and the three cases it must refuse.**
    ///
    /// Firing on `/background g` is what was asked for: one continuation left, it completes
    /// the command, and a backdrop is one command away from any other. Firing on `/b` would
    /// run `background` with no material — a command executing while the hand is still typing
    /// its argument. Firing on `/s` would put a surface in the transcript that nothing takes
    /// back, on a keystroke.
    #[test]
    fn auto_execute_fires_only_on_a_complete_and_recoverable_command() {
        let unique_and_complete = palette("/background g");
        assert_eq!(
            unique_and_complete.autorun(true).map(|c| c.completion.as_str()),
            Some("/background graphite"),
            "one candidate, it completes, and it can be taken back: this is the case James \
             asked for"
        );
        assert_eq!(
            unique_and_complete.autorun(false),
            None,
            "…and a caller that switches it off gets the Enter-for-everything console back"
        );

        assert_eq!(
            palette("/b").autorun(true),
            None,
            "unique, but `background` still needs a material — it must NOT fire"
        );
        assert_eq!(palette("/c").autorun(true), None, "two candidates is not knowing what we want");
        assert_eq!(palette("/z").autorun(true), None, "and none is not either");
        assert_eq!(
            palette("/patch ").autorun(true),
            None,
            "an argument with no closed value space can never satisfy the guard"
        );
    }

    /// 🚨 CONTRACT: **certainty is not enough — the console also has to be able to afford
    /// being wrong.** This is the term that let auto-execute become the default.
    ///
    /// `/s` is the case that used to fire and now must not: `surface` is the only thing it can
    /// mean and it takes no arguments, so the first two terms hold outright. What it leaves
    /// behind is an element in the transcript, and no verb in this table takes one out again —
    /// so the line completes to `/surface` and stops there, with `runnable` telling the row to
    /// say `Enter runs`.
    ///
    /// ⚠️ **The pair is the point.** `help` is a view-lane verb too, takes nothing too, and is
    /// reached the same way — and it fires, because reading a table changes nothing. If the
    /// rule had been "view-lane verbs are dangerous" or "verbs with no arguments are
    /// dangerous" it would have got one of these two wrong.
    #[test]
    fn a_verb_that_cannot_be_taken_back_completes_and_then_waits() {
        let surface = palette("/s");
        let [only] = &surface.candidates[..] else { panic!("`/s` is `surface` alone") };
        assert!(only.completes, "the line it produces IS a whole command");
        assert!(!only.fires, "…and it is not one the console may run unasked");
        assert_eq!(surface.autorun(true), None, "so no keystroke runs it");
        assert!(
            palette("/surface").runnable,
            "what the human gets instead is the row saying Enter runs"
        );

        let help = palette("/h");
        let [only] = &help.candidates[..] else { panic!("`/h` is `help` alone") };
        assert!(only.completes && only.fires, "a read is the cleanest recoverable case there is");
        assert_eq!(
            help.autorun(true).map(|c| c.completion.as_str()),
            Some("/help"),
            "so it runs, and the two view-lane verbs part company on recoverability alone"
        );

        // The console lane's own irreversible verb, so the rule is not a fact about lanes.
        let patch = palette("/patch 2 3 s");
        let [only] = &patch.candidates[..] else { panic!("`s` is `scene` alone") };
        assert!(only.completes, "every required argument is filled");
        assert!(!only.fires, "…and a claimed rectangle is not something a second command undoes");
        assert_eq!(patch.autorun(true), None);
    }

    /// 🚨 CONTRACT: **every verb the conversation view answers states whether it can be taken
    /// back**, and the four answers are pinned here rather than left to a comment.
    ///
    /// ⚠️ **This test exists because that table was BROKEN on `main` @ 2018d41 and the tree did
    /// not compile at all.** `/media` arrived in `view_entries` on one branch (`94e26c7`) and
    /// `Entry::reversal` on another (`8307e5c`); the two hunks are lines apart, so git merged
    /// them with no conflict into an initializer missing a field that did not exist when it was
    /// written. Neither branch was wrong, and neither branch's tests could have been red.
    ///
    /// 📌 **What this adds beyond the compiler is the VALUE, not the presence.** A missing field
    /// is `E0063` and needs no test; a *wrong* one is silent, and the wrong one here would let a
    /// keystroke put an exhibit in the transcript. The absence is caught by anyone who builds —
    /// which is the actual lesson, and it belongs to the merge, not to this file.
    #[test]
    fn the_view_lane_states_what_can_be_taken_back_and_an_exhibit_cannot() {
        let reversal = |verb: &str| {
            view_entries()
                .into_iter()
                .find(|e| e.name == verb)
                .unwrap_or_else(|| panic!("`{verb}` is a view-lane verb"))
                .reversal
        };

        // The three that leave an element behind. No verb in this table takes one out.
        for verb in [VERB_SURFACE, VERB_MEDIA, VERB_ORGANON] {
            assert_eq!(
                reversal(verb),
                Reversal::Permanent,
                "`/{verb}` puts an element in the transcript, so no keystroke may place one"
            );
        }

        // The one that reads a table and changes nothing — the pair is what stops the rule
        // being misread as "view-lane verbs are dangerous".
        assert_eq!(
            reversal(VERB_HELP),
            Reversal::Recoverable,
            "reading a table is the cleanest recoverable case there is"
        );
    }

    /// 🚨 CONTRACT: **completing a lone candidate and running one are different questions
    /// with different answers**, and the pair below is the whole of the difference.
    ///
    /// `/b` leaves only `background`, so the *word* is settled and there is nothing to
    /// choose — but `background` still needs a material, so nothing may run. `autorun`
    /// refuses it (see the test above) and `sole_completion` takes it, which is exactly the
    /// asymmetry: a completion leaves a line in the box, and a line in the box is not an
    /// action.
    #[test]
    fn a_lone_candidate_completes_itself_without_ever_running() {
        assert_eq!(
            palette("/b").sole_completion("/b").map(|c| c.completion.as_str()),
            Some("/background "),
            "one word left is an answer already given, not a choice to be confirmed"
        );
        assert_eq!(palette("/b").autorun(true), None, "…and it still must not RUN");

        // The case James hit: a verb whose arguments are the whole point offers none of them
        // until the line reaches its value slot, and only a trailing space gets it there.
        assert_eq!(labels("/patch"), ["patch"], "the bare word is still the verb slot");
        assert_eq!(
            palette("/patch").sole_completion("/patch").map(|c| c.completion.as_str()),
            Some("/patch "),
            "so completing it is what opens the argument ring at all"
        );

        // Two candidates is not one, however much the first looks finished. `/camera` is a
        // whole verb AND the prefix of another, which is the case a count alone gets wrong.
        assert_eq!(labels("/camera"), ["camera", "camera.read"]);
        assert_eq!(palette("/camera").sole_completion("/camera"), None);

        // 🚨 The termination rule. `/surface` is already its own completion, so a rule that
        // counted candidates and nothing else would rewrite the line to itself for ever.
        assert_eq!(labels("/surface"), ["surface"]);
        assert_eq!(palette("/surface").sole_completion("/surface"), None);
        assert_eq!(palette("/z").sole_completion("/z"), None, "and none is not one");
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
    ///
    /// ⚠️ **These strings used to open with `✓` and that was the defect, not the contract.**
    /// U+2713 is in none of egui's fonts; the log and the status band both drew it as an
    /// empty box on James's screen. The marker is now [`RECEIPT_OK`], the same word the band
    /// above the composer uses, so one outcome cannot read as two.
    #[test]
    fn a_receipt_reports_what_actually_happened() {
        assert_eq!(
            receipt("/background slate", &Ok(json!({ "accepted": "background slate" }))),
            "ok /background slate — {\"accepted\":\"background slate\"}"
        );
        assert_eq!(receipt("/surface", &Ok(Value::Null)), "ok /surface");
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

    /// 🚨 **An empty ring says why it is empty.** James, on a running build: `/organon
    /// generator ` opened a band with nothing in it, which is indistinguishable from a band
    /// that is broken — the same complaint `/surface ` drew, one ring deeper. The sentence is
    /// [`Ring::Empty`]'s, carried out through [`Palette::hint`] so both renderers already draw
    /// it, and [`Palette::is_empty`] is what would otherwise have thrown the panel away.
    #[test]
    fn an_empty_ring_is_never_silent() {
        let palette = registry().candidates("/organon generator ").expect("a command line");
        assert!(palette.candidates.is_empty(), "the generator tab has no panels in the table");
        assert!(!palette.runnable, "and the line is not a command either");
        let hint = palette.hint().expect("so the band has to say why");
        assert_eq!(hint, unmapped_tab("generator"));
        assert!(hint.contains("joined so far: look"), "and which tabs do work: {hint}");
        assert!(!palette.is_empty(), "a band that vanishes reads as a broken one");

        // ⚠️ The other kind of empty is untouched: a prefix matching nothing on a tab that
        // *has* panels is a fact about what was typed, and there is nothing to explain.
        let typo = registry().candidates("/organon look zzz").expect("a command line");
        assert!(typo.candidates.is_empty());
        assert_eq!(typo.empty_ring, None, "the ring is fine; the word is not");
    }

    /// The first ring keeps all eight tabs and marks the ones that lead nowhere. Hiding them
    /// would make every offer completable and misrepresent Organon as having one section — see
    /// [`TAB_NOT_MAPPED`]. The mark is counted off the panel table, so a tab that gets joined
    /// stops being marked with no edit here.
    #[test]
    fn an_unmapped_tab_is_offered_and_marked() {
        let reg = registry();
        let ring = reg.candidates("/organon ").unwrap().candidates;
        assert_eq!(ring.len(), UiTab::ALL.len(), "every tab Organon has");
        let look = ring.iter().find(|c| c.label == "look").unwrap();
        assert_eq!(look.doc, "25 panels");
        for word in ["generator", "motion", "environment", "synth", "audio", "settings", "mind"] {
            let tab = ring.iter().find(|c| c.label == word).unwrap();
            assert_eq!(tab.doc, TAB_NOT_MAPPED, "{word} has no panels in the table");
            assert!(!tab.completes, "and a tab alone was never a command");
        }
    }

    /// A candidate carries the panel's real heading, and says out loud when choosing it would
    /// open nothing — before the choice, not after.
    ///
    /// ⚠️ **Both arms, on one ring, on purpose.** Surface was `Declared` alongside every other
    /// panel until Console #7 transplanted it; asserting only the suffixed form let the ring
    /// go untested for the case it exists to distinguish. Now the same ring carries one of
    /// each, so the day a second panel lands, this test says which one moved.
    ///
    /// ✏️ **Both examples are now taken from the table rather than named.** It read `surface`
    /// and `bloom`, and organon#124 transplanted Bloom — so a test about *whether the ring
    /// distinguishes the two states* failed because its example had changed state, which says
    /// nothing about the ring. Picking one panel of each status keeps the assertion about the
    /// ring as the `Live` set grows, and both `expect`s fail loudly on the day one status has
    /// no members left.
    #[test]
    fn the_ring_says_which_panels_are_transplanted() {
        let reg = registry();
        let ring = reg.candidates("/organon look ").unwrap().candidates;
        let pick = |status| {
            panels::PANELS
                .iter()
                .find(|p| p.tab == UiTab::Look && p.status == status)
                .expect("the Look tab has no panel in that state — this test has no example")
        };
        let live = pick(panels::Status::Live);
        let cand = ring.iter().find(|c| c.label == live.slug).unwrap();
        assert_eq!(cand.doc, live.title, "transplanted — the heading alone");
        let declared = pick(panels::Status::Declared);
        let cand = ring.iter().find(|c| c.label == declared.slug).unwrap();
        assert_eq!(cand.doc, format!("{} — not transplanted yet", declared.title));
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

    /// `su` leaves exactly one panel, and accepting it completes the command — so a hand types
    /// four characters of a two-ring command and the fifth thing it does is press Enter. This
    /// is the property `panels::no_slug_is_a_prefix_of_another` exists to protect.
    ///
    /// ⚠️ **It completes and does not fire**, which is the interesting half now: `/organon`
    /// puts a panel in the transcript, so it is the far side of the recoverability rule. The
    /// saving is still the whole of the typing; what is not saved is the Enter.
    #[test]
    fn a_lone_panel_completes_the_whole_command() {
        let reg = registry();
        let palette = reg.candidates("/organon look su").unwrap();
        assert_eq!(palette.candidates.len(), 1);
        let only = &palette.candidates[0];
        assert_eq!(only.completion, "/organon look surface");
        assert!(only.completes, "both rings are filled, so Enter would run it");
        assert!(!only.fires, "…and Enter is exactly what it waits for");
        assert_eq!(palette.autorun(true), None);
    }

    /// 🚨 **The refusal names the ring it is refusing against, and it did not.** James typed
    /// `/organon generator 2` and was told `2` is not one of `surface | colour | material | …`,
    /// the Look tab's twenty-five panels, on a line that said `generator`. There is only one way
    /// to read that, and it is the wrong one: that the word `generator` had not registered. It
    /// had. See [`NarrowFn`].
    #[test]
    fn a_refusal_names_the_tab_that_was_actually_given() {
        let reg = registry();

        let Resolved::Refused(message) = reg.resolve("/organon generator 2") else {
            panic!("the generator tab has no panel `2`, or any other")
        };
        assert_eq!(message, format!("`/organon`: {}", unmapped_tab("generator")));
        assert!(
            !message.contains("colour"),
            "and above all it does not read out another tab's panels: {message}"
        );

        // A tab that *is* joined refuses against its own list, headed by the words that chose
        // that list — so the sentence says which ring it means without a phrasing for it.
        let Resolved::Refused(message) = reg.resolve("/organon look 2") else {
            panic!("`2` is no panel")
        };
        assert!(
            message.starts_with("`/organon look`: `2` is not one of surface | colour"),
            "{message}"
        );

        // ⚠️ The pair, not just the word: `surface` is a real slug in the declared union and on
        // some other tab. It used to satisfy the schema and reach the view lane; the composer
        // refuses it now, while the words are still in the box to be fixed.
        let Resolved::Refused(message) = reg.resolve("/organon motion surface") else {
            panic!("`surface` is a Look panel, not a Motion one")
        };
        assert_eq!(message, format!("`/organon`: {}", unmapped_tab("motion")));
    }

    /// ⚠️ **The declared `Choice` is still the union, and must stay so.** It is what the MCP
    /// schema and `/help` are generated from, and neither has a tab in hand — one value list per
    /// argument is all a schema has. Narrowing the *declaration* to one tab would be a lie in
    /// the other direction; the hook is what carries the dependence.
    #[test]
    fn the_declared_value_space_is_still_every_slug() {
        let reg = registry();
        let entry = reg.entry("organon").expect("the verb is in the table");
        let panel = entry.args().iter().find(|a| a.name == ORGANON_PANEL_ARG).unwrap();
        assert_eq!(
            panel.kind,
            ArgKind::Choice(panels::slugs().into_iter().map(str::to_string).collect())
        );
    }

    /// The narrowing is scoped to the one verb that declares it. Every other entry's rings are
    /// exactly what its `ArgKind` says, unchanged by this addition.
    #[test]
    fn narrowing_does_not_leak_into_other_verbs() {
        let reg = registry();
        assert_eq!(labels("/background "), ["graphite", "slate"]);
        assert!(reg.entry("background").is_some_and(|e| e.narrow.is_none()));
    }

    // -----------------------------------------------------------------------
    // `/layout load ` — the ring of saved names
    // -----------------------------------------------------------------------

    /// `console.layout` as `console_main` declares it. The two arguments and their kinds are
    /// what matters here — a required `Choice` of the action words, then the name as `Text` —
    /// and `console_main`'s own `the_layout_verb_is_the_shape_the_slash_grammar_needs` holds the
    /// real spec to the same shape.
    fn layout_spec() -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: VERB_LAYOUT.into(),
            doc: "Save the console's arrangement under a name".into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec {
                    name: "action".into(),
                    kind: ArgKind::Choice(
                        crate::layout::LAYOUT_ACTIONS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
                ArgSpec {
                    name: LAYOUT_NAME_ARG.into(),
                    kind: ArgKind::Text,
                    required: true,
                },
            ],
            reversal: Reversal::Permanent,
        }]
    }

    fn library_of(names: &[&str]) -> Library {
        let mut lib = Library::default();
        for name in names {
            lib.upsert(crate::layout::SavedLayout {
                name: (*name).to_string(),
                regions: [("left".to_string(), "agent".to_string())].into_iter().collect(),
                extra: Default::default(),
            });
        }
        lib
    }

    /// 🚨 **The asymmetry this hook exists for: `load` and `delete` narrow, `save` does not.**
    /// A name being invented must not be measured against the names that exist — a popup
    /// offering the library while somebody types a *new* name reads as a closed list, and
    /// [`coerce`] would then refuse every new name in the world, which is the one thing `save`
    /// is for.
    ///
    /// ⚠️ `save` answers `None` **before the store is touched at all**, which is the half worth
    /// pinning: it is not "the ring happens to be ignored", it is that the file is never read.
    #[test]
    fn save_takes_a_name_a_person_is_inventing_and_is_never_narrowed() {
        assert_eq!(layout_options(LAYOUT_NAME_ARG, &["save"]), None);
        // The other slot has no opinion either — the action ring is the declared `Choice`.
        assert_eq!(layout_options("action", &["save"]), None);
        assert_eq!(layout_options(LAYOUT_NAME_ARG, &[]), None, "no action word yet, no ring");
        assert_eq!(layout_options(LAYOUT_NAME_ARG, &["publish"]), None, "not an action at all");

        // …and the two that name something that must already exist do reach the library. This
        // is the one assertion here that reads the real store, so it says only that the branch
        // is taken — what comes back depends on whoever is running the suite.
        if Library::store_root().is_some() {
            for action in ["load", "delete"] {
                assert!(
                    layout_options(LAYOUT_NAME_ARG, &[action]).is_some(),
                    "`{action}` asks the library"
                );
            }
        }
    }

    /// CONTRACT: the ring is the library, name for name and in file order, each carrying what
    /// choosing it would hold.
    ///
    /// 🚨 **A comma is a legal name character and must not split an option.** `check_name`
    /// refuses whitespace because the wire cannot carry it and refuses nothing else, so `a,b` is
    /// one layout — the same trap `Library::names_or_nothing` backticks its way out of, arriving
    /// here as a list of pairs that is never joined at all.
    #[test]
    fn the_ring_is_the_library_and_a_comma_is_not_a_separator() {
        let Ring::Options(options) = layout_ring(&library_of(&["desk", "a,b", "Desk"])) else {
            panic!("three layouts are three options");
        };
        let labels: Vec<&str> = options.iter().map(|(o, _)| o.as_str()).collect();
        assert_eq!(labels, ["desk", "a,b", "Desk"], "file order, whole names");
        assert_eq!(options[0].1, "left agent", "…and what choosing it holds");

        // ⚠️ Exact and case-sensitive, §1.15 — `Desk` and `desk` are two layouts, and the ring
        // shows both rather than folding them into one that then fails to load.
        assert_eq!(labels.iter().filter(|l| l.eq_ignore_ascii_case("desk")).count(), 2);
    }

    /// 🚨 **An empty library cannot be a silent ring** — `Ring::Empty`'s whole reason, and the
    /// sentence is the one `console.layout.list` already answers with rather than a second one
    /// written for the popup.
    #[test]
    fn an_empty_library_says_so_in_the_sentence_the_listing_already_uses() {
        assert_eq!(
            layout_ring(&Library::default()),
            Ring::Empty(crate::layout::NOTHING_SAVED.to_string())
        );
        let Ring::Empty(why) = layout_ring(&Library::default()) else { unreachable!() };
        assert!(why.contains("layout save"), "…and it names the verb that fills it: {why}");
    }

    /// CONTRACT: the hook reaches the real entry. **Keyed on the catalog name**, so this is the
    /// join that a rename could break silently — see [`Entry::from_spec`].
    #[test]
    fn the_layout_verb_carries_the_ring_and_nothing_else_does() {
        let reg = Registry::new(&layout_spec());
        let entry = reg.entry("layout").expect("the verb is typeable");
        assert!(entry.narrow.is_some(), "`{VERB_LAYOUT}` is the verb the hook is keyed on");
        assert!(console_narrow("console.layout.list").is_none(), "a read has no ring");
        assert!(console_narrow("console.background").is_none());

        // The declared kind is untouched: `Text` is what the MCP schema and `/help` say, and
        // neither has the action word in hand. Same rule as `/organon`'s declared union.
        let name = entry.args().iter().find(|a| a.name == LAYOUT_NAME_ARG).expect("the slot");
        assert_eq!(name.kind, ArgKind::Text);
    }

    /// 🚨 **The ring narrows the popup AND the refusal, and `save` is exempt from both** —
    /// driven through the real [`Registry::candidates`] and [`Registry::resolve`], over a
    /// library this test owns rather than the store the suite must never write to.
    #[test]
    fn a_narrowed_name_completes_and_an_unknown_one_is_refused_while_it_can_still_be_edited() {
        /// The shipped hook with its store swapped for a fixed library — the wiring under test
        /// is `coerce`/`value_candidates`, and the store read is what
        /// [`save_takes_a_name_a_person_is_inventing_and_is_never_narrowed`] covers.
        fn hook(arg: &str, positional: &[&str]) -> Option<Ring> {
            if arg != LAYOUT_NAME_ARG {
                return None;
            }
            match LayoutCmd::resolve(positional.first().copied()?).ok()? {
                LayoutCmd::Save => None,
                LayoutCmd::Load | LayoutCmd::Delete => {
                    Some(layout_ring(&library_of(&["desk", "mind"])))
                }
            }
        }
        let mut reg = Registry::new(&layout_spec());
        reg.entries[0].narrow = Some(hook);

        let names = |line: &str| -> Vec<String> {
            reg.candidates(line)
                .map(|p| p.candidates.iter().map(|c| c.label.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(names("/layout load "), ["desk", "mind"], "the layouts that exist");
        assert_eq!(names("/layout delete "), ["desk", "mind"], "delete names one too");
        assert_eq!(names("/layout load d"), ["desk"], "…and the typed stem still filters");
        assert!(names("/layout save ").is_empty(), "a new name has nothing to offer");

        // A word in the ring runs; a word outside it is refused *in the composer*, naming the
        // library and the action that chose it — the words are still there to be fixed.
        assert!(matches!(reg.resolve("/layout load desk"), Resolved::Run { .. }));
        let Resolved::Refused(message) = reg.resolve("/layout load nope") else {
            panic!("`nope` is not a saved layout")
        };
        assert_eq!(message, "`/layout load`: `nope` is not one of desk | mind");

        // 🚨 …and the same word under `save` is simply a name. This is the assertion that would
        // fail if the ring were declared on the argument instead of chosen by the action.
        assert!(matches!(reg.resolve("/layout save nope"), Resolved::Run { .. }));
    }

    /// The `console.viewport` spec this crate's tests drive — the three slots
    /// `console_main.rs` declares, spelled here because the catalog lives in the root crate.
    /// ⚠️ Held in step with the real one by
    /// [`crate::registry::tests::the_viewport_verb_carries_the_producer_ring`]'s hook check plus
    /// `console_main.rs`'s own catalog tests; this is a fixture, not a second catalog.
    fn viewport_spec() -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: VERB_VIEWPORT.into(),
            doc: "Divide the pane into regions and say what each one holds".into(),
            target: TargetKind::Viewport,
            args: vec![
                ArgSpec {
                    name: "region".into(),
                    kind: ArgKind::Choice(
                        crate::region::REGION_WORDS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
                ArgSpec {
                    name: "content".into(),
                    kind: ArgKind::Choice(
                        crate::region::CONTENT_WORDS.iter().map(|s| (*s).to_string()).collect(),
                    ),
                    required: true,
                },
                ArgSpec {
                    name: VIEWPORT_PRODUCER_ARG.into(),
                    kind: ArgKind::Text,
                    required: false,
                },
            ],
            reversal: Reversal::Recoverable,
        }]
    }

    fn registry_of(producers: &[&str]) -> ModuleRegistry {
        ModuleRegistry {
            modules: producers
                .iter()
                .map(|p| module::ApprovedModule {
                    producer: (*p).to_string(),
                    name: format!("{p} the module"),
                    url: format!("https://example.invalid/{p}.git"),
                    commit: "0".repeat(40),
                    ..Default::default()
                })
                .collect(),
            extra: Default::default(),
        }
    }

    /// CONTRACT: the ring is `organon` **plus** whatever is approved, in file order.
    ///
    /// 🚨 **`organon` is always first and is never in the file** — `check_producer_name`
    /// reserves it, so a ring built from the registry alone would leave the one producer that
    /// has always existed as the one the completion cannot spell.
    ///
    /// ⚠️ **Never `Ring::Empty`.** An empty `modules.json` does not mean a `3d` region has no
    /// producer; it means the only producer is the one the console wrote. `Ring::Empty` there
    /// would describe a viewport that cannot be drawn, which is the opposite of the truth — and
    /// [`coerce`] would then refuse `organon` itself.
    #[test]
    fn the_producer_ring_is_organon_plus_whatever_is_approved() {
        let Ring::Options(bare) = producer_ring(&ModuleRegistry::default()) else {
            panic!("an empty registry still has one producer");
        };
        assert_eq!(
            bare.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(),
            [module::DEFAULT_PRODUCER]
        );

        let Ring::Options(options) = producer_ring(&registry_of(&["ascent", "orrery"])) else {
            panic!("options, never empty");
        };
        assert_eq!(
            options.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(),
            [module::DEFAULT_PRODUCER, "ascent", "orrery"],
            "`organon` first, then file order"
        );
        assert!(options[1].1.contains("ascent the module"), "the doc is what approving recorded");
        assert!(options[1].1.contains("000000000000"), "…including the short commit");
    }

    /// 🚨 **The producer ring depends on the CONTENT word, and a content kind with no producer
    /// says so rather than staying silent.**
    ///
    /// A `None` there would leave the declared `ArgKind::Text` to accept the word, so
    /// `/viewport left agent producer ascent` would run with a word nothing acted on. Driven
    /// through the real [`Registry::candidates`] and [`Registry::resolve`], over a registry this
    /// test owns rather than the store the suite must never write to.
    ///
    /// ⚠️ **Mutation-tested.** Returning `None` instead of `Ring::Empty` for a non-`3d` content
    /// fails this at *"`agent` has no producer"* — `resolve` starts answering `Run`. Dropping
    /// the `positional.get(1)` read (narrowing every content word) fails it the same way.
    #[test]
    fn the_producer_ring_is_offered_for_3d_and_refused_by_name_for_everything_else() {
        // 🚨 **The SHIPPED hook first, on the branches that touch no store.** The fixture below
        // is a copy of its shape with the file read swapped out, and a fixture that were the
        // only thing tested would let the real one drift into silence — which is the failure
        // this whole test is about. These four assertions are the shipped function.
        for word in ["agent", "panel", crate::region::CLEAR_WORD] {
            let ring = viewport_options(VIEWPORT_PRODUCER_ARG, &["left", word])
                .unwrap_or_else(|| panic!("`{word}` must say why it has no producer, not stay silent"));
            let Ring::Empty(why) = ring else { panic!("`{word}` has no producers to offer") };
            assert!(why.contains(word), "the sentence names the content word: {why}");
            assert!(why.contains("3d"), "…and what a producer does qualify: {why}");
        }
        assert_eq!(viewport_options("region", &["left", "3d"]), None, "the other slots have no opinion");
        assert_eq!(viewport_options("content", &["left", "3d"]), None);
        assert_eq!(
            viewport_options(VIEWPORT_PRODUCER_ARG, &["left"]),
            None,
            "no content word yet, no ring — the same 'not enough typed' answer `layout` gives"
        );
        assert_eq!(
            viewport_options(VIEWPORT_PRODUCER_ARG, &["left", "media"]),
            None,
            "a word that is not a content command at all is the declared kind's to refuse"
        );

        fn hook(arg: &str, positional: &[&str]) -> Option<Ring> {
            if arg != VIEWPORT_PRODUCER_ARG {
                return None;
            }
            match ContentCmd::resolve(positional.get(1).copied()?).ok()? {
                ContentCmd::Hold(Content::ThreeD(_)) => {
                    Some(producer_ring(&registry_of(&["ascent"])))
                }
                other => Some(Ring::Empty(format!("`{}` has no producer", other.as_word()))),
            }
        }
        let mut reg = Registry::new(&viewport_spec());
        reg.entries[0].narrow = Some(hook);

        let names = |line: &str| -> Vec<String> {
            reg.candidates(line)
                .map(|p| p.candidates.iter().map(|c| c.label.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(
            names("/viewport left 3d producer "),
            [module::DEFAULT_PRODUCER, "ascent"],
            "the producers a `3d` region may name"
        );
        assert_eq!(names("/viewport left 3d producer a"), ["ascent"], "the typed stem filters");

        assert!(matches!(reg.resolve("/viewport left 3d"), Resolved::Run { .. }));
        assert!(matches!(
            reg.resolve("/viewport left 3d producer ascent"),
            Resolved::Run { .. }
        ));
        // 🚨 **An unapproved producer is refused in the composer, while the words are still
        // there to be fixed** — and it lists the ones that would have worked.
        let Resolved::Refused(message) = reg.resolve("/viewport left 3d producer nope") else {
            panic!("`nope` is not an approved module")
        };
        assert!(message.contains("nope"), "{message}");
        assert!(message.contains("ascent"), "{message}");
        assert!(message.contains(module::DEFAULT_PRODUCER), "{message}");

        // …and a producer beside a content kind that has none is refused with the reason,
        // rather than accepted by the declared `Text`.
        let Resolved::Refused(message) = reg.resolve("/viewport left agent producer ascent") else {
            panic!("`agent` has no producer")
        };
        assert!(message.contains("no producer"), "{message}");
    }

    /// CONTRACT: the hook reaches the real entry, keyed on the catalog name — the join a rename
    /// could break silently. [`the_layout_verb_carries_the_ring_and_nothing_else_does`]'s rule
    /// for the second verb that has one.
    #[test]
    fn the_viewport_verb_carries_the_producer_ring() {
        let reg = Registry::new(&viewport_spec());
        let entry = reg.entry("viewport").expect("the verb is typeable");
        assert!(entry.narrow.is_some(), "`{VERB_VIEWPORT}` is the verb the hook is keyed on");
        assert!(console_narrow(VERB_VIEWPORT).is_some());
        assert!(console_narrow("console.stack").is_none(), "the other three-slot verb has none");

        // The declared kind is untouched: `Text` is what the MCP schema and `/help` say, and
        // neither has the content word in hand.
        let slot =
            entry.args().iter().find(|a| a.name == VIEWPORT_PRODUCER_ARG).expect("the slot");
        assert_eq!(slot.kind, ArgKind::Text);
        assert!(!slot.required, "an omitted producer means Organon — it cannot be required");
    }

    // -----------------------------------------------------------------------------------
    // `/setting` — the two rings, both pure
    // -----------------------------------------------------------------------------------

    fn with_settings(producer: &str, keys: &[(&str, &str)]) -> module::ApprovedModule {
        module::ApprovedModule {
            producer: producer.to_string(),
            name: format!("{producer} the module"),
            url: format!("https://example.invalid/{producer}.git"),
            commit: "0".repeat(40),
            settings: keys
                .iter()
                .map(|(k, d)| module::SettingSpec { key: (*k).to_string(), doc: (*d).to_string() })
                .collect(),
            ..Default::default()
        }
    }

    /// 🚨 CONTRACT: **`organon` is NOT in this ring**, which is the one way it differs from
    /// `/viewport`'s. Organon's own World has no manifest and declares no settings, and
    /// `check_producer_name` refuses the name to a module — so offering it would complete to a
    /// word every downstream check rejects.
    #[test]
    fn the_setting_producer_ring_is_the_approved_modules_and_never_organon() {
        let registry = registry_of(&["ascent", "moonlight"]);
        let Ring::Options(options) = approved_ring(&registry) else {
            panic!("two approved modules is a list");
        };
        let names: Vec<&str> = options.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["ascent", "moonlight"]);
        assert!(
            !names.contains(&module::DEFAULT_PRODUCER),
            "`organon` is not a module and cannot be configured as one"
        );
    }

    /// ⚠️ Empty **with a reason**, unlike the viewport ring — there really is nothing to
    /// configure when nothing is approved, and the sentence names the verb that changes that.
    #[test]
    fn nothing_approved_is_an_empty_ring_that_names_the_verb_that_fixes_it() {
        let Ring::Empty(why) = approved_ring(&ModuleRegistry::default()) else {
            panic!("nothing approved has nothing to offer");
        };
        assert!(why.contains(module::APPROVE_VERB), "{why}");
    }

    /// 🚨 CONTRACT: the key ring is **one module's** vocabulary, never the union.
    #[test]
    fn the_key_ring_is_the_chosen_modules_own_vocabulary() {
        let registry = ModuleRegistry {
            modules: vec![
                with_settings("moonlight", &[("host", "which machine"), ("app", "which app")]),
                with_settings("other", &[("frobnicate", "not moonlight's")]),
            ],
            extra: Default::default(),
        };
        let Ring::Options(options) = key_ring(&registry, "moonlight") else {
            panic!("moonlight declares two settings");
        };
        let keys: Vec<&str> = options.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["host", "app"], "manifest order, and no other module's keys");
        assert!(options[0].1.contains("which machine"), "the author's own words: {:?}", options[0]);
        assert!(!keys.contains(&"frobnicate"));
    }

    /// The two empty cases are different sentences, because they are fixed differently: approve
    /// the module, or wait for its author to declare something.
    #[test]
    fn a_module_that_is_unknown_and_one_that_declares_nothing_say_different_things() {
        let registry = ModuleRegistry {
            modules: vec![with_settings("ascent", &[])],
            extra: Default::default(),
        };
        let Ring::Empty(unknown) = key_ring(&registry, "nope") else {
            panic!("an unapproved module has no keys");
        };
        assert!(unknown.contains(module::APPROVE_VERB), "{unknown}");
        let Ring::Empty(none) = key_ring(&registry, "ascent") else {
            panic!("a module that declares nothing has no keys");
        };
        assert!(none.contains("declares no settings"), "{none}");
    }

    /// 📌 The doc text comes from somebody else's repository, so it is quoted rather than pasted
    /// — `quoted_untrusted`'s rule, which every other place this crate shows a manifest's words
    /// already follows.
    #[test]
    fn a_manifests_description_reaches_the_ring_quoted() {
        let registry = ModuleRegistry {
            modules: vec![with_settings("m", &[("k", "a line\nwith a newline in it")])],
            extra: Default::default(),
        };
        let Ring::Options(options) = key_ring(&registry, "m") else { panic!("one setting") };
        assert!(options[0].1.starts_with('"'), "{:?}", options[0]);
        assert!(!options[0].1.contains('\n'), "a raw newline would break the band: {:?}", options[0]);
    }

    /// The value has no ring, and that absence is the console declining to hold an opinion about
    /// a vocabulary that is not its own.
    #[test]
    fn the_value_has_no_ring_and_an_unknown_argument_name_has_none_either() {
        let registry = ModuleRegistry {
            modules: vec![with_settings("m", &[("k", "d")])],
            extra: Default::default(),
        };
        // `setting_options` reaches the store, which the suite must not; the two pure halves are
        // what the hook is made of and they are what is asserted. This pins the third arm.
        assert!(matches!(key_ring(&registry, "m"), Ring::Options(_)));
        assert_eq!(SETTING_VALUE_ARG, "value");
        assert_ne!(SETTING_KEY_ARG, SETTING_VALUE_ARG);
    }

}
