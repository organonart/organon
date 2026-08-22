//! **A panel column's own control — add a panel, remove a panel. Nothing else.**
//!
//! A region holding `panel` gets a one-line control along its bottom edge. It takes the two
//! words in [`crate::panel_stack::STACK_ACTIONS`] — `add` and `remove` — each followed by a
//! panel name out of [`crate::panel_stack::panel_words`], with
//! [`crate::panel_stack::ALL_WORD`] emptying the column. A region holding `agent` or `3d`, and
//! a region holding nothing at all, gets **no line**.
//!
//! # 🚨 This REVERSES the rule this module shipped with, and the reversal is the point
//!
//! #98 Tier C shipped this as *"the fifth front door onto one table"*: a command line in **every**
//! region, dispatching the **whole** console registry, under the rule **prune discovery, never
//! capability** — the band listed a region's own verbs and [`act`] accepted every verb in the
//! catalog, so `/theme dark` typed into a panel column ran. **It no longer does, by design.**
//!
//! James rejected the scope on a running console: *"I don't want every region to have commands
//! like this. At least that wasn't my original intention. … What I envisioned is not that I want
//! to be able to have slash commands to set each region to be what it could be. I only
//! particularly wanted to be able to add and remove panels from a panel section."*
//!
//! ⚠️ **The old rule is not wrong; it is about a different object, and that is the whole
//! reconciliation.** "Prune discovery, never capability" guards a **general command surface**
//! against becoming a jail: a surface that offers ten verbs and secretly refuses the eleventh has
//! taught the person using it that its list is a lie, and loosening a refusal later is far harder
//! than widening an offer. This is no longer a general command surface. It is a **dedicated
//! control for one job** — the way a scrollbar is not a crippled command line and a volume knob is
//! not a pruned synthesiser. A control that does one thing is not a pruned version of a thing that
//! does everything, so the rule does not reach it: there is no hidden capability to discover here,
//! and the list **is** the vocabulary.
//!
//! ⚠️ **What was given up, said plainly rather than left to be discovered.** `/theme dark`,
//! `/posture`, `/background` and every other console verb no longer run in a region; assigning a
//! region is `/viewport` on the console line or `organon console viewport …` from a terminal,
//! which is what it was before Tier C. An unknown first word is refused **by name** and says so.
//! Nothing was made unreachable — only un-typeable *here*.
//!
//! # 🚨 The defect this narrowing was also written to fix: egui gave the box a NEW ID
//!
//! The shipped Tier C line rendered its palette and then would not accept another keystroke —
//! James: *"When I type slash in one of the regions, I just get a list of choices and some text
//! below it, and I can't type or select anything."*
//!
//! **The cause was `egui::TextEdit`'s id, and it had nothing to do with key consumption.** The
//! box was added with no explicit id, so egui derived one from `Ui::next_auto_id`, which counts
//! the widgets allocated before it. The palette's rows are drawn *above* the box — so the instant
//! a `/` opened the band, two more labels existed ahead of it, the box's id changed, and egui
//! saw a **different widget**: focus stayed with an id nothing was drawing any more, and every
//! later character went nowhere. The first keystroke landing and none of the rest is exactly the
//! signature of an id that moves when the content above it does.
//!
//! The fix is [`BOX_ID`] — an explicit salt, so the box is the same widget whatever is drawn
//! around it — and it is pinned by [`tests::a_note_appearing_does_not_take_the_box_with_it`],
//! which drives real `egui::Event::Text` through a real frame. ⚠️ **That test's doc carries the
//! measured mutation table, and it corrects an easy assumption**: on today's code, deleting the
//! salt does *not* fail the suite, because nothing is drawn in the band above the box at all any
//! more and the widget count therefore never varies. The salt is kept so the property holds **by
//! construction** rather than by that coincidence — and the coincidence is now one edit thinner
//! than it was, since the only way to reintroduce the defect is to put a widget back in the band.
//!
//! ⚠️ **A second, independent defect was measured on the way**: in a 320 pt column the old
//! band's `elsewhere` sentence wrapped to several rows and pushed the box **35.6 pt past the
//! band's own clip rect** — invisible and unclickable even with a stable id. Two faults, one
//! symptom; do not stop at the first.
//!
//! # 🚨 ONE line, and where the other three rows went
//!
//! ✏️ **This section replaces "the row order is a safety property".** That section said:
//! *candidate row, then the box, then the note* — the note last because it is the only row whose
//! height is unbounded, so an overflow costs the tail of an explanation and never the input.
//! The reasoning was right and it no longer applies, because **there are no other rows**.
//!
//! James, 2026-08-20: *"completely remove the status lines in the add remove inputs on the
//! panels. It should just be a single line. If we need to pop something up over top of it, we
//! can do that, but it's way too messy. We have to clean all this up."*
//!
//! So the band is the box alone ([`BAND_ROWS`] = 1), and the candidate row and the refusal are
//! [`popover`] — a floating [`egui::Area`] anchored above the box. **The clip guarantee did not
//! survive by inheritance; it was re-established.** An `Area` allocates nothing in the band's
//! `Ui`, so it cannot displace the box at any height, and the band itself now holds exactly one
//! single-line `TextEdit`, which does not wrap. That makes the property structural where it used
//! to be ordinal — and structural *only while nothing else is put in the band*. [`BAND_ROWS`]
//! carries the rule for anyone adding a row; a test drives a real refusal at 320 pt and asserts
//! the box is still inside the clip rect.
//!
//! # 🚨 The hard part is still focus
//!
//! The console had exactly one composer, and §1.9's whole command panel is bound to it —
//! including `composer_keys`, which consumes Tab and the arrows out of the **raw event list**,
//! not out of a focused widget. Two of those are taken unconditionally on an empty box
//! (`arrow_owner` hands Up to the history when the composer is empty), so a second input would
//! have found its own Up already gone before it ran.
//!
//! **The owner is measured, never asserted.** [`Lines::owner`] is the region whose line had egui
//! focus *last frame* — recorded by [`draw`] from the `TextEdit`'s own
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
//! `draw_regions` already uses for the `3d` region's rectangle. The cost is that the frame on
//! which focus *moves* is arbitrated by the old owner, and that frame carries no keystroke:
//! focus moves by a **click**, and a click is not a key.
//!
//! ⚠️ **A default console has no region lines at all**, so none of this is reachable until a
//! region has been given a `panel`. `Lines::default()`'s owner is `None`, `composer_owns_keys`
//! is `true`, and the composer behaves exactly as it did before this module existed.
//!
//! ⚠️ **Escape is deliberately NOT consumed.** Tier C took it to shut the band; there is no band
//! to shut now — the two words are this control's own label and are always shown. Leaving Escape
//! alone means egui's `TextEdit` does what it does everywhere else with it: surrender focus. The
//! next frame's owner is `None` and the composer has its keys back, which is a way *out* of the
//! control that costs nothing to build and nothing to explain.
//!
//! # 🚨 Input parity with the composer — #131, and what it turned out to be
//!
//! James, on a running console: *"currently these do not auto-complete terms when we have
//! specified them with a few characters."*
//!
//! ⚠️ **The narrowing was never missing, and neither was Tab.** Driven through real frames on
//! the shipped code, `add su` narrowed to the single candidate `surface` and Tab took it. What
//! was missing was the composer's **self-completion** — §1.9's *"do not show me the single
//! choice, simply complete it because it is the only option"* — so a control whose whole
//! vocabulary is two verbs and a panel name still asked for a Tab it could not need. That is the
//! feature this section used to decline, and the paragraph declining it is the one it replaces.
//!
//! 🚨 **And a second defect fell out of the same probe, live and unnoticed: the caret did not
//! move.** Tab on `add su` produced `add surface ` with egui's cursor still at index 6, so the
//! next two characters landed as `add suXYrface `. That is the composer's `/hxelp` bug exactly,
//! on the surface nobody had driven — and it is why Tab *appearing* to work is not evidence that
//! completing works. Every path that rewrites the line wholesale now sets [`Line::want_caret`],
//! drained once at the end of [`draw`] through the composer's own `put_caret_at_end`.
//!
//! ⚠️ **The backspace latch is SHARED, not reproduced.** The reason this section gave for
//! declining self-completion was sound — `completion_held` is what stops a completion undoing
//! the deletion that provoked it (*"once I have typed slash surface, I am no longer able to
//! backspace out of it"*), and a second copy of that rule is a second answer to one question.
//! So `conversation_view::completion_held` is `pub(crate)` and this module calls it, over a
//! per-line [`Line::seen`] shadow that is `composer_seen`'s counterpart. One function, one
//! measurement, two surfaces.
//!
//! **Tab still accepts and Enter still runs**, which is the pair §1.9 says must never be the
//! same key. Self-completion changes *when* a line is finished, never *what* Enter does with it.
//!
//! # ⚠️ What this line still deliberately does NOT do
//!
//! **No autorun**, and here the classification decides it rather than a preference:
//! `console.stack` is declared [`crate::command::Reversal::Permanent`] — `remove all` discards a
//! column no single command rebuilds — so [`Candidate::fires`] is `false` for every candidate
//! this control can produce and autorun could not fire it even if it were wired. Saying so is
//! cheaper than a switch nothing can flip.
//!
//! **No history**, and this is a decision rather than an omission. Three reasons, in the order
//! they bind:
//!
//! 1. **The recall case is already covered by not clearing.** The composer's buffer earns its
//!    place mostly on refusals — a line with a typo you want back in front of you. A refused
//!    line *stays in this box* ([`Act::Refused`]), so the thing history is for has already
//!    happened by the time you would reach for it.
//! 2. **Retyping costs two keystrokes now.** `a` self-completes to `add `, `s` to `add surface `.
//!    A recall surface has to beat that, and it cannot.
//! 3. **`arrow_owner`'s rule would take Up off the ring in the one state that needs it most.**
//!    The composer hands Up to the history when the box is *empty*; an empty box here is the
//!    resting state of the control, where the ring **is** its label. Adopting the rule would
//!    make the arrows stop working on a resting column, which is a worse control than one with
//!    no history.
//!
//! **No Escape.** Unchanged, and argued above: egui's own defocus is the way out.

use egui::{Frame, RichText};
use serde_json::Value;

use crate::conversation_view::{compact_join, CompactWord};
use crate::panel_stack::{ALL_WORD, REGION_ARG, STACK_ACTIONS};
use crate::posture::Form;
use crate::region::{Region, REGION_COUNT};
use crate::registry::{Candidate, CandidateKind, Lane, Registry, Resolved};
use crate::theme::Theme;

/// Where a line is being typed. **Only the region** — a line exists at all only for a region
/// holding a `panel`, so there is no content to carry and no `None` case to answer for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub region: Region,
}

/// The catalog verb every line here resolves onto. `console_main` knows it as `CMD_STACK`; this
/// module never spells the name itself — it reaches it by expanding onto the registry, which is
/// what keeps the control and the CLI one vocabulary rather than two.
const STACK_VERB: &str = "stack";

/// What one action does to **this** column. A catch-all rather than a second literal, so a third
/// word added to [`STACK_ACTIONS`] without a case here gets a plain line rather than a panic in
/// the frame path — `StackCmd::resolve`'s own arrangement.
fn action_doc(action: &str) -> String {
    match action {
        "add" => "put one of Organon's editor panels at the bottom of this column".to_string(),
        "remove" => format!("take the last copy out of this column, or `{ALL_WORD}` to empty it"),
        other => format!("`{other}` this column"),
    }
}

/// The verb word and everything after it, from a line with an **optional** leading `/`.
///
/// ⚠️ **The slash is tolerated, not required, and that is a decision about hands rather than
/// grammar.** This is a control, so `add surface` is the natural thing to type; but Tier C
/// shipped a slash line and taught one, so refusing `/add surface` would be refusing the spelling
/// the only person using it already has in his fingers. One grammar, one optional ornament.
///
/// `None` when there is no word yet — an empty box, whitespace, or a bare `/`.
fn head(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('/').unwrap_or(trimmed);
    let word: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
    if word.is_empty() {
        return None;
    }
    Some((word.clone(), rest[word.len()..].to_string()))
}

/// Everything before the verb word — the leading whitespace and the optional slash. What a
/// completion has to be rebuilt on top of, so accepting one never silently adds or removes a
/// character a person typed.
fn stem(line: &str) -> String {
    let lead = &line[..line.len() - line.trim_start().len()];
    let slash = if line.trim_start().starts_with('/') { "/" } else { "" };
    format!("{lead}{slash}")
}

/// The line as [`Registry::resolve`] and [`Registry::candidates`] hear it: `add surface` becomes
/// `/stack add surface`.
///
/// Returns `None` when the first word is not one of [`STACK_ACTIONS`] — there is nothing to
/// expand onto, and [`act`] refuses it by name rather than handing an unrelated verb to the
/// registry.
///
/// ⚠️ **A head swap and nothing else**: everything a person typed after the verb survives byte
/// for byte, which is what makes [`unexpand`] total.
fn expand(line: &str) -> Option<String> {
    let (word, rest) = head(line)?;
    if !STACK_ACTIONS.contains(&word.as_str()) {
        return None;
    }
    Some(format!("/{STACK_VERB} {word}{rest}"))
}

/// Turn a completion built against the **expanded** line back into one for the line a person is
/// looking at.
///
/// 🚨 **A pure head swap, and that is why it is total.** [`expand`] replaces the head and leaves
/// every later character alone, so a completion produced from the expanded line agrees with the
/// typed line from the end of the expanded head onwards. Rebuilding it any other way would mean
/// re-deriving the stem, which is the one thing `Candidate::completion` exists so nobody has to.
fn unexpand(typed_head: &str, expanded_head: &str, completion: &str) -> Option<String> {
    let tail = completion.strip_prefix(expanded_head)?;
    Some(format!("{typed_head}{tail}"))
}

/// Case-insensitive prefix, [`crate::registry`]'s own matching rule. Prefix rather than fuzzy for
/// its reason: "press another key and it narrows" has to be literally true.
fn narrows(word: &str, typed: &str) -> bool {
    word.to_ascii_lowercase().starts_with(&typed.to_ascii_lowercase())
}

/// What this control could take next.
///
/// 🚨 **Never optional and never empty of meaning.** Tier C's palette answered `None` until a
/// slash was typed, which left an unlabelled box. A control that does exactly two things should
/// say which two the moment you look at it, so the row is drawn on an empty line as well.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionPalette {
    /// The words that narrow, in [`STACK_ACTIONS`] order for a verb and in the registry's own
    /// ring order for a panel name.
    pub candidates: Vec<Candidate>,
    /// The line **as it stands** is already a complete command, so Enter would run it.
    pub runnable: bool,
    /// What to say when nothing narrows — a mistyped word has to be told so *before* Enter, not
    /// only after it.
    pub hint: Option<String>,
}

impl RegionPalette {
    /// The one candidate left, when there is exactly one and taking it would change the line.
    ///
    /// 🚨 **[`crate::registry::Palette::sole_completion`]'s rule, restated over this palette
    /// rather than shared with it.** The two structs are different types — this one carries a
    /// `hint` the registry's carries as `empty_ring`, and its candidates have already been
    /// un-expanded — so there is nothing to reuse but three lines, and the *rule* is what has to
    /// match: one candidate, and it must not complete to the line it was derived from.
    ///
    /// ⚠️ **The second term is the termination rule and it is not defensive tidiness.** `add
    /// surface ` narrows to nothing further, but a line whose sole completion is itself would be
    /// rewritten to itself on every frame for ever — see the registry's own
    /// `a_lone_candidate_completes_itself_without_ever_running`.
    pub fn sole_completion(&self, line: &str) -> Option<&Candidate> {
        match &self.candidates[..] {
            [only] if only.completion != line => Some(only),
            _ => None,
        }
    }
}

/// What this control offers, given the line as it stands.
pub fn palette(registry: &Registry, ctx: Context, line: &str) -> RegionPalette {
    let runnable = matches!(act(registry, ctx, line), Act::Run { .. });
    let stem = stem(line);
    let Some((word, tail)) = head(line) else {
        // Nothing typed yet: the control's own two words, which is its label.
        return RegionPalette {
            candidates: STACK_ACTIONS
                .iter()
                .map(|a| action_candidate(registry, ctx, a, &stem))
                .collect(),
            runnable,
            hint: None,
        };
    };

    // 🚨 **Whitespace after the first word is what separates the two questions**, exactly as
    // `Registry::candidates` reads a trailing space: with none the verb is still being typed and
    // this ring is the control's own; with some the verb has settled and the ring is the panel
    // ring, which belongs to `panel_stack` and is fetched through the registry rather than
    // restated here.
    if tail.is_empty() {
        let candidates: Vec<Candidate> = STACK_ACTIONS
            .iter()
            .filter(|a| narrows(a, &word))
            .map(|a| action_candidate(registry, ctx, a, &stem))
            .collect();
        let hint = candidates.is_empty().then(|| taken_words(&word));
        return RegionPalette { candidates, runnable, hint };
    }

    let Some(expanded) = expand(line) else {
        return RegionPalette { candidates: Vec::new(), runnable, hint: Some(taken_words(&word)) };
    };
    let typed_head = format!("{stem}{word}");
    let expanded_head = format!("/{STACK_VERB} {word}");
    let Some(inner) = registry.candidates(&expanded) else {
        return RegionPalette { candidates: Vec::new(), runnable, hint: None };
    };
    let candidates = inner
        .candidates
        .iter()
        // 🚨 **The column this line lives in is never offered as a word.** `stack`'s `region`
        // keyword is a real part of the verb's grammar and the registry offers it, correctly —
        // but in a control whose whole premise is that it edits *this* column, offering the word
        // would invite somebody to name a second, contradicting one. It is still *accepted* if
        // typed in full (see [`act`]); it is simply not proposed.
        .filter(|c| c.label != REGION_ARG)
        .filter_map(|c| {
            unexpand(&typed_head, &expanded_head, &c.completion)
                .map(|completion| Candidate { completion, ..c.clone() })
        })
        .collect::<Vec<_>>();
    let hint = if candidates.is_empty() { inner.hint() } else { None };
    RegionPalette { candidates, runnable, hint }
}

/// The sentence a word this control does not take gets, in the palette and in a refusal alike.
///
/// 🚨 **Derived from [`STACK_ACTIONS`], never written out.** A hand-kept copy of the two words is
/// the second vocabulary §1.8 exists to prevent, reached from the friendliest direction — and it
/// would be wrong the day a third action was added rather than the day somebody noticed.
fn taken_words(typed: &str) -> String {
    let taken: Vec<String> = STACK_ACTIONS.iter().map(|a| format!("`{a} <panel>`")).collect();
    format!(
        "`{typed}` is not a word this panel column takes: it takes {} (`remove {ALL_WORD}` \
         empties it). Every other console verb runs on the console line or at an agent",
        taken.join(" and ")
    )
}

/// One action as a candidate, with the line accepting it would produce.
///
/// The trailing space is `verb_candidate`'s rule: a verb that still wants an argument opens its
/// ring the instant it is accepted. Whether it wants one is asked of the **expanded** line, so
/// the answer comes from the registry's own schema rather than from a second opinion here.
fn action_candidate(registry: &Registry, ctx: Context, action: &str, stem: &str) -> Candidate {
    let bare = format!("{stem}{action}");
    let wants_more = !matches!(act(registry, ctx, &bare), Act::Run { .. });
    let completion = if wants_more { format!("{bare} ") } else { bare };
    let completes = matches!(act(registry, ctx, &completion), Act::Run { .. });
    Candidate {
        label: action.to_string(),
        doc: action_doc(action),
        completion,
        // ⚠️ **`Verb`, and the group is the region's own word.** A renderer that groups by
        // `CandidateKind::Verb { group }` would otherwise file these under whichever catalog
        // group the expansion landed in, which is machinery rather than what a person typed.
        kind: CandidateKind::Verb { group: ctx.region.as_word().to_string(), lane: Lane::Console },
        completes,
        // 🚨 **Never**, and it is not a value copied from the entry. `fires` is autorun's third
        // term and this line has no autorun; answering `false` everywhere is the honest reading
        // of a switch that is off.
        fires: false,
    }
}

/// What a finished line turns out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Nothing to run: an empty box, whitespace, or a bare `/`.
    Idle,
    /// A console-lane command, validated and ready for the same dispatch every other door uses.
    Run {
        /// The catalog name — `console.stack`, never the typed word.
        name: String,
        /// The dispatch arguments, **with this region already in them**.
        args: Value,
    },
    /// Refused, carrying the sentence to show. The line is **not** cleared — `Registry::resolve`'s
    /// rule, and for its reason: a refusal a person can edit is recoverable, a swallow is not.
    Refused(String),
}

/// What this line does, given this region.
///
/// 🚨 **Two words, and everything else is refused by name.** See the module header: this is a
/// control rather than a command surface, so there is no capability hiding behind the offer. An
/// unknown first word gets [`taken_words`], which names what would have worked and where the rest
/// of the console's vocabulary lives.
///
/// ⚠️ **The region fills an empty slot and never overwrites a typed one.** The column is the word
/// you did not have to say, not a word you are forbidden to say: `add surface region right`
/// reaches `right`, because the caller named it.
pub fn act(registry: &Registry, ctx: Context, line: &str) -> Act {
    let Some((word, _)) = head(line) else { return Act::Idle };
    let Some(expanded) = expand(line) else { return Act::Refused(taken_words(&word)) };
    match registry.resolve(&expanded) {
        Resolved::Run { lane: Lane::Console, name, mut args } => {
            if let Value::Object(map) = &mut args {
                // 🚨 **Filled only when the slot is empty.** `stack`'s `region` is a real optional
                // keyword in the table, so `add surface region right` resolves with `right`
                // already in hand — and an unconditional insert replaced it with this region's
                // own word, silently editing a column nobody named. That is the *"edits a
                // different column, silently"* defect this module was written against, arriving
                // from inside. The malformed case (a bare trailing `region` with no value) never
                // reaches here: `parse_args` answers it first, named rather than defaulted.
                map.entry(REGION_ARG.to_string())
                    .or_insert(Value::String(ctx.region.as_word().to_string()));
            }
            Act::Run { name, args }
        }
        // 🚨 **A registry refusal is carried, never rephrased.** `stack`'s own words for an
        // unknown panel are the words the CLI and the composer already print; re-wording them
        // here would make one mistake read differently depending on which rectangle it was typed
        // in, which is one vocabulary becoming two.
        Resolved::Refused(message) => Act::Refused(message),
        // Unreachable against the real catalog — `expand` only ever produces `/stack …`, which is
        // a console-lane verb with a slash and no escape. A named refusal rather than a panic on
        // the frame path: a catalog change should say what it did, not take the console down.
        other => Act::Refused(format!(
            "`{STACK_VERB}` did not resolve as a console command ({other:?}); \
             `organon console stack add <panel>` still works from a terminal"
        )),
    }
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
    /// What happened last — a refusal, or a receipt for a command that ran. Shown under the box.
    pub note: Option<String>,
    /// Ask egui for focus on the next frame this line draws.
    pub want_focus: bool,
    /// The line as it stood when this frame began, so [`crate::conversation_view::completion_held`]
    /// can tell an insertion from a deletion. `composer_seen`'s counterpart, per line.
    seen: String,
    /// Whether self-completion is latched off because the last edit removed text. See
    /// [`self_complete`].
    held: bool,
    /// Put egui's caret at the end of the box on the way out of this frame. Set by every path
    /// that rewrites the line **wholesale** — Tab's accept and the self-completion — because a
    /// rewrite leaves egui's own cursor index wherever the typing left it.
    want_caret: bool,
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
    /// ⚠️ **Called once per frame whether or not any region line draws**, which is what makes the
    /// owner fall back to the composer when the last line is taken away — a layout reset to
    /// `full agent` removes every line, nothing records focus, and the next frame's owner is
    /// `None`. A latch that only cleared on an explicit blur would leave the composer's keys held
    /// by a rectangle that no longer exists.
    pub fn begin(&mut self) {
        self.owner = self.seen.take();
    }

    /// Which region's line owned the keyboard as of the last completed frame.
    pub fn owner(&self) -> Option<Region> {
        self.owner
    }

    /// Whether the composer may read this frame's Tab and arrows — the one value `console_main`
    /// hands the conversation front-end.
    pub fn composer_owns_keys(&self) -> bool {
        self.owner.is_none()
    }

    pub fn line(&self, region: Region) -> &Line {
        &self.lines[region.slot()]
    }

    pub fn line_mut(&mut self, region: Region) -> &mut Line {
        &mut self.lines[region.slot()]
    }

    /// Say what happened, under the box that caused it.
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

/// The hint an empty box carries.
///
/// ✏️ **It was `add or remove a panel in this column` and is now the two words themselves.** The
/// candidate row that used to carry them is no longer in the band ([`BAND_ROWS`]), so this is the
/// whole of what a resting column offers — and a sentence describing the control has to earn its
/// line against the words you would actually type. It does not: James built this console, and
/// `add | remove` tells him everything the sentence did in a third of the width.
const HINT: &str = "add | remove";

/// 🚨 **The box's own id, and it is the fix for the defect in the module header.** Without it
/// egui derives one from `Ui::next_auto_id`, which counts the widgets drawn before the box — so
/// the candidate row above it moved the box's identity and focus was lost on the first keystroke.
/// Salted under [`draw`]'s `push_id`, so two regions still cannot share egui state.
const BOX_ID: &str = "organon-region-line-box";

/// The padding inside the band. `i8`, because that is what [`egui::Margin`] takes and a second
/// spelling in `f32` is a number to keep in step.
const PAD: i8 = 4;

/// How many rows the band reserves. **One: the box.**
///
/// ✏️ **It was four** — a candidate row, the box, and two for a note. James, 2026-08-20:
/// *"completely remove the status lines in the add remove inputs on the panels. It should just be
/// a single line. If we need to pop something up over top of it, we can do that, but it's way too
/// messy."* So the other three rows did not move up or shrink; they left the band entirely and
/// became [`popover`], which floats over the column instead of claiming a share of it.
///
/// # 🚨 The clip hazard changed shape rather than going away — read this before adding a row
///
/// #112 measured the old band pushing its `TextEdit` **35.6 pt past its own clip rect** in a
/// 320 pt column, because an unbounded wrapping row was drawn above it. The fix then was the row
/// *order* (the only unbounded row goes last, so a long refusal clips its own tail). That
/// argument is gone with the rows, and this is what replaces it:
///
/// **The band holds exactly one widget, and it is a single-line [`egui::TextEdit`].** A
/// single-line box does not wrap, so its height is one row whatever is typed into it and nothing
/// above it can move it. The property is now structural rather than ordinal — but it is
/// structural *only while nothing else is added to the band*. Anything with an unbounded height
/// belongs in the popover, which is a floating [`egui::Area`] and therefore cannot displace the
/// box no matter how tall it grows.
/// [`tests::the_box_stays_inside_the_band_however_long_the_refusal_is`] pins it at a real width.
pub const BAND_ROWS: usize = 1;

/// How tall the whole band is, in points.
pub fn band_height(row: f32) -> f32 {
    BAND_ROWS as f32 * row + 2.0 * PAD as f32 + 2.0
}

/// Draw this column's control into the bottom of `ui`, and answer what Enter asked for.
///
/// 🚨 **The id pushes are why this is a function rather than three widgets at the call site**, on
/// `panel_stack::draw`'s rule: `("organon-region-line", region.as_word())` is pushed *here*, so
/// two regions' boxes cannot share egui state whatever `Ui` the caller supplies. A salt applied
/// in `console_main` would put the property in a crate that nothing here can test.
///
/// ⚠️ **Keys are consumed before the box runs**, because egui hands each widget a clone of the
/// event list: an event removed after the `TextEdit` has read it is an event acted on twice. Only
/// Tab and the arrows are taken, and only while this line owns the keyboard — **Enter is
/// deliberately left alone**, because a single-line `TextEdit` surrenders focus on it and
/// `lost_focus` is the reliable read; and **Escape is left alone** so egui's own defocus is the
/// way out (module header).
///
/// ✏️ **The band is one row now** and everything it used to say is in [`popover`], drawn after
/// the box because it is anchored to the box's own rectangle. [`BAND_ROWS`] carries what that
/// changed about the clip guarantee.
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
            // 🚨 **The line as this frame found it, taken before ANYTHING may rewrite it** —
            // `notice_edit`'s position in the composer's pass, for its reason. Tab's accept runs
            // below and grows the line, which must read as an insertion so the latch lets go and
            // the accepted line may cascade; a shadow taken after it would read as a still frame
            // and hold the completion off for one keystroke.
            {
                let state = lines.line_mut(region);
                state.seen.clear();
                state.seen.push_str(&state.text);
            }
            let line = lines.line(region).clone();
            let pal = palette(registry, ctx, &line.text);

            // What the popover will say, decided before the box is drawn (so it reads this
            // frame's palette) and *shown* after it (so it can be anchored to the box). The
            // note is re-read below rather than taken from `line`, for the same reason it
            // always was: a refusal produced by this frame's Enter has to appear on this frame.
            let columns = columns_in(ui);
            let words = compact_join(&compact_words(&pal, line.selected), columns);

            if owns {
                consume_keys(ui, lines, region, &pal);
            }
            let response = {
                let state = lines.line_mut(region);
                let edit = egui::TextEdit::singleline(&mut state.text)
                    // 🚨 The defect fix. See [`BOX_ID`].
                    .id_salt(BOX_ID)
                    .desired_width(f32::INFINITY)
                    .frame(false)
                    .margin(egui::Margin::ZERO)
                    .font(egui::TextStyle::Monospace)
                    .text_color(theme.human_text)
                    // `composer_box`'s rule and its reason: egui's focus manager reads Tab out of
                    // the raw input before any of this runs, so consuming it here is too late to
                    // stop focus leaving. This is the flag that passes tests.
                    .lock_focus(true)
                    .hint_text(format!("{PROMPT} {HINT}"));
                let response = ui.add(edit);
                if std::mem::take(&mut state.want_focus) {
                    response.request_focus();
                }
                // ⚠️ **An edit clears the last answer**, asked of egui's own `changed` rather than
                // of a shadow copy: the composer needs a shadow because its rule is about the
                // *direction* of the edit (`completion_held`), and this line has no completion to
                // hold off.
                if response.changed() {
                    state.selected = 0;
                    state.note = None;
                }
                response
            };
            // 🚨 **This frame's measurement of who has the keyboard** — read off the widget rather
            // than inferred from a click, so a focus egui moved for any reason at all is the one
            // that counts.
            if response.has_focus() {
                lines.seen = Some(region);
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let typed = lines.line(region).text.clone();
                act = self::act(registry, ctx, &typed);
                let state = lines.line_mut(region);
                // Enter surrendered focus; ask for it straight back, so a second command does not
                // need a second click.
                state.want_focus = true;
                match &act {
                    // ⚠️ **Cleared only on a run.** A refused line stays in the box to be edited —
                    // `Registry::resolve`'s rule, and the reason the composer does not clear
                    // either.
                    Act::Run { .. } => {
                        state.text.clear();
                        state.selected = 0;
                        state.note = None;
                        // A cleared box is not a deletion somebody is recovering from — it is a
                        // fresh line. Letting the latch go here is what makes the *next*
                        // character self-complete instead of waiting one keystroke.
                        state.held = false;
                    }
                    Act::Refused(message) => state.note = Some(message.clone()),
                    Act::Idle => {}
                }
            } else {
                // 🚨 **Insertion or deletion, decided here and nowhere else** — `composer`'s
                // arrangement exactly, and the reason the shadow is taken at the top of the
                // frame: this is the one point at which both halves of the edit exist, the box
                // having just written this frame's keystroke into `text`.
                {
                    let state = lines.line_mut(region);
                    state.held = crate::conversation_view::completion_held(
                        &state.seen,
                        &state.text,
                        state.held,
                    );
                }
                self_complete(registry, ctx, lines, region);
            }
            // 🚨 **LAST, after every path that can rewrite the line wholesale** — Tab's accept
            // above the box, the self-completion below it. `put_caret_at_end` writes state
            // *between* frames, so the next frame's `TextEdit` loads a caret already at the end
            // and the next character appends. Without it, completing `add su` left the cursor at
            // 6 and the next characters landed inside the word it had just finished.
            if std::mem::take(&mut lines.line_mut(region).want_caret) {
                crate::conversation_view::put_caret_at_end(
                    ui.ctx(),
                    response.id,
                    &lines.line(region).text,
                );
                // ⚠️ **The rewrite happened AFTER the box drew**, so this frame still shows the
                // line as it was typed and the popover still shows the ring it was narrowed to.
                // A keystroke schedules the next frame on its own; a completion is the console
                // editing the line, so it has to ask for one — otherwise a completed line could
                // sit unshown until the next unrelated event.
                ui.ctx().request_repaint();
            }
            // 📌 **Over the top, never under the box.** The `Area` allocates nothing in this
            // `Ui`, so however long a refusal is it cannot move the thing that would let you
            // correct it — the guarantee [`BAND_ROWS`] used to get from the row order.
            let said = overlay(lines.line(region).note.as_deref(), &words, owns);
            popover(ui, region, response.rect, said, theme, form);
        }
        framed.frame.stroke = egui::Stroke::new(
            1.0_f32,
            if owns { theme.composer_edge_focus } else { theme.composer_edge },
        );
        framed.end(ui);
    });
    act
}

/// What floats over the column this frame, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said<'a> {
    /// Nothing. **The resting state of a panel column**, and the whole of what Tier 2 is for.
    Nothing,
    /// The words this line could become next — offered only while a hand is in the box.
    Words(&'a str),
    /// Why the last line was refused. **Shown whatever the focus is doing** — see [`overlay`].
    Refusal(&'a str),
}

/// **The one rule about what the popover says**, pure and separate from the drawing so it can be
/// read without an `egui::Ui`.
///
/// 🚨 **A refusal outranks the candidates, and it is not conditional on focus.** The two mean
/// opposite things — one answers a line already sent, the other a line still being typed — which
/// is `conversation_view::command_panel`'s rule arriving on a second surface, and one rectangle
/// cannot say two things at once. What is *added* here is the focus asymmetry, and it is the
/// whole of the quiet/loud split this change turns on: **a success is discoverable, a refusal is
/// news.** Candidates appear because a hand is in the box asking what comes next; a refusal
/// appears because something did not happen, and a person who has clicked away is exactly the
/// person who would otherwise never learn it.
///
/// ⚠️ An empty `words` is `Nothing` rather than an empty plate — [`compact_join`] answers `""`
/// when there is nothing left to offer, and a bordered rectangle with no text in it reads as a
/// rendering fault.
pub fn overlay<'a>(note: Option<&'a str>, words: &'a str, owns: bool) -> Said<'a> {
    if let Some(note) = note {
        return Said::Refusal(note);
    }
    if owns && !words.trim().is_empty() {
        return Said::Words(words);
    }
    Said::Nothing
}

/// Draw [`Said`] in a floating layer sitting **on** the column, immediately above `anchor`.
///
/// 🚨 **An [`egui::Area`], not a row.** An `Area` is its own layer: it allocates nothing in the
/// calling `Ui`, so it cannot push the box out of the band however tall it grows — which is the
/// guarantee `BAND_ROWS` used to buy with the row order, now bought structurally. It is also
/// what makes James's *"if we need to pop something up over top of it, we can do that"* a
/// different object from the rows he asked to remove: this one is **over** the column's content
/// rather than carved out of it, and it is gone the moment there is nothing to say.
///
/// ⚠️ **`Align2::LEFT_BOTTOM` about the box's own top-left**, so the popover grows *upwards* from
/// a rectangle whose position is already known. Anchoring the top instead would need the
/// popover's height a frame before it is laid out, which is the measured-band problem this
/// change exists to stop having.
///
/// ⚠️ **`interactable(false)`.** Nothing here is a control — the words are taken with Tab, and a
/// layer that swallowed clicks would sit between the pointer and the column it floats over.
fn popover(
    ui: &egui::Ui,
    region: Region,
    anchor: egui::Rect,
    said: Said<'_>,
    theme: &Theme,
    form: &Form,
) {
    let (text, color) = match said {
        Said::Nothing => return,
        Said::Words(words) => (words, theme.panel_title),
        Said::Refusal(note) => (note, theme.bad),
    };
    // Clear of the band's own top edge, so the plate does not read as a fifth side of the box.
    let above = anchor.left_top() - egui::vec2(PAD as f32, PAD as f32 + 2.0);
    egui::Area::new(egui::Id::new(("organon-region-line-popover", region.as_word())))
        .order(egui::Order::Foreground)
        .fixed_pos(above)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .constrain(true)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            // The band's own width, so a refusal wraps inside the column rather than across
            // whatever happens to sit beside it.
            ui.set_max_width(anchor.width());
            let mut framed = Frame::new()
                .fill(theme.composer_fill)
                .stroke(egui::Stroke::new(1.0_f32, theme.composer_edge))
                .corner_radius(form.card_corner())
                .inner_margin(egui::Margin::symmetric(6, PAD));
            if let Some(shadow) = form.card_stroke(theme.panel_edge) {
                framed = framed.stroke(shadow);
            }
            framed.show(ui, |ui| {
                ui.label(RichText::new(text).monospace().color(color));
            });
        });
}

/// How many monospace cells fit across this `Ui`. `compact_fit` measures in **columns**, which is
/// exact because the row is drawn entirely in the mono face.
fn columns_in(ui: &egui::Ui) -> usize {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    // `fonts_mut`, not `fonts` — measuring a glyph may have to rasterise it, which is
    // `compact_band`'s own call and the reason it is spelled that way there too.
    let cell = ui.ctx().fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    (ui.available_width() / cell).floor().max(0.0) as usize
}

/// The compact row's words for this control — [`crate::conversation_view::compact_words`]'
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
        let text = if here { format!("[{}]", candidate.label) } else { candidate.label.clone() };
        CompactWord { text, here, runs: false }
    }));
    words
}

/// Take this frame's Tab and arrows on this line's behalf — and only while it owns the keyboard.
fn consume_keys(ui: &egui::Ui, lines: &mut Lines, region: Region, palette: &RegionPalette) {
    let keys: Vec<(egui::Key, egui::Modifiers)> = ui.input_mut(|i| {
        let mut taken = Vec::new();
        i.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else {
                return true;
            };
            let ours = matches!(
                key,
                egui::Key::Tab | egui::Key::ArrowUp | egui::Key::ArrowDown
            );
            if ours {
                taken.push((*key, *modifiers));
            }
            !ours
        });
        taken
    });
    let count = palette.candidates.len();
    for (key, modifiers) in keys {
        let state = lines.line_mut(region);
        match key {
            egui::Key::ArrowDown => state.selected = step(state.selected, count, true),
            egui::Key::ArrowUp => state.selected = step(state.selected, count, false),
            egui::Key::Tab if modifiers.shift => state.selected = step(state.selected, count, false),
            egui::Key::Tab => {
                if let Some(candidate) = palette.candidates.get(state.selected) {
                    state.text = candidate.completion.clone();
                    state.selected = 0;
                    state.want_focus = true;
                    // A wholesale rewrite; egui's caret does not follow it. See `draw`'s drain.
                    state.want_caret = true;
                }
            }
            _ => {}
        }
    }
}

/// The most completions one frame may chain here.
///
/// 🚨 **A bound rather than a `loop`**, `conversation_view::PALETTE_COMPLETE_STEPS`' rule and its
/// reason: completing rewrites the line, and the new line may have a lone candidate of its own.
/// That cascade is wanted — `a` becomes `add `, whose ring is every panel word — and what it must
/// not be able to do is spin. [`RegionPalette::sole_completion`] already refuses a candidate that
/// would rewrite the line to itself, so a cycle would need two completions that alternate, which
/// this grammar cannot produce and which a future panel table cannot be trusted not to.
///
/// ⚠️ **Two is the deepest this control's grammar goes** (action → panel; the `region` keyword is
/// filtered out of the ring by [`palette`] and so can never be completed into). Three is one step
/// of headroom, deliberately smaller than the composer's four because there is one fewer ring.
const COMPLETE_STEPS: usize = 3;

/// Take the completion when there is only one, with no Tab — the composer's §1.9 rule, arriving
/// on this surface as #131.
///
/// James: *"currently these do not auto-complete terms when we have specified them with a few
/// characters."* On a control with two verbs and one closed panel table, almost every line is
/// unambiguous within two keystrokes, which is exactly the case where asking for a Tab is asking
/// for a keystroke that carries no information.
///
/// 🚨 **It completes and never runs.** `console.stack` is [`crate::command::Reversal::Permanent`],
/// so [`Candidate::fires`] is false for everything this control produces and autorun is not
/// merely switched off here — it is unreachable. The line is left in the box for Enter, which is
/// the one thing that may act.
///
/// 🚨 **Never on a deletion.** Asked once, outside the loop, exactly as `palette_complete` asks
/// it: the cascade is one insertion's consequence, and re-asking per step would let a completion
/// argue with the edit that started it. [`crate::conversation_view::completion_held`] is the
/// shared rule and carries the measurements — without it, backspacing out of `add surface` would
/// put the deleted characters straight back on the same frame, for ever.
///
/// ⚠️ **NOT gated on [`Lines::owner`], and the asymmetry with [`consume_keys`] is deliberate.**
/// That function reads the *raw event list* on a line's behalf, so it must only run for the line
/// that will get those events — last frame's measurement. This one reacts to text the box has
/// already written, and the frame on which focus **arrives** is one where `owner` is still the
/// old answer while typing lands regardless; gating would silently skip the first character's
/// completion. Running it unconditionally costs nothing: a resting line has already completed,
/// so there is never anything left for it to do.
fn self_complete(registry: &Registry, ctx: Context, lines: &mut Lines, region: Region) {
    if lines.line(region).held {
        return;
    }
    for _ in 0..COMPLETE_STEPS {
        let text = lines.line(region).text.clone();
        let pal = palette(registry, ctx, &text);
        let Some(only) = pal.sole_completion(&text) else { return };
        let completion = only.completion.clone();
        let state = lines.line_mut(region);
        state.text = completion;
        state.selected = 0;
        // The answer to the previous line is not an answer to this one — `accept`'s rule.
        state.note = None;
        state.want_caret = true;
    }
}

/// Move a highlight, wrapping. `conversation_view::move_selection`'s rule: a ring of a handful of
/// words has no end worth feeling.
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

    /// The real stack spec, in the shape `console_main::console_specs` declares it — this crate
    /// cannot see that function, so the two are bound by
    /// `a_region_line_expands_onto_the_real_console_specs` in `console_main.rs`.
    ///
    /// ⚠️ **`console.theme` is still in the fixture on purpose.** It is not a verb this control
    /// takes; it is here so the tests can prove that a console verb typed into a column is
    /// **refused by name** rather than silently doing nothing — which is the whole of what the
    /// narrowing changed.
    fn console() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "console.stack".into(),
                doc: "What is in a panel region".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec {
                        name: "action".into(),
                        kind: ArgKind::Choice(
                            STACK_ACTIONS.iter().map(|s| (*s).to_string()).collect(),
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
                        name: REGION_ARG.into(),
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

    fn column() -> Context {
        Context { region: Region::Left }
    }

    /// 🚨 **The control's own sentence, as a test.** `add surface` in a panel column does what
    /// `organon console stack add surface` does — and it names the column it was typed in, which
    /// is the whole of "the region becomes context".
    #[test]
    fn add_in_a_panel_column_is_stack_add_in_that_column() {
        let act = act(&registry(), column(), "add surface");
        let Act::Run { name, args } = act else { panic!("{act:?}") };
        assert_eq!(name, "console.stack");
        assert_eq!(args["action"], "add");
        assert_eq!(args["panel"], "surface");
        assert_eq!(
            args[REGION_ARG], "left",
            "the column a person typed into is not in the arguments"
        );
    }

    /// ⚠️ **The slash is tolerated, and the two spellings are the same command.** Tier C taught a
    /// slash line; refusing it now would refuse the spelling already in James's fingers.
    #[test]
    fn a_leading_slash_is_optional_and_changes_nothing() {
        let reg = registry();
        for line in ["add surface", "/add surface", "  /add surface", "   add surface"] {
            let Act::Run { name, args } = act(&reg, column(), line) else {
                panic!("`{line}` did not run")
            };
            assert_eq!(name, "console.stack", "{line}");
            assert_eq!(args["panel"], "surface", "{line}");
        }
    }

    /// 🚨 **`remove all` is the emptying spelling, and it comes from `panel_stack`'s own ring
    /// rather than from a third verb invented here.** `StackCmd::resolve` turns it into
    /// `StackCmd::Clear`; this control only has to let the word through.
    #[test]
    fn remove_all_is_the_word_that_empties_the_column() {
        let act = act(&registry(), column(), "remove all");
        let Act::Run { name, args } = act else { panic!("{act:?}") };
        assert_eq!(name, "console.stack");
        assert_eq!(args["action"], "remove");
        assert_eq!(args["panel"], ALL_WORD);
        assert!(
            crate::panel_stack::panel_words().contains(&ALL_WORD),
            "the emptying word is not in the panel ring, so nothing would offer it"
        );
    }

    /// 🚨 **The reversal, pinned.** #98 Tier C accepted every console verb here on the rule
    /// *"prune discovery, never capability"*; James rejected that scope, so a console verb typed
    /// into a panel column is now **refused by name**. See the module header for why a dedicated
    /// control is not a pruned command surface.
    ///
    /// ⚠️ The verb is a real one in the fixture, so this is a refusal of something that resolves
    /// perfectly well elsewhere — not of a word nothing knows.
    #[test]
    fn a_console_verb_that_is_not_add_or_remove_is_refused_by_name() {
        let reg = registry();
        assert!(
            matches!(reg.resolve("/theme dark"), Resolved::Run { .. }),
            "the fixture does not know `theme`, so this test would prove nothing"
        );
        for line in ["/theme dark", "theme dark", "/viewport left panel", "/help"] {
            let act = act(&reg, column(), line);
            let Act::Refused(message) = act else { panic!("`{line}`: {act:?}") };
            let word = line.trim_start_matches('/').split(' ').next().unwrap();
            assert!(message.contains(word), "the refusal does not name the word: {message}");
            assert!(message.contains("add"), "{message}");
            assert!(message.contains("remove"), "{message}");
            assert!(
                message.contains("console line") || message.contains("agent"),
                "the refusal does not say where the verb does work: {message}"
            );
        }
    }

    /// A word nothing knows gets the same sentence a known-but-not-taken verb gets — one refusal,
    /// because from this control's side they are the same fact.
    #[test]
    fn an_unknown_word_is_refused_by_name_too() {
        let act = act(&registry(), column(), "nonesuch");
        let Act::Refused(message) = act else { panic!("{act:?}") };
        assert!(message.contains("nonesuch"), "{message}");
    }

    /// ⚠️ **A bad panel name keeps `panel_stack`'s own words**, carried rather than rephrased —
    /// one mistake must read the same whichever door it was typed at.
    #[test]
    fn a_bad_panel_name_keeps_the_registrys_own_refusal() {
        let reg = registry();
        let Resolved::Refused(from_registry) = reg.resolve("/stack add nonesuch") else {
            panic!("the registry refuses an unknown panel")
        };
        let Act::Refused(from_line) = act(&reg, column(), "add nonesuch") else {
            panic!("and this control refuses it too")
        };
        assert_eq!(
            from_line, from_registry,
            "the control rewrote the console's refusal; one mistake must be refused in one set \
             of words whichever rectangle it was typed in"
        );
    }

    /// The control's label: an empty box already says what the control takes.
    #[test]
    fn an_empty_line_offers_the_two_words() {
        let p = palette(&registry(), column(), "");
        let labels: Vec<&str> = p.candidates.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, STACK_ACTIONS, "the ring is `panel_stack::STACK_ACTIONS`, in its order");
        assert!(!p.runnable, "an empty line is not a command");
    }

    /// …and nothing else is ever offered, at any prefix — the list *is* the vocabulary.
    #[test]
    fn no_other_verb_is_ever_offered() {
        let reg = registry();
        for line in ["", "/", "a", "/a", "r", "t", "/t", "/th", "v"] {
            for c in palette(&reg, column(), line).candidates {
                assert!(
                    STACK_ACTIONS.contains(&c.label.as_str()),
                    "`{line}` offered `{}`, which this control does not take",
                    c.label
                );
            }
        }
    }

    /// A prefix that narrows to nothing says so on the row, before Enter rather than after it.
    #[test]
    fn a_prefix_that_narrows_to_nothing_says_so() {
        let p = palette(&registry(), column(), "/th");
        assert!(p.candidates.is_empty(), "no action begins `th`");
        let hint = p.hint.expect("a row that offers nothing must still say something");
        assert!(hint.contains("th"), "{hint}");
        assert!(hint.contains("add") && hint.contains("remove"), "{hint}");
    }

    /// 🚨 **The panel ring is the registry's, un-expanded back onto the line a person is looking
    /// at.** Getting this wrong would put `/stack add surface` in a box whose vocabulary has no
    /// `stack` in it.
    #[test]
    fn completions_come_back_un_expanded() {
        let reg = registry();
        for (typed, want) in [("add su", "add surface "), ("/add su", "/add surface ")] {
            let p = palette(&reg, column(), typed);
            let completions: Vec<&str> =
                p.candidates.iter().map(|c| c.completion.as_str()).collect();
            assert_eq!(completions, vec![want], "{typed}: {completions:?}");
            assert!(matches!(act(&reg, column(), want), Act::Run { .. }), "{want} does not run");
        }
    }

    /// 🚨 **The word this control supplies is never offered.** Offering `region` inside a control
    /// whose whole premise is that it edits *this* column would invite a second, contradicting
    /// one.
    #[test]
    fn the_supplied_region_keyword_is_never_offered() {
        let reg = registry();
        // The registry itself does offer it — this is what is being dropped, not a thing that
        // never existed.
        let inner = reg.candidates("/stack add surface ").expect("the registry's own ring");
        assert!(
            inner.candidates.iter().any(|c| c.label == REGION_ARG),
            "the fixture does not offer the keyword, so this test proves nothing"
        );
        let p = palette(&reg, column(), "add surface ");
        assert!(
            !p.candidates.iter().any(|c| c.label == REGION_ARG),
            "the control offered the word it supplies: {:?}",
            p.candidates.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
    }

    /// 🚨 **Not offered is not the same as not accepted, and a typed word wins.** `add surface
    /// region right` typed into the `left` column has to reach `right`. An unconditional insert of
    /// the supplied word made it reach `left` instead — silently editing a column nobody named,
    /// which is the one failure this module is written against.
    #[test]
    fn a_typed_region_keyword_beats_the_one_this_column_supplies() {
        let reg = registry();
        let region_of = |line: &str| match act(&reg, column(), line) {
            Act::Run { name, args } => {
                assert_eq!(name, "console.stack", "{line}");
                args[REGION_ARG].clone()
            }
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(
            region_of("add surface region right"),
            "right",
            "the column a person typed was replaced by the one they were standing in"
        );
        assert_eq!(region_of("add surface"), "left", "saying nothing still means `here`");

        // A named column with no name is malformed, never defaulted — the caller reached for a
        // column and the line lost it, and filling in this one would edit the one they did not
        // name. `parse_args` answers first, so the supplied word never gets the chance.
        let act = act(&reg, column(), "add surface region");
        let Act::Refused(message) = act else { panic!("{act:?}") };
        assert!(message.contains(REGION_ARG), "{message}");
    }

    /// An action always wants a panel, so accepting one opens its ring rather than leaving a line
    /// that looks finished and is not.
    #[test]
    fn an_action_gains_a_trailing_space_because_it_always_wants_a_panel() {
        let p = palette(&registry(), column(), "");
        for c in &p.candidates {
            assert!(c.completion.ends_with(' '), "`{}` -> `{}`", c.label, c.completion);
            assert!(!c.completes, "`{}` is not a whole command on its own", c.label);
        }
    }

    /// An empty box, whitespace and a bare slash all do nothing, and none of them says anything —
    /// there is nothing to report about a command nobody started.
    #[test]
    fn an_empty_line_and_a_bare_slash_are_idle() {
        let reg = registry();
        assert_eq!(act(&reg, column(), ""), Act::Idle);
        assert_eq!(act(&reg, column(), "   "), Act::Idle);
        assert_eq!(act(&reg, column(), "/"), Act::Idle);
        assert_eq!(act(&reg, column(), " / "), Act::Idle);
    }

    /// 🚨 **The keyboard owner defaults to the composer, and that is invariant #4.** A console with
    /// no panel column draws no region line, so nothing ever records focus and the composer keeps
    /// every key it had before this module existed.
    #[test]
    fn the_composer_owns_the_keyboard_until_a_region_line_takes_it() {
        let mut lines = Lines::default();
        assert!(lines.composer_owns_keys());
        lines.begin();
        assert!(lines.composer_owns_keys(), "a frame in which nothing drew moved the owner");

        lines.seen = Some(Region::Left);
        lines.begin();
        assert_eq!(lines.owner(), Some(Region::Left));
        assert!(!lines.composer_owns_keys());

        // …and a frame in which nothing did hands it straight back. See `Lines::begin` on why this
        // must not need an explicit blur.
        lines.begin();
        assert!(lines.composer_owns_keys(), "the owner outlived the line that held it");
    }

    /// Each region's line is its own — the text in one is not the text in another.
    #[test]
    fn a_line_belongs_to_its_region() {
        let mut lines = Lines::default();
        lines.line_mut(Region::Left).text = "add surface".into();
        assert_eq!(lines.line(Region::Left).text, "add surface");
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
    fn expansion_replaces_the_head_and_nothing_else() {
        assert_eq!(expand("add surface").as_deref(), Some("/stack add surface"));
        assert_eq!(expand("/add  surface  ").as_deref(), Some("/stack add  surface  "));
        assert_eq!(expand("remove all").as_deref(), Some("/stack remove all"));
        // Not a word this control takes: nothing to expand onto, and `act` refuses it by name.
        assert_eq!(expand("/theme dark"), None);
        assert_eq!(expand("hello"), None);
        assert_eq!(expand(""), None);
    }

    /// 🚨 **`fires` is false everywhere, and it is not copied from the entry.** This line has no
    /// autorun; a candidate claiming otherwise would invite a renderer to run a command on a
    /// keystroke that never earned one.
    #[test]
    fn no_candidate_ever_fires() {
        let reg = registry();
        for line in ["", "/", "a", "add ", "remove "] {
            for c in palette(&reg, column(), line).candidates {
                assert!(!c.fires, "`{line}`: `{}` claims it may run unasked", c.label);
            }
        }
    }

    /// 🚨 **Every string this module COMPOSES is ASCII**, which is `conversation_view`'s glyph
    /// allowlist reaching a surface that allowlist cannot see. That guard walks an enumerated list
    /// of draw sites in *that* file; these are draw sites in this one, and the defect it exists
    /// for shipped once already — a `✓` in none of egui's four bundled fonts, drawn as an empty
    /// box in the pane log and photographed on a running console.
    ///
    /// ⚠️ **"Composes" and not "draws".** A registry refusal is the **console's own words**,
    /// already drawn by the composer on every build that has ever shipped; this module passes it
    /// through byte-for-byte and adds nothing. Asserting a glyph policy over it would put a second
    /// copy of that policy here, and the copy would win arguments it has no standing in. The
    /// pass-through is pinned as *unmodified* by `a_bad_panel_name_keeps_the_registrys_own_refusal`
    /// instead.
    #[test]
    fn no_string_this_control_composes_needs_a_glyph_egui_lacks() {
        let reg = registry();
        let mut drawn: Vec<String> = vec![PROMPT.to_string(), HINT.to_string()];
        for action in STACK_ACTIONS {
            drawn.push(action_doc(action));
        }
        for line in ["", "/", "a", "add ", "add surface ", "th", "/th", "nonesuch"] {
            let p = palette(&reg, column(), line);
            drawn.extend(p.hint);
            for c in p.candidates {
                drawn.push(c.label);
                drawn.push(c.doc);
            }
            if let Act::Refused(message) = act(&reg, column(), line) {
                // Only the sentences this module writes; `panel_stack`'s are its own.
                if message.contains("panel column takes") {
                    drawn.push(message);
                }
            }
        }
        for text in drawn {
            assert!(
                text.is_ascii(),
                "this control composes a non-ASCII glyph, which egui's bundled fonts may not \
                 have: {text:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The defect, driven through a real egui frame
    // -----------------------------------------------------------------------

    /// One frame of this control at a realistic column width, headless.
    ///
    /// The rectangle and the child `Ui` are `console_main::draw_regions`' own arrangement — a band
    /// of [`band_height`] at the bottom of the region, clipped to itself — because the defect
    /// being pinned is about what happens *inside* that clip rect. `egui::RawInput::default()`
    /// carries `focused: true`, which is what makes `Response::has_focus` mean anything with no
    /// window in sight.
    ///
    /// Answers the band rectangle it reserved, so a test can ask whether what was drawn stayed
    /// inside it. ⚠️ **Recomputed here rather than taken from a constant**, because the row
    /// height is a font measurement and a hard-coded band would be a second spelling of
    /// [`band_height`] that could agree with it and stop.
    fn frame(
        ctx: &egui::Context,
        reg: &Registry,
        lines: &mut Lines,
        events: Vec<egui::Event>,
        width: f32,
    ) -> egui::Rect {
        let mut band_rect = egui::Rect::NOTHING;
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let row = ui.text_style_height(&egui::TextStyle::Monospace);
                let band = band_height(row);
                let rect = egui::Rect::from_min_max(
                    egui::pos2(0.0, 600.0 - band),
                    egui::pos2(width, 600.0),
                );
                band_rect = rect;
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(rect);
                let _ = draw(
                    &mut child,
                    column(),
                    reg,
                    lines,
                    &crate::theme::Theme::organon(),
                    &Form::at(0.0),
                );
            });
        });
        band_rect
    }

    fn typed(s: &str) -> Vec<egui::Event> {
        vec![egui::Event::Text(s.to_string())]
    }

    /// 🚨 **The defect James reported, as a test**: *"When I type slash in one of the regions, I
    /// just get a list of choices and some text below it, and I can't type or select anything."*
    ///
    /// The first character landed and none of the rest did, because `egui::TextEdit` with no
    /// explicit id takes one from `Ui::next_auto_id` — a counter over the widgets already
    /// allocated. The candidate row is drawn *above* the box, so the frame on which the palette
    /// appeared gave the box a **different id**, and focus stayed with a widget that no longer
    /// existed.
    ///
    /// ⚠️ **Two widths, because there were two defects.** The first diagnosis — the band's old
    /// `elsewhere` sentence wrapping the box **35.6 pt past its own clip rect** in a 320 pt
    /// column — was real and independent: the narrow case must pass for the layout reason and
    /// the wide case for the id reason.
    ///
    /// 🚨 **On today's code this test does NOT fail if [`BOX_ID`] is deleted, and that is worth
    /// knowing rather than hiding.** Measured: remove the salt and every test here still passes.
    /// The reason was the narrowing (the candidate row became unconditional, so the widget count
    /// above the box stopped varying) and is now the *popover* — nothing at all is drawn in the
    /// band above the box, so the auto id cannot move. The salt is kept as the thing that makes
    /// the property true **by construction** rather than by a coincidence of the current layout,
    /// and [`tests::a_note_appearing_does_not_take_the_box_with_it`] is the case that does catch
    /// its removal.
    ///
    /// ✏️ **The expected line changed with #131 and the ODD-LOOKING VALUE IS THE POINT — do not
    /// "fix" it.** These six keystrokes used to leave `add su`. Self-completion now takes `a` at
    /// the first keystroke (it is the only action beginning with it) and rewrites the line to
    /// `add `, so the `d`, `d` that follow are typed *after* a word that is already finished.
    /// `add dd su` is therefore the honest result of pressing those six keys, and it is kept as
    /// the assertion for two reasons: it still discriminates the defect exactly — one keystroke
    /// landing leaves `add `, two leaves `add d`, all six leave this — and it is the **cost of
    /// self-completion, standing in the suite** rather than argued about in prose. Muscle memory
    /// that types a verb in full now overshoots it. That cost is the composer's too (`/b`
    /// completes to `/background ` and `ackground` lands after it) and it is accepted here for
    /// the same reason: a second completion rule would be a second vocabulary. See
    /// [`a_prefix_completes_itself_all_the_way_to_a_runnable_line`] for the path this buys.
    #[test]
    fn typing_survives_the_palette_opening() {
        for width in [320.0_f32, 2400.0] {
            let ctx = egui::Context::default();
            let reg = registry();
            let mut lines = Lines::default();
            lines.line_mut(Region::Left).want_focus = true;
            // Two frames to take focus: the request only lands on the frame after it is made.
            for _ in 0..2 {
                lines.begin();
                frame(&ctx, &reg, &mut lines, Vec::new(), width);
            }
            for c in ["a", "d", "d", " ", "s", "u"] {
                lines.begin();
                frame(&ctx, &reg, &mut lines, typed(c), width);
            }
            assert_eq!(
                lines.line(Region::Left).text,
                "add dd su",
                "at {width} pt: a keystroke went missing — one landing leaves `add `, two leave \
                 `add d`; all six leave this"
            );
        }
    }

    /// 🚨 **A refusal appearing must not cost the box the keyboard — the defect's general form.**
    ///
    /// A note is the one thing in this band whose presence comes and goes and whose height is
    /// unbounded, so it is the thing that would move the box if it were drawn above it. Both
    /// halves of the fix are pinned here at a real column width, and **both mutations are
    /// measured rather than assumed**:
    ///
    /// | [`BOX_ID`] | note drawn | this test |
    /// |---|---|---|
    /// | present | in the popover | **passes** — as shipped |
    /// | deleted | in the popover | passes — a floating layer allocates nothing, so the id holds |
    /// | deleted | **before** the box, in the band | **FAILS**, `left: ""` — #112's defect exactly |
    /// | present | before the box, in the band | passes — the salt alone holds it |
    ///
    /// ✏️ The middle two rows read *"after the box"* / *"the order alone happens to hold the id
    /// still"* before Tier 2; the note is not in the band at all now, which is a stronger version
    /// of the same protection rather than a different one.
    ///
    /// 🚨 **Read the table rather than the two fixes.** Neither is redundant and neither is
    /// sufficient on its own for the reason the other exists: the salt makes focus survive
    /// *whatever* is drawn around the box, and keeping the unbounded text out of the band makes a
    /// long refusal clip its own tail instead of the box. Together, a later edit is free to move
    /// a row without silently re-opening the defect — which is the only arrangement in which this
    /// stays fixed once nobody remembers why.
    #[test]
    fn a_note_appearing_does_not_take_the_box_with_it() {
        let ctx = egui::Context::default();
        let reg = registry();
        let mut lines = Lines::default();
        lines.line_mut(Region::Left).want_focus = true;
        for _ in 0..2 {
            lines.begin();
            frame(&ctx, &reg, &mut lines, Vec::new(), 320.0);
        }
        // The real refusal, at its real length — `panel_stack`'s own words for an unknown panel
        // name, which wrap to several rows in a 320 pt column.
        let Act::Refused(message) = act(&reg, column(), "add nonesuch") else {
            panic!("the fixture no longer refuses an unknown panel")
        };
        assert!(message.len() > 60, "the refusal got short; this no longer tests wrapping");
        lines.note(Region::Left, message);
        // Two frames with the note on screen: the first draws it, the second is the one on which
        // a moved id would have stranded the focus.
        for _ in 0..2 {
            lines.begin();
            frame(&ctx, &reg, &mut lines, Vec::new(), 320.0);
        }
        for c in ["a", "d", "d"] {
            lines.begin();
            frame(&ctx, &reg, &mut lines, typed(c), 320.0);
        }
        // ✏️ `add dd`, not `add`, since #131 — see
        // [`typing_survives_the_palette_opening`]'s doc for why the value looks wrong and is
        // right. One keystroke landing would leave `add `, which is what this distinguishes.
        assert_eq!(
            lines.line(Region::Left).text,
            "add dd",
            "a note appearing took the keyboard away from the box under it"
        );
        assert!(
            lines.line(Region::Left).note.is_none(),
            "an edit must clear the answer it is editing away from"
        );
    }

    /// …and a candidate is selectable, by the key the row says takes it.
    ///
    /// ⚠️ **Tab, not a click** — the row is a label rather than a set of buttons, and this pins
    /// the path that actually exists rather than one somebody might assume. `Lines::begin` is
    /// called per frame exactly as `console_main` calls it, because `consume_keys` runs only for
    /// the region that owned the keyboard *last* frame and skipping it would make the test pass
    /// for the wrong reason.
    #[test]
    fn tab_takes_the_highlighted_candidate() {
        let ctx = egui::Context::default();
        let reg = registry();
        let mut lines = Lines::default();
        lines.line_mut(Region::Left).want_focus = true;
        for _ in 0..2 {
            lines.begin();
            frame(&ctx, &reg, &mut lines, Vec::new(), 320.0);
        }
        // Down moves the highlight from `add` to `remove`; Tab accepts it.
        for key in [egui::Key::ArrowDown, egui::Key::Tab] {
            lines.begin();
            frame(
                &ctx,
                &reg,
                &mut lines,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                320.0,
            );
        }
        assert_eq!(
            lines.line(Region::Left).text,
            "remove ",
            "Tab did not take the highlighted candidate"
        );
    }

    // -----------------------------------------------------------------------
    // Tier 2 — one line, and the guarantee that replaced the row order
    // -----------------------------------------------------------------------

    /// 🚨 **The clip guarantee, re-established after the rows it used to rest on were removed.**
    ///
    /// #112 measured the old band pushing its box **35.6 pt past its own clip rect** in a 320 pt
    /// column, because an unbounded wrapping row shared the band with it. The answer then was the
    /// row order; the answer now is that the band holds one single-line widget and everything
    /// unbounded floats in an [`egui::Area`]. That is a claim about geometry, so it is asserted
    /// on geometry rather than inferred from typing still working.
    ///
    /// ⚠️ **The box is found through egui's own focus rather than by rebuilding its id.** The id
    /// is `push_id` ∘ [`BOX_ID`] ∘ whatever `Ui` the caller supplied — reconstructing that here
    /// would be a copy of the very arrangement under test, and it would keep passing if the
    /// production path stopped using it.
    ///
    /// ⚠️ **Mutation-checked, and the number is the point**: draw the note as a `ui.label` in the
    /// band above the box instead of in the popover and this fails at 320 pt with the box at
    /// `y 657.9…673.0` against a band of `574.9…600.0` — **73 pt outside**, twice #112's
    /// original 35.6, because a one-row band has less slack to absorb a wrap than a four-row one
    /// did. ⚠️ `a_note_appearing_does_not_take_the_box_with_it` still *passes* under that
    /// mutation, which is exactly what its own table says and why both tests exist.
    #[test]
    fn the_box_stays_inside_the_band_however_long_the_refusal_is() {
        for width in [320.0_f32, 2400.0] {
            let ctx = egui::Context::default();
            let reg = registry();
            let mut lines = Lines::default();
            lines.line_mut(Region::Left).want_focus = true;
            let Act::Refused(message) = act(&reg, column(), "add nonesuch") else {
                panic!("the fixture no longer refuses an unknown panel")
            };
            assert!(message.len() > 60, "the refusal got short; this no longer tests wrapping");
            lines.note(Region::Left, message);
            let mut band = egui::Rect::NOTHING;
            for _ in 0..3 {
                lines.begin();
                band = frame(&ctx, &reg, &mut lines, Vec::new(), width);
            }
            let focused = ctx.memory(|m| m.focused()).expect("the box asked for focus");
            let box_rect =
                ctx.read_response(focused).expect("the focused widget drew last frame").rect;
            assert!(
                band.contains_rect(box_rect),
                "at {width} pt the box at {box_rect:?} left its band {band:?} — a refusal \
                 pushed the input somebody needs in order to correct it out of the clip rect"
            );
        }
    }

    /// **The resting state of a panel column is one line and nothing else** — the whole of what
    /// James asked for, as an assertion rather than a screenshot.
    #[test]
    fn a_column_nobody_is_typing_in_floats_nothing_over_itself() {
        assert_eq!(overlay(None, "add | remove", false), Said::Nothing);
    }

    /// 🚨 **A refusal is shown whatever the focus is doing; candidates are not.** The asymmetry
    /// is the rule — errors always reach a person, offers only answer a hand already in the box —
    /// and it is the one thing about [`overlay`] that a reader might reasonably assume the other
    /// way round.
    #[test]
    fn a_refusal_outranks_the_candidates_and_survives_losing_focus() {
        assert_eq!(overlay(Some("no such panel"), "add | remove", true), Said::Refusal("no such panel"));
        assert_eq!(
            overlay(Some("no such panel"), "add | remove", false),
            Said::Refusal("no such panel"),
            "a refusal vanished because the pointer moved on"
        );
        assert_eq!(overlay(None, "add | remove", true), Said::Words("add | remove"));
    }

    /// An empty word list is nothing at all, never a bordered rectangle with no text in it —
    /// which reads as a rendering fault rather than as silence.
    #[test]
    fn nothing_to_offer_draws_no_plate() {
        assert_eq!(overlay(None, "", true), Said::Nothing);
        assert_eq!(overlay(None, "   ", true), Said::Nothing);
    }

    // -----------------------------------------------------------------------
    // #131 — parity with the composer's input affordances
    // -----------------------------------------------------------------------

    /// Two frames of empty input to take focus, which is what every driven test below starts on.
    fn focused(ctx: &egui::Context, reg: &Registry, lines: &mut Lines) {
        lines.line_mut(Region::Left).want_focus = true;
        for _ in 0..2 {
            lines.begin();
            frame(ctx, reg, lines, Vec::new(), 320.0);
        }
    }

    /// Type `s`, one character per frame, through the real widget.
    fn type_in(ctx: &egui::Context, reg: &Registry, lines: &mut Lines, s: &str) {
        for c in s.chars() {
            lines.begin();
            frame(ctx, reg, lines, typed(&c.to_string()), 320.0);
        }
    }

    fn press(ctx: &egui::Context, reg: &Registry, lines: &mut Lines, key: egui::Key) {
        lines.begin();
        frame(
            ctx,
            reg,
            lines,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            320.0,
        );
    }

    /// 🚨 **The report, as a test.** James: *"currently these do not auto-complete terms when we
    /// have specified them with a few characters."*
    ///
    /// Three keystrokes and the line is a whole command. `a` is the only action beginning with
    /// it, so it becomes `add ` — whose trailing space opens the panel ring; `s` narrows that
    /// ring to several panels and completes nothing; `u` leaves `surface` alone and the line
    /// finishes itself. **No Tab is pressed anywhere in this test**, which is the whole point:
    /// the machinery this control already had needed one, and a control with two verbs and one
    /// closed table almost never has anything to disambiguate.
    ///
    /// ⚠️ **Driven through real `egui::Event::Text` rather than asserted on the palette**, on
    /// `typing_survives_the_palette_opening`'s rule: the shipped code's palette narrowed
    /// correctly and its `Tab` accepted correctly, so a test that inspected either would have
    /// passed with the reported defect fully present.
    #[test]
    fn a_prefix_completes_itself_all_the_way_to_a_runnable_line() {
        let ctx = egui::Context::default();
        let reg = registry();
        let mut lines = Lines::default();
        focused(&ctx, &reg, &mut lines);
        type_in(&ctx, &reg, &mut lines, "a");
        assert_eq!(
            lines.line(Region::Left).text,
            "add ",
            "one unambiguous character did not finish the verb"
        );
        type_in(&ctx, &reg, &mut lines, "s");
        assert_eq!(
            lines.line(Region::Left).text,
            "add s",
            "`s` names several panels, so nothing may be chosen for the hand yet"
        );
        type_in(&ctx, &reg, &mut lines, "u");
        assert_eq!(
            lines.line(Region::Left).text,
            "add surface ",
            "a prefix that leaves one panel did not complete"
        );
        assert!(
            matches!(act(&reg, column(), &lines.line(Region::Left).text), Act::Run { .. }),
            "…and what it completed to must be a line Enter would run"
        );
    }

    /// 🚨 **The defect the report's own machinery was hiding: the caret did not follow a
    /// rewrite.** Measured on the shipped code — `add su`, Tab, then two characters produced
    /// **`add suXYrface `**, because egui keeps its cursor as an index of its own and a wholesale
    /// rewrite leaves it where the typing left it. That is `conversation_view`'s `/hxelp` bug
    /// arriving on the surface nobody had driven, and it is why *"Tab appears to work"* was not
    /// evidence that completing worked.
    ///
    /// ⚠️ **Both rewrite paths are checked**, because they are two different lines of code that
    /// each have to remember [`Line::want_caret`]: Tab's accept in [`consume_keys`], and
    /// [`self_complete`]. **Both mutations were run rather than assumed**: drop `want_caret` from
    /// the Tab arm and this fails with `add sXYurface ` (Tab is pressed on `add s`, so the caret
    /// is stranded at 5); drop it from [`self_complete`] and three tests fail, this one at
    /// `asdd ` — the caret left at 1 by `a` becoming `add `.
    #[test]
    fn the_caret_follows_a_line_the_console_rewrote() {
        // Tab's accept.
        let ctx = egui::Context::default();
        let reg = registry();
        let mut lines = Lines::default();
        focused(&ctx, &reg, &mut lines);
        // `add ` arrives by self-completion; `su` then narrows the panel ring to one, which
        // completes on its own — so Tab is driven from a line that is already whole, where the
        // remaining candidate list is the one the caret bug was measured against.
        type_in(&ctx, &reg, &mut lines, "as");
        assert_eq!(lines.line(Region::Left).text, "add s");
        press(&ctx, &reg, &mut lines, egui::Key::Tab);
        let after_tab = lines.line(Region::Left).text.clone();
        assert!(after_tab.starts_with("add "), "Tab took nothing: {after_tab:?}");
        type_in(&ctx, &reg, &mut lines, "XY");
        assert_eq!(
            lines.line(Region::Left).text,
            format!("{after_tab}XY"),
            "the caret stayed where the typing left it, so the next characters landed inside the \
             word Tab had just finished"
        );

        // …and the self-completion's own rewrite.
        let ctx = egui::Context::default();
        let mut lines = Lines::default();
        focused(&ctx, &reg, &mut lines);
        type_in(&ctx, &reg, &mut lines, "rZ");
        assert_eq!(
            lines.line(Region::Left).text,
            "remove Z",
            "a character typed after a self-completion landed inside the completed word"
        );
    }

    /// 🚨 **Backspacing out of a completed line, which is the whole reason the latch is shared.**
    ///
    /// The composer's worst defect was a completion undoing the deletion that provoked it —
    /// James: *"once I have typed slash surface, I am no longer able to backspace out of it."*
    /// `conversation_view::completion_held` is that fix, and this control now calls **it** rather
    /// than a copy: a second answer to *"may this frame complete?"* is the drift the crate's one
    /// -vocabulary rule exists to stop.
    ///
    /// ⚠️ **Two deletions, not one, and the second is the case a one-frame rule gets wrong.** The
    /// frame *after* a backspace changes nothing at all, so a rule that merely refused shrinking
    /// frames would complete on the next one and undo the deletion one frame late — the same bug
    /// at 60 Hz, presenting as a flicker. The latch is held until an insertion releases it, which
    /// the last leg here proves it still does.
    ///
    /// ⚠️ **Mutation-measured**: remove the guard at the top of [`self_complete`] and this fails
    /// on the *first* backspace — `add surface ` where `add surface` was asked for, the deletion
    /// put back on the frame it happened. Nothing else in the suite notices.
    #[test]
    fn a_deletion_is_never_undone_by_the_completion_that_produced_the_line() {
        let ctx = egui::Context::default();
        let reg = registry();
        let mut lines = Lines::default();
        focused(&ctx, &reg, &mut lines);
        type_in(&ctx, &reg, &mut lines, "asu");
        assert_eq!(lines.line(Region::Left).text, "add surface ");
        for expected in ["add surface", "add surfac", "add surfa"] {
            press(&ctx, &reg, &mut lines, egui::Key::Backspace);
            assert_eq!(
                lines.line(Region::Left).text,
                expected,
                "a backspace was put straight back by the completion it provoked"
            );
        }
        // A quiet frame must not complete either — the latch is held, not merely refused.
        lines.begin();
        frame(&ctx, &reg, &mut lines, Vec::new(), 320.0);
        assert_eq!(lines.line(Region::Left).text, "add surfa", "a still frame completed");
        // …and an insertion lets it go again.
        type_in(&ctx, &reg, &mut lines, "c");
        assert_eq!(
            lines.line(Region::Left).text,
            "add surface ",
            "typing after a deletion must complete again"
        );
    }

    /// 🚨 **The panel ring is the registry's, un-expanded and otherwise untouched.**
    ///
    /// This is what makes [`crate::registry::NarrowFn`] — the hook that lets one argument's
    /// options depend on an earlier word — reach this control for free: [`palette`] asks
    /// [`Registry::candidates`] and does nothing to the answer but rebuild each completion onto
    /// the typed stem. `console.stack` declares no narrowing today (only `console.layout` does),
    /// so there is no behaviour here to observe directly; what can be pinned is the property that
    /// would carry it, and this is it.
    ///
    /// ⚠️ **A second filter here is exactly what this forbids**, `REGION_ARG` excepted — that one
    /// is deliberate, argued in [`palette`], and asserted by
    /// [`the_supplied_region_keyword_is_never_offered`]. Anything else that narrowed the ring
    /// locally would be a second matching rule over one table, and a hook added to the registry
    /// would silently not apply here.
    #[test]
    fn the_argument_ring_is_the_registrys_own_so_a_narrowing_hook_would_reach_it() {
        let reg = registry();
        for line in ["add ", "add s", "add su", "remove a", "add surface region "] {
            let expanded = expand(line).expect("the fixture takes both actions");
            let inner = reg.candidates(&expanded).expect("an expanded line is a command line");
            let expected: Vec<&str> = inner
                .candidates
                .iter()
                .map(|c| c.label.as_str())
                .filter(|label| *label != REGION_ARG)
                .collect();
            let mine = palette(&reg, column(), line);
            let got: Vec<&str> = mine.candidates.iter().map(|c| c.label.as_str()).collect();
            assert_eq!(got, expected, "`{line}` was narrowed by something other than the registry");
        }
    }

    /// 🚨 **A control this narrow cannot autorun, and the classification is what says so** —
    /// not a switch, and not a preference. `console.stack` is [`Reversal::Permanent`], so
    /// [`Candidate::fires`] is false for every candidate reachable here and the composer's
    /// `palette_autorun` would decline every one of them even if it were wired in.
    ///
    /// ⚠️ Distinct from [`no_candidate_ever_fires`], which pins what this module *writes* into
    /// the field. This pins the *reason* it is safe to write it: the catalog agrees.
    #[test]
    fn nothing_this_control_can_reach_is_a_command_autorun_would_fire() {
        let stack = console()
            .into_iter()
            .find(|s| s.name == "console.stack")
            .expect("the fixture declares the verb this control expands onto");
        assert_eq!(
            stack.reversal,
            Reversal::Permanent,
            "if `stack` ever became recoverable, autorun becomes a decision rather than a fact"
        );
    }
}
