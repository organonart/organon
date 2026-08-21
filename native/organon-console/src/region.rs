//! **Regions — how the console's one pane is divided, on an axis of its own.**
//!
//! [`crate::theme`] answers *what the console is made of*, [`crate::posture`] *how it stands*,
//! [`crate::screen`] *how much of the display it takes*. This answers a fourth question that is
//! none of those: **how the pane inside the window is divided, and what each part holds.**
//! James's words: *"split the viewports … into four or two and two or one on one side and two on
//! the other"*.
//!
//! # 🚨 Orthogonal to posture, which is the same argument [`crate::screen`] had to make
//!
//! [`crate::posture::Posture`] is a **scalar** — `Form::at(t)` lerps componentwise and
//! `Posture::from_scalar` takes anything between the ends — so there are no slots to add a
//! third to. And a division of the pane is not a point on that axis anyway: every one of
//! `Form`'s tokens is a margin, a corner, a padding, a line height, a gap, a tracking, or the
//! presence of a border, and **a split changes none of them**. It changes how many rectangles
//! there are.
//!
//! It passes §1.6's own orthogonality test verbatim: a **split terminal-posture console** and a
//! **split desktop-posture** one are both real, and neither is a variant of the other. So this
//! is a fourth orthogonal state, exactly as the screen was a third.
//!
//! # 🚨 Flat, never nested — a region holds one thing and never splits again
//!
//! [`Region`] is twelve words over a 3×2 grid, not a tree. The tree is the obvious model and it
//! is the wrong one here for a reason that is about the *vocabulary* rather than the geometry:
//! a tree has no names. `/viewport left agent` is a sentence a person says and an agent writes;
//! the same intent in a tree is a path through splits that have to have been created first, and
//! a command lane with no return path (`console_ops::console_cmd_path`) cannot ask what the
//! tree currently looks like in order to describe a place in it. Twelve fixed words are
//! addressable from a fire-and-forget line, which is the only transport this verb has.
//!
//! What that costs is stated rather than hidden: **no uneven splits and no dragging a divider**.
//! Those are real wants and they are a later tier's; the seam for them is that [`region_rect`]
//! is the only place a rectangle is computed.
//!
//! # 🚨 Three columns, and the vocabulary is DERIVED rather than listed
//!
//! Tier 1 was four cells over a 2×2 grid, and `topcenter` was not a missing word — it was a
//! missing rectangle. The grid is now **3 columns × 2 rows**, and the words follow from it by a
//! rule rather than by a list, which is what makes the count defensible:
//!
//! A region has to be an **axis-aligned run of cells**, or [`region_rect`] has no rectangle to
//! return. Over a 3×2 grid there are eighteen such runs — six contiguous column-spans times
//! three contiguous row-spans. Twelve of them get words and six do not, and the discriminator is
//! the module's own rule that **a region is a word a person says**: the column-spans English
//! names are *left*, *center*, *right* and *all three*; the row-spans are *top*, *bottom* and
//! *both*. Four times three is the vocabulary, exactly:
//!
//! | row span ╲ column span | all three | left | center | right |
//! |---|---|---|---|---|
//! | **both rows** | `full` | `left` | `center` | `right` |
//! | **top** | `top` | `topleft` | `topcenter` | `topright` |
//! | **bottom** | `bottom` | `bottomleft` | `bottomcenter` | `bottomright` |
//!
//! The six with no word are the two-column runs — *left and centre*, *centre and right* — and
//! they are excluded because naming them (`leftcenter`?) would mint a word nobody says in order
//! to complete a table. ⚠️ **Nothing breaks by leaving them out**: [`plan`]'s vacancy walk
//! describes a two-column gap as two vacant regions, which is the same honest decomposition it
//! already gives for the three-quarters case a 2×2 grid had no word for either.
//!
//! ⚠️ **`left` and `right` changed meaning, and that is the intended change rather than a
//! regression.** They were **half** the pane; they are now the **outer column** of three, at a
//! fixed width ([`SIDE_COLUMN`]). Anyone with muscle memory gets a narrower column than they got
//! yesterday, and `/viewport left panel` does not look the way it looked before. It is said out
//! loud here, in `CONSOLE_ARCHITECTURE.md` §1.14 and in the changelog, because a word quietly
//! meaning something new is exactly the drift this module's refusals exist to prevent.
//!
//! # The grid, and the one rule that makes an assignment decidable
//!
//! Every region is a set of the six **cells** ([`Region::cells`]) — `Full` is all six, `Left` is
//! the two in the left column, `TopLeft` is one. Two regions may be occupied at once **iff their
//! cell sets are disjoint**, which is the whole of the geometry: there is no layout arithmetic to
//! get wrong, only a bitmask. 📌 Every property the four-cell model had survives verbatim; what
//! changed is the width of the mask.
//!
//! An assignment that overlaps something already held is resolved by *containment*:
//!
//! * **Disjoint** — both stand. `Left` and `Right`, or the four corners.
//! * **One contains the other** — the container gives up its place, and the displacement is
//!   **reported** ([`Change::displaced`]). This is what makes the vocabulary usable at all: the
//!   console opens holding `Full`, so a rule that refused every overlap would refuse the first
//!   word of every split, and `full off` cannot be the way out because it would leave no agent
//!   (below). ⚠️ **This is the one place this module does something rather than refuse it**, and
//!   the reason it is not an approximation is that containment has exactly one reading —
//!   `Left` is the only occupied region `TopLeft` can be displacing, and it is displaced whole.
//! * **Partial overlap** — `Top` asked for while `Left` is held. Neither contains the other,
//!   the grid it implies cannot be drawn, and there is no unambiguous thing to displace. That
//!   is [`Refusal::Overlap`], and it **names both regions** rather than approximating one.
//!
//! # 🚨 Two refusals that are about meaning rather than geometry
//!
//! **The last `agent` region cannot be evicted.** A console with no region holding an agent is
//! a window with nothing to talk to — and the way in is not obvious from inside it, because the
//! verb that would fix it is typed *at* an agent. So any command whose result would hold no
//! [`Content::Agent`] is [`Refusal::LastAgent`], whether it got there by clearing the region or
//! by re-assigning it. One invariant on the *resulting layout*, checked once, rather than a
//! special case per verb.
//!
//! **A region that already holds nothing cannot be cleared.** [`Refusal::AlreadyEmpty`], because
//! a command that silently does nothing is indistinguishable from one that did not arrive.
//!
//! # 📌 The uniqueness rule, now that something needs it
//!
//! A content kind that may exist **only once** is, on a second assignment, **refused by name,
//! saying what already holds it** — never moved. That follows §1.3's "refused, not clamped":
//! moving a thing somebody can see, because they named a second place for it, is a guess about
//! which of the two they meant.
//!
//! Tier 1 stated this rule and built no machinery for it, on the grounds that an unreachable
//! arm is an untested branch pretending to be a design. Tier 2b makes it reachable:
//! [`Content::ThreeD`] is the one kind that may be held once.
//!
//! 🚨 **The limit belongs to the PRODUCER, not to the idea of a viewport**, and
//! [`Content::only_one_because`] is where that is said rather than assumed. A region holding
//! `3d` is a rectangle a producer draws into; today the only producer is Organon's `World`, and
//! *Organon* is what cannot be drawn twice in a frame — `console_main.rs`'s `engine_plan`
//! renders it at most once because `frame_index` and the TAA jitter phase riding it are shared
//! between targets. A producer that could fill four regions at once would inherit a refusal it
//! has no reason to obey, so the reason travels with the refusal instead of being folded into
//! the word `3d`. ⚠️ What is **not** available today is attributing it in the type system: there
//! is one producer, and inventing a `Producer` enum with one variant to hang the limit off would
//! be exactly the untested branch this module refused to build in Tier 1. So the attribution is
//! a reason string and a doc, and the seam for a second producer is that
//! `only_one_because` is the single site that decides.
//!
//! ⚠️ **A displacement is still allowed to move it.** `full 3d` while `left` holds `3d`
//! displaces `left` and stands, because containment has exactly one reading — the refusal is
//! for a *second* copy, not for the one copy being widened.
//!
//! # What is NOT here
//!
//! No egui drawing and no `Console` state — the same discipline [`crate::portal`] keeps, for
//! the same payoff: every decision this axis makes is a headless test rather than something
//! only a machine with a window server can answer. `console_main.rs` owns the child `Ui`s, the
//! notices and the clip rects, and maps [`plan`]'s answer onto them.

/// One of the twelve addressable parts of the pane.
///
/// ⚠️ **Flat on purpose** — see the module header. The ordering is **largest first**, and it is
/// load-bearing twice: [`plan`] walks it to coalesce vacant space into the widest word that
/// describes it, and `console_main.rs` walks it to decide which [`Content::Agent`] region gets
/// the live tab. Within one size the order is the grid's own: rows before columns, then reading
/// order — which is what puts `Center` between `Left` and `Right` rather than at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    /// The whole pane. What the console opens holding.
    Full,
    Top,
    Bottom,
    /// ⚠️ **The outer LEFT COLUMN, not the left half** — see the module header. A fixed
    /// [`SIDE_COLUMN`] wide.
    Left,
    /// The middle column, which takes whatever the two side columns leave. **The word Tier 1
    /// could not spell**, and the reason three columns are a geometry change rather than a
    /// vocabulary one.
    Center,
    /// ⚠️ **The outer RIGHT COLUMN, not the right half** — see [`Region::Left`].
    Right,
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Bit per cell, in reading order across a 3×2 grid. Private because it is an implementation of
/// the overlap rule and not a thing a caller should be reasoning in.
const C_TL: u8 = 1 << 0;
const C_TC: u8 = 1 << 1;
const C_TR: u8 = 1 << 2;
const C_BL: u8 = 1 << 3;
const C_BC: u8 = 1 << 4;
const C_BR: u8 = 1 << 5;

/// The three columns and the two rows, as cell masks — the axes [`region_rect`] cuts along, and
/// the terms the vocabulary table in the module header is a cross product of.
const COL_LEFT: u8 = C_TL | C_BL;
const COL_CENTER: u8 = C_TC | C_BC;
const COL_RIGHT: u8 = C_TR | C_BR;
const ROW_TOP: u8 = C_TL | C_TC | C_TR;
const ROW_BOTTOM: u8 = C_BL | C_BC | C_BR;

impl Region {
    /// Every region, **largest first**. The order [`plan`] and the seam both walk.
    pub const ALL: &'static [Region] = &[
        Region::Full,
        Region::Top,
        Region::Bottom,
        Region::Left,
        Region::Center,
        Region::Right,
        Region::TopLeft,
        Region::TopCenter,
        Region::TopRight,
        Region::BottomLeft,
        Region::BottomCenter,
        Region::BottomRight,
    ];

    /// The cells this region covers, as a bitmask. **The whole of the geometry model** —
    /// overlap, containment and disjointness are all read off this one number.
    ///
    /// ✏️ **Was `quadrants` over four bits.** The word was renamed with the grid rather than
    /// kept: a quadrant is a quarter, and six of them is a sentence that cannot be true.
    pub fn cells(self) -> u8 {
        match self {
            Region::Full => ROW_TOP | ROW_BOTTOM,
            Region::Top => ROW_TOP,
            Region::Bottom => ROW_BOTTOM,
            Region::Left => COL_LEFT,
            Region::Center => COL_CENTER,
            Region::Right => COL_RIGHT,
            Region::TopLeft => C_TL,
            Region::TopCenter => C_TC,
            Region::TopRight => C_TR,
            Region::BottomLeft => C_BL,
            Region::BottomCenter => C_BC,
            Region::BottomRight => C_BR,
        }
    }

    /// The word this region travels as — on the wire, in `--help`, and in every refusal.
    pub fn as_word(self) -> &'static str {
        match self {
            Region::Full => "full",
            Region::Top => "top",
            Region::Bottom => "bottom",
            Region::Left => "left",
            Region::Center => "center",
            Region::Right => "right",
            Region::TopLeft => "topleft",
            Region::TopCenter => "topcenter",
            Region::TopRight => "topright",
            Region::BottomLeft => "bottomleft",
            Region::BottomCenter => "bottomcenter",
            Region::BottomRight => "bottomright",
        }
    }

    /// The region a word names, or a refusal carrying the words that would have worked.
    ///
    /// ⚠️ **No case folding and no prefixes** — [`crate::screen::ScreenCmd::resolve`]'s rule and
    /// [`crate::posture::Posture::resolve`]'s before it. An approximation here would rearrange a
    /// window somebody is looking at into a shape they did not name.
    pub fn resolve(word: &str) -> Result<Self, UnknownWord> {
        Region::ALL
            .iter()
            .copied()
            .find(|r| r.as_word() == word)
            .ok_or_else(|| UnknownWord { word: word.to_string(), kind: "region", known: REGION_WORDS })
    }
}

/// The region words, in [`Region::ALL`] order — the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`'s possible-values parser, by the console's command schema
/// and by [`Region::resolve`]'s refusal — [`crate::screen::SCREEN_WORDS`]' arrangement, for its
/// reason: a second hand-maintained copy is how a CLI comes to accept a word nothing can act on.
pub const REGION_WORDS: &[&str] = &[
    "full",
    "top",
    "bottom",
    "left",
    "center",
    "right",
    "topleft",
    "topcenter",
    "topright",
    "bottomleft",
    "bottomcenter",
    "bottomright",
];

/// How many regions there are — [`Layout`]'s array size, **read off [`Region::ALL`]** rather
/// than written down beside it.
///
/// ⚠️ It was the literal `9` in three places before this tier, which is the shape of thing that
/// goes wrong when a vocabulary grows: `Layout::slot` is a position in `ALL`, so the array and
/// the list have to agree, and the only way to guarantee that is for one to *be* the other.
pub const REGION_COUNT: usize = Region::ALL.len();

/// What a region holds.
///
/// Three kinds, and the remaining absence is the scope rather than an oversight: **`media` waits
/// on §1.13's placement question.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// A live agent — the tab the console is showing, whichever front-end it is.
    Agent,
    /// One of Organon's own editor panels.
    Panel,
    /// **A live 3D viewport**: a rectangle a producer draws into, orbited by a drag and zoomed
    /// by the wheel.
    ///
    /// 🚨 **The viewport is the general thing and Organon is the first application of it**,
    /// which is James's own ordering and is why the word is `3d` rather than `world`: the region
    /// says *a 3D picture belongs here*, and which engine draws it is the producer's business.
    /// Today there is exactly one producer — Organon's `World`, the same one
    /// [`crate::portal`] shows — and everything specific to it lives behind
    /// [`Content::only_one_because`] and in `console_main.rs`, never in this word.
    ///
    /// ⚠️ **The Rust spelling is `ThreeD` because an identifier cannot begin with a digit.** The
    /// *word* is `3d`, and the word is what travels — on the wire, in `--help`, in the ring and
    /// in every refusal.
    ThreeD,
}

impl Content {
    /// The word this content travels as.
    pub fn as_word(self) -> &'static str {
        match self {
            Content::Agent => "agent",
            Content::Panel => "panel",
            Content::ThreeD => "3d",
        }
    }

    /// Why at most one region may hold this kind — `None` when any number may.
    ///
    /// 🚨 **The single site that decides uniqueness, and it answers with a REASON rather than a
    /// bool** — because the reason is what says whose limit it is. `3d` is limited by the
    /// producer behind it (Organon's `World` renders once per frame), not by anything about
    /// viewports, and a caller reading the refusal should learn that rather than concluding the
    /// console only ever wants one picture. See the module header on why this is a reason string
    /// and not a `Producer` type.
    pub fn only_one_because(self) -> Option<&'static str> {
        match self {
            Content::Agent | Content::Panel => None,
            Content::ThreeD => Some(
                "its producer is Organon, and Organon draws at most one frame per console \
                 frame — `frame_index` and the TAA jitter phase riding it are shared between \
                 targets, so two live pictures would trade phases and flicker",
            ),
        }
    }
}

/// What a `/viewport <region> <word>` command **asks for**, which is not the same type as what a
/// region **holds**.
///
/// 🚨 **[`ContentCmd::Clear`] is the split's doing, and it is not ceremony** — the same shape
/// [`crate::screen::ScreenCmd`] needed for `toggle`. `off` is not a content kind: no region
/// holds "off", and giving [`Content`] such a variant would put a value in the enum that the
/// draw path must then match and refuse to draw. The precedent for a clearing word riding the
/// same argument as the real values is `console.background`, whose `Choice` carries the three
/// backdrop **sources** (`world`/`off`/`substrate`) beside the materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentCmd {
    Hold(Content),
    /// Empty the region. Refused if it is already empty, or if it holds the last agent.
    Clear,
}

/// The word [`ContentCmd::Clear`] is spelled as.
pub const CLEAR_WORD: &str = "off";

/// The content words, in the order `--help` should list them — the three kinds in [`Content`]'s
/// own order, then the clearing word. [`REGION_WORDS`]' arrangement, for its reason.
pub const CONTENT_WORDS: &[&str] = &["agent", "panel", "3d", CLEAR_WORD];

impl ContentCmd {
    /// The word this command travels as.
    pub fn as_word(self) -> &'static str {
        match self {
            ContentCmd::Hold(c) => c.as_word(),
            ContentCmd::Clear => CLEAR_WORD,
        }
    }

    /// The command a word names, or a refusal carrying the words that do. [`Region::resolve`]'s
    /// rule: exact, never approximated.
    pub fn resolve(word: &str) -> Result<Self, UnknownWord> {
        match word {
            "agent" => Ok(ContentCmd::Hold(Content::Agent)),
            "panel" => Ok(ContentCmd::Hold(Content::Panel)),
            "3d" => Ok(ContentCmd::Hold(Content::ThreeD)),
            CLEAR_WORD => Ok(ContentCmd::Clear),
            _ => Err(UnknownWord {
                word: word.to_string(),
                kind: "content",
                known: CONTENT_WORDS,
            }),
        }
    }
}

/// A word no region or content answers to, carrying the words that do.
///
/// One type for both vocabularies because the sentence is the same sentence and the only thing
/// that differs is which table to quote — two structs would be two `Display` impls to keep in
/// step for no gain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownWord {
    /// Exactly what was asked for, unmodified.
    pub word: String,
    /// `"region"` or `"content"` — what the word was being read as.
    pub kind: &'static str,
    /// The table that would have worked.
    pub known: &'static [&'static str],
}

impl std::fmt::Display for UnknownWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not a {} — known: {}", self.word, self.kind, self.known.join(", "))
    }
}

/// Why an assignment was refused. **Every arm names what was asked and what stood in the way**
/// — a refusal that only says "no" is the defect this console keeps a running tally of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The asked-for region partially overlaps regions already held, and neither contains the
    /// other, so there is no unambiguous thing to displace.
    Overlap { asked: Region, with: Vec<Region> },
    /// `off` on a region that is already empty.
    AlreadyEmpty { asked: Region },
    /// The result would hold no [`Content::Agent`] at all.
    LastAgent { asked: Region },
    /// A kind that may exist only once is already held somewhere else.
    ///
    /// ⚠️ **`because` is carried rather than looked up at the display site**, so the refusal and
    /// [`Content::only_one_because`] cannot drift into two explanations of one rule.
    AlreadyHeld { asked: Region, content: Content, by: Region, because: &'static str },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Overlap { asked, with } => {
                // Bound once and named twice — the sentence needs the obstacle list in both
                // clauses, and building it twice is how the two come to differ.
                let names = with
                    .iter()
                    .map(|r| format!("`{}`", r.as_word()))
                    .collect::<Vec<_>>()
                    .join(" and ");
                write!(
                    f,
                    "`{}` overlaps {names} and neither contains the other, so the grid it asks \
                     for cannot be drawn — clear {names} first, or say `full` and start again",
                    asked.as_word(),
                )
            }
            Refusal::AlreadyEmpty { asked } => {
                write!(f, "`{}` holds nothing, so there is nothing to clear", asked.as_word())
            }
            Refusal::LastAgent { asked } => write!(
                f,
                "`{}` holds the last agent — emptying it would leave the console with nothing \
                 to talk to, and the verb that undoes it is typed at an agent",
                asked.as_word()
            ),
            // The region that was asked for is named as well as the one standing in the way, so
            // the sentence still identifies the command that produced it when it is read out of
            // a log with no context.
            Refusal::AlreadyHeld { asked, content, by, because } => write!(
                f,
                "`{}` asked for `{}`, which `{}` already holds, and there can be only one: \
                 {because} — clear it with `viewport {} off` and ask again, or name a region \
                 that contains it",
                asked.as_word(),
                content.as_word(),
                by.as_word(),
                by.as_word(),
            ),
        }
    }
}

/// Which region holds what, right now.
///
/// A fixed array indexed by [`Region::ALL`] position rather than a map: the vocabulary is twelve
/// words, so the layout is `Copy`-cheap, has no allocation, and cannot hold a region twice.
///
/// ✏️ **Was nine, over four cells.** The array is sized from the vocabulary rather than from a
/// count somebody keeps in step: `Self::slot` is a position in [`Region::ALL`], and a test walks
/// every region through it, so a mismatch is a panic in the suite rather than a silent
/// out-of-bounds arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    held: [Option<Content>; REGION_COUNT],
}

impl Default for Layout {
    /// 🚨 **Exactly today's console: one region, `Full`, holding the agent.** A console that has
    /// had no `/viewport` typed must be indistinguishable from one built before this module
    /// existed (`CLAUDE.md` invariant #4), and `console_main.rs` takes that further — it
    /// compares against this value and draws the pre-region path unchanged when they match,
    /// so the default is not merely equivalent but *the same code*.
    fn default() -> Self {
        let mut held = [None; REGION_COUNT];
        held[0] = Some(Content::Agent); // Region::Full is ALL[0].
        Layout { held }
    }
}

/// One assignment's result: the new layout, and what had to give up its place for it.
///
/// The displacement is carried out rather than swallowed because it is a change to what is on
/// screen that nobody asked for in so many words — the caller says it out loud.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub layout: Layout,
    /// In [`Region::ALL`] order. Empty in the ordinary case.
    pub displaced: Vec<Region>,
}

impl Layout {
    /// The empty layout. **Not reachable through [`Layout::assign`]** — every command's result
    /// must hold an agent — and it exists for tests and for building a layout from scratch.
    pub fn vacant() -> Self {
        Layout { held: [None; REGION_COUNT] }
    }

    fn slot(region: Region) -> usize {
        Region::ALL.iter().position(|r| *r == region).expect("Region::ALL is total over Region")
    }

    /// What this region holds, if anything.
    pub fn get(&self, region: Region) -> Option<Content> {
        self.held[Self::slot(region)]
    }

    /// Every occupied region and what it holds, in [`Region::ALL`] order.
    pub fn occupied(&self) -> Vec<(Region, Content)> {
        Region::ALL
            .iter()
            .copied()
            .filter_map(|r| self.get(r).map(|c| (r, c)))
            .collect()
    }

    /// Does anything hold an agent? The invariant every command's *result* must satisfy.
    pub fn has_agent(&self) -> bool {
        self.occupied().iter().any(|(_, c)| *c == Content::Agent)
    }

    /// The first region holding `content`, in [`Region::ALL`] order — `None` if nothing does.
    ///
    /// For a kind [`Content::only_one_because`] limits there is at most one, so "first" is
    /// "the one"; for the others this is the same determinism `console_main.rs` already relies
    /// on to decide which agent region gets the live tab.
    pub fn region_holding(&self, content: Content) -> Option<Region> {
        self.occupied().into_iter().find(|(_, c)| *c == content).map(|(r, _)| r)
    }

    /// Put `cmd` in `region`, or say why not. **Pure** — the layout that comes back is a new
    /// value and `self` is untouched, which is what lets the refusal path leave the console
    /// exactly as it was with no unwinding to get wrong.
    ///
    /// The order of the checks is the order of the module header's rules, and it matters: the
    /// overlap check runs before the agent check, so `top` while `left` is held is refused for
    /// the reason a person can act on (the grid) rather than for a consequence of it. The
    /// uniqueness check sits between them, and is asked of what **survives** the displacement —
    /// so widening a `3d` region from `left` to `full` is allowed while a second copy is not.
    pub fn assign(&self, region: Region, cmd: ContentCmd) -> Result<Change, Refusal> {
        let mut next = *self;
        let mut displaced = Vec::new();
        match cmd {
            ContentCmd::Clear => {
                if self.get(region).is_none() {
                    return Err(Refusal::AlreadyEmpty { asked: region });
                }
                next.held[Self::slot(region)] = None;
            }
            ContentCmd::Hold(content) => {
                let asked = region.cells();
                let mut partial = Vec::new();
                for (held, _) in self.occupied() {
                    if held == region {
                        continue;
                    }
                    let theirs = held.cells();
                    if asked & theirs == 0 {
                        continue; // Disjoint — it stands.
                    }
                    // Contained either way is one unambiguous displacement; anything else is
                    // two regions that cannot both be drawn and cannot be told apart.
                    if asked & theirs == theirs || asked & theirs == asked {
                        displaced.push(held);
                    } else {
                        partial.push(held);
                    }
                }
                if !partial.is_empty() {
                    return Err(Refusal::Overlap { asked: region, with: partial });
                }
                // 🚨 **Asked of what survives, not of what is held now.** A region being
                // displaced is giving up its copy in this same command, so counting it would
                // refuse `full 3d` from a console holding `left 3d` — a widening, not a second
                // copy. The check runs before anything is written because `assign` is pure and
                // a refusal must leave `self` untouched with nothing to unwind.
                if let Some(because) = content.only_one_because() {
                    if let Some(other) = self
                        .occupied()
                        .into_iter()
                        .find(|(held, c)| {
                            *c == content && *held != region && !displaced.contains(held)
                        })
                        .map(|(held, _)| held)
                    {
                        return Err(Refusal::AlreadyHeld {
                            asked: region,
                            content,
                            by: other,
                            because,
                        });
                    }
                }
                for r in &displaced {
                    next.held[Self::slot(*r)] = None;
                }
                next.held[Self::slot(region)] = Some(content);
            }
        }
        if !next.has_agent() {
            return Err(Refusal::LastAgent { asked: region });
        }
        Ok(Change { layout: next, displaced })
    }
}

/// The smallest side, in points, a region is worth drawing at.
///
/// Below this there is no room for a line of text and the word saying what the region is, so
/// the honest answer is that the pane cannot hold this layout — see [`plan`]. A number rather
/// than an adjective: two lines of monospace plus the padding around them, on the same
/// defensiveness [`crate::portal::portal_rect`] carries for a pane egui has not laid out yet.
pub const MIN_SIDE: f32 = 48.0;

/// How wide a **side column** is, in points — `left` and `right`, and the six cells in them.
///
/// 🚨 **Fixed, and the centre takes the remainder — NOT equal thirds.** James's decision, and it
/// is grounded in what this tree already does rather than in taste: Organon's own editor sizes
/// its control columns absolutely and lets the subject absorb the rest — `SidePanel::right`'s
/// `default_width(320.0)` for the theme dock, `exact_width(150.0)` for the presets rail, and
/// `mind_shell::DockSizes::default()`'s `left: 260.0, right: 300.0` beside a viewport that takes
/// what is left. Equal thirds would pin the instrument to a third of the window whatever the
/// window is, which is not what anyone wants to look at.
///
/// **320 is the widest fixed control column in the tree** (the theme dock), chosen so a panel
/// that fits Organon's own side dock fits a console `panel` region without the region being the
/// thing that decides. It is the one number here that is a **taste call standing on a
/// precedent**: nobody has yet looked at a three-column console, and whether 320 reads right
/// beside a live transcript is a question only a hand and a screen answer (§3).
///
/// ⚠️ **It is a width, never a ratio**, so it does not scale with the pane — which is the whole
/// point, and is also why the narrow-pane rule on [`region_rect`] has to exist.
pub const SIDE_COLUMN: f32 = 320.0;

/// The narrowest pane that can be divided into columns at all: two fixed sides plus a centre
/// worth drawing. **688 points** — see [`region_rect`] for what happens below it.
pub const MIN_COLUMNS_WIDTH: f32 = 2.0 * SIDE_COLUMN + MIN_SIDE;

/// A region's rectangle inside a pane — **derived from the pane every frame, never remembered**,
/// exactly as [`crate::portal::portal_rect`] is and for its reason: it is a function of where
/// the window is now.
///
/// 🚨 **`Full` returns the pane itself, bit for bit** — no margin, no inset, no arithmetic. That
/// is what makes the default layout not merely *similar* to the pre-region console but the same
/// rectangle, so nothing about invariant #4 rests on a float comparison.
///
/// `None` for a degenerate pane, and `None` for a rectangle smaller than [`MIN_SIDE`] on either
/// side. A squashed region is worse than an absent one: the caller can say the window is too
/// small for the layout, which is actionable, whereas a two-point-wide region is a sliver
/// somebody has to guess about.
///
/// ⚠️ **There is no gutter, on either axis.** A gutter would need a rule about which side owns
/// it, and the rule would be the first thing a drag-to-resize tier had to undo. Separation is
/// drawn, not reserved.
///
/// # 🚨 Two vertical cuts at a fixed width — the rows are still halves
///
/// The **row** split is the exact midpoint, unchanged since Tier 1: two rows, nothing to choose.
/// The **column** cuts are at [`SIDE_COLUMN`] in from each edge, so `left` and `right` are a
/// fixed width and `center` is whatever is left. See [`SIDE_COLUMN`] for why fixed rather than
/// thirds.
///
/// # ⚠️ The narrow-pane rule — the columns vanish, the rows survive
///
/// Fixed sides mean a pane can be too narrow to seat them: below
/// [`MIN_COLUMNS_WIDTH`] (**688 pt** — two sides plus a centre of at least [`MIN_SIDE`]) the two
/// cuts would cross, and `left` and `right` would overlap each other in the middle. The rule is
/// **decided rather than discovered**, and it is:
///
/// > **The side columns keep their width or there are no columns.** A region that needs a cut —
/// > anything not spanning all three columns — returns `None`. A region that spans all three
/// > (`full`, `top`, `bottom`) needs no cut and is unaffected.
///
/// So a narrow console can still be split into **rows**, and every column word refuses until the
/// window is wide enough. [`plan`] then answers `None` for a column layout and the seam says the
/// window is too small for it, naming the command that undoes it — which is actionable, where a
/// 20-point `left` is a sliver somebody has to guess about.
///
/// ⚠️ **The rejected rule was "the sides shrink"**, and `mind_shell::layout_workstation` is the
/// precedent for it — its docks yield proportionally so the viewport survives. It is right there
/// and wrong here: those docks are *chrome* around a subject, while every region here is somebody's
/// assigned content and none of them outranks the others. Shrinking would also make `left` mean a
/// different width at different window sizes with no word to explain it, and a side column
/// narrowed to `MIN_SIDE` is a column that can no longer hold what it exists to hold — which is
/// this module's "refused, not clamped" rule arriving as geometry.
pub fn region_rect(pane: egui::Rect, region: Region) -> Option<egui::Rect> {
    let (pw, ph) = (pane.width(), pane.height());
    if !pw.is_finite() || !ph.is_finite() || pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    let c = region.cells();
    // A region spanning every column is bounded by the pane itself and asks nothing of the cuts —
    // which is what keeps `Full` the pane bit for bit (invariant #4) at any width at all.
    let all_columns = c & COL_LEFT != 0 && c & COL_CENTER != 0 && c & COL_RIGHT != 0;
    let (left, right) = if all_columns {
        (pane.left(), pane.right())
    } else {
        if pw < MIN_COLUMNS_WIDTH {
            return None; // The narrow-pane rule above.
        }
        let cut_l = pane.left() + SIDE_COLUMN;
        let cut_r = pane.right() - SIDE_COLUMN;
        let left = if c & COL_LEFT != 0 {
            pane.left()
        } else if c & COL_CENTER != 0 {
            cut_l
        } else {
            cut_r
        };
        let right = if c & COL_RIGHT != 0 {
            pane.right()
        } else if c & COL_CENTER != 0 {
            cut_r
        } else {
            cut_l
        };
        (left, right)
    };
    let mid_y = pane.top() + ph * 0.5;
    let top = if c & ROW_TOP != 0 { pane.top() } else { mid_y };
    let bottom = if c & ROW_BOTTOM != 0 { pane.bottom() } else { mid_y };
    let rect = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
    (rect.width() >= MIN_SIDE && rect.height() >= MIN_SIDE).then_some(rect)
}

/// One rectangle of the divided pane, and what belongs in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub region: Region,
    /// `None` is an **unassigned** region, and it is not a gap in this list — see [`plan`].
    pub content: Option<Content>,
    pub rect: egui::Rect,
}

/// The whole pane, divided: every occupied region **and every unassigned one**, each with its
/// rectangle, in [`Region::ALL`] order.
///
/// 🚨 **Unassigned space is in this list on purpose.** §1.9's `Ring::Empty` exists because an
/// empty band that draws nothing is indistinguishable from a broken one, and the same is true a
/// hundred times over for a quarter of a window. A region nobody has filled has to say what it
/// is and how to fill it, which it can only do if something tells the seam it is there. Vacancy
/// is **coalesced largest-first**, so a layout holding only `TopLeft` reports one vacant `bottom`
/// rather than three vacant cells — the word in the notice is then the word a person would type.
///
/// ⚠️ **A gap two columns wide is two vacant regions, and that is the honest answer rather than a
/// gap in the coalescing.** The vocabulary has no word for *left and centre* (the module header
/// says why), so a layout holding only `right` reports a vacant `left` **and** a vacant `center`.
/// The 2×2 grid already behaved this way for the three-quarters case; three columns simply make
/// it commoner.
///
/// `None` when the pane cannot hold this layout: some region's rectangle is degenerate, below
/// [`MIN_SIDE`], or refused by [`region_rect`]'s narrow-pane rule. The caller says so rather than
/// drawing slivers.
pub fn plan(pane: egui::Rect, layout: &Layout) -> Option<Vec<Placed>> {
    let mut out = Vec::new();
    let mut seen: u8 = 0;
    for (region, content) in layout.occupied() {
        out.push(Placed { region, content: Some(content), rect: region_rect(pane, region)? });
        seen |= region.cells();
    }
    // Largest-first, so vacancy is described by the widest word that fits it exactly.
    for region in Region::ALL.iter().copied() {
        let q = region.cells();
        if q & seen == 0 {
            out.push(Placed { region, content: None, rect: region_rect(pane, region)? });
            seen |= q;
        }
    }
    out.sort_by_key(|p| Layout::slot(p.region));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane the shape the console actually runs at: the `CentralPanel` under a 30-point tab
    /// strip, in points. [`crate::portal`]'s test helper exactly, and deliberately the same
    /// numbers — the two modules divide the same rectangle.
    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1100.0, 690.0))
    }

    fn agent() -> ContentCmd {
        ContentCmd::Hold(Content::Agent)
    }
    fn panel() -> ContentCmd {
        ContentCmd::Hold(Content::Panel)
    }

    /// Does this intersection enclose nothing? ⚠️ **`Rect::area()` will not answer this** — an
    /// `intersect` of two disjoint rectangles comes back inverted, and a negative width times a
    /// negative height is a *positive* area, so an overlap test written that way passes exactly
    /// where it should fail hardest.
    fn flat(r: egui::Rect) -> bool {
        r.width() <= f32::EPSILON || r.height() <= f32::EPSILON
    }

    /// 🚨 **The console opens exactly as it did before this module existed.** Invariant #4, and
    /// the one property here that a person would notice being wrong the first time they opened
    /// the window.
    #[test]
    fn the_default_layout_is_one_region_holding_the_agent() {
        let d = Layout::default();
        assert_eq!(d.occupied(), vec![(Region::Full, Content::Agent)]);
        assert!(d.has_agent());
        assert_eq!(d.get(Region::Full), Some(Content::Agent));
        for r in Region::ALL.iter().copied().filter(|r| *r != Region::Full) {
            assert_eq!(d.get(r), None, "{} holds something", r.as_word());
        }
        // …and the rectangle it plans is the pane itself, bit for bit, so nothing about the
        // claim above rests on a float comparison.
        let p = plan(pane(), &d).expect("a 1100x690 pane holds the default layout");
        assert_eq!(p.len(), 1, "no vacant region beside a full one: {p:?}");
        assert_eq!(p[0], Placed { region: Region::Full, content: Some(Content::Agent), rect: pane() });
    }

    /// 🚨 **The whole geometry model, as a table.** Two regions may be occupied at once iff
    /// their cell sets are disjoint, so this pins the sets themselves — every other rule in
    /// this module reads off them.
    ///
    /// ✏️ **Six cells now, not four**, and the structure of the assertion is deliberately the
    /// same: each named span is the union of the cells under it, so a mistyped bit in `cells`
    /// fails here rather than showing up as two regions quietly sharing a rectangle.
    #[test]
    fn a_regions_cells_are_the_whole_of_the_geometry() {
        assert_eq!(Region::Full.cells(), C_TL | C_TC | C_TR | C_BL | C_BC | C_BR);
        assert_eq!(
            Region::Top.cells(),
            Region::TopLeft.cells() | Region::TopCenter.cells() | Region::TopRight.cells()
        );
        assert_eq!(
            Region::Bottom.cells(),
            Region::BottomLeft.cells()
                | Region::BottomCenter.cells()
                | Region::BottomRight.cells()
        );
        assert_eq!(Region::Left.cells(), Region::TopLeft.cells() | Region::BottomLeft.cells());
        assert_eq!(
            Region::Center.cells(),
            Region::TopCenter.cells() | Region::BottomCenter.cells()
        );
        assert_eq!(Region::Right.cells(), Region::TopRight.cells() | Region::BottomRight.cells());
        // 🚨 **`topcenter` exists and is disjoint from both side columns** — the one thing Tier B
        // is for, stated as a fact about the bitmask rather than about the word.
        assert_eq!(Region::TopCenter.cells() & Region::Left.cells(), 0);
        assert_eq!(Region::TopCenter.cells() & Region::Right.cells(), 0);
        // The six cells partition the pane: disjoint, and together they are `Full`.
        let cells = [
            Region::TopLeft,
            Region::TopCenter,
            Region::TopRight,
            Region::BottomLeft,
            Region::BottomCenter,
            Region::BottomRight,
        ];
        let mut union = 0u8;
        for c in cells {
            assert_eq!(union & c.cells(), 0, "{} overlaps an earlier cell", c.as_word());
            assert_eq!(c.cells().count_ones(), 1, "a cell is one bit");
            union |= c.cells();
        }
        assert_eq!(union, Region::Full.cells());
        // The three columns partition it too, and so do the two rows — which is what makes the
        // vocabulary a cross product of spans rather than a list somebody curated.
        assert_eq!(
            Region::Left.cells() | Region::Center.cells() | Region::Right.cells(),
            Region::Full.cells()
        );
        assert_eq!(Region::Top.cells() | Region::Bottom.cells(), Region::Full.cells());
        assert_eq!(Region::Top.cells() & Region::Bottom.cells(), 0);
        // No two distinct regions have the same footprint — otherwise two words would name one
        // rectangle and the refusals would be describing something invisible.
        for (i, a) in Region::ALL.iter().enumerate() {
            for b in &Region::ALL[i + 1..] {
                assert_ne!(a.cells(), b.cells(), "{a:?} and {b:?} are one rectangle");
            }
        }
        // `Layout`'s array is indexed by `ALL` position, so every region must have a slot.
        assert_eq!(REGION_COUNT, Region::ALL.len());
        for r in Region::ALL.iter().copied() {
            assert!(Layout::slot(r) < REGION_COUNT, "{} has no slot", r.as_word());
        }
    }

    /// The three splits James asked for, each reached from the default by the words a person
    /// would actually type. **This is the acceptance test for the vocabulary**: if a shape he
    /// named cannot be spelled, the words are wrong however good the geometry is.
    #[test]
    fn the_three_shapes_asked_for_are_all_reachable_from_the_default() {
        // "one on one side and one on the other"
        let l = Layout::default().assign(Region::Left, agent()).expect("left").layout;
        let two = l.assign(Region::Right, panel()).expect("right").layout;
        assert_eq!(
            two.occupied(),
            vec![(Region::Left, Content::Agent), (Region::Right, Content::Panel)]
        );

        // "one on one side and two on the other"
        let one_and_two = two
            .assign(Region::TopRight, panel())
            .expect("topright displaces right")
            .layout
            .assign(Region::BottomRight, panel())
            .expect("bottomright")
            .layout;
        assert_eq!(
            one_and_two.occupied(),
            vec![
                (Region::Left, Content::Agent),
                (Region::TopRight, Content::Panel),
                (Region::BottomRight, Content::Panel),
            ]
        );

        // "four" — ✏️ **and it is now four regions with a vacant CENTRE column**, not four
        // quarters. `left` and `right` are the outer columns, so this shape lost nothing it could
        // express before; what it gained is that the middle is addressable rather than implied.
        let four = one_and_two
            .assign(Region::TopLeft, agent())
            .expect("topleft displaces left")
            .layout
            .assign(Region::BottomLeft, panel())
            .expect("bottomleft")
            .layout;
        assert_eq!(four.occupied().len(), 4);
        assert!(four.has_agent());
        for r in [Region::TopLeft, Region::TopRight, Region::BottomLeft, Region::BottomRight] {
            assert!(four.get(r).is_some(), "{} is empty in the four-way", r.as_word());
        }

        // …and one command puts it back exactly where it started.
        let back = four.assign(Region::Full, agent()).expect("full").layout;
        assert_eq!(back, Layout::default(), "`/viewport full agent` is the way home");
    }

    /// 🚨 **The displacement rule, and the deadlock it exists to avoid.** The console opens
    /// holding `Full`, so if a contained assignment were refused instead of displacing, the very
    /// first word of every split would be refused — and `full off` cannot be the escape because
    /// it takes the last agent with it. That is measured here rather than argued: without the
    /// containment arm, this test's first line is a refusal and the module is unusable.
    #[test]
    fn containment_displaces_and_says_what_it_displaced() {
        let change = Layout::default().assign(Region::Left, agent()).expect("left is contained");
        assert_eq!(change.displaced, vec![Region::Full], "the container gave up its place");
        assert_eq!(change.layout.occupied(), vec![(Region::Left, Content::Agent)]);

        // The other direction: a container asked for while something it contains is held.
        let corners = Layout::vacant()
            .assign(Region::TopLeft, agent())
            .expect("topleft")
            .layout
            .assign(Region::BottomLeft, panel())
            .expect("bottomleft")
            .layout;
        let widened = corners.assign(Region::Left, agent()).expect("left contains both");
        assert_eq!(widened.displaced, vec![Region::TopLeft, Region::BottomLeft]);
        assert_eq!(widened.layout.occupied(), vec![(Region::Left, Content::Agent)]);

        // A disjoint neighbour is never displaced.
        let two = Layout::default()
            .assign(Region::Left, agent())
            .expect("left")
            .layout
            .assign(Region::Right, panel())
            .expect("right");
        assert!(two.displaced.is_empty(), "right displaced {:?}", two.displaced);
    }

    /// 🚨 **Partial overlap is refused, and the refusal names both regions.** `Left` and
    /// `TopLeft` can never both be occupied — that is the invalid state the rule exists to
    /// prevent — and this is the case where prevention has to be a refusal rather than a
    /// displacement, because neither region contains the other and there is nothing unambiguous
    /// to take away.
    #[test]
    fn a_partial_overlap_is_refused_by_name() {
        let left = Layout::default().assign(Region::Left, agent()).expect("left").layout;
        let e = left.assign(Region::Top, panel()).expect_err("top half-overlaps the left half");
        assert_eq!(e, Refusal::Overlap { asked: Region::Top, with: vec![Region::Left] });
        let text = e.to_string();
        assert!(text.contains("top"), "the refusal drops what was asked: {text}");
        assert!(text.contains("left"), "the refusal drops what stood in the way: {text}");
        assert!(text.contains("cannot be drawn"), "{text}");
        // The layout is untouched — `assign` is pure, so a refusal cannot half-apply.
        assert_eq!(left.occupied(), vec![(Region::Left, Content::Agent)]);

        // Two obstacles are both named. `top` crosses the top of both halves of a two-and-two,
        // containing neither — the case where naming one and displacing it would be a guess.
        let two = left.assign(Region::Right, panel()).expect("right").layout;
        let e = two.assign(Region::Top, panel()).expect_err("top crosses both halves");
        assert_eq!(
            e,
            Refusal::Overlap { asked: Region::Top, with: vec![Region::Left, Region::Right] }
        );
        let text = e.to_string();
        assert!(text.contains("left") && text.contains("right"), "both must be named: {text}");

        // ⚠️ The diagonal is the case that looks like a partial overlap and is not: `top`
        // *contains* `topleft`, and does not touch `bottomright` at all, so it displaces one
        // and leaves the other exactly where it is.
        let diagonal = Layout::vacant()
            .assign(Region::TopLeft, agent())
            .expect("topleft")
            .layout
            .assign(Region::BottomRight, panel())
            .expect("bottomright")
            .layout;
        let ok = diagonal.assign(Region::Top, agent()).expect("top contains topleft");
        assert_eq!(ok.displaced, vec![Region::TopLeft]);
        assert_eq!(
            ok.layout.occupied(),
            vec![(Region::Top, Content::Agent), (Region::BottomRight, Content::Panel)]
        );

        // …and the invalid state itself is unreachable by any route.
        for a in Region::ALL.iter().copied() {
            for b in Region::ALL.iter().copied() {
                let Ok(first) = Layout::vacant().assign(a, agent()) else { continue };
                let Ok(second) = first.layout.assign(b, agent()) else { continue };
                let held = second.layout.occupied();
                for (i, (x, _)) in held.iter().enumerate() {
                    for (y, _) in &held[i + 1..] {
                        assert_eq!(
                            x.cells() & y.cells(),
                            0,
                            "{} and {} are both held and overlap",
                            x.as_word(),
                            y.as_word()
                        );
                    }
                }
            }
        }
    }

    /// 🚨 **The last agent cannot be evicted, by either route.** The rule is one invariant on
    /// the *resulting* layout rather than a special case per verb, so clearing the region and
    /// re-assigning it to something else are refused by the same line of code — which is what
    /// stops the second route from being the one nobody remembered to close.
    #[test]
    fn the_last_agent_region_cannot_be_evicted_however_it_is_asked() {
        let d = Layout::default();
        assert_eq!(
            d.assign(Region::Full, ContentCmd::Clear),
            Err(Refusal::LastAgent { asked: Region::Full })
        );
        assert_eq!(
            d.assign(Region::Full, panel()),
            Err(Refusal::LastAgent { asked: Region::Full }),
            "re-assigning the only agent region is the same eviction by another name"
        );
        // …and so is displacing it: `left panel` would take `full`'s agent with it.
        assert_eq!(
            d.assign(Region::Left, panel()),
            Err(Refusal::LastAgent { asked: Region::Left }),
            "a displacement that leaves no agent is still an eviction"
        );
        let text = Refusal::LastAgent { asked: Region::Full }.to_string();
        assert!(text.contains("last agent"), "{text}");
        assert!(text.contains("nothing to talk to"), "the refusal must say why: {text}");

        // With a second agent held, the first may go.
        let two = d
            .assign(Region::Left, agent())
            .expect("left")
            .layout
            .assign(Region::Right, agent())
            .expect("right")
            .layout;
        let one = two.assign(Region::Left, ContentCmd::Clear).expect("the other agent remains");
        assert_eq!(one.layout.occupied(), vec![(Region::Right, Content::Agent)]);
    }

    /// Clearing a region that holds nothing is refused rather than quietly doing nothing — a
    /// command that changes nothing and says nothing is indistinguishable from one that never
    /// arrived.
    #[test]
    fn clearing_an_empty_region_is_refused_rather_than_silent() {
        let d = Layout::default();
        assert_eq!(
            d.assign(Region::Right, ContentCmd::Clear),
            Err(Refusal::AlreadyEmpty { asked: Region::Right })
        );
        assert!(
            Refusal::AlreadyEmpty { asked: Region::Right }.to_string().contains("nothing to clear")
        );
    }

    /// 🚨 **Unassigned space is planned, not skipped**, and it is coalesced into the widest word
    /// that describes it — so the notice a person reads names the region they would type.
    #[test]
    fn vacant_space_is_planned_and_named_by_the_widest_word_that_fits() {
        // ✏️ **Two vacant regions here, where the 2×2 grid reported one.** `left` no longer means
        // half the pane, so what it leaves is a centre column and a right column — and the
        // vocabulary has no word for the two of them together, deliberately (module header). The
        // decomposition is the honest one rather than a hole in the coalescing.
        let left = Layout::default().assign(Region::Left, agent()).expect("left").layout;
        let p = plan(pane(), &left).expect("the pane holds it");
        assert_eq!(p.len(), 3, "one held and two vacant: {p:?}");
        assert_eq!(p[0].region, Region::Left);
        assert_eq!(p[0].content, Some(Content::Agent));
        assert_eq!(
            p.iter().filter(|q| q.content.is_none()).map(|q| q.region).collect::<Vec<_>>(),
            vec![Region::Center, Region::Right],
        );
        // Together they are the pane, with no overlap and nothing left over.
        let union = p.iter().fold(egui::Rect::NOTHING, |u, q| u.union(q.rect));
        assert_eq!(union, pane());
        for (i, a) in p.iter().enumerate() {
            for b in &p[i + 1..] {
                assert!(flat(a.rect.intersect(b.rect)), "{:?} and {:?} overlap", a.region, b.region);
            }
        }
        // 🚨 …and the side columns are the fixed width, with the centre taking the remainder —
        // the one arithmetic claim this tier makes, checked against real numbers rather than a
        // ratio. 1100 − 320 − 320 = 460.
        assert_eq!(p[0].rect.width(), SIDE_COLUMN);
        assert_eq!(p[2].rect.width(), SIDE_COLUMN);
        assert_eq!(p[1].rect.width(), 1100.0 - 2.0 * SIDE_COLUMN);

        // A cell held alone leaves five, and no word covers exactly five — so it is described as
        // the largest pieces that do fit, not approximated by one.
        let corner = Layout::vacant().assign(Region::TopLeft, agent()).expect("topleft").layout;
        let p = plan(pane(), &corner).expect("the pane holds it");
        let vacant: Vec<Region> =
            p.iter().filter(|q| q.content.is_none()).map(|q| q.region).collect();
        assert_eq!(vacant, vec![Region::Bottom, Region::TopCenter, Region::TopRight]);
    }

    /// 🚨 **James's layout, the one Tier B exists for**, reached from the default by the words a
    /// person would actually type: two scrolling control columns flanking the instrument, with
    /// the agent beneath it.
    ///
    /// This is the acceptance test for the tier in the same sense
    /// `the_three_shapes_asked_for_are_all_reachable_from_the_default` was Tier 1's: the geometry
    /// is only worth changing if the shape it was changed for can be *spelled*.
    ///
    /// ⚠️ **The order of the four commands is not arbitrary and the test says so.** The console
    /// opens holding `full agent`, so the agent has to be re-homed **first** — every other
    /// assignment displaces `full` and would leave no agent, which is `Refusal::LastAgent`. That
    /// is Tier 1's rule unchanged, and it is the one thing about this layout somebody typing it
    /// for the first time will meet.
    #[test]
    fn the_editor_layout_two_columns_flanking_the_instrument_is_reachable() {
        let three_d = ContentCmd::Hold(Content::ThreeD);
        // The agent moves out of `full` first, or nothing else may be assigned at all.
        let too_soon = Layout::default().assign(Region::Left, panel());
        assert_eq!(too_soon, Err(Refusal::LastAgent { asked: Region::Left }));

        let editor = Layout::default()
            .assign(Region::BottomCenter, agent())
            .expect("the agent moves under the instrument")
            .layout
            .assign(Region::TopCenter, three_d)
            .expect("topcenter is the word Tier 1 could not spell")
            .layout
            .assign(Region::Left, panel())
            .expect("a control column on the left")
            .layout
            .assign(Region::Right, panel())
            .expect("and one on the right")
            .layout;
        assert_eq!(
            editor.occupied(),
            vec![
                (Region::Left, Content::Panel),
                (Region::Right, Content::Panel),
                (Region::TopCenter, Content::ThreeD),
                (Region::BottomCenter, Content::Agent),
            ],
        );
        // Nothing is vacant: the four regions are the whole pane.
        let p = plan(pane(), &editor).expect("a 1100x690 pane holds it");
        assert_eq!(p.len(), 4, "the layout leaves nothing unassigned: {p:?}");
        assert!(p.iter().all(|q| q.content.is_some()));
        // The instrument gets the remainder and the columns get their fixed width — which is the
        // whole argument against equal thirds, as a number.
        let rect = |r: Region| p.iter().find(|q| q.region == r).expect("planned").rect;
        assert_eq!(rect(Region::Left).width(), SIDE_COLUMN);
        assert_eq!(rect(Region::Right).width(), SIDE_COLUMN);
        assert_eq!(rect(Region::TopCenter).width(), 1100.0 - 2.0 * SIDE_COLUMN);
        assert!(
            rect(Region::TopCenter).width() > rect(Region::Left).width(),
            "equal thirds would have pinned the instrument to a third of the window",
        );
        // …and one command puts it back exactly where it started, from four regions as from one.
        assert_eq!(
            editor.assign(Region::Full, agent()).expect("full").layout,
            Layout::default(),
            "`/viewport full agent` is still the way home",
        );
    }

    /// 🚨 **The narrow-pane rule: the columns vanish and the rows survive.** Decided rather than
    /// discovered — [`region_rect`]'s doc carries the reasoning, and this pins the boundary.
    ///
    /// ⚠️ **The boundary is exact and both sides of it are checked**, because "somewhere around
    /// 688" is how a rule becomes a thing people rediscover. At exactly `MIN_COLUMNS_WIDTH` the
    /// centre is `MIN_SIDE` wide and stands; one point under, every column word refuses.
    #[test]
    fn a_pane_too_narrow_for_two_fixed_sides_keeps_its_rows_and_loses_its_columns() {
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 400.0));
        // Spans every column, so it asks nothing of the cuts.
        assert_eq!(region_rect(narrow, Region::Full), Some(narrow));
        assert!(region_rect(narrow, Region::Top).is_some(), "a narrow console still splits rows");
        assert!(region_rect(narrow, Region::Bottom).is_some());
        // Everything that needs a cut refuses — including the sides, which is the part that would
        // otherwise silently overlap in the middle.
        for r in [
            Region::Left,
            Region::Center,
            Region::Right,
            Region::TopLeft,
            Region::TopCenter,
            Region::TopRight,
            Region::BottomLeft,
            Region::BottomCenter,
            Region::BottomRight,
        ] {
            assert_eq!(region_rect(narrow, r), None, "{} drew on a 500pt pane", r.as_word());
        }
        // A row split still plans; a column split does not, and the seam says so.
        let rows = Layout::default().assign(Region::Bottom, agent()).expect("bottom").layout;
        assert!(plan(narrow, &rows).is_some(), "rows are unaffected by a narrow pane");
        let cols = Layout::default().assign(Region::Center, agent()).expect("center").layout;
        assert_eq!(plan(narrow, &cols), None, "no plan rather than two sides overlapping");

        // The boundary itself, from both sides.
        let at = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(MIN_COLUMNS_WIDTH, 400.0));
        assert_eq!(MIN_COLUMNS_WIDTH, 688.0, "two fixed sides plus a centre worth drawing");
        let centre = region_rect(at, Region::Center).expect("the centre is exactly MIN_SIDE wide");
        assert_eq!(centre.width(), MIN_SIDE);
        assert_eq!(region_rect(at, Region::Left).map(|r| r.width()), Some(SIDE_COLUMN));
        let under =
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(MIN_COLUMNS_WIDTH - 1.0, 400.0));
        assert_eq!(region_rect(under, Region::Center), None);
        assert_eq!(region_rect(under, Region::Left), None, "the sides do not shrink to fit");
        assert!(region_rect(under, Region::Full).is_some(), "the whole pane always draws");
    }

    /// Every planned rectangle sits inside the pane, no two overlap, and together they are the
    /// whole of it. The property that makes "the console is divided" true rather than merely
    /// drawn — checked over every layout two commands can build.
    #[test]
    fn a_plan_tiles_the_pane_exactly() {
        let p = pane();
        for a in Region::ALL.iter().copied() {
            for b in Region::ALL.iter().copied() {
                let Ok(first) = Layout::default().assign(a, agent()) else { continue };
                let Ok(second) = first.layout.assign(b, panel()) else { continue };
                let placed = plan(p, &second.layout).expect("a 1100x690 pane holds any layout");
                let mut union = egui::Rect::NOTHING;
                for (i, x) in placed.iter().enumerate() {
                    assert!(p.contains_rect(x.rect), "{:?} escapes the pane", x.region);
                    for y in &placed[i + 1..] {
                        assert!(
                            flat(x.rect.intersect(y.rect)),
                            "{:?} and {:?} overlap",
                            x.region,
                            y.region
                        );
                    }
                    union = union.union(x.rect);
                }
                assert_eq!(union, p, "the plan leaves a hole for {a:?}/{b:?}");
            }
        }
    }

    /// A pane egui has not laid out yet — or one too small to divide — yields no plan at all,
    /// rather than slivers nobody can read. [`crate::portal::portal_rect`]'s defensiveness, at
    /// the same seam and for the same reason.
    #[test]
    fn a_pane_too_small_to_divide_has_no_plan() {
        for bad in [
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0)),
            egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(0.0, 0.0)),
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(f32::NAN, 100.0)),
        ] {
            assert_eq!(region_rect(bad, Region::Full), None, "{bad:?}");
            assert_eq!(plan(bad, &Layout::default()), None, "{bad:?}");
        }
        // A pane that holds `Full` and cannot hold a half: the default still plans, the split
        // does not, and the difference is what lets the seam say "too small for this layout"
        // instead of drawing a two-point-wide agent.
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 400.0));
        assert!(region_rect(narrow, Region::Full).is_some());
        // ✏️ **Refused by the narrow-pane rule now, not by `MIN_SIDE`** — 80pt cannot seat two
        // fixed side columns at all. The observable answer is the one this test always asserted;
        // `a_pane_too_narrow_for_two_fixed_sides_keeps_its_rows_and_loses_its_columns` is where
        // the new boundary itself is pinned.
        assert_eq!(region_rect(narrow, Region::Left), None, "80pt cannot hold a side column");
        let split = Layout::default().assign(Region::Left, agent()).expect("left").layout;
        assert_eq!(plan(narrow, &split), None, "no plan rather than a sliver");
        assert!(plan(narrow, &Layout::default()).is_some(), "the default still draws");
    }

    /// 🚨 **The two word tables are exactly the sets the two resolvers accept.** A word listed
    /// but unresolvable is a CLI offering a shape the console cannot reach; a resolvable word
    /// that is unlisted is a shape `--help` never mentions. Both directions, and the round trip
    /// through `as_word` — [`crate::screen`]'s arrangement, for its reason.
    #[test]
    fn the_word_tables_and_the_resolvers_are_one_vocabulary() {
        for word in REGION_WORDS {
            let r = Region::resolve(word).unwrap_or_else(|_| panic!("`{word}` is unresolvable"));
            assert_eq!(r.as_word(), *word, "`{word}` does not spell itself back");
        }
        for r in Region::ALL.iter().copied() {
            assert!(REGION_WORDS.contains(&r.as_word()), "{r:?} is unlisted");
        }
        assert_eq!(REGION_WORDS.len(), Region::ALL.len());
        for word in CONTENT_WORDS {
            let c = ContentCmd::resolve(word).unwrap_or_else(|_| panic!("`{word}` is unresolvable"));
            assert_eq!(c.as_word(), *word);
        }
        for c in [
            ContentCmd::Hold(Content::Agent),
            ContentCmd::Hold(Content::Panel),
            ContentCmd::Hold(Content::ThreeD),
            ContentCmd::Clear,
        ] {
            assert!(CONTENT_WORDS.contains(&c.as_word()), "{c:?} is unlisted");
        }
        // ⚠️ **`off` is a content WORD and not a content KIND** — the tables differ by exactly
        // one entry, and that difference is the whole reason `ContentCmd` exists beside
        // `Content`. If they ever match, a clearing word has become something a region holds.
        assert_eq!(CONTENT_WORDS.len(), 4);
        assert!(!["agent", "panel", "3d"].contains(&CLEAR_WORD));
    }

    /// 🚨 **At most one region holds `3d`, and the refusal says whose limit it is.**
    ///
    /// The rule Tier 1 stated and deliberately did not build. Two halves are worth pinning
    /// separately: that a second copy is refused *by name*, and that the refusal quotes
    /// [`Content::only_one_because`]'s reason — which attributes the limit to Organon rather
    /// than to viewports, and is the sentence a future producer would change.
    #[test]
    fn only_one_region_may_hold_the_live_3d_and_the_refusal_says_whose_limit_it_is() {
        let three_d = ContentCmd::Hold(Content::ThreeD);
        let split = Layout::default()
            .assign(Region::Left, agent())
            .expect("left agent")
            .layout
            .assign(Region::Right, three_d)
            .expect("right 3d")
            .layout;
        assert_eq!(split.region_holding(Content::ThreeD), Some(Region::Right));

        // …and a second one, in a region that overlaps nothing, is refused rather than moved.
        let corners = split
            .assign(Region::TopLeft, agent())
            .expect("topleft displaces left")
            .layout;
        let e = corners.assign(Region::BottomLeft, three_d).expect_err("a second 3d");
        let Refusal::AlreadyHeld { asked, content, by, because } = e.clone() else {
            panic!("{e:?} is not the uniqueness refusal");
        };
        assert_eq!((asked, content, by), (Region::BottomLeft, Content::ThreeD, Region::Right));
        assert_eq!(Some(because), Content::ThreeD.only_one_because());
        let text = e.to_string();
        assert!(text.contains("bottomleft") && text.contains("right"), "{text}");
        assert!(text.contains("Organon"), "the refusal must name whose limit it is: {text}");
        assert!(text.contains("viewport right off"), "…and the way out: {text}");

        // ⚠️ **A widening is not a second copy**, which is why the check is asked of what
        // survives the displacement rather than of what is held now. A `3d` corner asked to
        // become a `3d` half displaces itself and stands.
        let corner = Layout::default()
            .assign(Region::Left, agent())
            .expect("left agent")
            .layout
            .assign(Region::BottomRight, three_d)
            .expect("bottomright 3d")
            .layout;
        let widened = corner.assign(Region::Right, three_d).expect("right contains bottomright");
        assert_eq!(widened.displaced, vec![Region::BottomRight]);
        assert_eq!(widened.layout.region_holding(Content::ThreeD), Some(Region::Right));

        // The other two kinds are unlimited, and that is a property of the kind rather than an
        // accident of this layout: two agents and two panels are both ordinary.
        assert_eq!(Content::Agent.only_one_because(), None);
        assert_eq!(Content::Panel.only_one_because(), None);
        let two_panels = Layout::default()
            .assign(Region::Left, agent())
            .expect("left")
            .layout
            .assign(Region::TopRight, panel())
            .expect("topright")
            .layout
            .assign(Region::BottomRight, panel())
            .expect("a second panel is ordinary")
            .layout;
        assert_eq!(two_panels.occupied().len(), 3);
    }

    /// Neither resolver approximates, and both refusals carry the table that would have worked.
    #[test]
    fn an_unknown_word_is_refused_with_the_words_that_would_have_worked() {
        for bad in ["Left", "LEFT", "lef", "left ", "centre", "middle", "", "0.5"] {
            assert!(Region::resolve(bad).is_err(), "`{bad}` resolved and must not");
        }
        // ✏️ **`centre` stays on that list and `center` is now a region** — the spelling that
        // resolves is the one the Rust identifier and the grid's own axis use, and the other is
        // still refused rather than folded, on this module's no-approximation rule.
        assert!(Region::resolve("center").is_ok());
        // 🚨 **The two-column runs are deliberately unnamed**, so a plausible-looking word for one
        // must refuse rather than resolve to something near it. The module header says why.
        for unnamed in ["leftcenter", "centerright", "centerleft", "twothirds"] {
            assert!(Region::resolve(unnamed).is_err(), "`{unnamed}` resolved and must not");
        }
        for bad in ["Agent", "AGENT", "3D", "3", "media", "on", ""] {
            assert!(ContentCmd::resolve(bad).is_err(), "`{bad}` resolved and must not");
        }
        let e = Region::resolve("middle").expect_err("not a region").to_string();
        assert!(e.contains("middle"), "the refusal drops what was typed: {e}");
        for word in REGION_WORDS {
            assert!(e.contains(word), "`{word}` is missing from the refusal: {e}");
        }
        let e = ContentCmd::resolve("media").expect_err("not yet").to_string();
        assert!(e.contains("media"), "{e}");
        for word in CONTENT_WORDS {
            assert!(e.contains(word), "`{word}` is missing from the refusal: {e}");
        }
    }
}
