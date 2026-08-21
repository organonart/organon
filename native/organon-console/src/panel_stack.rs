//! **The panel stack — what a region holding `panel` actually holds.**
//!
//! [`crate::region`] divides the pane and says what each part is for; this answers what one of
//! those parts *contains* when the answer is `panel`: **a scrolling column of Organon's own
//! editor panels**, added and removed by verb. James's framing, 2026-08-20: *"instead of
//! popping up a panel right above us like we do now in the agent panel, we should be able to
//! pop up panels in one of the viewports we have assigned as a panel … so we could create our
//! own stacks that would scroll. And that means even if a viewport took up only the top left or
//! top right, we could still scroll many panels with the same scrolling mechanism."*
//!
//! # 🚨 The stack REMOVES the blocker; it does not work around it
//!
//! `CONSOLE_ARCHITECTURE.md` §2 recorded the obstacle to giving `panel` a body as *"a third
//! word naming which panel, since two rings cannot say it"* — `/viewport <region> <content>`
//! already spends both of its argument rings. A stack dissolves that: **the region and the
//! panel are named by different commands.** `/viewport left panel` declares the region;
//! `/organon look surface` (or `console stack add surface`) puts a panel in it. Neither
//! sentence ever needs three rings, and that is a property of the *split*, not of a longer
//! grammar. [`StackCmd`] owns why emptying the column rides the *panel* ring rather than being
//! a third action word.
//!
//! # 📌 Region size is independent of panel count
//!
//! One [`egui::ScrollArea`] per stack, sized to the region and nothing else. A top-left corner
//! scrolls twenty panels exactly as a full-height column does, which is what makes assigning a
//! small region worth doing at all.
//!
//! # 🚨 A stack PER REGION — and the two reasons that used to say otherwise
//!
//! ✏️ **This section said the opposite until #98 Tier C, and both of its reasons are now
//! answered rather than merely overruled.** It is rewritten in place rather than annotated,
//! because a header asserting a constraint the code no longer has is worse than one that is
//! merely out of date: somebody reads it and declines to do the thing that already works.
//!
//! **Reason (a), the mechanical one, is dissolved.** It read: *"the add verb has two rings
//! (`<action> <panel>`) and no room for a region word, so a per-region stack would give every
//! region after the first a column nothing can ever put anything into."* Tier C's command line
//! makes the region **context** — `/add surface` typed inside a region names that region by
//! being typed there — and the one dispatch path carries it as [`REGION_ARG`], an *optional*
//! word rather than a third required ring. Every column is now fillable, so no arm is unreachable.
//!
//! **Reason (b) conflated two objects, and only one of them was ever really console-wide.**
//! It read: *"two `panel` regions are two views of one instrument"*, on
//! `panel_surface::OrganonPanels`' precedent (§1.11, *"one mirror per console, not one per
//! element"*). 🚨 **The mirror and the composition are different things.** The *mirror* — the
//! `PresetValues` a control writes, which is what reaches `Shared` — genuinely stays **one per
//! console**, and must: two mirrors would be two claims about one instrument's state. The
//! *composition* — which panels a column lists, and in what order — is a property of the
//! **column**, not of the instrument, and there was never an argument that two columns must
//! list the same panels. Tier A could not tell them apart because there was only one column.
//!
//! 📌 **Measured rather than argued.** James built the four-region layout on a running console
//! on 2026-08-20 (`left` panel, `topcenter` 3d, `right` panel, `bottomcenter` agent), typed one
//! `stack add surface`, and both columns rendered an identical `organon · look · Surface`. His
//! verdict: *"you can see why we need to have the update for the stacks because it addressed
//! both of them."* One stack, two views, is a thing somebody has now looked at — and it is not
//! what a person assembling two control columns means.
//!
//! ⚠️ **Two copies of one panel still share the instrument, and that is unchanged.** Surface in
//! the left column and Surface in the right column drive the same parameters through the same
//! mirror; what differs is only which panels each column *holds*. So `/organon`'s claim — *this
//! is the same panel* — survives verbatim.
//!
//! ⚠️ The **scroll position** was already per-region (the [`egui::ScrollArea`] is keyed by the
//! region) and still is. It is now the smaller half of the per-region story rather than the
//! whole of it, and it is still why the id namespace below has to carry the region.
//!
//! # 🚨 A command that names no region still has to land somewhere
//!
//! Three of §1.8's four front doors have **no region to be typed into**: the CLI's
//! `organon console stack add surface`, the agent's MCP tool, and a `/organon look surface`
//! typed at a conversation. [`Home`] is their destination rule — **the first region holding
//! `panel` in [`Region::ALL`] order**, largest first, the same determinism that already decides
//! which `agent` region gets the live tab — and with several columns it is doing real work
//! rather than merely naming the one that exists. [`resolve_target`] is the one place the two
//! cases meet: a named region is checked against the layout and used, an unnamed one falls to
//! [`Home`], and both refusals name what would have worked.
//!
//! # 🚨 The id namespace: WHERE it is drawn, never WHAT is drawn
//!
//! §1.11 records this bug being fixed twice, and a stack is the same case from a third
//! direction. `organon_element` scoped its widgets by the panel's **slug**, which separates two
//! *different* panels (which could never have collided) while merging two elements of the
//! **same** panel; and the typed-value box's key was absolute (`Id::new("om_value_edit")` plus
//! the param pointer), so clicking one value box opened a text field in both. Both fixes say
//! the same thing: **an Organon panel's egui namespace is its position on screen, not its
//! identity.**
//!
//! So [`draw`] pushes `(STACK_ID, region.as_word())` and then, per panel, the entry's
//! [`Entry::serial`] — a number issued once per push and **never reused**, so removing the
//! third panel does not hand its widget state to the fourth. The pair `(region, serial)` is
//! unique across every panel this console draws, and it is pushed *inside this function* rather
//! than inherited from whatever `Ui` the caller supplies — which is what makes the property
//! testable here instead of resting on a salt in `console_main.rs`.
//!
//! # 📌 [`draw`] takes its target, and that is a spelling choice made on purpose
//!
//! It paints into the [`egui::Ui`] it is handed, through that `Ui`'s own painter, and never
//! reaches for the context, a named layer or the window. James's standing note is that *"the
//! entire surface of the console is still itself a 3D surface that can have physically based
//! rendering applied to it"* (organon#17), and `egui → texture` is the half the console does
//! **not** have today — unlike `World → texture`, which the backdrop, the portal and
//! `/surface` all use. 🚨 **Nothing here is built toward that**: no offscreen path, no texture
//! per region, no producer machinery, because a texture and a copy per region per frame is a
//! real cost for a capability nothing uses, and an arm nothing can select is an untested branch
//! pretending to be a design. All this buys is that a stack does not *assume* the window.
//!
//! # What is NOT here
//!
//! No `Console` state and no parameters. A panel's **body** arrives through [`OrganonDraw`],
//! exactly as it did when a panel was an element in the transcript: this crate knows a panel by
//! its tab, slug and title and cannot see `OrganicMathParams`, a `ParamSetter` or a `World`.

use egui::{Frame, RichText};
use organon_core::panels::{self, Panel};

use crate::posture::Form;
use crate::region::{Content, Layout, Region, REGION_COUNT};
use crate::theme::Theme;

/// **How the console draws one of Organon's own panels** — the seam between the two crates.
///
/// 🚨 **A callback, not a render list, and the difference is forced.** A *surface* is a
/// picture: the view says what it laid out, `console_main` renders into a texture and the
/// answer arrives next frame — deferral costs one frame of "rendering…". A panel is *widgets*:
/// a dropdown must open where it was clicked and a drag must be read in the pass it was drawn.
/// There is no texture to hand back later, so the console's drawing happens **inside** this
/// crate's layout, at the point in the column the panel occupies.
///
/// ⚠️ It moved here from `conversation_view` when the transcript stopped being a home for
/// panels; the contract kept its shape through that move and changed *width* in #117.
///
/// # 🚨 It hands over the whole CARD now, not only the body — and that is the tier
///
/// James, looking at this column beside Organon's own editor stack: *"I want us to adopt the
/// styling … so that it looks just like it does here in terms of the padding and the fact that
/// it just has the one word for the panel. **In fact, it should use the same exact code
/// somehow.**"*
///
/// The chrome he is pointing at is `lib.rs`'s `card()` — `theme::framed`, the three-stop silver
/// header band, an `egui::CollapsingHeader` — and it lives in the **root** crate, which this one
/// cannot depend on (`doc/arch/topology.md`: the root crate depends on *this*). There are two
/// ways to run one copy of that code: move the chrome down into a crate both can see, or widen
/// this seam so the crate that already owns the chrome draws it. There is nowhere to move it —
/// `organon-core` is host-free *by acceptance test* (`cargo tree -p organon-core` must show no
/// egui) and `theme.rs` reaches `nih_plug_egui`, `theme_config` and the paint helpers. So the
/// seam widened.
///
/// The callback is therefore handed **every** panel in the column, `Declared` ones included, and
/// draws the card around whatever it decides to put inside. [`NOT_TRANSPLANTED`] is still this
/// crate's sentence — the caller reads the constant and places it *inside* the card, which is
/// what stops twenty-four of the twenty-five panels from being the ones with different chrome.
///
/// # ⚠️ The `bool` is not decoration
///
/// `false` means **this build drew no card**: every test in this crate, and any front-of-house
/// with no Organon editor behind it. The stack then draws [`plain_card`], which is the frame and
/// heading it drew before this widened. Without that answer an inert callback would render a
/// column of nothing, and a panel that is in the column but invisible is precisely the failure
/// [`NOT_TRANSPLANTED`] exists to prevent, one level up.
pub type OrganonDraw<'a> = &'a mut dyn FnMut(&mut egui::Ui, &'static Panel) -> bool;

/// One panel in the column, and the number that gives it its egui namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    serial: u64,
    panel: &'static Panel,
}

impl Entry {
    /// The number this entry was issued when it was pushed. **Never reused within a stack** —
    /// see the module header on why that is what keeps two panels' widget state apart.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn panel(&self) -> &'static Panel {
        self.panel
    }
}

/// **A column per region** — the console's panel stacks, indexed by [`Region::slot`].
///
/// 🚨 **An array over the whole vocabulary, not a map of the occupied ones**, which is
/// [`crate::region::Layout`]'s own arrangement for its reason: a region that stops holding
/// `panel` and is given it back should find its column where it left it, and a map keyed by
/// what is *currently* held would throw the contents away on the way through `off`. ⚠️ The
/// consequence is stated rather than hidden: a column belonging to a region that holds nothing
/// is **kept and not drawn**, so `viewport left off` then `viewport left panel` restores the
/// column. Nothing empties a column but `stack remove`.
///
/// ⚠️ **Serials are per column, and the egui key is `(region, serial)`** — see [`draw`]. Two
/// columns each issuing serial 0 is therefore not a collision, and never was: the region half
/// of the key is what separates them, which is exactly what
/// `the_region_and_the_serial_are_both_doing_work` pins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stacks {
    columns: [Stack; REGION_COUNT],
}

impl Stacks {
    /// The column this region holds. Every region has one; most are empty.
    pub fn get(&self, region: Region) -> &Stack {
        &self.columns[region.slot()]
    }

    pub fn get_mut(&mut self, region: Region) -> &mut Stack {
        &mut self.columns[region.slot()]
    }

    /// How many panels are in every column together. **Counted, never tracked** — a second
    /// number kept in step with the arrays is how the two come to disagree.
    pub fn total(&self) -> usize {
        self.columns.iter().map(Stack::len).sum()
    }
}

/// Which column a stack command edits, or why none will do.
///
/// 🚨 **The one place a named region and an unnamed one meet.** Three of the four front doors
/// (§1.8) cannot name a region — a CLI line, an agent's tool call and a `/organon` typed at a
/// conversation are all region-less — and Tier C's region command line always can. Resolving
/// both here rather than at each caller is what stops the destination rule from being spelled
/// twice and drifting.
///
/// - `Some(region)` that holds `panel` → that column.
/// - `Some(region)` holding anything else → [`Refusal::NotAPanelRegion`], naming what it holds
///   and which regions do hold a stack. ⚠️ **Refused rather than redirected**: putting a panel
///   in a column somebody did not name is the "refused, not clamped" rule (§1.3) at the scale
///   of a rectangle.
/// - `None` → [`Home::of`], i.e. the first `panel` region in [`Region::ALL`] order, or
///   [`Refusal::NoRegion`] when no region holds one at all.
pub fn resolve_target(layout: &Layout, named: Option<Region>) -> Result<Region, Refusal> {
    let Some(region) = named else {
        return match Home::of(layout) {
            Home::Shown(region) => Ok(region),
            Home::Nowhere => Err(Refusal::NoRegion),
        };
    };
    match layout.get(region) {
        Some(Content::Panel) => Ok(region),
        held => Err(Refusal::NotAPanelRegion {
            region: region.as_word().to_string(),
            holding: held.map_or("nothing", Content::as_word).to_string(),
            panel_regions: word_list(&layout.regions_holding(Content::Panel)),
        }),
    }
}

/// A region list for a refusal to quote — `"nothing"` rather than an empty string, so the
/// sentence reads as a sentence. [`crate::region`]'s own arrangement in `held_panel_slugs`.
fn word_list(regions: &[Region]) -> String {
    if regions.is_empty() {
        return "nothing".to_string();
    }
    regions.iter().map(|r| r.as_word()).collect::<Vec<_>>().join(", ")
}

/// One scrolling column of Organon panels — what **one region** holding `panel` holds.
///
/// ⚠️ **Deliberately uncapped.** A cap would need a refusal, and the refusal could not reach
/// the person who caused it: `/organon look surface` is answered in a conversation pane one
/// frame *before* the console applies the push, so a stack that filled up would answer "added"
/// and then quietly not add. A long column is recoverable with one word (`stack clear`); a
/// receipt that lies is not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stack {
    entries: Vec<Entry>,
    /// The next serial to issue. Monotonic for the life of the console — it is **not** the
    /// length, because a removal must not let the next push inherit a departed panel's id.
    next_serial: u64,
}

impl Stack {
    /// The column, top to bottom. The order panels were added in — nothing reorders.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Put a panel at the bottom of the column and answer the serial it was given.
    ///
    /// **Duplicates are allowed on purpose.** Two Surface panels in one column are two views of
    /// one instrument (§1.11), which is a thing somebody comparing two disclosure states
    /// genuinely wants — and refusing it would be a rule about the *panel* where every other
    /// rule here is about the *column*.
    pub fn push(&mut self, panel: &'static Panel) -> u64 {
        let serial = self.next_serial;
        self.next_serial += 1;
        self.entries.push(Entry { serial, panel });
        serial
    }

    /// Take out the **last** panel with this slug, or `None` if the column has none.
    ///
    /// Last rather than first, because the gesture a person means by `stack remove surface` is
    /// *undo the one I just added* — and with duplicates allowed, the one they just added is
    /// the one at the bottom.
    pub fn remove_last(&mut self, slug: &str) -> Option<Entry> {
        let at = self.entries.iter().rposition(|e| e.panel.slug.eq_ignore_ascii_case(slug))?;
        Some(self.entries.remove(at))
    }

    /// Empty the column. The serial counter is **not** rewound — see [`Stack::next_serial`].
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// What `console stack <action> <panel>` asks for.
///
/// **Two words, both required, both a closed `Choice`** — `/viewport`'s shape exactly, and the
/// reason nothing here ever needs a third ring.
///
/// 🚨 **Why "clear" is not a third action word, and this is not ceremony.** The slash grammar
/// fills *required* arguments positionally and *optional* ones by keyword
/// (`registry::parse_args`), so an optional trailing panel would make the typed line
/// `/stack add panel surface` while the CLI stayed `organon console stack add surface` — one
/// verb with two spellings, which is the drift this tree spends most of its refusals
/// preventing. Both words are therefore required, and emptying the column rides the **panel**
/// ring as [`ALL_WORD`]. That is `region::CLEAR_WORD`'s own arrangement one module over: a
/// clearing word travelling in the same argument as the real values, on the precedent
/// `console.background`'s three backdrop *sources* set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackCmd {
    /// Add this panel to the bottom of the column.
    Add(&'static Panel),
    /// Take out the last copy of this panel.
    Remove(&'static Panel),
    /// Empty the column — `stack remove all`.
    Clear,
}

/// The action words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`'s possible-values parser, by the console's command schema
/// and by [`StackCmd::resolve`]'s refusal — [`crate::region::REGION_WORDS`]' arrangement, for
/// its reason: a second hand-maintained copy is how a CLI comes to accept a word nothing can
/// act on.
pub const STACK_ACTIONS: &[&str] = &["add", "remove"];

/// The word that means *the whole column* in the panel ring. See [`StackCmd`] on why it lives
/// there rather than being a third action.
///
/// ⚠️ **It is a word, not a panel**, exactly as `region::CLEAR_WORD` is a word and not a
/// content kind: no [`Entry`] ever holds it, and giving [`StackCmd`] an `Add(all)` arm would put
/// a value in the enum the draw path must then match and refuse to draw.
pub const ALL_WORD: &str = "all";

/// The **optional** third slot on the stack verb: which region's column to edit.
///
/// 🚨 **Optional, and that is the whole of why it is not the "third ring" this module's header
/// used to refuse.** The two required rings are unchanged, so `stack add surface` still means
/// exactly what it meant — the destination rule answers, as it must for the CLI and the MCP
/// door, neither of which has a region to be typed into. What the word adds is a way for the
/// **one** dispatch path to carry a context the region command line already knows.
///
/// ⚠️ **Nobody is expected to type it.** `registry::parse_args` fills optional arguments by
/// keyword, so the typed spelling is `/stack add surface region left` and the CLI's is
/// `organon console stack add surface --region left` — both writing the same sidecar line, which
/// is the property that keeps the four doors one vocabulary. In a region's own line you type
/// `/add surface` and the line supplies this word.
pub const REGION_ARG: &str = "region";

/// The panel ring's whole value space: every slug, then [`ALL_WORD`].
///
/// Built from `panels::slugs()` rather than restated, so a panel added to Organon's table
/// reaches the MCP schema, the slash palette and the CLI's `--help` in the commit that adds it.
///
/// ⚠️ **`&'static str`, not `String`, and that is not tidiness.** Both halves already are
/// `'static` — the slugs are `pub const`s in Organon's table and [`ALL_WORD`] is one here — and
/// clap's `PossibleValuesParser` converts from `&'static str` but **not** from an owned
/// `String`, so answering `Vec<String>` would make `bin/ctl.rs` either leak each word or
/// restate the list. The schema side converts once at its own call site, where an owned
/// `String` is what `ArgKind::Choice` wants anyway.
pub fn panel_words() -> Vec<&'static str> {
    let mut out = panels::slugs();
    out.push(ALL_WORD);
    out
}

impl StackCmd {
    /// The action word this command travels as.
    pub fn action_word(&self) -> &'static str {
        match self {
            StackCmd::Add(_) => "add",
            StackCmd::Remove(_) | StackCmd::Clear => "remove",
        }
    }

    /// The panel word this command travels with.
    pub fn panel_word(&self) -> &'static str {
        match self {
            StackCmd::Add(p) | StackCmd::Remove(p) => p.slug,
            StackCmd::Clear => ALL_WORD,
        }
    }

    /// Read an action word and a panel word, or say why neither will do.
    ///
    /// ⚠️ **Exact, never approximated** — [`crate::region::Region::resolve`]'s rule. The one
    /// place case is folded is the *panel slug*, because `panels::find` already folds it and
    /// two spellings of one lookup is how they come to disagree.
    pub fn resolve(action: &str, panel: &str) -> Result<Self, Refusal> {
        if !STACK_ACTIONS.contains(&action) {
            return Err(Refusal::UnknownAction { word: action.to_string() });
        }
        if panel.eq_ignore_ascii_case(ALL_WORD) {
            // 🚨 **Refused by name rather than quietly adding twenty-five panels.** `all` is the
            // clearing word; reading it as "every panel in the table" would fill the column from
            // a word a person typed meaning the opposite, and this lane has no undo but the
            // command itself.
            return match action {
                "remove" => Ok(StackCmd::Clear),
                _ => Err(Refusal::AllIsNotAnAddition),
            };
        }
        let resolved = panels::find_by_slug(panel).ok_or_else(|| Refusal::UnknownPanel {
            word: panel.to_string(),
            known: panels::slugs().join(", "),
        })?;
        Ok(match action {
            "add" => StackCmd::Add(resolved),
            // `remove` is the only other word `STACK_ACTIONS` holds. A catch-all rather than a
            // second literal, so a third action added to that table without a case here is a
            // wrong answer `every_action_word_resolves` catches, not a panic in the frame path.
            _ => StackCmd::Remove(resolved),
        })
    }
}

/// Why a stack command was refused. **Every arm names what was asked and what would have
/// worked** — a refusal that only says "no" is the defect this console keeps a tally of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    UnknownAction { word: String },
    /// A slug no panel in `organon_core::panels::PANELS` answers to.
    UnknownPanel { word: String, known: String },
    /// [`ALL_WORD`] given to `add`. See [`StackCmd::resolve`].
    AllIsNotAnAddition,
    /// `remove` of a panel the column is not holding.
    NotHeld { slug: String, held: String },
    /// `remove all` on a column that is already empty. [`crate::region::Refusal::AlreadyEmpty`]'s
    /// rule: a command that changes nothing and says nothing is indistinguishable from one
    /// that never arrived.
    AlreadyEmpty,
    /// 🚨 **No region holds a stack**, so there is nowhere for a panel to go. Named rather than
    /// silently dropped, and it carries the command that fixes it: this is the refusal a person
    /// meets the *first* time they type `/organon look surface`, and it is the whole of how
    /// they learn a region has to be declared first.
    NoRegion,
    /// A region was named and it does not hold `panel`. **Refused rather than redirected** —
    /// see [`resolve_target`].
    NotAPanelRegion { region: String, holding: String, panel_regions: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::UnknownAction { word } => write!(
                f,
                "`{word}` is not a stack action — known: {}",
                STACK_ACTIONS.join(", ")
            ),
            Refusal::UnknownPanel { word, known } => write!(
                f,
                "`{word}` is not one of Organon's panels — known: {known}, or `{ALL_WORD}`"
            ),
            Refusal::AllIsNotAnAddition => write!(
                f,
                "`{ALL_WORD}` is the word for the whole column, and this verb does not fill a \
                 column from one command — name a panel to add, or say `stack remove \
                 {ALL_WORD}` to empty it"
            ),
            Refusal::NotHeld { slug, held } => write!(
                f,
                "the panel stack is not holding `{slug}` — it holds: {held}"
            ),
            Refusal::AlreadyEmpty => write!(
                f,
                "the panel stack is already empty, so there is nothing to clear"
            ),
            Refusal::NoRegion => write!(
                f,
                "no region holds a panel stack, so there is nowhere to put a panel — \
                 `organon console viewport left panel` makes one (any region word will do), \
                 then ask again"
            ),
            Refusal::NotAPanelRegion { region, holding, panel_regions } => write!(
                f,
                "`{region}` holds `{holding}`, not a panel stack — regions with a column: \
                 {panel_regions}"
            ),
        }
    }
}

/// Which region's stack a panel summoned **without a region word** lands in.
///
/// ✏️ **Doing real work now that there are several columns.** With one console-wide stack this
/// only decided which region's *name* an answer quoted; it now decides which column is written
/// to, for every door that has no region to be typed into — see [`resolve_target`].
///
/// 🚨 **A panel lives only in a stack; there is no transcript home.** James, 2026-08-20:
/// *"Would we ever want a panel inline? A panel should not scroll away. That doesn't make
/// sense."* The sharp form is that **a transcript is a log and a control is not a log entry** —
/// a panel is used while watching its result, and a control that scrolls off mid-drag was never
/// usable. The inline route was the answer to "where does a panel go" before regions existed;
/// [`Home::Nowhere`] is refused by name rather than falling back to it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Home {
    /// No region holds `panel`. `/organon` refuses with [`Refusal::NoRegion`].
    #[default]
    Nowhere,
    /// The region a summoned panel goes to. **The first region holding `panel` in
    /// [`Region::ALL`] order**, which is largest-first — the same determinism `console_main`
    /// already uses to decide which `agent` region gets the live tab. Carried so the answer can
    /// *say* which region it used — which is now the only way a person can tell, since the
    /// several columns no longer read alike.
    Shown(Region),
}

impl Home {
    /// Where a panel named without a region goes, given what the console is holding.
    ///
    /// ⚠️ **Largest-first is the whole rule, and it is deliberately not "the nearest" or "the
    /// last one you typed at".** Both of those are guesses about intent that a fire-and-forget
    /// lane has no way to check; `Region::ALL` order is a fact about the vocabulary, and the
    /// answer quotes the region so a person can see where it went.
    pub fn of(layout: &Layout) -> Self {
        match layout.region_holding(Content::Panel) {
            Some(region) => Home::Shown(region),
            None => Home::Nowhere,
        }
    }
}

/// The heading one panel wears, wherever it is drawn: **its own name, and nothing else.**
///
/// ✏️ **It said `◈ organon · look · Surface` until #117.** James, holding this column up against
/// Organon's own editor stack: *"it just has the one word for the panel and not all of the words
/// you have now."* Organon's card says `Surface`, so this does.
///
/// 🚨 **The breadcrumb was answering a question this surface no longer asks.** It was written
/// when a panel was an *element in a transcript*, scrolling past between an agent's messages —
/// where "which of Organon's tabs is this from" is a real question, because nothing around the
/// card says. In a panel column every card is one of Organon's, the column is the answer, and
/// repeating it on all twenty-five headings is the console explaining itself to somebody who
/// built it.
///
/// ⚠️ **Still a function, and still `pub`.** It is a one-liner over a field, which looks like
/// something to inline — but it is the one place that decides what a panel is *called on
/// screen*, it is pinned by a test, and the caller that matters is `console_main`, which passes
/// the same value to Organon's own `card()`. A second surface that ever draws a panel asks here
/// rather than reaching for a field and quietly picking a different one.
pub fn heading(panel: &Panel) -> &'static str {
    panel.title
}

/// What a [`panels::Status::Declared`] panel says where its controls would be.
///
/// ⚠️ **It says so rather than drawing an empty box**, for `Ring::Empty`'s reason one scale up:
/// a panel that opened to nothing at all would be indistinguishable from one that failed. Only
/// Look ▸ Surface is `Live` today and the other twenty-four earn this line.
///
/// ✏️ **Three words, where it was a sixteen-word sentence.** The sentence explained the
/// console's own construction — *"named in Organon's editor but has not been transplanted into
/// the console yet"* — to a reader who is doing the transplanting; and with twenty-four of the
/// twenty-five panels `Declared`, it was that explanation repeated two dozen times down a 320 pt
/// column. What a person needs from the card is the **fact**: this panel is real, and its
/// controls are not here. That is what survives. The empty-box hazard above is unchanged — the
/// card still says something rather than nothing, which is the whole reason the constant exists.
pub const NOT_TRANSPLANTED: &str = "no controls yet";

/// The gap under a [`plain_card`], in points.
///
/// ✏️ **It used to be the gap between *every* two panels, added by [`draw`].** It became the
/// fallback's own trailing space in #117, because Organon's `card()` adds its own after each
/// card and a stack that added a second one would have been twice the editor's — which is
/// exactly the padding James asked to match.
///
/// ✏️ **6.0 → 0.0 in #120**, following the card it exists to look like: `card()`'s trailing
/// space in a Console column is `panel_surface::PANEL_COLUMN_GAP`, which is now zero, and this
/// fallback is drawn in the same column beside cards that use it.
///
/// ⚠️ **`pub` so the agreement can be *asserted* rather than described.** The root crate's
/// number is not visible from here and a dependency on it would point the wrong way through the
/// graph — but the root crate can see both, and
/// `panel_surface::the_fallback_leaves_the_same_gap_organons_card_does` compares them. A
/// constant two crates keep equal by comment is a constant that stops being equal.
pub const GAP: f32 = 0.0;

/// The namespace every stacked panel hangs under. A constant rather than a literal at the
/// `push_id` site so the test below is naming the same thing the draw path is.
const STACK_ID: &str = "organon-panel-stack";

/// Draw the column into `ui`, scrolling.
///
/// 🚨 **The id pushes are the point of this function existing** — see the module header. The
/// caller's `Ui` is not trusted to separate anything: `(STACK_ID, region.as_word())` then
/// `entry.serial()` is pushed here, so two regions showing the same stack, and two copies of
/// one panel inside a stack, are all independently operable.
///
/// ⚠️ **`auto_shrink` is off on both axes**, which is what makes the region's size independent
/// of the panel count: the scroll area fills the rectangle it was given whether it holds one
/// panel or twenty.
///
/// # 🚨 The column owns its own half of the seam between two cards (#120)
///
/// A card's trailing space is not the whole gap. egui inserts `item_spacing.y` between the
/// entries of a vertical layout as well, and **the two surfaces that draw this card run
/// different ones** — Organon's editor sets 6 (`theme::install` → `apply_style`), the Console
/// never touches spacing and takes egui's default 3. So the card's own constant cannot decide
/// the gap on its own, and a stack that left `item_spacing` alone would have had a floor of
/// 3 pt no card setting could reach.
///
/// The loop therefore zeroes it and **each card restores it inside its own scope**, which is
/// what keeps this from being a change to the panels: the rows *within* a card — labels,
/// sliders, value boxes — space themselves exactly as they did, because the child `Ui` is
/// handed back the spacing its parent had before the loop touched it. Only the distance
/// *between* two cards moves, and it is then exactly what the card leaves under itself.
pub fn draw(
    ui: &mut egui::Ui,
    region: Region,
    stack: &Stack,
    theme: &Theme,
    form: &Form,
    organon: OrganonDraw,
) {
    ui.painter().rect_filled(ui.max_rect(), 0.0, theme.panel_fill);
    ui.push_id((STACK_ID, region.as_word()), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let inherited = ui.spacing().item_spacing;
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in stack.entries() {
                ui.push_id(entry.serial(), |ui| {
                    ui.spacing_mut().item_spacing = inherited;
                    card(ui, entry.panel(), theme, form, organon)
                });
            }
        });
    });
}

/// One panel in the column — **Organon's own card if this build has one, else [`plain_card`].**
///
/// ✏️ **The whole card is the caller's now.** This used to draw the frame and heading here and
/// call down only for the body; [`OrganonDraw`] carries why that changed and why it could not be
/// done the other way round.
///
/// 🚨 **No `Status` match here any more, and that is deliberate rather than an omission.** The
/// caller draws the card, so the caller is the only place that can decide what goes *inside*
/// it — and a status test on this side would be a second copy of that decision, differing from
/// the first the day a twenty-sixth panel goes `Live`. [`plain_card`] keeps the test, because
/// there the console really is the one drawing.
fn card(
    ui: &mut egui::Ui,
    panel: &'static Panel,
    theme: &Theme,
    form: &Form,
    organon: OrganonDraw,
) {
    if organon(ui, panel) {
        return;
    }
    plain_card(ui, panel, theme, form);
}

/// What a panel looks like with **no Organon behind the console** — a frame, the panel's name,
/// and the sentence [`NOT_TRANSPLANTED`] when there is nothing to put in it.
///
/// ⚠️ **Reachable in this crate's tests and nowhere in `console_main`, and it still has to be
/// right.** `organon-console` is a library: it compiles, draws and is tested without the root
/// crate, and the alternative to this function is a column that renders as blank space whenever
/// the seam is unfilled. That is the same failure `NOT_TRANSPLANTED` answers one level up —
/// a panel that opened to nothing would be indistinguishable from one that failed.
fn plain_card(ui: &mut egui::Ui, panel: &'static Panel, theme: &Theme, form: &Form) {
    let mut framed = Frame::new()
        .fill(theme.panel_fill)
        .corner_radius(form.card_corner())
        .inner_margin(form.card_margin());
    if let Some(stroke) = form.card_stroke(theme.panel_edge) {
        framed = framed.stroke(stroke);
    }
    framed.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(heading(panel)).monospace().strong().color(theme.panel_title));
        if let Some(absent) = absent_body(panel) {
            ui.label(RichText::new(absent).color(theme.dim).italics());
        }
    });
    ui.add_space(GAP);
}

/// What a panel says where its controls would be, or `None` when it has controls to draw.
///
/// Pure, and split out of the drawing, so the rule is checkable without an `egui::Ui` — and so
/// `console_main` can ask the same question when it fills Organon's card, instead of writing a
/// second `match` on [`panels::Status`] that would answer differently the day a twenty-sixth
/// panel goes `Live`.
pub fn absent_body(panel: &Panel) -> Option<&'static str> {
    match panel.status {
        panels::Status::Live => None,
        panels::Status::Declared => Some(NOT_TRANSPLANTED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Content, ContentCmd, Layout};

    fn surface() -> &'static Panel {
        &panels::LOOK_SURFACE
    }
    /// A panel this build cannot draw a body for — the fallback card's subject.
    ///
    /// 🚨 **Re-derived from the table, never named.** It was `&panels::LOOK_BLOOM`, and the
    /// day Bloom was transplanted three tests here failed for a reason that had nothing to do
    /// with what they assert: they wanted *a* `Declared` panel and had been handed *that*
    /// one. Asking the table for the first one it still calls `Declared` keeps them about the
    /// fallback card as more panels go `Live` — and on the day none is left, `expect` says so
    /// out loud rather than quietly testing nothing.
    fn declared() -> &'static Panel {
        panels::PANELS
            .iter()
            .find(|p| p.status == panels::Status::Declared)
            .expect("every panel is transplanted — these fallback-card tests have no subject left")
    }

    #[test]
    fn a_new_stack_is_empty_and_a_push_puts_a_panel_at_the_bottom() {
        let mut stack = Stack::default();
        assert!(stack.is_empty());
        stack.push(surface());
        stack.push(declared());
        assert_eq!(stack.len(), 2);
        assert_eq!(
            stack.entries().iter().map(|e| e.panel().slug).collect::<Vec<_>>(),
            vec![surface().slug, declared().slug],
            "the column is in the order panels were added"
        );
    }

    /// 🚨 **The property the whole id namespace rests on.** A serial is issued once and never
    /// reused, so removing a panel cannot hand its egui widget state — an open dropdown, a
    /// half-typed value box, a drag in flight — to whatever is pushed next.
    #[test]
    fn a_serial_is_never_reused_after_a_removal() {
        let mut stack = Stack::default();
        let first = stack.push(surface());
        let second = stack.push(declared());
        assert_ne!(first, second);
        stack.remove_last(declared().slug);
        let third = stack.push(declared());
        assert_ne!(third, second, "the departed panel's serial came back");
        assert_ne!(third, first);
        stack.clear();
        let fourth = stack.push(surface());
        assert!(fourth > third, "clearing rewound the counter: {fourth} after {third}");
    }

    /// Two copies of one panel are two entries with two serials — see [`Stack::push`] on why
    /// duplicates are allowed at all.
    #[test]
    fn two_copies_of_one_panel_are_two_entries() {
        let mut stack = Stack::default();
        let a = stack.push(surface());
        let b = stack.push(surface());
        assert_ne!(a, b);
        assert_eq!(stack.len(), 2);
    }

    #[test]
    fn remove_takes_the_last_copy_and_answers_none_for_a_panel_not_held() {
        let mut stack = Stack::default();
        let first = stack.push(surface());
        stack.push(declared());
        let last = stack.push(surface());
        let gone = stack.remove_last("surface").expect("a surface was held");
        assert_eq!(gone.serial(), last, "the LAST copy came out, not the first");
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.entries()[0].serial(), first);
        assert!(stack.remove_last("kaleidoscope").is_none());
    }

    /// The whole action vocabulary, resolved against a real slug. ⚠️ Driven off
    /// [`STACK_ACTIONS`] rather than a list written here, so a third action cannot be added
    /// without this failing — `resolve`'s catch-all arm would otherwise answer `Remove` for it
    /// in silence.
    #[test]
    fn every_action_word_resolves_and_keeps_its_own_word() {
        assert_eq!(STACK_ACTIONS, &["add", "remove"], "the actions this test knows about");
        for action in STACK_ACTIONS {
            let cmd = StackCmd::resolve(action, "surface").expect("a real slug on a real action");
            assert_eq!(cmd.action_word(), *action);
            assert_eq!(cmd.panel_word(), "surface");
        }
    }

    /// 🚨 **The emptying word rides the panel ring**, and only one action may carry it — see
    /// [`StackCmd`] on why it is not a third action, and `resolve` on why `add all` is refused
    /// rather than read as "every panel".
    #[test]
    fn all_empties_the_column_on_remove_and_is_refused_on_add() {
        assert_eq!(StackCmd::resolve("remove", ALL_WORD), Ok(StackCmd::Clear));
        assert_eq!(StackCmd::resolve("remove", "ALL"), Ok(StackCmd::Clear));
        assert_eq!(StackCmd::resolve("add", ALL_WORD), Err(Refusal::AllIsNotAnAddition));
        // …and it survives the round trip, so the sidecar line a `Clear` writes is one this
        // vocabulary reads back as a `Clear`.
        let clear = StackCmd::Clear;
        assert_eq!(
            StackCmd::resolve(clear.action_word(), clear.panel_word()),
            Ok(StackCmd::Clear)
        );
    }

    /// The panel ring is the slug table plus the one word that is not a panel.
    #[test]
    fn the_panel_ring_is_organons_own_slugs_plus_the_clearing_word() {
        let words = panel_words();
        assert_eq!(words.last().copied(), Some(ALL_WORD));
        assert_eq!(words.len(), panels::slugs().len() + 1);
        assert!(words.contains(&"surface"));
        assert!(
            !panels::slugs().contains(&ALL_WORD),
            "`{ALL_WORD}` became a real slug — it can no longer be the clearing word"
        );
    }

    #[test]
    fn an_unknown_action_and_an_unknown_panel_are_refused_by_name() {
        let e = StackCmd::resolve("push", "surface").expect_err("`push` is not an action");
        assert!(e.to_string().contains("push"), "{e}");
        assert!(e.to_string().contains("add"), "the refusal lists what would work: {e}");
        let e = StackCmd::resolve("add", "nonesuch").expect_err("no such panel");
        assert!(e.to_string().contains("nonesuch"), "{e}");
        assert!(e.to_string().contains("surface"), "the refusal lists the slugs: {e}");
    }

    /// The slug lookup is case-folded exactly where `panels::find` is, and nowhere else.
    #[test]
    fn a_slug_resolves_regardless_of_case() {
        assert_eq!(StackCmd::resolve("add", "SURFACE"), Ok(StackCmd::Add(surface())));
    }

    /// 🚨 **Every refusal names the thing it is refusing.** The console's running tally of
    /// "it knew and said nothing" defects is long enough; this is the cheapest guard against
    /// adding to it.
    #[test]
    fn every_refusal_says_what_would_have_worked() {
        let cases = [
            Refusal::UnknownAction { word: "push".into() },
            Refusal::UnknownPanel { word: "nope".into(), known: "surface".into() },
            Refusal::AllIsNotAnAddition,
            Refusal::NotHeld { slug: "bloom".into(), held: "surface".into() },
            Refusal::AlreadyEmpty,
            Refusal::NoRegion,
            Refusal::NotAPanelRegion {
                region: "bottomcenter".into(),
                holding: "agent".into(),
                panel_regions: "left, right".into(),
            },
        ];
        for case in cases {
            let text = case.to_string();
            assert!(text.len() > 30, "a refusal that short cannot be naming anything: {text}");
            assert!(
                text.contains('`') || text.contains("stack"),
                "the refusal names nothing concrete: {text}"
            );
        }
    }

    /// `NoRegion` is the one refusal a person meets before they have learned the vocabulary, so
    /// it has to carry the command that fixes it verbatim.
    #[test]
    fn the_no_region_refusal_names_the_command_that_makes_one() {
        let text = Refusal::NoRegion.to_string();
        assert!(text.contains("viewport"), "{text}");
        assert!(text.contains("panel"), "{text}");
    }

    /// 🚨 **The destination rule, stated as a test.** The first region holding `panel` in
    /// `Region::ALL` order — largest first — is the one the answer names.
    #[test]
    fn the_home_is_the_first_panel_region_in_region_all_order() {
        assert_eq!(Home::of(&Layout::default()), Home::Nowhere, "a fresh console has no stack");

        let split = Layout::default()
            .assign(Region::Left, ContentCmd::Hold(Content::Agent))
            .expect("left agent")
            .layout;
        let one = split
            .assign(Region::Right, ContentCmd::Hold(Content::Panel))
            .expect("right panel")
            .layout;
        assert_eq!(Home::of(&one), Home::Shown(Region::Right));

        // Two regions holding `panel`, and the answer is the earlier of the two in
        // `Region::ALL` — `TopLeft` before `BottomLeft`, largest-first order.
        let corners = Layout::vacant()
            .assign(Region::TopRight, ContentCmd::Hold(Content::Agent))
            .expect("an agent first, or the last-agent rule refuses everything")
            .layout
            .assign(Region::TopLeft, ContentCmd::Hold(Content::Panel))
            .expect("topleft panel")
            .layout
            .assign(Region::BottomLeft, ContentCmd::Hold(Content::Panel))
            .expect("bottomleft panel")
            .layout;
        assert_eq!(Home::of(&corners), Home::Shown(Region::TopLeft));
    }

    /// 🚨 **One word, and it is the panel's own** — the heading Organon's editor draws over the
    /// same card. ✏️ This asserted `"◈ organon · look · Surface"` until #117; [`heading`] carries
    /// why the mark and the breadcrumb went.
    ///
    /// ⚠️ **Walked over the whole table rather than sampled**, because "one word" is a claim
    /// about every panel: the failure this forbids is a heading that quietly grows a prefix, and
    /// a prefix added to a shared formatter would show up on all twenty-five at once while a
    /// single-panel assertion caught it just as well. What it *cannot* catch — a `title` in the
    /// table that is itself two words — is not this module's to police; `organon_core::panels`
    /// owns those strings and the editor draws the same ones.
    #[test]
    fn a_panel_heading_is_its_editor_title_and_nothing_else() {
        assert_eq!(heading(surface()), "Surface");
        for panel in panels::PANELS {
            assert_eq!(
                heading(panel),
                panel.title,
                "`{}`'s heading is not simply its title",
                panel.slug
            );
        }
    }

    /// A panel with controls has nothing to say where they go; one without says so.
    #[test]
    fn only_a_declared_panel_carries_the_not_transplanted_sentence() {
        assert_eq!(surface().status, panels::Status::Live);
        assert_eq!(absent_body(surface()), None);
        assert_eq!(declared().status, panels::Status::Declared);
        assert_eq!(absent_body(declared()), Some(NOT_TRANSPLANTED));
    }

    // -----------------------------------------------------------------------
    // The id namespace, on a real headless egui context
    // -----------------------------------------------------------------------

    /// Draw `stacks` — one per region — in one frame, and answer the egui `Id` each panel's
    /// **card** was handed, in draw order.
    ///
    /// ⚠️ **`ui.id()` is what the caller actually gets**, and it is the same value
    /// `param_sink::value_box` folds into its typed-value key. Recording anything else would be
    /// testing a proxy for the property rather than the property.
    ///
    /// ✏️ **Every panel is offered now, `Declared` ones included** — see [`OrganonDraw`]. The
    /// closure answers `true`, i.e. it stands in for a build that *has* Organon behind it; the
    /// declining case is [`a_console_with_no_organon_behind_it_still_draws_the_panel`].
    fn body_ids(stacks: &[(Region, &Stack)]) -> Vec<egui::Id> {
        let ctx = egui::Context::default();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for (region, stack) in stacks {
                    // 🚨 **Every region gets a parent `Ui` with the SAME id salt**, which is
                    // the whole point of the arrangement: the parent contributes nothing that
                    // could tell two regions apart, so anything this test observes as separate
                    // was separated by [`draw`] itself. `console_main` does salt its child
                    // `Ui`s per region — and if that were what this rested on, the property
                    // would be pinned in another crate by a line nothing here can see.
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("one-parent-for-every-region")
                            .max_rect(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(400.0, 500.0),
                            ))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    draw(
                        &mut child,
                        *region,
                        stack,
                        &Theme::default(),
                        &Form::TERMINAL,
                        &mut |ui, _panel| {
                            sink.lock().expect("no panic in the closure").push(ui.id());
                            true
                        },
                    );
                }
            });
        });
        let out = seen.lock().expect("no panic").clone();
        out
    }

    /// 🚨 **THE test the third instance of the egui-id collision needed.** §1.11 records this
    /// bug being fixed twice; a stack makes it reachable again from two directions at once —
    /// **two regions showing one stack**, and **two copies of one panel inside a stack**. Every
    /// Surface body drawn in a frame must land in its own namespace, or a dropdown opened in
    /// one opens in all of them.
    ///
    /// ⚠️ **It is not enough that they draw.** What is asserted is the exact key
    /// `param_sink::value_box` builds — `ui.id().with("om_value_edit").with(<param ptr hash>)`,
    /// with the *same* pointer hash at every site, because there is one params instance behind
    /// every Surface panel. That is the collision that actually shipped once, and it is what
    /// "independently operable" means here.
    #[test]
    fn two_surface_panels_never_share_a_widget_namespace() {
        let mut left = Stack::default();
        left.push(surface());
        left.push(surface());

        // One stack, shown by two regions — the arrangement §1.14's "one stack, console-wide"
        // makes ordinary. Four Surface bodies in one frame, over one params instance.
        let ids = body_ids(&[(Region::Left, &left), (Region::Right, &left)]);
        assert_eq!(ids.len(), 4, "four panel bodies were drawn");

        let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
        assert_eq!(unique.len(), 4, "two panel bodies share a namespace: {ids:?}");

        // The real key, with one param pointer standing behind every copy.
        const PARAM: u64 = 0x0bad_c0de_dead_beef;
        let keys: std::collections::HashSet<_> =
            ids.iter().map(|id| id.with("om_value_edit").with(PARAM)).collect();
        assert_eq!(
            keys.len(),
            4,
            "two value boxes over one param share a slot — clicking one would open a text \
             field in both, which is exactly the bug §1.11 fixed twice"
        );
    }

    /// The counterpart of the test above, and the one that makes it a *mutation* test rather
    /// than an assertion that happens to hold: strip either push and the namespaces collapse.
    /// Two stacks whose entries carry the same serials, drawn under one region, must collide —
    /// which proves the region half of the key is doing work.
    #[test]
    fn the_region_and_the_serial_are_both_doing_work() {
        let mut a = Stack::default();
        a.push(surface());
        let mut b = Stack::default();
        b.push(surface());
        // Same serial (0) in both stacks, same panel: only the region differs.
        assert_eq!(a.entries()[0].serial(), b.entries()[0].serial());
        let ids = body_ids(&[(Region::Left, &a), (Region::Right, &b)]);
        assert_ne!(ids[0], ids[1], "the region is not part of the namespace");

        // …and within one region, only the serial differs.
        let mut two = Stack::default();
        two.push(surface());
        two.push(surface());
        let ids = body_ids(&[(Region::Left, &two)]);
        assert_ne!(ids[0], ids[1], "the serial is not part of the namespace");
    }

    // -----------------------------------------------------------------------
    // A column per region — #98 Tier C
    // -----------------------------------------------------------------------

    /// 🚨 **Two columns are two columns.** The whole of what Tier C changed here, stated as the
    /// thing James watched fail on a running console: one `stack add surface` filled both side
    /// columns, and it should fill one.
    #[test]
    fn each_region_has_its_own_column() {
        let mut stacks = Stacks::default();
        stacks.get_mut(Region::Left).push(surface());
        assert_eq!(stacks.get(Region::Left).len(), 1);
        assert_eq!(stacks.get(Region::Right).len(), 0, "one push filled two columns");
        stacks.get_mut(Region::Right).push(declared());
        assert_eq!(
            stacks.get(Region::Right).entries()[0].panel().slug,
            declared().slug,
            "the columns hold different panels"
        );
        assert_eq!(stacks.total(), 2);
    }

    /// Every region has a slot, and no two share one — the property the array rests on, walked
    /// over the whole vocabulary rather than sampled.
    #[test]
    fn every_region_addresses_a_column_of_its_own() {
        let mut stacks = Stacks::default();
        for (n, region) in Region::ALL.iter().enumerate() {
            for _ in 0..=n {
                stacks.get_mut(*region).push(surface());
            }
        }
        for (n, region) in Region::ALL.iter().enumerate() {
            assert_eq!(
                stacks.get(*region).len(),
                n + 1,
                "`{}` shares a column with another region",
                region.as_word()
            );
        }
        assert_eq!(stacks.total(), (1..=Region::ALL.len()).sum::<usize>());
    }

    /// ⚠️ **A column belongs to the region, not to what the region holds.** Clearing a region
    /// and giving it `panel` again finds the column where it was left — `Layout`'s own
    /// arrangement, and the reason `Stacks` is an array over the vocabulary rather than a map
    /// of the occupied regions.
    #[test]
    fn a_column_survives_its_region_being_emptied() {
        let mut stacks = Stacks::default();
        stacks.get_mut(Region::Right).push(surface());
        let layout = Layout::vacant()
            .assign(Region::Left, ContentCmd::Hold(Content::Agent))
            .expect("an agent first")
            .layout
            .assign(Region::Right, ContentCmd::Hold(Content::Panel))
            .expect("right panel")
            .layout;
        let emptied = layout.assign(Region::Right, ContentCmd::Clear).expect("off").layout;
        assert_eq!(resolve_target(&emptied, None), Err(Refusal::NoRegion));
        assert_eq!(stacks.get(Region::Right).len(), 1, "the column was thrown away");
        let back = emptied
            .assign(Region::Right, ContentCmd::Hold(Content::Panel))
            .expect("right panel again")
            .layout;
        assert_eq!(resolve_target(&back, None), Ok(Region::Right));
        assert_eq!(stacks.get(Region::Right).len(), 1);
    }

    /// 🚨 **The destination rule, with and without a region word.** Three of the four front
    /// doors cannot name one, so the unnamed case is not a fallback — it is what most commands
    /// take.
    #[test]
    fn a_named_region_is_used_and_an_unnamed_one_falls_to_the_home_rule() {
        let two = Layout::vacant()
            .assign(Region::BottomCenter, ContentCmd::Hold(Content::Agent))
            .expect("an agent first")
            .layout
            .assign(Region::Left, ContentCmd::Hold(Content::Panel))
            .expect("left panel")
            .layout
            .assign(Region::Right, ContentCmd::Hold(Content::Panel))
            .expect("right panel")
            .layout;
        // Unnamed: the first `panel` region in `Region::ALL` order, largest first.
        assert_eq!(resolve_target(&two, None), Ok(Region::Left));
        // Named: exactly what was named, whichever it is.
        assert_eq!(resolve_target(&two, Some(Region::Right)), Ok(Region::Right));
        assert_eq!(resolve_target(&two, Some(Region::Left)), Ok(Region::Left));
    }

    /// ⚠️ **Refused, never redirected.** Naming a region that holds something else is the
    /// "refused, not clamped" rule at the scale of a rectangle — and the refusal has to name
    /// what it holds *and* where the columns actually are, or it is a "no" with nothing to act
    /// on.
    #[test]
    fn a_region_that_holds_no_column_is_refused_by_name() {
        let split = Layout::vacant()
            .assign(Region::BottomCenter, ContentCmd::Hold(Content::Agent))
            .expect("an agent first")
            .layout
            .assign(Region::Left, ContentCmd::Hold(Content::Panel))
            .expect("left panel")
            .layout;
        let e = resolve_target(&split, Some(Region::BottomCenter))
            .expect_err("bottomcenter holds the agent");
        let text = e.to_string();
        assert!(text.contains("bottomcenter"), "{text}");
        assert!(text.contains("agent"), "the refusal does not say what it holds: {text}");
        assert!(text.contains("left"), "the refusal does not say where a column is: {text}");

        // …and a region holding nothing at all reads as `nothing` rather than as an empty gap
        // in the sentence.
        let e = resolve_target(&split, Some(Region::TopRight)).expect_err("topright is vacant");
        assert!(e.to_string().contains("nothing"), "{e}");
    }

    /// The refusal every door meets first: no region holds a column at all.
    #[test]
    fn a_console_with_no_column_refuses_by_name_whether_or_not_a_region_was_given() {
        let plain = Layout::default();
        assert_eq!(resolve_target(&plain, None), Err(Refusal::NoRegion));
        let e = resolve_target(&plain, Some(Region::Left)).expect_err("left holds nothing");
        assert!(e.to_string().contains("nothing"), "{e}");
    }

    /// An empty stack draws nothing and asks for no id at all — the region's own "empty" notice
    /// is what a person sees, and `console_main` owns that sentence.
    #[test]
    fn an_empty_stack_draws_no_bodies() {
        assert!(body_ids(&[(Region::Full, &Stack::default())]).is_empty());
    }

    /// 🚨 **A `Declared` panel is offered to the caller too, and that reverses this test.**
    ///
    /// ✏️ It read *"only the transplanted panel asked for a body"* and expected **1**. That was
    /// right while the console drew the card and asked down only for a body — a panel with no
    /// body had nothing to ask for. Now the caller draws the *card*, so it must be offered every
    /// panel or twenty-four of the twenty-five would wear the console's chrome while one wore
    /// Organon's, which is the opposite of what #117 is for. [`NOT_TRANSPLANTED`] has not moved;
    /// [`absent_body`] is what says which panels get it, and `console_main` places it inside the
    /// card it draws.
    #[test]
    fn every_panel_in_the_column_is_offered_to_the_caller_declared_ones_included() {
        let mut stack = Stack::default();
        stack.push(declared());
        stack.push(surface());
        assert_eq!(declared().status, panels::Status::Declared);
        assert_eq!(
            body_ids(&[(Region::Left, &stack)]).len(),
            2,
            "a declared panel was not offered a card"
        );
    }

    /// 🚨 **A console with an inert seam still draws a visible panel.** The `bool` in
    /// [`OrganonDraw`] is the whole of what stops a column from rendering as blank space when
    /// nothing behind it can draw a card, and a claim about pixels is worth asserting *on*
    /// pixels — so this reads the frame's own text shapes rather than a return value.
    ///
    /// ⚠️ **Mutation-checked rather than merely asserted**: make [`card`] return early on a
    /// `false` answer instead of falling through to [`plain_card`] and this fails on both
    /// counts, with `drawn` empty.
    #[test]
    fn a_console_with_no_organon_behind_it_still_draws_the_panel() {
        let mut stack = Stack::default();
        stack.push(declared());
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw(
                    ui,
                    Region::Left,
                    &stack,
                    &Theme::default(),
                    &Form::TERMINAL,
                    // The declining seam: this build has no Organon card to draw.
                    &mut |_ui, _panel| false,
                );
            });
        });
        let drawn = painted_text(&out);
        assert!(
            drawn.iter().any(|t| t.contains(heading(declared()))),
            "the fallback card drew no heading: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t.contains(NOT_TRANSPLANTED)),
            "the fallback card lost the sentence that says why it is empty: {drawn:?}"
        );
    }

    /// 🚨 **The column adds nothing between two cards** (#120), measured rather than argued.
    ///
    /// James, on the live build: *"we need to remove that spacing. See the dark spacing between
    /// them? We need to tighten it up and stack them more tightly together."* A card's own
    /// trailing space is the root crate's number; **this** is the other half — egui's
    /// `item_spacing.y` between the stack's entries, which the Console inherits from egui's
    /// default at 3 pt and which no card constant could have reached below.
    ///
    /// ⚠️ **The seam is measured between what the SEAM callback was handed**, not between
    /// painted rects, because the property is about layout rather than about pigment: each card
    /// allocates a known height and the distance between consecutive allocations is exactly what
    /// [`draw`] contributed. Delete the `item_spacing.y = 0.0` line and this reports 3.
    ///
    /// ⚠️ And the restoration is asserted in the same breath: the spacing a card sees *inside*
    /// its own scope is still the caller's, or tightening the column would silently have
    /// tightened every row inside every panel.
    #[test]
    fn the_column_contributes_no_space_between_two_cards() {
        let mut stack = Stack::default();
        stack.push(surface());
        stack.push(declared());
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(500.0, 900.0),
            )),
            ..Default::default()
        };
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(egui::Rect, f32)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        const BODY_H: f32 = 40.0;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // A caller with a spacing of its own, so "restored" means something other than
                // "happened to equal egui's default".
                ui.spacing_mut().item_spacing.y = 7.0;
                let mut seam = |ui: &mut egui::Ui, _panel: &'static Panel| {
                    let inside = ui.spacing().item_spacing.y;
                    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), BODY_H));
                    sink.lock().expect("no panic in the closure").push((rect, inside));
                    true
                };
                draw(ui, Region::Left, &stack, &Theme::default(), &Form::TERMINAL, &mut seam);
            });
        });
        let seen = seen.lock().expect("no panic").clone();
        assert_eq!(seen.len(), 2, "both panels should have been offered a card");
        let seam = seen[1].0.top() - seen[0].0.bottom();
        assert_eq!(
            seam, 0.0,
            "the column put {seam} pt between two cards; the card's own gap is the only seam"
        );
        for (i, (_, inside)) in seen.iter().enumerate() {
            assert_eq!(
                *inside, 7.0,
                "card {i} was drawn with the column's zeroed spacing instead of the caller's — \
                 every row inside every panel would tighten with it"
            );
        }
    }

    /// Every string this frame actually painted, in draw order.
    fn painted_text(out: &egui::FullOutput) -> Vec<String> {
        out.shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }
}
