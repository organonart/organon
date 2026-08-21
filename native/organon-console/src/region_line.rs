//! **A command line inside a region — the fifth front door onto one table.**
//!
//! [`crate::registry`] is the console's vocabulary and §1.8 lists the doors onto it: a CLI line,
//! an agent's MCP tool, a `/`-command in the composer, and a pie menu that is shaped for but not
//! built. This is the fifth, and it is **not a fifth vocabulary**: every line typed here goes
//! through [`Registry::resolve`] and arrives at the same dispatch as the other four. What the
//! region adds is **context** — the rectangle you typed into is a word you no longer have to
//! say.
//!
//! # 🚨 The hard part is focus, and it is not parsing
//!
//! The console had exactly one composer, and §1.9's whole command panel is bound to it —
//! including `composer_keys`, which consumes Tab, Escape and the arrows out of the **raw event
//! list**, not out of a focused widget. Two of those are taken unconditionally on an empty box
//! (`arrow_owner` hands Up to the history when the composer is empty), so a second input would
//! have found its own Up already gone before it ran. Parsing a slash line in a region is the
//! easy half; deciding who owns the keyboard is the tier.
//!
//! **The owner is measured, never asserted.** [`Lines::owner`] is the region whose line had
//! egui focus *last frame* — recorded by [`draw`] from the `TextEdit`'s own
//! [`egui::Response::has_focus`], which is the same fact `composer_box` already reads to decide
//! whether Enter sends. Nothing invents a focus state and nothing fights egui's: clicking a box
//! is what moves focus, egui guarantees at most one focused widget, and this only *observes*
//! which. [`Lines::composer_owns_keys`] is the one value `console_main` hands the conversation
//! front-end, and `composer_keys` returns early when it is false.
//!
//! ⚠️ **One frame behind, deliberately and boundedly.** The region walk visits regions in
//! `Region::ALL` order, so a line drawn *after* the agent region cannot tell the composer
//! anything before the composer has already read the frame's keys. The previous frame's
//! measurement is therefore what both sides gate on — the same one-frame-behind arrangement
//! `draw_regions` already uses for the `3d` region's rectangle, and for its reason (an answer
//! that is an output of the layout that produced it). The cost is that the frame on which focus
//! *moves* is arbitrated by the old owner, and that frame carries no keystroke: focus moves by a
//! **click**, and a click is not a key.
//!
//! ⚠️ **A default console has no region lines at all**, so none of this is reachable until a
//! `/viewport` has been typed. `Lines::default()`'s owner is `None`, `composer_owns_keys` is
//! `true`, and the composer behaves exactly as it did before this module existed.
//!
//! # 🚨 Prune discovery, never capability
//!
//! **The list is narrow; the table is whole.** [`palette`] offers a region's *own* verbs — what
//! it can hold, and what its column can be given — and nothing else. [`act`] accepts every
//! console verb in the registry: `/theme dark` typed into a panel column **works**, because a
//! palette is a console-wide setting and refusing it here would turn a region into a jail.
//! Loosening a refusal later is much harder than tightening an offer.
//!
//! ⚠️ **The one thing genuinely refused is the view lane**, and it is refused by name.
//! `/surface`, `/media` and `/organon` put an element in *a transcript*, and a region holding a
//! column is not one — so the refusal says that and names where the verb does work. That is a
//! statement about which surface answers, not a restriction invented here.
//!
//! # ⚠️ The pruned surface says what it left out
//!
//! A verb silently absent is the defect this console keeps a tally of, arriving through the
//! newest surface. [`RegionPalette::elsewhere`] is `Ring::Empty`'s precedent one scale up — a
//! thing that cannot exist without its sentence — so the field is a `String` and never an
//! `Option<String>`: *"`/background` belongs to the console line, not this panel column, but it
//! runs here anyway"* is an answer, and an absence is not.
//!
//! # 📌 What an unassigned region shows
//!
//! Its own command line, offering the four content words. A region that holds nothing stops
//! apologising for itself and becomes the thing that fixes it: type `panel` into the empty
//! rectangle and it holds a column. That is the free consequence of the region being context —
//! `/viewport <region> panel` sheds its region word and becomes `/panel`.
//!
//! # ⚠️ What this line deliberately does NOT do
//!
//! **No self-completion and no autorun.** Both are §1.9 rules of the composer's, and both rest
//! on `completion_held` — the latch that keeps a completion from undoing a backspace on the
//! frame it happens. That latch reads a shadow copy of the composer taken at the top of the
//! frame, and reproducing it here without its measurement would reintroduce the worst defect
//! the command panel has had (*"once I have typed slash surface, I am no longer able to
//! backspace out of it"*). **Tab accepts, Enter runs**, which is the pair §1.9 says must never
//! be the same key.
//!
//! **No history.** Up and Down move the palette highlight and mean nothing else here. The
//! composer's recall buffer exists because that box is also where prose is written and a
//! command scrolls away among it; a region line holds one command at a time and the last one
//! is still in the box until it is replaced.

use egui::{Frame, RichText};
use serde_json::Value;

use crate::conversation_view::{compact_join, CompactWord};
use crate::posture::Form;
use crate::region::{Content, Region, CONTENT_WORDS, REGION_COUNT};
use crate::registry::{Candidate, CandidateKind, Lane, Registry, Resolved};
use crate::theme::Theme;

/// Where a line is being typed: which region, and what that region holds.
///
/// ⚠️ **The content is `Option`, and the `None` is the interesting case** — a region that holds
/// nothing still gets a line, and it is the only surface from which an unassigned rectangle can
/// be assigned without leaving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub region: Region,
    pub content: Option<Content>,
}

/// One verb a region's line supplies a word for — **what you type, and what the registry hears**.
///
/// 🚨 **The region is supplied in two shapes, because the two verbs' grammars differ, and both
/// are the same claim: you did not type it.** `viewport` takes its region as a *required
/// positional*, so it has to be spelled into [`Shed::heard`] — `/panel` is heard as
/// `viewport left panel`. `stack` takes it as the *optional keyword* [`crate::panel_stack::
/// REGION_ARG`], which `registry::parse_args` fills by name after the required words, so it
/// cannot be a prefix and rides [`Shed::supplies`] into the resolved arguments instead — where
/// a typed keyword would have landed.
///
/// ⚠️ **[`Shed::heard`] replaces the first word and nothing else**, which is what makes the
/// completions un-expandable: everything a person typed after the verb is untouched, so a
/// completion built against the expanded line differs from the one they should see only in its
/// head. See [`unexpand`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shed {
    /// The word typed after the slash — `add`, `panel`, `off`.
    pub typed: String,
    /// The words [`Registry::resolve`] hears in its place — `stack add`, `viewport left panel`.
    pub heard: String,
    /// An argument the region supplies after resolution, as `(name, value)`. Never typed.
    pub supplies: Option<(String, String)>,
    /// One line about it, for the ring.
    pub doc: String,
}

/// The verbs this region's line offers, in the order they should be listed.
///
/// 🚨 **Derived from the region and its content, never a curated list.** The four content words
/// are `region::CONTENT_WORDS`, which is the same table `/viewport`'s second ring is built from,
/// so a content kind added there reaches this line in the commit that adds it. The two stack
/// actions are `panel_stack::STACK_ACTIONS`, the same table the CLI's `--help` reads.
///
/// ⚠️ **Every content word is offered, including the one the region already holds**, and that
/// is a decision rather than an oversight. Dropping `panel` from a panel region's list would be
/// a *second* kind of absence — one that means "this would do nothing" rather than "this
/// belongs elsewhere" — and every absence in this surface has to carry its own sentence. One
/// kind of absence, one sentence. Re-assigning what is already held is harmless, and `off` on
/// an empty region is refused by name by `region::Refusal::AlreadyEmpty`, which is a better
/// answer than a missing word.
pub fn shed(ctx: Context) -> Vec<Shed> {
    let mut out: Vec<Shed> = CONTENT_WORDS
        .iter()
        .map(|word| Shed {
            typed: (*word).to_string(),
            heard: format!("viewport {} {word}", ctx.region.as_word()),
            supplies: None,
            doc: content_doc(word),
        })
        .collect();
    if ctx.content == Some(Content::Panel) {
        out.extend(crate::panel_stack::STACK_ACTIONS.iter().map(|action| Shed {
            typed: (*action).to_string(),
            heard: format!("stack {action}"),
            supplies: Some((
                crate::panel_stack::REGION_ARG.to_string(),
                ctx.region.as_word().to_string(),
            )),
            doc: action_doc(action),
        }));
    }
    out
}

/// What one stack action does to **this** column. A catch-all rather than a second literal, so
/// a third action added to `panel_stack::STACK_ACTIONS` without a case here gets a plain line
/// rather than a panic in the frame path — `StackCmd::resolve`'s own arrangement.
fn action_doc(action: &str) -> String {
    match action {
        "add" => "put one of Organon's editor panels at the bottom of this column".to_string(),
        "remove" => format!(
            "take the last copy out of this column, or `{}` to empty it",
            crate::panel_stack::ALL_WORD
        ),
        other => format!("`{other}` this column"),
    }
}

/// What one content word does to the region you typed it into. **Written here rather than read
/// off `Content`**, because these sentences are about *this rectangle* — "hold a scrolling
/// column" reads differently from a `--help` line describing a kind in the abstract.
fn content_doc(word: &str) -> String {
    match word {
        "agent" => "show a conversation here",
        "panel" => "hold a scrolling column of Organon's editor panels",
        "3d" => "hold a live, orbitable view of the world",
        "off" => "empty this region",
        _ => "assign this region",
    }
    .to_string()
}

/// The line as [`Registry::resolve`] and [`Registry::candidates`] hear it.
///
/// Returns the line unchanged when its first word is not one this region sheds — which is the
/// whole of "prune discovery, never capability": an unshed verb is passed to the registry
/// verbatim and answered exactly as the composer would answer it.
pub fn expand(ctx: Context, line: &str) -> String {
    let Some((word, rest)) = head(line) else { return line.to_string() };
    let Some(found) = shed(ctx).into_iter().find(|s| s.typed == word) else {
        return line.to_string();
    };
    format!("/{}{rest}", found.heard)
}

/// The typed verb word and everything after it, for a line that begins with one slash.
///
/// `None` for a line that is not a command line at all — no slash, an escaped `//`, or a bare
/// `/` with no word yet. All three are the registry's own rules 1–3, read here so the expansion
/// cannot invent a verb out of prose.
fn head(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('/')?;
    if rest.starts_with('/') {
        return None;
    }
    let word: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    if word.is_empty() {
        return None;
    }
    let tail = rest[word.len()..].to_string();
    Some((word, tail))
}

/// Turn a completion built against the **expanded** line back into one for the line a person is
/// looking at.
///
/// 🚨 **A pure head swap, and that is why it is total.** [`expand`] replaces the first word and
/// leaves every later character alone, so a completion produced from the expanded line agrees
/// with the typed line from the end of the expanded head onwards. Rebuilding the completion any
/// other way would mean re-deriving the stem, which is the one thing `Candidate::completion`
/// exists so that nobody has to.
fn unexpand(typed_head: &str, expanded_head: &str, completion: &str) -> Option<String> {
    let tail = completion.strip_prefix(expanded_head)?;
    Some(format!("{typed_head}{tail}"))
}

/// Every continuation of a half-typed region line, pruned to this region's own vocabulary, with
/// the sentence naming what the pruning left out.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionPalette {
    /// The narrow list, in [`shed`] order. May be empty; [`RegionPalette::elsewhere`] is not.
    pub candidates: Vec<Candidate>,
    /// 🚨 **What this list left out, always.** Never empty and never optional — see the module
    /// header on why a pruned surface that can be silent is #92's defect class arriving through
    /// a new door.
    pub elsewhere: String,
    /// The line **as it stands** is already a complete command, so Enter would run it.
    pub runnable: bool,
    /// What to type when there is nothing to list — the registry's own hint, carried through.
    pub hint: Option<String>,
}

/// How many verb names [`RegionPalette::elsewhere`] names before it starts counting.
///
/// ⚠️ **A count, never an ellipsis**, on `compact_fit`'s rule and for its reason: egui's own
/// truncation appends U+2026, which is in none of its bundled fonts and ships as a box.
const ELSEWHERE_NAMED: usize = 4;

/// What this region's line could become next, or `None` if the line is not a command line.
///
/// ⚠️ **`None` is the same answer `Registry::candidates` gives**, for a weaker version of its
/// reason: nothing here is prose, but a line that has not reached a slash yet has nothing to
/// offer, and drawing a band over it would be drawing a band over an empty box.
pub fn palette(registry: &Registry, ctx: Context, line: &str) -> Option<RegionPalette> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('/')?;
    if rest.starts_with('/') {
        return None;
    }
    let sheds = shed(ctx);
    let runnable = matches!(act(registry, ctx, line), Act::Run { .. });
    // A bare `/` has no word yet; `head` says so with `None` and the two collapse to the same
    // question — *which verb* — asked with nothing typed.
    let (word, tail) = head(line).unwrap_or_default();

    // 🚨 **Whitespace after the first word is what separates the two questions**, exactly as
    // `Registry::candidates` reads a trailing space: with none, the verb is still being typed
    // and this ring is the region's own; with some, the verb has settled and every later ring
    // is the registry's, because those are its arguments and it owns their value spaces.
    if tail.is_empty() {
        let stem = &trimmed[..trimmed.len() - word.len()];
        let candidates = sheds
            .iter()
            .filter(|s| narrows(&s.typed, &word))
            .map(|s| shed_candidate(registry, ctx, s, stem))
            .collect();
        return Some(RegionPalette {
            candidates,
            elsewhere: elsewhere(registry, ctx, &sheds, &word),
            runnable,
            hint: None,
        });
    }

    let found = sheds.iter().find(|s| s.typed == word);
    let typed_head = format!("/{word}");
    let expanded_head = match found {
        Some(s) => format!("/{}", s.heard),
        None => typed_head.clone(),
    };
    // 🚨 **The argument this region supplies is never offered.** `stack`'s optional `region`
    // keyword is a real part of the verb's grammar and the registry offers it, correctly — but
    // in a line whose whole point is that the region is context, offering the word would invite
    // somebody to type a *second*, contradicting one. It is dropped from the ring and named in
    // [`RegionPalette::elsewhere`], because an absence without its sentence is the defect this
    // surface exists not to repeat.
    let supplied = found.and_then(|s| s.supplies.as_ref()).map(|(name, _)| name.as_str());
    let inner = registry.candidates(&expand(ctx, line))?;
    let candidates = inner
        .candidates
        .iter()
        .filter(|c| supplied != Some(c.label.as_str()))
        .filter_map(|c| {
            unexpand(&typed_head, &expanded_head, &c.completion)
                .map(|completion| Candidate { completion, ..c.clone() })
        })
        .collect();
    Some(RegionPalette {
        candidates,
        elsewhere: supplied_note(ctx, supplied, elsewhere(registry, ctx, &sheds, &word)),
        runnable,
        hint: inner.hint(),
    })
}

/// Put the supplied argument's own sentence in front of the pruning's.
///
/// ⚠️ **One field, two absences, and both are named.** The verb list is pruned and says so; a
/// keyword the region fills is dropped and says so. They travel in the same string because a
/// renderer that had to decide which of two sentences to draw would eventually draw neither.
fn supplied_note(ctx: Context, supplied: Option<&str>, rest: String) -> String {
    match supplied {
        Some(name) => format!(
            "`{name}` is supplied by the line you are typing in: it is `{}`. {rest}",
            ctx.region.as_word()
        ),
        None => rest,
    }
}

fn is_shed(sheds: &[Shed], word: &str) -> bool {
    sheds.iter().any(|s| s.typed == word)
}

/// Case-insensitive prefix, [`crate::registry`]'s own matching rule. Prefix rather than fuzzy
/// for its reason: "press another key and it narrows" has to be literally true.
fn narrows(word: &str, typed: &str) -> bool {
    word.to_ascii_lowercase().starts_with(&typed.to_ascii_lowercase())
}

/// One shed verb as a candidate, with the line accepting it would produce.
///
/// The trailing space is `verb_candidate`'s rule: a verb that still wants an argument opens its
/// ring the instant it is accepted, and one that does not is left without a space it will never
/// fill. Whether it wants one is asked of the **expanded** line, so the answer comes from the
/// registry's own schema rather than from a second opinion here.
fn shed_candidate(registry: &Registry, ctx: Context, s: &Shed, stem: &str) -> Candidate {
    let bare = format!("{stem}{}", s.typed);
    let wants_more = !matches!(act(registry, ctx, &bare), Act::Run { .. });
    let completion = if wants_more { format!("{bare} ") } else { bare };
    let completes = matches!(act(registry, ctx, &completion), Act::Run { .. });
    Candidate {
        label: s.typed.clone(),
        doc: s.doc.clone(),
        completion,
        // ⚠️ **`Verb`, and the group is the region's own word.** A renderer that groups by
        // `CandidateKind::Verb { group }` — `/help`, a pie menu's root ring — would otherwise
        // file these under whichever catalog group the expansion happened to land in, which is
        // machinery rather than what a person typed.
        kind: CandidateKind::Verb { group: ctx.region.as_word().to_string(), lane: Lane::Console },
        completes,
        // 🚨 **Never**, and it is not a value copied from the entry. `fires` is autorun's third
        // term, and this line has no autorun — see the module header on why reproducing it
        // without `completion_held` would reintroduce the backspace trap. Answering `false`
        // everywhere is the honest reading of a switch that is off.
        fires: false,
    }
}

/// The sentence a pruned list owes: which verbs are not in it, and where they belong.
///
/// 🚨 **Generated from the registry, never written out.** A hand-kept list of "the other verbs"
/// is the second vocabulary §1.8 exists to prevent, reached from the friendliest direction —
/// and it would be wrong the day a verb was added rather than the day somebody noticed.
fn elsewhere(registry: &Registry, ctx: Context, sheds: &[Shed], typed: &str) -> String {
    let held = ctx.content.map_or("nothing".to_string(), |c| format!("`{}`", c.as_word()));
    let others: Vec<&str> = registry
        .verbs()
        .into_iter()
        .filter(|verb| !is_shed(sheds, verb))
        .filter(|verb| narrows(verb, typed))
        .collect();
    if others.is_empty() {
        return format!(
            "this list is `{}`'s own: it holds {held}. Nothing else in the console's \
             vocabulary begins `{typed}`; `/help` at an agent lists every verb",
            ctx.region.as_word()
        );
    }
    // ⚠️ **Backticked, on `layout`'s own rule one module over**: a bare join renders a list of
    // words that read as prose, and the reader cannot tell where one verb ends and the sentence
    // resumes. The mark is the console's house style for a thing you type.
    let named: Vec<String> =
        others.iter().take(ELSEWHERE_NAMED).map(|v| format!("`/{v}`")).collect();
    let hidden = others.len().saturating_sub(named.len());
    let tail = if hidden > 0 { format!(" +{hidden}") } else { String::new() };
    if typed.is_empty() {
        format!(
            "this list is `{}`'s own: it holds {held}. The console line's verbs run here too: \
             {}{tail}",
            ctx.region.as_word(),
            named.join(", ")
        )
    } else {
        // ⚠️ The number agrees, because a sentence that reads as though it were assembled is a
        // sentence people stop reading — and this one is the whole of what the pruning owes.
        let (belong, runs) = if named.len() == 1 && hidden == 0 {
            ("belongs", "it runs")
        } else {
            ("belong", "they run")
        };
        format!(
            "{}{tail} {belong} to the console line, not this `{}` region, but {runs} here anyway",
            named.join(", "),
            ctx.region.as_word()
        )
    }
}

/// What a finished region line turns out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Nothing to run: an empty box, a bare `/`, or a line that never reached a slash.
    Idle,
    /// A console-lane command, validated and ready for the same dispatch every other door uses.
    Run {
        /// The catalog name — `console.stack`, never the typed word.
        name: String,
        /// The dispatch arguments, **with anything the region supplies already in them**.
        args: Value,
    },
    /// Refused, carrying the sentence to show. The line is **not** cleared — `Registry::resolve`'s
    /// rule, and for its reason: a refusal a person can edit is recoverable, a swallow is not.
    Refused(String),
}

/// What this line does, given the whole table and this region's context.
///
/// 🚨 **Every console verb is accepted, and that is deliberate.** The pruning is
/// [`palette`]'s and is about *discovery*; refusing a verb here because it was not offered
/// would turn a region into a jail, and loosening a refusal later is far harder than tightening
/// an offer. `/theme dark` typed into a panel column runs.
///
/// ⚠️ **The view lane is the one refusal, and it names where the verb does work.** `/surface`,
/// `/media` and `/organon` put an element in a transcript; a region holding a column is not a
/// transcript, and there is no honest thing for them to do here. `/help` is view-lane too and
/// falls under the same sentence — which is right rather than merely convenient: this line's
/// own band *is* its help, and it names what it left out.
pub fn act(registry: &Registry, ctx: Context, line: &str) -> Act {
    if line.trim().is_empty() {
        return Act::Idle;
    }
    let expanded = expand(ctx, line);
    match registry.resolve(&expanded) {
        // A bare `/`, or a line with no slash at all. The registry calls both a message; there
        // is no agent here to send one to, so there is nothing to do and nothing to say.
        Resolved::Message => Act::Idle,
        Resolved::Escaped(_) => Act::Refused(
            "`//` escapes a line to the agent, and there is no agent in this region; type it \
             in a conversation instead"
                .to_string(),
        ),
        Resolved::Refused(message) => Act::Refused(message),
        Resolved::Run { lane: Lane::View, name, .. } => Act::Refused(format!(
            "`/{}` puts something in a conversation, and `{}` is not one; type it at an agent",
            verb_of(&name),
            ctx.region.as_word()
        )),
        Resolved::Run { lane: Lane::Console, name, mut args } => {
            if let Some((key, value)) = head(line)
                .and_then(|(word, _)| shed(ctx).into_iter().find(|s| s.typed == word))
                .and_then(|s| s.supplies)
            {
                if let Value::Object(map) = &mut args {
                    map.insert(key, Value::String(value));
                }
            }
            Act::Run { name, args }
        }
    }
}

/// The typed word of a catalog name — `registry::Entry::verb`'s rule, applied to a name this
/// module has in hand without the entry beside it.
fn verb_of(name: &str) -> &str {
    name.split_once('.').map(|(_, v)| v).unwrap_or(name)
}

// ---------------------------------------------------------------------------
// State: one line per region, and the single keyboard owner
// ---------------------------------------------------------------------------

/// One region's line, as a value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// What is in the box.
    pub text: String,
    /// Which candidate Tab would take.
    pub selected: usize,
    /// Escape was pressed and the band is shut until the line changes.
    pub dismissed: bool,
    /// What happened last — a refusal, or a receipt for a command that ran. Shown above the box.
    pub note: Option<String>,
    /// Ask egui for focus on the next frame this line draws.
    pub want_focus: bool,
}

/// Every region's line, plus **the console's single answer to "who owns the keyboard"**.
///
/// 🚨 **The owner is a measurement, not a setting.** See the module header. [`Lines::begin`] is
/// called once per frame before the region walk; [`draw`] records this frame's focus as it goes;
/// [`Lines::owner`] answers with the *previous* frame's record, which is the only answer
/// available to a consumer that draws before the line does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    lines: [Line; REGION_COUNT],
    /// Last frame's answer — what everything reads.
    owner: Option<Region>,
    /// This frame's, still being filled in.
    seen: Option<Region>,
}

impl Lines {
    /// Start a frame: last frame's observation becomes the answer, and a fresh one begins.
    ///
    /// ⚠️ **Called once per frame whether or not any region line draws**, which is what makes
    /// the owner fall back to the composer when the last line is taken away — a layout reset to
    /// `full agent` removes every line, nothing records focus, and the next frame's owner is
    /// `None`. A latch that only cleared on an explicit blur would leave the composer's keys
    /// held by a rectangle that no longer exists.
    pub fn begin(&mut self) {
        self.owner = self.seen.take();
    }

    /// Which region's line owned the keyboard as of the last completed frame.
    pub fn owner(&self) -> Option<Region> {
        self.owner
    }

    /// Whether the composer may read this frame's Tab, Escape and arrows — the one value
    /// `console_main` hands the conversation front-end.
    pub fn composer_owns_keys(&self) -> bool {
        self.owner.is_none()
    }

    pub fn line(&self, region: Region) -> &Line {
        &self.lines[region.slot()]
    }

    pub fn line_mut(&mut self, region: Region) -> &mut Line {
        &mut self.lines[region.slot()]
    }

    /// Say what happened, above the box that caused it.
    pub fn note(&mut self, region: Region, text: impl Into<String>) {
        self.line_mut(region).note = Some(text.into());
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// The prompt in front of the box. **ASCII on purpose** — `conversation_view`'s glyph allowlist
/// exists because a `✓` shipped once as an empty box, and a prompt is exactly the kind of small
/// mark nobody re-checks.
const PROMPT: &str = ">";

/// The hint an empty box carries. It names the one key that opens the band, because a box that
/// says nothing is indistinguishable from one that does nothing.
const HINT: &str = "/ for this region's commands";

/// The padding inside the band. `i8`, because that is what [`egui::Margin`] takes and a second
/// spelling in `f32` is a number to keep in step.
const PAD: i8 = 4;

/// How many rows the band reserves: the note, the palette row, the sentence, and the box.
///
/// ⚠️ **A fixed reservation rather than a measured one**, and the consequence is stated: a note
/// or a sentence long enough to wrap is clipped by the region's own clip rect rather than
/// growing the band. Measuring it would need the previous frame's content size — `composer_box`'s
/// arrangement — and a band that changes height under a person's hand while they are typing into
/// a rectangle that is *already* somebody's assigned content is a worse trade here than there.
pub const BAND_ROWS: usize = 4;

/// How tall the whole band is, in points.
pub fn band_height(row: f32) -> f32 {
    BAND_ROWS as f32 * row + 2.0 * PAD as f32 + 2.0
}

/// Draw this region's command line into the bottom of `ui`, and answer what Enter asked for.
///
/// 🚨 **The id pushes are why this is a function rather than three widgets at the call site**,
/// on `panel_stack::draw`'s rule: `("organon-region-line", region.as_word())` is pushed *here*,
/// so two regions' boxes cannot share egui state whatever `Ui` the caller supplies. A salt
/// applied in `console_main` would put the property in a crate that nothing here can test.
///
/// ⚠️ **Keys are consumed before the box runs**, because egui hands each widget a clone of the
/// event list: an event removed after the `TextEdit` has read it is an event acted on twice.
/// Only Tab and the arrows and Escape are taken, and only while this line owns the keyboard —
/// **Enter is deliberately left alone**, because a single-line `TextEdit` surrenders focus on it
/// and `lost_focus` is the reliable read. That is the opposite of the composer's arrangement and
/// for the opposite reason: the composer is multiline, so Enter never leaves it.
pub fn draw(
    ui: &mut egui::Ui,
    ctx: Context,
    registry: &Registry,
    lines: &mut Lines,
    theme: &Theme,
    form: &Form,
) -> Act {
    let region = ctx.region;
    let owns = lines.owner() == Some(region);
    let mut act = Act::Idle;
    ui.push_id(("organon-region-line", region.as_word()), |ui| {
        let mut framed = Frame::new()
            .fill(theme.composer_fill)
            .stroke(egui::Stroke::new(1.0_f32, theme.composer_edge))
            .corner_radius(form.card_corner())
            .inner_margin(egui::Margin::symmetric(6, PAD))
            .begin(ui);
        {
            let ui = &mut framed.content_ui;
            ui.set_width(ui.available_width());
            let line = lines.line(region).clone();
            let pal =
                if line.dismissed { None } else { palette(registry, ctx, &line.text) };

            if let Some(note) = &line.note {
                ui.label(RichText::new(note).monospace().color(theme.dim));
            }
            if let Some(p) = &pal {
                let columns = columns_in(ui);
                ui.label(
                    RichText::new(compact_join(&compact_words(p, line.selected), columns))
                        .monospace()
                        .color(theme.panel_title),
                );
                // 🚨 **The pruning's own sentence, drawn every time the band is.** It is a
                // field rather than an `Option` precisely so this cannot become conditional.
                ui.label(RichText::new(&p.elsewhere).monospace().color(theme.dim));
            }

            if owns {
                consume_keys(ui, lines, region, pal.as_ref());
            }
            let response = {
                let state = lines.line_mut(region);
                let edit = egui::TextEdit::singleline(&mut state.text)
                    .desired_width(f32::INFINITY)
                    .frame(false)
                    .margin(egui::Margin::ZERO)
                    .font(egui::TextStyle::Monospace)
                    .text_color(theme.human_text)
                    // `composer_box`'s rule and its reason: egui's focus manager reads Tab out
                    // of the raw input before any of this runs, so consuming it here is too
                    // late to stop focus leaving. This is the flag that pass tests.
                    .lock_focus(true)
                    .hint_text(format!("{PROMPT} {HINT}"));
                let response = ui.add(edit);
                if std::mem::take(&mut state.want_focus) {
                    response.request_focus();
                }
                // ⚠️ **An edit lets go of Escape**, and it is asked of egui's own `changed`
                // rather than of a shadow copy: the composer needs a shadow because its rule is
                // about the *direction* of the edit (`completion_held`), and this line has no
                // completion to hold off. What it needs is only "has this changed since the
                // band was shut", which is what `Response::changed` answers.
                if response.changed() {
                    state.dismissed = false;
                    state.selected = 0;
                    state.note = None;
                }
                response
            };
            // 🚨 **This frame's measurement of who has the keyboard** — read off the widget
            // rather than inferred from a click, so a focus egui moved for any reason at all
            // is the one that counts.
            if response.has_focus() {
                lines.seen = Some(region);
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let typed = lines.line(region).text.clone();
                act = self::act(registry, ctx, &typed);
                let state = lines.line_mut(region);
                // Enter surrendered focus; ask for it straight back, so a second command does
                // not need a second click.
                state.want_focus = true;
                match &act {
                    // ⚠️ **Cleared only on a run.** A refused line stays in the box to be
                    // edited — `Registry::resolve`'s rule, and the reason the composer does not
                    // clear either.
                    Act::Run { .. } => {
                        state.text.clear();
                        state.selected = 0;
                        state.note = None;
                    }
                    Act::Refused(message) => state.note = Some(message.clone()),
                    Act::Idle => {}
                }
            }
        }
        framed.frame.stroke = egui::Stroke::new(
            1.0_f32,
            if owns { theme.composer_edge_focus } else { theme.composer_edge },
        );
        framed.end(ui);
    });
    act
}

/// How many monospace cells fit across this `Ui`. `compact_fit` measures in **columns**, which
/// is exact because the row is drawn entirely in the mono face.
fn columns_in(ui: &egui::Ui) -> usize {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    // `fonts_mut`, not `fonts` — measuring a glyph may have to rasterise it, which is
    // `compact_band`'s own call and the reason it is spelled that way there too.
    let cell = ui.ctx().fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    (ui.available_width() / cell).floor().max(0.0) as usize
}

/// The compact row's words for a region line — [`crate::conversation_view::compact_words`]'
/// arrangement over this palette instead of the composer's.
///
/// ⚠️ **A second producer of words, never a second fitting rule.** The measuring and joining is
/// `compact_fit`/`compact_join`, shared, so the two rows cannot come to disagree about what a
/// hidden count looks like.
fn compact_words(palette: &RegionPalette, selected: usize) -> Vec<CompactWord> {
    let mut words = Vec::new();
    if palette.runnable {
        words.push(CompactWord { text: "Enter runs".to_string(), here: false, runs: true });
    }
    if palette.candidates.is_empty() {
        words.extend(
            palette.hint.clone().map(|hint| CompactWord { text: hint, here: false, runs: false }),
        );
        return words;
    }
    words.extend(palette.candidates.iter().enumerate().map(|(index, candidate)| {
        let here = index == selected;
        let text =
            if here { format!("[{}]", candidate.label) } else { candidate.label.clone() };
        CompactWord { text, here, runs: false }
    }));
    words
}

/// Take this frame's Tab, arrows and Escape on this line's behalf — and only while it owns the
/// keyboard.
fn consume_keys(
    ui: &egui::Ui,
    lines: &mut Lines,
    region: Region,
    palette: Option<&RegionPalette>,
) {
    let open = palette.is_some();
    let keys: Vec<(egui::Key, egui::Modifiers)> = ui.input_mut(|i| {
        let mut taken = Vec::new();
        i.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else {
                return true;
            };
            let ours = match key {
                egui::Key::Tab | egui::Key::Escape => open,
                egui::Key::ArrowUp | egui::Key::ArrowDown => open,
                _ => false,
            };
            if ours {
                taken.push((*key, *modifiers));
            }
            !ours
        });
        taken
    });
    let count = palette.map_or(0, |p| p.candidates.len());
    for (key, modifiers) in keys {
        let state = lines.line_mut(region);
        match key {
            egui::Key::Escape => {
                state.dismissed = true;
                state.want_focus = true;
            }
            egui::Key::ArrowDown => state.selected = step(state.selected, count, true),
            egui::Key::ArrowUp => state.selected = step(state.selected, count, false),
            egui::Key::Tab if modifiers.shift => state.selected = step(state.selected, count, false),
            egui::Key::Tab => {
                if let Some(candidate) = palette.and_then(|p| p.candidates.get(state.selected)) {
                    state.text = candidate.completion.clone();
                    state.selected = 0;
                    state.want_focus = true;
                }
            }
            _ => {}
        }
    }
}

/// Move a highlight, wrapping. `conversation_view::move_selection`'s rule: a ring of a handful
/// of words has no end worth feeling.
fn step(selected: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (selected + 1) % len
    } else {
        (selected + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{ArgKind, ArgSpec, CommandSpec, Reversal, TargetKind};
    use crate::region::REGION_WORDS;

    /// The real stack and viewport specs, in the shape `console_main::console_specs` declares
    /// them — this crate cannot see that function, so the two are bound by
    /// `the_region_line_expands_onto_the_real_console_specs` in `console_main.rs`.
    fn console() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "console.viewport".into(),
                doc: "How the pane is divided".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec {
                        name: "region".into(),
                        kind: ArgKind::Choice(
                            REGION_WORDS.iter().map(|s| (*s).to_string()).collect(),
                        ),
                        required: true,
                    },
                    ArgSpec {
                        name: "content".into(),
                        kind: ArgKind::Choice(
                            CONTENT_WORDS.iter().map(|s| (*s).to_string()).collect(),
                        ),
                        required: true,
                    },
                ],
                reversal: Reversal::Recoverable,
            },
            CommandSpec {
                name: "console.stack".into(),
                doc: "What is in a panel region".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec {
                        name: "action".into(),
                        kind: ArgKind::Choice(
                            crate::panel_stack::STACK_ACTIONS
                                .iter()
                                .map(|s| (*s).to_string())
                                .collect(),
                        ),
                        required: true,
                    },
                    ArgSpec {
                        name: "panel".into(),
                        kind: ArgKind::Choice(
                            crate::panel_stack::panel_words()
                                .into_iter()
                                .map(str::to_string)
                                .collect(),
                        ),
                        required: true,
                    },
                    ArgSpec {
                        name: crate::panel_stack::REGION_ARG.into(),
                        kind: ArgKind::Choice(
                            REGION_WORDS.iter().map(|s| (*s).to_string()).collect(),
                        ),
                        required: false,
                    },
                ],
                reversal: Reversal::Permanent,
            },
            CommandSpec {
                name: "console.theme".into(),
                doc: "The palette".into(),
                target: TargetKind::Viewport,
                args: vec![ArgSpec {
                    name: "name".into(),
                    kind: ArgKind::Choice(vec!["dark".into(), "chocolate".into()]),
                    required: true,
                }],
                reversal: Reversal::Recoverable,
            },
        ]
    }

    fn registry() -> Registry {
        Registry::new(&console())
    }

    fn panel_ctx() -> Context {
        Context { region: Region::Left, content: Some(Content::Panel) }
    }

    fn empty_ctx() -> Context {
        Context { region: Region::TopRight, content: None }
    }

    /// 🚨 **The tier's own sentence, as a test.** `/add surface` typed in a panel region does
    /// what `console stack add surface` does — and it names the region it was typed in, which
    /// is the whole of "the region becomes context".
    #[test]
    fn add_in_a_panel_region_is_stack_add_in_that_region() {
        let act = act(&registry(), panel_ctx(), "/add surface");
        let Act::Run { name, args } = act else { panic!("{act:?}") };
        assert_eq!(name, "console.stack");
        assert_eq!(args["action"], "add");
        assert_eq!(args["panel"], "surface");
        assert_eq!(
            args[crate::panel_stack::REGION_ARG], "left",
            "the region a person typed into is not in the arguments"
        );
    }

    /// The other half of the shed vocabulary: a content word assigns the region it was typed in.
    #[test]
    fn a_content_word_assigns_the_region_it_was_typed_in() {
        for (word, region) in
            [("panel", Region::TopRight), ("agent", Region::TopRight), ("off", Region::TopRight)]
        {
            let ctx = Context { region, content: None };
            let act = act(&registry(), ctx, &format!("/{word}"));
            let Act::Run { name, args } = act else { panic!("{word}: {act:?}") };
            assert_eq!(name, "console.viewport");
            assert_eq!(args["region"], region.as_word());
            assert_eq!(args["content"], word);
        }
    }

    /// 🚨 **Capability is NOT pruned.** A console verb the region does not offer still runs —
    /// see the module header on why refusing it would turn a region into a jail.
    #[test]
    fn a_verb_this_region_does_not_offer_still_runs() {
        let act = act(&registry(), panel_ctx(), "/theme dark");
        let Act::Run { name, args } = act else { panic!("{act:?}") };
        assert_eq!(name, "console.theme");
        assert_eq!(args["name"], "dark");
        assert!(
            !palette(&registry(), panel_ctx(), "/")
                .expect("a bare slash opens the band")
                .candidates
                .iter()
                .any(|c| c.label == "theme"),
            "`theme` was offered — discovery is supposed to be the pruned half"
        );
    }

    /// ⚠️ **…and the pruned list says so, in the sentence the issue asks for.**
    #[test]
    fn the_pruned_list_names_what_it_left_out() {
        let reg = registry();
        let bare = palette(&reg, panel_ctx(), "/").expect("a bare slash opens the band");
        assert!(bare.elsewhere.contains("/theme"), "{}", bare.elsewhere);
        assert!(bare.elsewhere.contains("run here too"), "{}", bare.elsewhere);

        let narrowed = palette(&reg, panel_ctx(), "/th").expect("still a command line");
        assert!(narrowed.candidates.is_empty(), "no shed verb begins `th`");
        assert!(
            narrowed.elsewhere.contains("`/theme` belongs to the console line")
                && narrowed.elsewhere.contains("here anyway"),
            "{}",
            narrowed.elsewhere
        );
    }

    /// 🚨 **The sentence is not optional and the type says so.** Every reachable palette carries
    /// one — this walks the states a person passes through rather than asserting the field is a
    /// `String`, which the compiler already knows.
    #[test]
    fn every_palette_carries_its_sentence() {
        let reg = registry();
        for ctx in [panel_ctx(), empty_ctx()] {
            for line in ["/", "/a", "/add", "/add ", "/add surf", "/theme ", "/nonesuch"] {
                let Some(p) = palette(&reg, ctx, line) else {
                    panic!("{line} in {:?} opened no band", ctx.region.as_word())
                };
                assert!(
                    p.elsewhere.len() > 30,
                    "{line} in {}: a sentence that short names nothing: {}",
                    ctx.region.as_word(),
                    p.elsewhere
                );
            }
        }
    }

    /// An empty region's line offers the four content words and nothing else — the free
    /// consequence: a rectangle that holds nothing is the surface that assigns it.
    #[test]
    fn an_empty_region_offers_the_content_words() {
        let p = palette(&registry(), empty_ctx(), "/").expect("band");
        let labels: Vec<&str> = p.candidates.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, CONTENT_WORDS, "the ring is `region::CONTENT_WORDS`, in its order");
        assert!(
            !labels.contains(&"add"),
            "a region with no column offered a verb that edits one"
        );
    }

    /// …and a panel region's line offers those **plus** the stack actions.
    #[test]
    fn a_panel_region_offers_the_stack_actions_too() {
        let p = palette(&registry(), panel_ctx(), "/").expect("band");
        let labels: Vec<&str> = p.candidates.iter().map(|c| c.label.as_str()).collect();
        for word in CONTENT_WORDS {
            assert!(labels.contains(word), "{word} missing from {labels:?}");
        }
        for action in crate::panel_stack::STACK_ACTIONS {
            assert!(labels.contains(action), "{action} missing from {labels:?}");
        }
    }

    /// 🚨 **A completion is a line a person can look at, not the expanded one.** This is the
    /// property `unexpand` exists for, and getting it wrong would put `/stack add surface` in a
    /// box whose vocabulary has no `stack` in it.
    #[test]
    fn completions_come_back_un_expanded() {
        let reg = registry();
        let p = palette(&reg, panel_ctx(), "/add su").expect("band");
        let completions: Vec<&str> = p.candidates.iter().map(|c| c.completion.as_str()).collect();
        // ⚠️ The trailing space is the registry's own answer — `stack` still has one optional
        // keyword left, so `value_candidates` says the line is not finished. That the keyword is
        // one this line never offers is a separate fact, carried in `elsewhere` below.
        assert_eq!(completions, vec!["/add surface "], "{completions:?}");
        // …and taking it produces a line that runs, which is the loop a renderer implements.
        assert!(matches!(act(&reg, panel_ctx(), "/add surface "), Act::Run { .. }));
    }

    /// 🚨 **The word the region supplies is never offered, and the band says why.** Offering
    /// `region` inside a line whose whole premise is that the region is context would invite a
    /// second, contradicting one — and dropping it silently is the defect class this surface
    /// exists not to repeat.
    #[test]
    fn the_supplied_region_keyword_is_dropped_and_named() {
        let reg = registry();
        // The registry itself does offer it — this is what is being pruned, not a thing that
        // never existed. Asked of the expanded line, which is what the region line delegates to.
        let inner = reg.candidates("/stack add surface ").expect("the registry's own ring");
        assert!(
            inner.candidates.iter().any(|c| c.label == crate::panel_stack::REGION_ARG),
            "the fixture does not offer the keyword, so this test proves nothing"
        );

        let p = palette(&reg, panel_ctx(), "/add surface ").expect("band");
        assert!(
            !p.candidates.iter().any(|c| c.label == crate::panel_stack::REGION_ARG),
            "the region line offered the word it supplies: {:?}",
            p.candidates.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
        assert!(p.elsewhere.contains(crate::panel_stack::REGION_ARG), "{}", p.elsewhere);
        assert!(p.elsewhere.contains("left"), "{}", p.elsewhere);
    }

    /// A shed verb that still wants an argument opens its ring when it is taken;
    /// one that is already a whole command does not gain a space it will never fill.
    #[test]
    fn a_shed_verb_gains_a_trailing_space_only_if_it_wants_one() {
        let reg = registry();
        let p = palette(&reg, panel_ctx(), "/").expect("band");
        let by = |label: &str| {
            p.candidates.iter().find(|c| c.label == label).expect(label).completion.clone()
        };
        assert_eq!(by("add"), "/add ", "`add` still needs a panel");
        assert_eq!(by("panel"), "/panel", "`panel` is already the whole command");
    }

    /// ⚠️ **The view lane is refused by name**, and the refusal says where the verb works.
    #[test]
    fn a_view_lane_verb_is_refused_by_name() {
        let act = act(&registry(), panel_ctx(), "/help");
        let Act::Refused(message) = act else { panic!("{act:?}") };
        assert!(message.contains("/help"), "{message}");
        assert!(message.contains("agent"), "the refusal does not say where it works: {message}");
    }

    /// A refusal from the registry itself travels through unchanged — one gate, one sentence.
    #[test]
    fn an_unknown_verb_keeps_the_registrys_own_refusal() {
        let act = act(&registry(), panel_ctx(), "/nonesuch");
        let Act::Refused(message) = act else { panic!("{act:?}") };
        assert!(message.contains("nonesuch"), "{message}");
    }

    /// An empty box and a bare slash both do nothing, and neither says anything — there is
    /// nothing to report about a command nobody finished typing.
    #[test]
    fn an_empty_line_and_a_bare_slash_are_idle() {
        let reg = registry();
        assert_eq!(act(&reg, panel_ctx(), ""), Act::Idle);
        assert_eq!(act(&reg, panel_ctx(), "   "), Act::Idle);
        assert_eq!(act(&reg, panel_ctx(), "/"), Act::Idle);
    }

    /// 🚨 **The keyboard owner defaults to the composer, and that is invariant #4.** A console
    /// that has had no `/viewport` typed draws no region line, so nothing ever records focus and
    /// the composer keeps every key it had before this module existed.
    #[test]
    fn the_composer_owns_the_keyboard_until_a_region_line_takes_it() {
        let mut lines = Lines::default();
        assert!(lines.composer_owns_keys());
        lines.begin();
        assert!(lines.composer_owns_keys(), "a frame in which nothing drew moved the owner");

        // A frame in which a line had focus.
        lines.seen = Some(Region::Left);
        lines.begin();
        assert_eq!(lines.owner(), Some(Region::Left));
        assert!(!lines.composer_owns_keys());

        // …and a frame in which nothing did hands it straight back. See `Lines::begin` on why
        // this must not need an explicit blur.
        lines.begin();
        assert!(lines.composer_owns_keys(), "the owner outlived the line that held it");
    }

    /// Each region's line is its own — the text in one is not the text in another.
    #[test]
    fn a_line_belongs_to_its_region() {
        let mut lines = Lines::default();
        lines.line_mut(Region::Left).text = "/add surface".into();
        assert_eq!(lines.line(Region::Left).text, "/add surface");
        assert!(lines.line(Region::Right).text.is_empty());
    }

    /// The highlight wraps in both directions and an empty ring cannot move it off zero.
    #[test]
    fn the_highlight_wraps_and_an_empty_ring_stays_put() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(0, 0, true), 0);
        assert_eq!(step(0, 0, false), 0);
    }

    /// `expand` is a head swap and nothing else — everything after the verb survives byte for
    /// byte, which is what makes `unexpand` total.
    #[test]
    fn expansion_replaces_the_first_word_and_nothing_else() {
        let ctx = panel_ctx();
        assert_eq!(expand(ctx, "/add surface"), "/stack add surface");
        assert_eq!(expand(ctx, "/add  surface  "), "/stack add  surface  ");
        assert_eq!(expand(ctx, "/panel"), "/viewport left panel");
        // Not a shed word: untouched, so the registry answers exactly as it would in a composer.
        assert_eq!(expand(ctx, "/theme dark"), "/theme dark");
        // Not a command line at all.
        assert_eq!(expand(ctx, "hello"), "hello");
        assert_eq!(expand(ctx, "//add"), "//add");
    }

    /// 🚨 **`fires` is false everywhere, and it is not copied from the entry.** This line has no
    /// autorun; a candidate claiming otherwise would invite a renderer to run a command on a
    /// keystroke that never earned one.
    #[test]
    fn no_region_line_candidate_ever_fires() {
        let reg = registry();
        for line in ["/", "/a", "/add ", "/p"] {
            for c in palette(&reg, panel_ctx(), line).expect(line).candidates {
                assert!(!c.fires, "{line}: `{}` claims it may run unasked", c.label);
            }
        }
    }

    /// 🚨 **Every string this module COMPOSES is ASCII**, which is `conversation_view`'s glyph
    /// allowlist reaching a surface that allowlist cannot see. That guard walks an enumerated
    /// list of draw sites in *that* file; these are draw sites in this one, and the defect it
    /// exists for shipped once already — a `✓` in none of egui's four bundled fonts, drawn as an
    /// empty box in the pane log and photographed on a running console.
    ///
    /// ⚠️ **"Composes" and not "draws", and the difference is a real boundary rather than a
    /// hole cut to make the test pass.** A registry refusal — *"`/nonesuch` is not a command"* —
    /// is the **console line's own words**, already drawn by the composer on every build that
    /// has ever shipped; this module passes it through byte-for-byte and adds nothing. Asserting
    /// a glyph policy over it would put a second copy of that policy here, and the copy would
    /// win arguments it has no standing in: the first thing it would do is refuse a string the
    /// composer draws happily two rectangles away. So the pass-through is pinned as
    /// *unmodified* instead, which is the property this module is actually responsible for, and
    /// the glyph question stays where the string is written.
    #[test]
    fn no_string_this_line_composes_needs_a_glyph_egui_lacks() {
        let reg = registry();
        let mut drawn: Vec<String> = vec![PROMPT.to_string(), HINT.to_string()];
        for ctx in [panel_ctx(), empty_ctx()] {
            for s in shed(ctx) {
                drawn.push(s.typed);
                drawn.push(s.doc);
            }
            for line in ["/", "/a", "/add ", "/add surface ", "/th", "/theme "] {
                let Some(p) = palette(&reg, ctx, line) else { continue };
                drawn.push(p.elsewhere);
                drawn.extend(p.hint);
                for c in p.candidates {
                    drawn.push(c.label);
                    drawn.push(c.doc);
                }
            }
            // `/help` is the view-lane refusal and `//x` the escape refusal — both written
            // here, both this module's to answer for. `/nonesuch` is deliberately absent; it
            // is the registry's, and it is checked below instead.
            for line in ["/help", "//x"] {
                if let Act::Refused(message) = act(&reg, ctx, line) {
                    drawn.push(message);
                }
            }
        }
        for text in drawn {
            assert!(
                text.is_ascii(),
                "a region line composes a non-ASCII glyph, which egui's bundled fonts may not \
                 have: {text:?}"
            );
        }
    }

    /// 🚨 **A registry refusal reaches the region line unmodified** — the other half of the
    /// glyph rule above, and the more important half of the two.
    ///
    /// ⚠️ The tempting shape for a per-region line is to re-word the console's refusals so they
    /// read as though the region wrote them. That is how one vocabulary becomes two: the same
    /// mistyped verb would be refused in different words depending on which rectangle it was
    /// typed in, and a person comparing the two would be right to conclude they are different
    /// commands. So the message is carried, never rephrased — the region contributes the
    /// *context*, and §1.8's registry keeps the *words*.
    #[test]
    fn a_registry_refusal_is_carried_not_rephrased() {
        let reg = registry();
        let Resolved::Refused(from_registry) = reg.resolve(&expand(panel_ctx(), "/nonesuch"))
        else {
            panic!("the registry refuses an unknown verb")
        };
        let Act::Refused(from_line) = act(&reg, panel_ctx(), "/nonesuch") else {
            panic!("and the region line refuses it too")
        };
        assert_eq!(
            from_line, from_registry,
            "the region line rewrote the console's refusal; one verb must be refused in one \
             set of words whichever rectangle it was typed in"
        );
    }

    /// A line that is not a command line opens no band — the same `None` the registry answers,
    /// so nothing is drawn over an empty box.
    #[test]
    fn a_line_that_is_not_a_command_line_opens_no_band() {
        let reg = registry();
        assert!(palette(&reg, panel_ctx(), "").is_none());
        assert!(palette(&reg, panel_ctx(), "hello").is_none());
        assert!(palette(&reg, panel_ctx(), "//add").is_none());
    }
}
