//! **Saved layouts — an arrangement of the pane, by name, that survives the process.**
//!
//! [`crate::region`] divides the pane and says what each part holds; [`crate::panel_stack`] says
//! what a `panel` part contains. This says how an arrangement is **named, written down and
//! brought back**: `organon console layout save <name>` — `/layout save <name>` in a composer —
//! and `load`, `delete`, plus the read `/layout.list`.
//!
//! # 🚨 Why this is not a convenience
//!
//! `doc/organon_is_the_product.md` §4 is the reframe this module is built against: **a layout is
//! the unit of product identity.** "Claude Code Desktop", "Organon standalone" and "an LLM
//! visualiser" become *named arrangements of one program* rather than three programs — so the
//! thing being written to disk here is not a window position, it is what somebody means when
//! they say which program they are running.
//!
//! ⚠️ **That document is a proposal, not a ratified change.** Nothing here touches
//! `organon_core::edition::Edition`, and nothing here begins collapsing editions. What it builds
//! is the mechanism the reframe would need, and the mechanism is worth having either way.
//!
//! # 🚨 The constraint that decides the design: a load is a transaction
//!
//! §4 again, and it is not optional:
//!
//! > *A layout must not be able to produce a window nobody can recover from. Region assignment
//! > already refuses by name and keeps the last agent region; a saved layout is an assignment
//! > that arrives **all at once, from a file, possibly written by someone else**. It needs the
//! > same refusals and one more: a layout that cannot be drawn must say so and leave the current
//! > one standing, never half-apply.*
//!
//! So [`resolve`] validates the **whole** arrangement — every word, every pair of regions, the
//! uniqueness rule, the last-agent rule, and whether today's window can hold it — and answers
//! either one finished [`crate::region::Layout`] or one sentence. There is no partially-built
//! value for a caller to leak, which makes "never half-apply" a property of the signature rather
//! than discipline at the call site: the console assigns `self.layout` in a single statement, or
//! it prints the refusal and assigns nothing.
//!
//! ⚠️ **A word this build no longer has is refused, never dropped.** A file naming a region that
//! was renamed, a content kind that was removed, or `off` (which empties a region and is
//! therefore not something a region *holds*) is refused with a sentence naming what is missing.
//! Loading the rest would silently give somebody a different arrangement from the one they saved
//! — and they would have no way to tell.
//!
//! # 📌 The file, and whose precedent it follows
//!
//! `layouts.json`, at the store root beside `harnesses.json` and `preferences.json`.
//! [`crate::harness`] is the precedent §4 names — *"built-ins seeded in code, a user's file
//! merged over them by id, serde defaults, unknown fields tolerated"* — and this follows the
//! **discipline**, not the literal shape: the object here has room to grow a top-level key,
//! which a bare array has not.
//!
//! Three properties come from [`crate::prefs`] instead, because unlike `harnesses.json` this
//! file is one the product **writes**:
//!
//! 1. **A write cannot destroy what is stored** — temp file in the same directory, then rename.
//! 2. **Reading is total** — missing, unreadable or malformed is "nothing saved", never an error
//!    and never a crash. A corrupt library must cost you your layouts, never your console.
//! 3. **Unknown fields survive a round trip.** `harnesses.json` tolerates them because it is only
//!    ever read; this file is rewritten by `save` and `delete`, so tolerating-and-dropping would
//!    quietly delete a newer console's fields from every layout it did not touch.
//!    [`SavedLayout::extra`] and [`Library::extra`] keep them.
//!
//! ⚠️ **No environment variable overrides any of this**, on [`crate::prefs`]'s stated rule: a
//! variable baked into a launch shim years ago silently outranking the file is indistinguishable
//! from the evaporation a stored choice exists to end.
//!
//! # 🚨 The library ships EMPTY, and that is the scope rather than an omission
//!
//! [`builtin`] returns nothing. Naming the presets — "desktop", "standalone", "mind" — is James's
//! call, and a preset nobody has looked at on a screen is worse than none: it would be a shape
//! the product asserts is good, arrived at by a machine that cannot see. The **seam** is built
//! and tested ([`merge_over`], which is parameterised precisely so the merge is exercised with a
//! seeded built-in), so filling the library is data rather than code.
//!
//! # What is NOT here
//!
//! **The panel stack is not part of a layout.** A saved arrangement records that a region holds
//! `panel`, never which panels are in the column — the stack is documented as not remembered
//! across a launch, and changing that is a decision about the stack, not about layouts.
//!
//! No `Console` state and no egui drawing: [`resolve`] takes the pane it should check against as
//! an argument, so every decision here is a headless test.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::region::{self, Content, Layout, LayoutFault, Region, UnknownWord, CLEAR_WORD};
use crate::session::SessionLog;

/// The layout library, at the store root beside `harnesses.json` and `preferences.json`.
pub const LAYOUTS_FILE: &str = "layouts.json";

/// The longest name a layout may carry.
///
/// A ceiling rather than an adjective: a name is typed at a prompt, echoed in a refusal that
/// lists every other name, and carried on a whitespace-delimited sidecar line. Sixty-four is
/// longer than anything a person types and short enough that a library of them still reads as a
/// list.
pub const MAX_NAME: usize = 64;

/// What `console layout <action> <name>` asks for.
///
/// 🚨 **Three actions, not four, and `list` is deliberately not among them.** The slash grammar
/// fills *required* arguments positionally and *optional* ones by keyword
/// ([`crate::registry`]'s `parse_args`), so a verb whose name argument were optional would be
/// typed `/layout save name mine` while the CLI stayed `console layout save mine` — one verb
/// with two spellings, which is the drift this tree spends its refusals preventing. Both words
/// are therefore required, exactly as [`crate::panel_stack::StackCmd`]'s two are.
///
/// That settles `save` / `load` / `delete`, which all name a layout. It does **not** admit
/// `list`, because a name is not a thing a listing takes and there is no honest word to put in
/// that slot — `panel_stack`'s `all` works there because `all` genuinely names a value in the
/// panel ring, and no layout name means "every layout". So the listing is a **separate verb**,
/// `console.layout.list`, on the precedent `console.camera.read` already set: a *read* is
/// answered in-process, because `organon console …` is fire-and-forget with no return path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutCmd {
    /// Write the console's current arrangement down under this name, replacing any layout
    /// already stored under it.
    Save,
    /// Replace the console's arrangement with the stored one — **transactionally**; see
    /// [`resolve`].
    Load,
    /// Take a layout out of the library.
    Delete,
}

/// The action words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`'s possible-values parser, by the console's command schema and
/// by [`LayoutCmd::resolve`]'s refusal — [`crate::region::REGION_WORDS`]' arrangement, for its
/// reason: a second hand-maintained copy is how a CLI comes to accept a word nothing can act on.
pub const LAYOUT_ACTIONS: &[&str] = &["save", "load", "delete"];

impl LayoutCmd {
    /// The word this command travels as.
    pub fn as_word(self) -> &'static str {
        match self {
            LayoutCmd::Save => "save",
            LayoutCmd::Load => "load",
            LayoutCmd::Delete => "delete",
        }
    }

    /// The command a word names, or a refusal carrying the words that do.
    ///
    /// ⚠️ **Exact, never approximated** — [`crate::region::Region::resolve`]'s rule, and here an
    /// approximation would rearrange a window somebody is looking at, or delete something.
    pub fn resolve(word: &str) -> Result<Self, Refusal> {
        match word {
            "save" => Ok(LayoutCmd::Save),
            "load" => Ok(LayoutCmd::Load),
            "delete" => Ok(LayoutCmd::Delete),
            _ => Err(Refusal::UnknownAction { word: word.to_string() }),
        }
    }
}

/// Is this a name a layout may be stored under?
///
/// 🚨 **Whitespace is refused because the wire format cannot carry it.** A console op crosses the
/// sidecar as `layout save <name>` and `parse_console_op` splits on whitespace, so a two-word
/// name would arrive truncated — a command that appears to work and stores something else. The
/// remaining rules are the same kind of fact: a control character corrupts the line, an empty
/// name cannot be typed back, and a name longer than [`MAX_NAME`] makes every refusal that lists
/// the library unreadable.
///
/// ⚠️ **Matching is exact, and `Desk` is therefore a different layout from `desk`.** Folding case
/// would be a guess, and the refusal for an unknown name lists every name that exists — so a
/// mismatch is visible and one keystroke from fixed, which silent folding never is.
pub fn check_name(name: &str) -> Result<(), Refusal> {
    let bad = |why: &'static str| Err(Refusal::BadName { word: name.to_string(), why });
    if name.is_empty() {
        return bad("a layout needs a name to be brought back by");
    }
    if name.chars().count() > MAX_NAME {
        return bad("it is longer than 64 characters");
    }
    if name.chars().any(char::is_whitespace) {
        return bad(
            "it contains whitespace, and a command crosses the console's channel as one \
             whitespace-delimited line — the name would arrive truncated",
        );
    }
    if name.chars().any(char::is_control) {
        return bad("it contains a control character, which would corrupt the line it travels on");
    }
    Ok(())
}

/// One saved arrangement: a name, and what each occupied region holds.
///
/// 📌 **A map keyed by the region word, not a list of pairs**, because the file has to be legible
/// — `"left": "agent"` is the sentence somebody would say — and because a map cannot name a
/// region twice. ⚠️ A JSON object with a duplicated key is decided by `serde_json` (the last
/// wins); that is a pathological file rather than a shape this format offers, and the alternative
/// costs the legibility the format exists for.
///
/// **An unassigned region is simply absent.** [`CLEAR_WORD`] never appears in a saved layout: it
/// is what a *command* says to empty a region, not something a region holds, and a file naming it
/// is refused ([`Refusal::ClearWordStored`]) rather than read as an empty placement.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedLayout {
    /// The name it is stored, merged and recalled under. See [`check_name`].
    pub name: String,
    /// Region word → content word, in [`Region::ALL`] order when this console wrote it.
    ///
    /// ⚠️ **A `BTreeMap`, so the file is written in a stable order** — [`crate::prefs`]'s reason:
    /// a file whose lines shuffle on every save is one nobody can diff or keep in version
    /// control. The cost is that the stored order is alphabetical rather than largest-first,
    /// which is a fact about the *file*; nothing reads order out of it, because
    /// [`Layout::occupied`] re-derives it.
    pub regions: BTreeMap<String, String>,
    /// Every field this build does not know, kept so a rewrite does not delete it.
    ///
    /// 🚨 **This is what makes "unknown fields tolerated" mean *preserved*.** `harnesses.json`
    /// can tolerate-and-drop because nothing ever writes it back; `save` and `delete` rewrite
    /// this file whole, so without this a console one version behind would silently strip a
    /// newer one's fields off every layout in the library — including the ones it never touched.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl SavedLayout {
    /// Write down what the console is holding right now, under `name`.
    ///
    /// The name is **not** checked here: capturing is a pure transformation and the boundary that
    /// accepts a name is where the refusal can be spoken. [`check_name`] is that boundary.
    pub fn capture(name: &str, layout: &Layout) -> Self {
        SavedLayout {
            name: name.to_string(),
            regions: layout
                .occupied()
                .into_iter()
                .map(|(r, c)| (r.as_word().to_string(), c.as_word().to_string()))
                .collect(),
            extra: BTreeMap::new(),
        }
    }
}

/// The layout library: what is stored, in file order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub layouts: Vec<SavedLayout>,
    /// Top-level fields this build does not know. [`SavedLayout::extra`]'s reason exactly.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Distinguishes concurrent temp files written by this process — [`crate::prefs`]'s counter, for
/// its reason: the pid separates processes, this separates two saves inside one.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// The layouts this console ships with. **Empty on purpose** — see the module header: naming the
/// presets is James's call, and this tier builds the mechanism rather than the library.
///
/// It is a function rather than an absence so that the seam is a value somebody fills, and so
/// [`merge_over`] has something real to be the identity over.
pub fn builtin() -> Vec<SavedLayout> {
    Vec::new()
}

/// Merge a user library over a set of built-ins **by name**: a user entry replaces the built-in
/// it shares a name with, and a new name appends in file order.
///
/// [`crate::harness::load`]'s rule exactly, taken as a parameter rather than reading [`builtin`]
/// directly for [`crate::harness::builtin_for`]'s reason: a merge whose inputs cannot be chosen
/// is a merge that can only be tested against whatever ships, and what ships today is nothing.
pub fn merge_over(builtin: Vec<SavedLayout>, user: Library) -> Library {
    let mut layouts = builtin;
    for saved in user.layouts {
        match layouts.iter_mut().find(|l| l.name == saved.name) {
            Some(slot) => *slot = saved,
            None => layouts.push(saved),
        }
    }
    Library { layouts, extra: user.extra }
}

impl Library {
    /// The store root the library lives in: [`SessionLog::store_root`], reused, not re-derived —
    /// [`crate::prefs::Preferences::store_root`]'s rule, and its reason in full: two resolvers
    /// that *can* disagree eventually do, and the failure is a layouts file written beside a
    /// `harnesses.json` the console reads from somewhere else.
    pub fn store_root() -> Option<PathBuf> {
        SessionLog::store_root()
    }

    /// Read `<store_root>/layouts.json` and merge it over [`builtin`].
    ///
    /// **Total by construction** — missing, unreadable or malformed all mean "nothing stored",
    /// which for a fresh install is also the correct answer. [`crate::prefs::Preferences::load`]'s
    /// posture, for its reason: a corrupt library must cost you your layouts, never your console.
    pub fn load(store_root: &Path) -> Self {
        let user: Library = fs::read_to_string(store_root.join(LAYOUTS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        merge_over(builtin(), user)
    }

    /// [`Library::load`] against the real store. A platform with no data directory yields an
    /// empty library, for the same reason every other failure does.
    pub fn load_default() -> Self {
        Self::store_root().map(|r| Self::load(&r)).unwrap_or_default()
    }

    /// Write `<store_root>/layouts.json`, replacing any existing file **atomically**.
    ///
    /// Temp file in the *same directory*, then rename — [`crate::prefs::Preferences::save`]'s
    /// mechanism and its caveats in full: the directory matters because a rename is only atomic
    /// within one volume, `std::fs::rename` replaces an existing destination on Windows too, and
    /// the bytes are plain UTF-8 because `serde_json` refuses a BOM outright and a total reader
    /// would turn that refusal into silence.
    ///
    /// 🚨 **A built-in the user has not changed is not written into their file.** Only entries
    /// that differ from — or are absent from — [`builtin`] are stored, so filling the library
    /// later cannot bake today's presets into everybody's file and freeze them there. With no
    /// built-ins this is the identity, which is why
    /// [`tests::an_unchanged_builtin_is_not_written_into_the_user_file`] seeds one through
    /// [`Library::save_over`].
    pub fn save(&self, store_root: &Path) -> io::Result<()> {
        self.save_over(store_root, &builtin())
    }

    /// [`Library::save`] against a chosen set of built-ins — [`merge_over`]'s arrangement, and
    /// its reason: a rule whose inputs cannot be chosen can only be tested against whatever
    /// ships, and what ships today is nothing.
    pub fn save_over(&self, store_root: &Path, builtin: &[SavedLayout]) -> io::Result<()> {
        let mine = Library {
            layouts: self
                .layouts
                .iter()
                .filter(|l| !builtin.iter().any(|b| b == *l))
                .cloned()
                .collect(),
            extra: self.extra.clone(),
        };
        fs::create_dir_all(store_root)?;
        let mut json = serde_json::to_string_pretty(&mine).map_err(io::Error::other)?;
        json.push('\n');

        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = store_root.join(format!("{LAYOUTS_FILE}.tmp-{}-{seq}", std::process::id()));
        fs::write(&temp, json.as_bytes())?;
        match fs::rename(&temp, store_root.join(LAYOUTS_FILE)) {
            Ok(()) => Ok(()),
            Err(e) => {
                // One stranded temp per failed save would accumulate forever, and each looks
                // like a torn write to anyone reading the directory.
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    /// [`Library::save`] to the real store. No data directory is an **error** here, mirroring
    /// [`crate::prefs::Preferences::save_default`]: a caller asking to persist has nowhere to
    /// fall back to.
    pub fn save_default(&self) -> io::Result<()> {
        let root = Self::store_root()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no platform data directory"))?;
        self.save(&root)
    }

    /// The layout stored under this exact name. See [`check_name`] on why it is exact.
    pub fn get(&self, name: &str) -> Option<&SavedLayout> {
        self.layouts.iter().find(|l| l.name == name)
    }

    /// Store `saved`, replacing any layout of the same name. `true` if something was replaced —
    /// which the caller says out loud, because overwriting an arrangement somebody assembled is
    /// a change they did not name in so many words.
    pub fn upsert(&mut self, saved: SavedLayout) -> bool {
        match self.layouts.iter_mut().find(|l| l.name == saved.name) {
            Some(slot) => {
                *slot = saved;
                true
            }
            None => {
                self.layouts.push(saved);
                false
            }
        }
    }

    /// Take a layout out. `false` if no layout answered to that name — the caller refuses by
    /// name rather than reporting a deletion that did not happen.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.layouts.len();
        self.layouts.retain(|l| l.name != name);
        self.layouts.len() != before
    }

    /// Every stored name, in file order — what a refusal quotes and what `/layout.list` reads.
    pub fn names(&self) -> Vec<&str> {
        self.layouts.iter().map(|l| l.name.as_str()).collect()
    }

    /// The names as one sentence fragment, or `"nothing"` — [`crate::panel_stack`]'s
    /// `held_panel_slugs` arrangement, so a refusal reads as a sentence on an empty library.
    ///
    /// 🚨 **Each name is backticked, and that is not decoration — it is what makes the list
    /// unambiguous.** [`check_name`] refuses whitespace because the *wire* cannot carry it, and
    /// deliberately refuses nothing else about what a person may call their arrangement — so a
    /// comma is a perfectly legal name character. A bare `join(", ")` would then render a library
    /// holding `a,b` and `c` as `a,b, c`, which is indistinguishable from three layouts, and the
    /// whole job of this fragment is to let somebody spot the name they actually typed (§1.15's
    /// exact matching leans on it: `Desk` and `desk` are two layouts and the list is where you
    /// see both). ⚠️ **The fix belongs here rather than in `check_name`** — narrowing the names a
    /// person may choose to suit a separator would be a rule about the *display* leaking into the
    /// data, where the whitespace rule is a genuine fact about the transport.
    pub fn names_or_nothing(&self) -> String {
        if self.layouts.is_empty() {
            return "nothing".to_string();
        }
        self.names().iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
    }
}

/// Validate a saved layout against **today's** vocabulary, today's rules and today's window, and
/// answer the layout it becomes — or refuse, naming what is wrong.
///
/// 🚨 **This is the transaction.** Everything is checked before anything is returned, and what
/// comes back is one whole [`Layout`] — so a caller that assigns it in one statement cannot
/// half-apply a layout, and a refusal leaves whatever was on screen exactly where it was. See the
/// module header for why that is the constraint the whole design is arranged around.
///
/// The order of the checks is the order a reader meets the file: the words first (a region word,
/// then the content word beside it), then the arrangement as a whole
/// ([`Layout::from_placements`]), then the window.
///
/// ⚠️ **`pane` is optional and the check it gates is a fact about the window, not about the
/// file.** A layout too small to draw *right now* is refused with the size that refused it,
/// because "it must say so and leave the current one standing" is exactly that case — but with
/// no pane measured yet (no frame drawn) there is nothing to check against, and the draw path
/// carries the same sentence every frame as the backstop.
pub fn resolve(saved: &SavedLayout, pane: Option<egui::Rect>) -> Result<Layout, Refusal> {
    let mut places: Vec<(Region, Content)> = Vec::new();
    for (region_word, content_word) in &saved.regions {
        let region = Region::resolve(region_word).map_err(Refusal::UnknownRegion)?;
        // Named separately from every other unknown word, because it is the one a person is
        // likeliest to write by hand — it is a word the *command* takes — and "not a content" is
        // a poor answer to it. What is wrong is that a region holding nothing is simply absent.
        if content_word == CLEAR_WORD {
            return Err(Refusal::ClearWordStored { region: region_word.clone() });
        }
        let content = Content::resolve(content_word).map_err(Refusal::UnknownContent)?;
        places.push((region, content));
    }
    // Largest-first, so an overlap is reported against the wider region — the word a person would
    // type — rather than against whichever key sorted first alphabetically.
    places.sort_by_key(|(r, _)| Region::ALL.iter().position(|x| x == r).unwrap_or(usize::MAX));
    let layout = Layout::from_placements(&places).map_err(Refusal::NotALayout)?;
    if let Some(pane) = pane {
        if region::plan(pane, &layout).is_none() {
            // 🚨 **`plan` says no for two different reasons, and a refusal that quoted only one
            // of them would be true-but-irrelevant** — raised in review on this tier, after #98
            // Tier B added the second. A region needing a **column cut** is refused outright
            // below `MIN_COLUMNS_WIDTH`, however much room its own rectangle would have had; a
            // region that needs no cut is refused only when a *side* falls under `MIN_SIDE`. So
            // the coarser rule is asked first, and the sentence names the threshold that
            // actually tripped.
            //
            // ⚠️ Both can be true at once (a narrow pane and a short one). The column rule wins
            // then, which is the right order to fix them in: widening is what makes the layout
            // expressible at all, and the height refusal is still waiting afterwards if it
            // applies. One refusal at a time, each of them true.
            if pane.width() < region::MIN_COLUMNS_WIDTH {
                if let Some((region, _)) =
                    layout.occupied().into_iter().find(|(r, _)| r.needs_column_cut())
                {
                    return Err(Refusal::TooNarrowForColumns {
                        region,
                        width: pane.width(),
                        min_width: region::MIN_COLUMNS_WIDTH,
                        side: region::SIDE_COLUMN,
                    });
                }
            }
            return Err(Refusal::TooSmall {
                width: pane.width(),
                height: pane.height(),
                min_side: region::MIN_SIDE,
            });
        }
    }
    Ok(layout)
}

/// Why a layout command was refused. **Every arm names what was asked and what stood in the
/// way** — a refusal that only says "no" is the defect this console keeps a running tally of.
///
/// ⚠️ **`PartialEq` but not `Eq`**, because [`Refusal::TooSmall`] carries the measured pane and a
/// measurement is an `f32`. Rounding it to integers to win the trait would be a number that is
/// not the one that refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// A word [`LAYOUT_ACTIONS`] does not carry.
    UnknownAction { word: String },
    /// A name that cannot be stored or cannot travel. See [`check_name`].
    BadName { word: String, why: &'static str },
    /// `load` or `delete` of a name the library does not hold. Carries every name it does.
    NoSuchLayout { name: String, known: String },
    /// A stored region word this build does not have.
    UnknownRegion(UnknownWord),
    /// A stored content word this build does not have.
    UnknownContent(UnknownWord),
    /// [`CLEAR_WORD`] stored as what a region holds. See [`resolve`].
    ClearWordStored { region: String },
    /// The arrangement is not a layout — overlapping regions, two live pictures, no agent.
    /// ⚠️ **Carried rather than restated**, so [`LayoutFault`] and this cannot drift into two
    /// explanations of one rule.
    NotALayout(LayoutFault),
    /// The pane is too narrow to seat the fixed side columns at all, and this layout uses a
    /// region that needs a column cut. **A different refusal from [`Refusal::TooSmall`] because
    /// it is a different rule with a different threshold** — see [`resolve`], and
    /// [`crate::region::region_rect`]'s narrow-pane rule for why the columns vanish rather than
    /// shrink. It names the region that needs the cut, so the sentence says which word to drop.
    TooNarrowForColumns { region: Region, width: f32, min_width: f32, side: f32 },
    /// Today's window cannot draw it: some region's rectangle would be under
    /// [`crate::region::MIN_SIDE`] on a
    /// side. Names the pane it was measured against, because the same layout loads once the
    /// window is bigger and a refusal that did not say so would read as the layout being broken.
    TooSmall { width: f32, height: f32, min_side: f32 },
    /// The library could not be written. ⚠️ Reported rather than swallowed, on
    /// [`crate::prefs::Preferences::save`]'s asymmetry: a read that fails has a sane answer to
    /// fall back on, and a write that fails has none.
    NotWritten { path: String, error: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::UnknownAction { word } => write!(
                f,
                "`{word}` is not a layout action — known: {}, and the listing is `/layout.list`",
                LAYOUT_ACTIONS.join(", ")
            ),
            Refusal::BadName { word, why } => {
                write!(f, "`{word}` cannot be a layout name: {why}")
            }
            Refusal::NoSuchLayout { name, known } => {
                write!(f, "no layout is saved as `{name}` — saved: {known}")
            }
            Refusal::UnknownRegion(e) => write!(
                f,
                "this layout names a region this build does not have — {e}. It is refused whole \
                 rather than loaded in part: an arrangement missing a region is not the one that \
                 was saved, and nothing on screen would say so"
            ),
            Refusal::UnknownContent(e) => write!(
                f,
                "this layout names something to hold that this build does not have — {e}. It is \
                 refused whole rather than loaded in part, for the reason a missing region is"
            ),
            Refusal::ClearWordStored { region } => write!(
                f,
                "this layout stores `{CLEAR_WORD}` as what `{region}` holds, and `{CLEAR_WORD}` \
                 is what a command says to *empty* a region rather than something a region holds \
                 — a region that holds nothing is simply left out of the file"
            ),
            Refusal::NotALayout(fault) => {
                write!(f, "this layout cannot be drawn: {fault}. Nothing has changed")
            }
            Refusal::TooNarrowForColumns { region, width, min_width, side } => write!(
                f,
                "the window is too narrow for this layout right now — it holds `{}`, which needs \
                 a column cut, and the side columns are a fixed {side:.0} points each, so a pane \
                 has to be {min_width:.0} wide to seat them. This one is {width:.0}. Nothing has \
                 changed; widen the window and ask again, or load an arrangement of rows \
                 (`top`/`bottom`), which need no cut and work at any width",
                region.as_word()
            ),
            Refusal::TooSmall { width, height, min_side } => write!(
                f,
                "the window is too small for this layout right now — the pane is {width:.0}×\
                 {height:.0} points and every region needs {min_side:.0} on a side. Nothing has \
                 changed; make the window bigger and ask again"
            ),
            Refusal::NotWritten { path, error } => {
                write!(f, "the layout library at {path} could not be written: {error}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::ContentCmd;

    /// A pane the shape the console actually runs at — [`crate::region`]'s test helper exactly,
    /// and deliberately the same numbers.
    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1100.0, 690.0))
    }

    /// A private directory per test. ⚠️ **Never the real store** — a test that wrote
    /// `%APPDATA%\OrganonShell\layouts.json` would destroy the layouts of whoever ran
    /// `cargo test`. [`crate::prefs`]'s helper, for its reason.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("organon-console-layouts-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// The two-column arrangement, built by the commands a person would type.
    ///
    /// ✏️ **Called `two_halves` until #98 Tier B**, which made `left` and `right` the outer
    /// *columns* of three rather than the two halves — the one word-level break in that axis's
    /// vocabulary. The commands are unchanged; only what they mean on screen moved.
    fn two_columns() -> Layout {
        Layout::default()
            .assign(Region::Left, ContentCmd::Hold(Content::Agent))
            .expect("left")
            .layout
            .assign(Region::Right, ContentCmd::Hold(Content::Panel))
            .expect("right")
            .layout
    }

    fn saved(name: &str, pairs: &[(&str, &str)]) -> SavedLayout {
        SavedLayout {
            name: name.to_string(),
            regions: pairs.iter().map(|(r, c)| (r.to_string(), c.to_string())).collect(),
            extra: BTreeMap::new(),
        }
    }

    /// CONTRACT: what the console is holding is what comes back — the round trip that makes
    /// `save` then `load` mean anything at all.
    #[test]
    fn an_arrangement_survives_capture_and_resolution_unchanged() {
        let live = two_columns();
        let stored = SavedLayout::capture("desk", &live);
        assert_eq!(stored.name, "desk");
        assert_eq!(
            stored.regions,
            BTreeMap::from([
                ("left".to_string(), "agent".to_string()),
                ("right".to_string(), "panel".to_string()),
            ])
        );
        assert_eq!(resolve(&stored, Some(pane())), Ok(live));

        // …and through the file, which is where the round trip is actually spent.
        let text = serde_json::to_string(&stored).unwrap();
        let back: SavedLayout = serde_json::from_str(&text).unwrap();
        assert_eq!(back, stored);
        assert!(text.contains(r#""left":"agent""#), "the file says what a person would say: {text}");
    }

    /// 🚨 **The four-way, the default, and every layout two commands can reach** — captured and
    /// resolved back. The property is that the two representations are one arrangement, not that
    /// one example works.
    #[test]
    fn every_reachable_arrangement_round_trips_through_the_stored_form() {
        for a in Region::ALL.iter().copied() {
            for b in Region::ALL.iter().copied() {
                for kind in Content::ALL.iter().copied() {
                    let Ok(first) = Layout::default().assign(a, ContentCmd::Hold(Content::Agent))
                    else {
                        continue;
                    };
                    let Ok(second) = first.layout.assign(b, ContentCmd::Hold(kind)) else {
                        continue;
                    };
                    let live = second.layout;
                    let stored = SavedLayout::capture("x", &live);
                    assert_eq!(
                        resolve(&stored, Some(pane())),
                        Ok(live),
                        "{a:?}/{b:?}/{kind:?} did not survive the round trip"
                    );
                }
            }
        }
    }

    /// 🚨 **A word this build does not have is refused by name, and the layout does NOT half
    /// load.** The constraint `doc/organon_is_the_product.md` §4 makes non-negotiable, from the
    /// direction it is likeliest to arrive: a file written by another build.
    ///
    /// ✏️ **The example word used to be `topcenter`, and #98 Tier B made it real while this was
    /// in review** — which is the case this test exists for, arriving in the space of one merge.
    /// The word here is now one no grid has: `middleleft` is what a future 3×3 would call its
    /// middle row, so it stands for a *newer* build's vocabulary rather than for a typo. ⚠️ Do
    /// not re-point this at a word somebody is about to add; the assertion is about the refusal,
    /// and it stops meaning anything the moment the word resolves.
    #[test]
    fn a_layout_naming_something_this_build_lacks_is_refused_whole() {
        assert!(Region::resolve("middleleft").is_err(), "the premise: no such region today");
        let renamed = saved("future", &[("left", "agent"), ("middleleft", "3d")]);
        let e = resolve(&renamed, Some(pane())).expect_err("`middleleft` is not a region here");
        assert!(matches!(e, Refusal::UnknownRegion(_)), "{e:?}");
        let text = e.to_string();
        assert!(text.contains("middleleft"), "the refusal drops what is missing: {text}");
        assert!(text.contains("left"), "…and lists what would have worked: {text}");
        assert!(text.contains("refused whole"), "…and says it loaded nothing: {text}");

        // 🚨 **And the half that Tier B just proved in the field: a word this build HAS still
        // loads, whatever it came to mean.** `topcenter` was the unknown word in this test's
        // first version and is a region now, so a file naming it is a layout rather than a
        // refusal — which is the forward-compatibility story stated from the other side.
        let widened = saved("newer", &[("left", "agent"), ("topcenter", "3d")]);
        let built = resolve(&widened, Some(pane())).expect("`topcenter` resolves in this build");
        assert_eq!(built.get(Region::TopCenter), Some(Content::ThreeD));

        let gone = saved("future", &[("left", "agent"), ("right", "media")]);
        let e = resolve(&gone, Some(pane())).expect_err("`media` is not in the vocabulary yet");
        assert!(matches!(e, Refusal::UnknownContent(_)), "{e:?}");
        assert!(e.to_string().contains("media"), "{e}");

        // ⚠️ The clearing word gets its own sentence, because "not a content" is a poor answer to
        // a word the command beside this one takes.
        let cleared = saved("odd", &[("left", "agent"), ("right", "off")]);
        let e = resolve(&cleared, Some(pane())).expect_err("`off` is not something a region holds");
        assert_eq!(e, Refusal::ClearWordStored { region: "right".into() });
        assert!(e.to_string().contains("left out of the file"), "{e}");
    }

    /// 🚨 **A saved layout meets every refusal an assignment meets, and one more.** These are the
    /// files that would produce a console nobody can recover from, each refused with the current
    /// arrangement left standing.
    #[test]
    fn a_stored_arrangement_obeys_every_rule_a_typed_one_does() {
        // No agent: the eviction §1.14 refuses by command, arriving by file instead.
        let mute = saved("mute", &[("left", "panel"), ("right", "3d")]);
        assert_eq!(
            resolve(&mute, Some(pane())),
            Err(Refusal::NotALayout(LayoutFault::NoAgent))
        );
        assert!(resolve(&mute, Some(pane())).unwrap_err().to_string().contains("nothing to talk to"));

        // Two regions that cannot both be drawn.
        let crossed = saved("crossed", &[("left", "agent"), ("top", "panel")]);
        let e = resolve(&crossed, Some(pane())).expect_err("half-overlap");
        assert!(matches!(e, Refusal::NotALayout(LayoutFault::Overlap { .. })), "{e:?}");
        assert!(e.to_string().contains("left") && e.to_string().contains("top"), "{e}");

        // Two live pictures, refused with whose limit it is.
        let doubled = saved("doubled", &[("left", "agent"), ("topright", "3d"), ("bottomright", "3d")]);
        let e = resolve(&doubled, Some(pane())).expect_err("one producer");
        assert!(e.to_string().contains("Organon"), "the refusal names whose limit it is: {e}");

        // A file that places nothing.
        assert_eq!(
            resolve(&saved("empty", &[]), Some(pane())),
            Err(Refusal::NotALayout(LayoutFault::Empty))
        );
    }

    /// ⚠️ **The window's size is the one refusal that is about the window rather than the file**,
    /// so it is checked only when a pane has been measured — and the same layout loads once the
    /// window is bigger, which the sentence says.
    #[test]
    fn a_layout_too_big_for_todays_window_is_refused_with_the_size_that_refused_it() {
        let split = SavedLayout::capture("split", &two_columns());
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 400.0));

        // 🚨 **The COLUMN rule, which is the one a narrow pane actually trips.** Raised in
        // review: `plan` says no for two reasons and this refusal used to quote only `MIN_SIDE`,
        // so an 80-point pane holding `left`/`right` was told "every region needs 48 on a side"
        // — true, irrelevant, and misleading about why the load was refused.
        let e = resolve(&split, Some(narrow)).expect_err("80pt cannot seat two 320pt columns");
        let Refusal::TooNarrowForColumns { region, width, min_width, side } = e.clone() else {
            panic!("{e:?} is not the column refusal");
        };
        assert_eq!(region, Region::Left, "…and it names a region that needs the cut");
        assert_eq!((width, min_width, side), (80.0, region::MIN_COLUMNS_WIDTH, region::SIDE_COLUMN));
        let text = e.to_string();
        assert!(text.contains("688") && text.contains("320"), "the real threshold: {text}");
        assert!(text.contains("80"), "…and the pane that refused it: {text}");
        assert!(!text.contains("48"), "the OTHER rule's number must not appear: {text}");
        assert!(text.contains("`top`"), "…and the arrangement that works at any width: {text}");
        assert!(text.contains("Nothing has changed"), "{text}");

        // 🚨 **And `MIN_SIDE` is still its own refusal, reached by a pane wide enough for the
        // columns and too short for the rows** — which is what makes the two genuinely different
        // rules rather than one with two spellings.
        let rows = saved("rows", &[("top", "agent"), ("bottom", "panel")]);
        let short = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(700.0, 60.0));
        let e = resolve(&rows, Some(short)).expect_err("30pt rows are under MIN_SIDE");
        assert!(matches!(e, Refusal::TooSmall { .. }), "{e:?}");
        let text = e.to_string();
        assert!(text.contains("700") && text.contains("60"), "the size that refused it: {text}");
        assert!(text.contains("48"), "…and this rule's threshold: {text}");
        assert!(!text.contains("688"), "the OTHER rule's number must not appear: {text}");
        // ⚠️ `top`/`bottom` span all three columns, so the column rule cannot be what refused
        // them — the premise the assertion above rests on, asked of the geometry rather than
        // assumed.
        assert!(!Region::Top.needs_column_cut() && !Region::Bottom.needs_column_cut());
        assert!(Region::Left.needs_column_cut() && Region::TopCenter.needs_column_cut());

        // The same file, a window that can hold it.
        assert!(resolve(&split, Some(pane())).is_ok());
        // …and with nothing measured yet, the size question is not asked at all — the draw path
        // carries the sentence instead.
        assert!(resolve(&split, None).is_ok());
        // The undivided default fits anywhere — `full` spans every column and needs no cut — so
        // no window is ever too small for the layout that gets a person back.
        assert!(resolve(&SavedLayout::capture("home", &Layout::default()), Some(narrow)).is_ok());
        assert!(resolve(&SavedLayout::capture("home", &Layout::default()), Some(short)).is_ok());
    }

    /// 🚨 **A refusal yields no layout at all** — the property that makes a load transactional,
    /// pinned as the signature rather than as discipline. There is no partially-built value the
    /// caller could take a piece of.
    #[test]
    fn a_refused_layout_leaves_the_caller_with_what_it_already_had() {
        let standing = two_columns();
        let mut live = standing;
        for bad in [
            saved("a", &[("left", "agent"), ("top", "panel")]),
            saved("b", &[("full", "panel")]),
            saved("c", &[("nowhere", "agent")]),
            saved("d", &[]),
        ] {
            // The whole of the apply path: assign what comes back, or print and change nothing.
            match resolve(&bad, Some(pane())) {
                Ok(next) => live = next,
                Err(e) => assert!(!e.to_string().is_empty()),
            }
            assert_eq!(live, standing, "`{}` moved the console", bad.name);
        }
    }

    /// CONTRACT: the actions are exactly the three the table carries, and the listing is not one
    /// of them — see [`LayoutCmd`] on why.
    #[test]
    fn the_action_table_and_the_resolver_are_one_vocabulary() {
        for word in LAYOUT_ACTIONS {
            let c = LayoutCmd::resolve(word).unwrap_or_else(|_| panic!("`{word}` is unresolvable"));
            assert_eq!(c.as_word(), *word, "`{word}` does not spell itself back");
        }
        for c in [LayoutCmd::Save, LayoutCmd::Load, LayoutCmd::Delete] {
            assert!(LAYOUT_ACTIONS.contains(&c.as_word()), "{c:?} is unlisted");
        }
        assert_eq!(LAYOUT_ACTIONS.len(), 3);
        for bad in ["Save", "SAVE", "sav", "list", "ls", "", "remove"] {
            let e = LayoutCmd::resolve(bad).expect_err("exact, never approximated");
            assert_eq!(e, Refusal::UnknownAction { word: bad.to_string() });
            let text = e.to_string();
            for word in LAYOUT_ACTIONS {
                assert!(text.contains(word), "`{word}` missing from the refusal: {text}");
            }
            assert!(text.contains("/layout.list"), "…and where the listing is: {text}");
        }
    }

    /// 🚨 **A name with whitespace is refused because the wire cannot carry it**, which is the
    /// one rule here that is a fact about the transport rather than about taste.
    #[test]
    fn a_name_that_cannot_travel_is_refused_before_it_is_stored() {
        for good in ["desk", "two-up", "james.mind", "A", "café", "1"] {
            assert_eq!(check_name(good), Ok(()), "`{good}` is a perfectly good name");
        }
        for bad in ["", "two words", "tab\there", "line\nbreak", " lead", "trail "] {
            let e = check_name(bad).expect_err("`{bad}` must not be storable");
            assert!(matches!(e, Refusal::BadName { .. }), "{e:?}");
            assert!(!e.to_string().is_empty());
        }
        assert!(check_name(&"x".repeat(MAX_NAME)).is_ok(), "the ceiling itself is allowed");
        let long = check_name(&"x".repeat(MAX_NAME + 1)).expect_err("one over");
        assert!(long.to_string().contains("64"), "{long}");
        // ⚠️ Exact, so two spellings are two layouts — and the refusal that names the miss lists
        // every name, which is what makes the difference visible.
        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("Desk", &Layout::default()));
        assert!(lib.get("desk").is_none(), "case is not folded");
        assert_eq!(lib.get("Desk").map(|l| l.name.as_str()), Some("Desk"));
    }

    /// 🚨 **The list of names a refusal quotes has to be readable as a LIST**, and a comma is a
    /// legal name character — [`check_name`] refuses whitespace because the wire cannot carry it
    /// and refuses nothing else about what a person may call their arrangement.
    ///
    /// Raised in review on this tier: joined bare, a library holding `a,b` and `c` renders as
    /// `a,b, c`, which is exactly what three layouts called `a`, `b` and `c` would render as. The
    /// fragment's whole job is to let somebody find the name they typed, so an ambiguous one
    /// undercuts §1.15's exact-matching argument at the moment it matters most.
    #[test]
    fn a_comma_in_a_name_cannot_make_the_list_read_as_more_layouts_than_there_are() {
        assert_eq!(check_name("a,b"), Ok(()), "a comma is not a transport problem");
        let mut lib = Library::default();
        lib.upsert(saved("a,b", &[("full", "agent")]));
        lib.upsert(saved("c", &[("full", "agent")]));
        assert_eq!(lib.names(), vec!["a,b", "c"], "two layouts, whatever they are called");
        assert_eq!(lib.names_or_nothing(), "`a,b`, `c`", "…and the list says two");

        let mut three = Library::default();
        for n in ["a", "b", "c"] {
            three.upsert(saved(n, &[("full", "agent")]));
        }
        assert_ne!(
            three.names_or_nothing(),
            lib.names_or_nothing(),
            "three layouts must not render as two do"
        );
        // The empty case is still a word rather than an empty quote, so the sentence reads.
        assert_eq!(Library::default().names_or_nothing(), "nothing");
        // …and the refusal that quotes it names both the miss and the library.
        let e = Refusal::NoSuchLayout { name: "a".into(), known: lib.names_or_nothing() };
        let text = e.to_string();
        assert!(text.contains("`a,b`") && text.contains("`c`"), "{text}");
    }

    /// CONTRACT: saved, then loaded from the file, is the same library.
    #[test]
    fn the_library_round_trips_through_the_store() {
        let root = temp_root("round-trip");
        let mut lib = Library::default();
        assert!(!lib.upsert(SavedLayout::capture("desk", &two_columns())), "nothing to replace");
        lib.save(&root).unwrap();
        assert_eq!(Library::load(&root), lib);
        assert_eq!(Library::load(&root).names(), vec!["desk"]);

        // Saving the same name again replaces it, and says so.
        let mut lib = Library::load(&root);
        assert!(lib.upsert(SavedLayout::capture("desk", &Layout::default())), "replaced");
        lib.save(&root).unwrap();
        let back = Library::load(&root);
        assert_eq!(back.layouts.len(), 1, "a replacement is not an addition");
        assert_eq!(resolve(back.get("desk").unwrap(), Some(pane())), Ok(Layout::default()));

        assert!(back.get("nothing-like-this").is_none());
        let mut back = back;
        assert!(back.remove("desk"));
        assert!(!back.remove("desk"), "a second delete has nothing to take");
        assert_eq!(back.names_or_nothing(), "nothing");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a fresh install is not an error, and neither is garbage. The console still
    /// opens, and the answer is the same answer.
    #[test]
    fn a_missing_or_malformed_library_is_nothing_saved() {
        let root = temp_root("malformed");
        assert_eq!(Library::load(&root), Library::default());
        for bad in ["{", "", "not json at all", r#"{"layouts": 7}"#, "[]"] {
            fs::write(root.join(LAYOUTS_FILE), bad).unwrap();
            assert_eq!(
                Library::load(&root),
                Library::default(),
                "{bad:?} must not panic and must not error"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// 🚨 **A field this build does not know survives a rewrite.** `harnesses.json` may
    /// tolerate-and-drop because nothing writes it back; this file is rewritten by `save` and
    /// `delete`, so dropping would silently strip a newer console's data off every layout in a
    /// library — including the ones this build never touched.
    #[test]
    fn an_unknown_field_survives_being_rewritten_by_an_older_build() {
        let root = temp_root("forward-compat");
        fs::write(
            root.join(LAYOUTS_FILE),
            r#"{"layouts":[{"name":"desk","regions":{"left":"agent"},
                "weight":0.6,"note":{"by":"a newer console"}}],"schema":"2"}"#,
        )
        .unwrap();
        let mut lib = Library::load(&root);
        assert_eq!(lib.names(), vec!["desk"], "the known fields still load");
        assert_eq!(lib.layouts[0].extra.get("weight").and_then(Value::as_f64), Some(0.6));
        assert_eq!(lib.extra.get("schema").and_then(Value::as_str), Some("2"));

        // A second layout is added and the file rewritten — by a build that has never heard of
        // `weight`, `note` or `schema`.
        lib.upsert(SavedLayout::capture("other", &Layout::default()));
        lib.save(&root).unwrap();
        let back = Library::load(&root);
        assert_eq!(back.names(), vec!["desk", "other"]);
        assert_eq!(back.layouts[0].extra.get("weight").and_then(Value::as_f64), Some(0.6));
        assert!(back.layouts[0].extra.contains_key("note"), "a nested unknown survives too");
        assert_eq!(back.extra.get("schema").and_then(Value::as_str), Some("2"));
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT, both halves: we never write a BOM, and a BOM'd file is simply malformed —
    /// [`crate::prefs`]'s rule, and two files in one directory that disagreed about their own
    /// encoding would be worse than one rule applied everywhere.
    #[test]
    fn a_bom_is_never_written_and_is_not_tolerated_on_read() {
        let root = temp_root("bom");
        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("desk", &two_columns()));
        lib.save(&root).unwrap();
        let bytes = fs::read(root.join(LAYOUTS_FILE)).unwrap();
        assert_ne!(&bytes[..3], b"\xEF\xBB\xBF".as_slice(), "a BOM here is silently unreadable");
        assert_eq!(bytes[0], b'{', "the first byte is the JSON itself");
        assert!(bytes.ends_with(b"\n"), "a trailing newline, so it reads like a text file");

        let mut bommed = b"\xEF\xBB\xBF".to_vec();
        bommed.extend_from_slice(&bytes);
        fs::write(root.join(LAYOUTS_FILE), &bommed).unwrap();
        assert_eq!(Library::load(&root), Library::default());
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a save replaces the file whole and strands no temp. ⚠️ The temp is the half that
    /// has to be checked — one per save would accumulate, and on Windows the
    /// rename-over-an-existing-target is the step that fails outright with the wrong API.
    #[test]
    fn a_save_replaces_the_file_and_strands_no_temp() {
        let root = temp_root("replace");
        for name in ["first", "second"] {
            let mut lib = Library::load(&root);
            lib.upsert(SavedLayout::capture(name, &Layout::default()));
            lib.save(&root).unwrap();
        }
        let left: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec![LAYOUTS_FILE.to_string()], "the store holds one file, not a litter");
        assert_eq!(Library::load(&root).names(), vec!["first", "second"]);
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the first layout a person ever saves is also the first time the directory is
    /// needed, so a save creates it rather than failing.
    #[test]
    fn a_save_creates_the_store_directory() {
        let root = temp_root("create").join("not-yet");
        assert!(!root.exists());
        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("desk", &Layout::default()));
        lib.save(&root).unwrap();
        assert_eq!(Library::load(&root).names(), vec!["desk"]);
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    /// CONTRACT: **one resolver.** Layouts land beside `harnesses.json` and `preferences.json`,
    /// in the directory the session log already names.
    #[test]
    fn the_store_root_is_the_one_the_rest_of_the_crate_uses() {
        assert_eq!(Library::store_root(), SessionLog::store_root());
        assert_eq!(Library::store_root(), crate::prefs::Preferences::store_root());
    }

    /// 🚨 **The library ships empty, and the merge seam is still exercised.** The scope line is
    /// that naming the presets is James's call; the machinery that would carry them is tested by
    /// seeding one here rather than by shipping one nobody has looked at.
    #[test]
    fn the_shipped_library_is_empty_and_the_merge_over_it_is_still_a_merge() {
        assert!(builtin().is_empty(), "no preset ships until somebody has seen it on a screen");
        assert_eq!(Library::load(&temp_root("empty-builtin")), Library::default());

        let seeded = vec![
            SavedLayout::capture("desktop", &two_columns()),
            SavedLayout::capture("standalone", &Layout::default()),
        ];
        let user = Library {
            layouts: vec![
                saved("desktop", &[("full", "3d"), ("bottom", "agent")]),
                saved("mine", &[("full", "agent")]),
            ],
            extra: BTreeMap::new(),
        };
        let merged = merge_over(seeded, user);
        assert_eq!(merged.names(), vec!["desktop", "standalone", "mine"], "replace by name, append new");
        assert_eq!(
            merged.get("desktop").unwrap().regions.get("full").map(String::as_str),
            Some("3d"),
            "the user's entry wins outright"
        );
        assert_eq!(merged.layouts.iter().filter(|l| l.name == "desktop").count(), 1, "no duplicates");
    }

    /// 🚨 **An unchanged built-in is not written into the user's file**, so filling the library
    /// later cannot freeze today's presets into everybody's store. With nothing shipped this is
    /// the identity, which is exactly why the built-ins are seeded here.
    #[test]
    fn an_unchanged_builtin_is_not_written_into_the_user_file() {
        let root = temp_root("builtin-not-written");
        let shipped = SavedLayout::capture("desktop", &two_columns());
        let seeded = vec![shipped];
        let mut lib = merge_over(seeded.clone(), Library::default());
        lib.upsert(SavedLayout::capture("mine", &Layout::default()));
        lib.save_over(&root, &seeded).unwrap();

        let text = fs::read_to_string(root.join(LAYOUTS_FILE)).unwrap();
        assert!(!text.contains("desktop"), "an untouched built-in stays in the code: {text}");
        assert!(text.contains("mine"), "…and the user's own layout is stored: {text}");

        // …and one the user HAS changed is stored, because it is theirs now.
        let mut lib = merge_over(seeded.clone(), Library::load(&root));
        lib.upsert(saved("desktop", &[("full", "agent")]));
        lib.save_over(&root, &seeded).unwrap();
        let text = fs::read_to_string(root.join(LAYOUTS_FILE)).unwrap();
        assert!(text.contains("desktop"), "an overridden built-in is the user's: {text}");
        let _ = fs::remove_dir_all(&root);
    }
}
