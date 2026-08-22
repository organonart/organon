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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::region::{self, Content, Layout, LayoutFault, Region, UnknownWord, CLEAR_WORD};
use crate::session::SessionLog;

/// The layout library, at the store root beside `harnesses.json` and `preferences.json`.
pub const LAYOUTS_FILE: &str = "layouts.json";

/// What an empty library says, in the **one** sentence every surface that meets one reads.
///
/// 🚨 **Two surfaces, one string.** `console.layout.list` answers a read against an empty
/// library with it, and `registry`'s `layout_options` hands it to `Ring::Empty` so that
/// `/layout load ` with nothing saved says the same thing rather than drawing a band with
/// nothing in it — the precedent `Ring::Empty` exists for. A second sentence written for the
/// ring would be a second answer to one question, and the two would drift the day the verb
/// were renamed.
///
/// ⚠️ It names the verb that fills the library rather than the file, because only one of the
/// two readers has a file path in hand — the read carries it separately, under `file`.
pub const NOTHING_SAVED: &str = "nothing has been saved yet — `console layout save <name>` \
                                 writes the console's current arrangement to the layout library";

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
    /// Region word → what it holds, in [`Region::ALL`] order when this console wrote it.
    ///
    /// ✏️ **The value is a PHRASE since T4, not always one word** — `3d ascent` where a viewport
    /// names a producer, `3d` where it is Organon's. [`crate::region::Content::as_words`] writes
    /// it and [`resolve`] splits it on whitespace. 🚨 **Every file written before T4 is
    /// byte-identical under this rule**, because Organon's producer contributes no word at all;
    /// that is the whole reason an omitted qualifier means Organon rather than the reverse.
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
                // ⚠️ **The whole PHRASE, not the kind word** — `3d ascent` where a producer was
                // named, `3d` where it was Organon. [`Content::as_words`] owns that asymmetry,
                // and it is why a `layouts.json` written before T4 and one written after it are
                // the same bytes for the same arrangement.
                .map(|(r, c)| (r.as_word().to_string(), c.as_words()))
                .collect(),
            extra: BTreeMap::new(),
        }
    }

    /// What this arrangement holds, as one line — `left agent, right panel`.
    ///
    /// The doc beside a name in `/layout load `'s ring, on `/organon`'s arrangement: an option
    /// carries what choosing it is *worth*, so a library of four names is not four words a
    /// person has to remember the meaning of. **Read out of the stored regions**, so it cannot
    /// describe an arrangement other than the one that would load.
    ///
    /// ⚠️ A layout that places nothing says so rather than rendering as an empty line. It is a
    /// file `resolve` refuses ([`LayoutFault::Empty`]), and a blank doc would read as a ring
    /// that had failed to describe it.
    pub fn holds(&self) -> String {
        if self.regions.is_empty() {
            return "places nothing".to_string();
        }
        self.regions.iter().map(|(r, c)| format!("{r} {c}")).collect::<Vec<_>>().join(", ")
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

/// How long [`Library::for_completion`] may answer from memory.
///
/// 🚨 **A ceiling on how wrong the ring may be, chosen against two clocks.** A person types at
/// 5–10 keystrokes a second and the console draws at 60 frames a second, so anything above one
/// frame collapses the ~12 draws between two keystrokes into one read; and 200 ms is under the
/// ~250 ms at which a delay stops feeling immediate, so a file edited by hand or by a second
/// console is in the ring before anybody could notice it was not. **This console's own writes
/// do not wait for it at all** — see [`Library::forget_completion_cache`].
const RING_TTL: Duration = Duration::from_millis(200);

/// What [`Library::for_completion`] is holding, if anything.
struct RingCache {
    /// The store it was read from. Part of the key — see [`Library::for_completion`].
    root: PathBuf,
    /// ⚠️ **An `Arc`, so a caller pays a refcount rather than a clone.** The whole point is that
    /// the walk asks n + 1 times per call; cloning a hundred-layout library that often would
    /// have moved the cost from parsing to allocating rather than removed it.
    library: Arc<Library>,
    taken: Instant,
}

/// ⚠️ **Poisoning is absorbed, never unwrapped.** A panic elsewhere while this lock was held
/// must not turn a completion ring into a second panic — [`Library::load`]'s totality rule, which
/// is the module's posture throughout: a broken library costs you your layouts, never your
/// console.
fn ring_cache() -> &'static Mutex<Option<RingCache>> {
    static CACHE: OnceLock<Mutex<Option<RingCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

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

    /// [`Library::load`] for a **completion ring** — the same answer, at a hundredth of the
    /// cost, because this one is asked while somebody is typing.
    ///
    /// 🚨 **The measurement is why this exists, and it is the one `console_main`'s
    /// `console.layout.list` note explicitly did not have.** `/layout load ` narrows to the
    /// names that exist (`registry`'s `layout_options`), and the candidate walk runs on the
    /// **draw path** — `ConversationPane::palette` is called while the composer band is drawn,
    /// so it is per *frame*, not per keystroke. Worse, `value_candidates` asks the ring once
    /// and then calls `settled` per candidate, each of which reaches `coerce` and asks again:
    /// **n + 1** reads for a library of n. Measured in release by [`tests::library_read_cost`]
    /// on organon-one, 2026-08-20:
    ///
    /// | n | read + parse (median of 3) | per call (n+1) |
    /// |---|---|---|
    /// | 1 | 24.2 µs | 0.05 ms |
    /// | 10 | 24.5 µs | 0.27 ms |
    /// | 100 | 100.2 µs | **10.1 ms** |
    ///
    /// A frame is 16.7 ms. So an uncached ring is fine at the size a library actually has and
    /// falls over at a size it may reach — and it would fall over *while typing*, which is the
    /// worst place to spend a frame. **A `stat` is not the fix**: 12 µs measured, so ×101 is
    /// still 1.2 ms per call, and this is asked several times a frame.
    ///
    /// ⚠️ **Three runs, because the first one was wrong and said so.** A single run taken while
    /// other builds were on the machine reported 75.2 µs at n=1 against 43.6 µs at n=10 — a
    /// 138-byte file costing more than a 1182-byte one, which cannot be true and is the tell
    /// that the number is measuring contention rather than the file. The spread across three
    /// quiet runs is 23.1–27.3 / 24.0–27.1 / 98.8–101.4 µs. **Re-take it on a quiet machine and
    /// distrust any run whose n=1 is not the cheapest.**
    ///
    /// 📌 The real library on organon-one is **197 bytes, one layout, four regions** — between
    /// the n=1 and n=10 rows, so what ships today sits at the cheap end of this table. The
    /// hundred-layout row is the one the cache exists for.
    ///
    /// ⚠️ **What invalidates it, and both halves are needed.** A `save` and a `delete` both
    /// rewrite the file through [`Library::save_over`], which forgets this cache outright — so
    /// a name you have just saved is in the ring on the very next frame, with no window in
    /// which the console contradicts itself. Everything *else* — a hand-edited file, a second
    /// console — is covered by [`RING_TTL`], which is why the cache cannot "fight a hand-edited
    /// one and win silently": it can only win for 200 ms.
    ///
    /// ⚠️ **The reads keep re-reading.** `console.layout.list` and `Console::set_layout`'s own
    /// load path deliberately do **not** use this: they are asked once, by a person, and for
    /// them the file is the truth with no staleness worth trading for. This is the completion
    /// surface's cache, not the module's.
    pub fn for_completion(store_root: &Path) -> Arc<Self> {
        let mut slot = ring_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = slot.as_ref() {
            // The root is part of the key: a test store and the real one are two libraries, and
            // a cache that answered one for the other would be a wrong ring rather than a slow
            // one.
            if cached.root == store_root && cached.taken.elapsed() < RING_TTL {
                return Arc::clone(&cached.library);
            }
        }
        let library = Arc::new(Self::load(store_root));
        *slot = Some(RingCache {
            root: store_root.to_path_buf(),
            library: Arc::clone(&library),
            taken: Instant::now(),
        });
        library
    }

    /// Drop what [`Library::for_completion`] is holding. Called by [`Library::save_over`] — the
    /// **one** path every write takes, which is what makes "a save and a delete both invalidate
    /// it" a property of the writer rather than of two call sites remembering.
    fn forget_completion_cache() {
        *ring_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
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
            Ok(()) => {
                // 🚨 **Here, because this is the one path a write can take.** `save`,
                // `save_default` and every `delete` (which is a `remove` and then a rewrite)
                // funnel through this function, so the completion ring cannot be left holding a
                // name that has just been taken out — or missing one just put in. Doing it at
                // the call sites instead would be two places to remember and a third the day a
                // fourth action is added.
                Self::forget_completion_cache();
                Ok(())
            }
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
    for (region_word, held) in &saved.regions {
        let region = Region::resolve(region_word).map_err(Refusal::UnknownRegion)?;
        // 🚨 **The stored value is a PHRASE since T4** — `3d` or `3d ascent` — so it is split
        // here rather than read as one word. Splitting on whitespace is the same rule the
        // sidecar line lives by, and it is why `module::check_producer_name` forbids whitespace
        // in a producer name: a two-word producer would arrive as a producer and a stray word.
        let mut words = held.split_whitespace();
        let content_word = words.next().unwrap_or("");
        let producer_word = words.next();
        if let Some(extra) = words.next() {
            return Err(Refusal::TooManyWords { region: region_word.clone(), extra: extra.to_string() });
        }
        // Named separately from every other unknown word, because it is the one a person is
        // likeliest to write by hand — it is a word the *command* takes — and "not a content" is
        // a poor answer to it. What is wrong is that a region holding nothing is simply absent.
        if content_word == CLEAR_WORD {
            return Err(Refusal::ClearWordStored { region: region_word.clone() });
        }
        // 🚨 **`resolve_stored`, which does NOT ask whether the producer is approved.** Modules
        // plan §10 and `doc/organon_module_viewport.md` §3.5: *a layout referencing a module you
        // have stopped trusting must not fail to open.* A revoked module is a region whose
        // producer declines to run — a sentence in the rectangle — and refusing the load here
        // would turn a revocation into an arrangement that will not come back.
        //
        // ⚠️ **The unknown-CONTENT arm is kept as it was** rather than folded into the new one:
        // a stored word this build does not have is the refusal §1.15's table already describes
        // and the sentence a person may already have seen, and re-routing it would change what
        // an old `layouts.json` says on a new console for no gain.
        let content = Content::resolve_stored(content_word, producer_word).map_err(|e| match e {
            region::ContentRefusal::UnknownContent(u) => Refusal::UnknownContent(u),
            other => Refusal::NotHoldable(other),
        })?;
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
    /// The stored phrase is two legal-looking words that are not something a region can hold —
    /// a producer name nothing could answer to, or a producer beside a content kind that has
    /// none. ⚠️ **Carried rather than restated**, so [`crate::region::ContentRefusal`] and this
    /// cannot drift into two explanations of one rule.
    ///
    /// 🚨 **An UNAPPROVED producer is not in here**, and that absence is §3.5 rather than an
    /// oversight — see [`resolve`].
    NotHoldable(region::ContentRefusal),
    /// The stored phrase has a third word. A region holds a content and at most a producer;
    /// anything after that is a file this build cannot read whole, and reading it in part is
    /// how an arrangement quietly becomes a different one.
    TooManyWords { region: String, extra: String },
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
            Refusal::NotHoldable(e) => write!(
                f,
                "this layout names something no region can hold — {e}. It is refused whole \
                 rather than loaded in part, for the reason a missing region is"
            ),
            Refusal::TooManyWords { region, extra } => write!(
                f,
                "this layout says `{region}` holds more than a content and a producer — `{extra}` \
                 is a third word, and a file this build cannot read whole is refused rather than \
                 read in part"
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
                for kind in Content::ALL.iter().cloned() {
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
                        "{a:?}/{b:?} did not survive the round trip"
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
        assert_eq!(built.get(Region::TopCenter), Some(Content::ThreeD(region::Producer::Organon)));

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

    /// 🚨 **A file written before T4 loads as exactly what it loaded before T4, and one written
    /// after it round-trips its producer.**
    ///
    /// The first half is the whole of *"an omitted producer means Organon"*, checked as **bytes**
    /// rather than as an intention: a layout captured from an Organon viewport stores the string
    /// `3d`, so an older console reads it and a newer one reads it the same way.
    ///
    /// ⚠️ **Mutation-tested.** Making `SavedLayout::capture` store `c.as_word()` again (the
    /// pre-T4 line) fails this at *"a hosted producer survives the file"* — the stored value
    /// becomes `3d` and the layout comes back as Organon's, which is the silent wrong-renderer
    /// outcome §4.2 forbids. Making `Producer::as_word` answer `Some("organon")` fails it at the
    /// first assertion instead.
    #[test]
    fn a_producer_survives_the_file_and_an_organon_viewport_still_writes_one_word() {
        let organon = Layout::default()
            .assign(Region::Left, ContentCmd::Hold(Content::Agent))
            .expect("left")
            .layout
            .assign(Region::Right, ContentCmd::Hold(Content::ThreeD(region::Producer::Organon)))
            .expect("right 3d")
            .layout;
        let stored = SavedLayout::capture("desk", &organon);
        assert_eq!(
            stored.regions.get("right").map(String::as_str),
            Some("3d"),
            "Organon's viewport writes the one word it has always written"
        );
        assert_eq!(resolve(&stored, Some(pane())), Ok(organon));

        let hosted_content =
            Content::ThreeD(region::Producer::Hosted("ascent".into()));
        let hosted = Layout::default()
            .assign(Region::Left, ContentCmd::Hold(Content::Agent))
            .expect("left")
            .layout
            .assign(Region::Right, ContentCmd::Hold(hosted_content.clone()))
            .expect("right 3d ascent")
            .layout;
        let stored = SavedLayout::capture("game", &hosted);
        assert_eq!(stored.regions.get("right").map(String::as_str), Some("3d ascent"));
        assert!(stored.holds().contains("right 3d ascent"), "{}", stored.holds());
        assert_eq!(
            resolve(&stored, Some(pane())),
            Ok(hosted),
            "a hosted producer survives the file"
        );

        // 🚨 **§3.5: a stored producer nothing has approved still LOADS.** `resolve` never
        // consults the approved set, so revoking a module leaves a region whose producer
        // declines to run rather than an arrangement that will not come back.
        let revoked = saved("was-approved", &[("left", "agent"), ("right", "3d never-approved")]);
        let built = resolve(&revoked, Some(pane())).expect("a revoked module is not a bad layout");
        assert_eq!(
            built.get(Region::Right),
            Some(Content::ThreeD(region::Producer::Hosted("never-approved".into())))
        );

        // …and a word no manifest could have declared is still refused, because that is a
        // different question from approval.
        let e = resolve(
            &saved("bad", &[("left", "agent"), ("right", "3d ascent extra")]),
            Some(pane()),
        )
        .expect_err("three words is not something a region holds");
        assert_eq!(
            e,
            Refusal::TooManyWords { region: "right".into(), extra: "extra".into() }
        );
        let e = resolve(&saved("bad", &[("left", "agent"), ("right", "agent ascent")]), Some(pane()))
            .expect_err("a producer qualifies `3d` and nothing else");
        assert!(matches!(e, Refusal::NotHoldable(_)), "{e:?}");
        assert!(e.to_string().contains("ascent"), "{e}");
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
        let mut live = standing.clone();
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

    /// 🚨 **Whichever of the two window rules a refusal names, that is the rule that actually
    /// refused it** — checked over every layout two placements can build, at panes that trip each
    /// rule, both, and neither.
    ///
    /// The property is stated as a *counterfactual* rather than by re-deriving the classification
    /// (which would just be the implementation, asserted against itself): if the answer is
    /// [`Refusal::TooSmall`], then **widening the pane must not rescue it** — because if widening
    /// did rescue it, the width was the real reason and the sentence quoting `MIN_SIDE` was
    /// describing something else.
    ///
    /// ⚠️ **This is the edge case the review of this fix went looking for, pinned rather than
    /// argued.** `region::plan` calls `region_rect` on the *vacant* regions it fills in as well
    /// as the occupied ones, while [`resolve`] inspects only what the layout holds — so a
    /// column-shaped **gap** would fail the plan with no occupied region needing a cut, and the
    /// refusal would fall through to the wrong sentence. Today that is unreachable: the only
    /// regions needing no cut are `full`, `top` and `bottom`, and a layout built from those
    /// leaves a filler that is itself `top`, `bottom` or nothing. That is a fact about **this**
    /// grid, and #98 Tier B is the proof that the grid moves — so it is measured here instead of
    /// resting on the walk somebody did once.
    #[test]
    fn a_window_refusal_always_names_the_rule_that_actually_refused_it() {
        let sizes = [
            (80.0, 400.0),   // too narrow for columns, tall enough for rows
            (500.0, 800.0),  // the review's own example: narrow only
            (700.0, 60.0),   // wide enough for columns, too short for rows
            (600.0, 60.0),   // both rules at once
            (1100.0, 690.0), // neither
        ];
        let mut seen_narrow = 0;
        let mut seen_small = 0;
        for a in Region::ALL.iter().copied() {
            for b in Region::ALL.iter().copied() {
                for kind in Content::ALL.iter().cloned() {
                    let Ok(built) =
                        Layout::from_placements(&[(a, Content::Agent), (b, kind)])
                    else {
                        continue;
                    };
                    let stored = SavedLayout::capture("x", &built);
                    for (w, h) in sizes {
                        let pane =
                            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h));
                        match resolve(&stored, Some(pane)) {
                            Ok(_) => assert!(
                                region::plan(pane, &built).is_some(),
                                "{a:?}/{b:?} at {w}×{h} loaded but does not plan"
                            ),
                            Err(Refusal::TooNarrowForColumns { region, width, .. }) => {
                                seen_narrow += 1;
                                assert!(width < region::MIN_COLUMNS_WIDTH);
                                assert!(
                                    region.needs_column_cut(),
                                    "{region:?} was named and needs no cut"
                                );
                                assert!(
                                    built.get(region).is_some(),
                                    "{region:?} was named and the layout does not hold it"
                                );
                            }
                            Err(Refusal::TooSmall { .. }) => {
                                seen_small += 1;
                                // 🚨 The counterfactual: the same layout, the same height, a pane
                                // wide enough for the columns. If it plans there, the width was
                                // the reason and this refusal named the wrong rule.
                                let wide = egui::Rect::from_min_size(
                                    egui::pos2(0.0, 0.0),
                                    egui::vec2(w.max(region::MIN_COLUMNS_WIDTH), h),
                                );
                                assert!(
                                    region::plan(wide, &built).is_none(),
                                    "{a:?}/{b:?} at {w}×{h} was refused for MIN_SIDE, but \
                                     widening to {} rescues it — the width was the real reason",
                                    region::MIN_COLUMNS_WIDTH
                                );
                            }
                            Err(other) => panic!("{a:?}/{b:?} at {w}×{h}: {other:?}"),
                        }
                    }
                }
            }
        }
        // Both arms were actually reached — a property test that silently exercised neither
        // would pass on a `resolve` that never refuses at all.
        assert!(seen_narrow > 0 && seen_small > 0, "{seen_narrow} narrow, {seen_small} small");
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

    /// 🚨 **A save and a delete are both visible to the completion ring immediately** — the
    /// property [`Library::for_completion`]'s cache is only allowed to exist because of. A
    /// person who saves `desk` and then types `/layout load ` must see `desk`, and one who
    /// deletes it must not; a cache that made either wait on [`RING_TTL`] would be the console
    /// contradicting itself about a file it wrote a moment ago.
    ///
    /// ⚠️ **The invalidation lives in [`Library::save_over`]**, the one path every write takes,
    /// which is why `delete` — a `remove` and then a rewrite — needs no second call site.
    #[test]
    fn what_the_console_just_wrote_is_in_the_ring_on_the_next_frame() {
        let root = temp_root("ring-cache");
        assert!(Library::for_completion(&root).layouts.is_empty(), "nothing saved yet");

        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("desk", &two_columns()));
        lib.save(&root).unwrap();
        assert_eq!(
            Library::for_completion(&root).names(),
            vec!["desk"],
            "a save is not waiting on a TTL"
        );

        let mut lib = Library::load(&root);
        assert!(lib.remove("desk"));
        lib.save(&root).unwrap();
        assert!(
            Library::for_completion(&root).layouts.is_empty(),
            "a delete is not waiting on a TTL either"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the store the ring was asked about is the store it answers. ⚠️ The root is part
    /// of the cache key precisely so a stale entry can only ever be *stale*, never *another
    /// library* — which would be a wrong ring rather than a slow one.
    #[test]
    fn the_completion_cache_never_answers_one_store_with_another() {
        let mine = temp_root("ring-mine");
        let theirs = temp_root("ring-theirs");
        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("mine", &Layout::default()));
        lib.save(&mine).unwrap();
        let mut lib = Library::default();
        lib.upsert(SavedLayout::capture("theirs", &Layout::default()));
        lib.save(&theirs).unwrap();

        for _ in 0..3 {
            assert_eq!(Library::for_completion(&mine).names(), vec!["mine"]);
            assert_eq!(Library::for_completion(&theirs).names(), vec!["theirs"]);
        }
        // ⚠️ **No assertion here that two calls return the same `Arc`**, tempting as it is: the
        // cache is process-global and `cargo test` runs this module in parallel, so any other
        // test's save legitimately clears it between the two calls. Sharing is a cost property
        // and belongs to [`tests::library_read_cost`]'s `cached` column; a test that could fail
        // because a *different* test wrote a file would be measuring the scheduler.
        let _ = fs::remove_dir_all(&mine);
        let _ = fs::remove_dir_all(&theirs);
    }

    /// **What a layout ring costs: `layouts.json` read and parsed, once.** A measurement rather
    /// than a gate — the finding it produced is `CONSOLE_ARCHITECTURE.md` §1.15, and the
    /// instrument is kept here so the number can be re-taken on another machine after a
    /// `serde_json` bump rather than believed.
    ///
    /// The question it was built to answer: `/layout load ` narrows to the names that exist
    /// (`registry`'s `layout_options`), and the candidate walk runs on **every keystroke**.
    /// Worse than once — `value_candidates` asks the ring, then calls `settled` per candidate,
    /// and `settled` runs `resolve` → `coerce`, which consults the same hook again. So one
    /// keystroke against a library of `n` costs **n + 1** reads, which is the column that
    /// decided the design.
    ///
    /// ```text
    /// cargo test --release -p organon-console --lib -- --ignored --nocapture library_read_cost
    /// ```
    ///
    /// ⚠️ **`--release`, because the console ships release** — `conversation_view::rewrap_bench`'s
    /// rule, for its reason: `[profile.dev]` is `opt-level = 1` and reports high. `#[ignore]`
    /// because a timing is not a correctness gate, and nothing here asserts a duration: a
    /// machine slower than this one would then fail a suite for being slow.
    #[test]
    #[ignore]
    fn library_read_cost() {
        use std::time::Instant;
        const ROUNDS: u32 = 200;
        println!(
            "   n | read+parse | stat only  | cached     | per call (n+1) raw → cached"
        );
        for n in [1usize, 10, 100] {
            let root = temp_root(&format!("cost-{n}"));
            let mut lib = Library::default();
            for i in 0..n {
                // The shape a real library has: a name somebody would type, a two-region
                // arrangement. A library of empty layouts would measure the wrong file.
                lib.upsert(SavedLayout::capture(&format!("desk-{i:03}"), &two_columns()));
            }
            lib.save(&root).unwrap();
            let bytes = fs::metadata(root.join(LAYOUTS_FILE)).unwrap().len();
            assert_eq!(Library::load(&root).names().len(), n, "the file really holds {n}");

            // Warmed first: the question is what a keystroke costs while somebody is typing,
            // and the very first read of a file nobody has touched is not that.
            for _ in 0..20 {
                std::hint::black_box(Library::load(&root));
            }
            let t = Instant::now();
            for _ in 0..ROUNDS {
                std::hint::black_box(Library::load(&root));
            }
            let read = t.elapsed().as_secs_f64() / f64::from(ROUNDS);

            // The cheaper thing a cache would do instead, measured so that "cache it" is a
            // comparison rather than an assumption.
            let t = Instant::now();
            for _ in 0..ROUNDS {
                std::hint::black_box(fs::metadata(root.join(LAYOUTS_FILE)).map(|m| m.len()));
            }
            let stat = t.elapsed().as_secs_f64() / f64::from(ROUNDS);

            // What the ring actually pays. ⚠️ The first call is a real read, so it is taken
            // outside the loop — averaging it in would report a cache that does not exist.
            std::hint::black_box(Library::for_completion(&root));
            let t = Instant::now();
            for _ in 0..ROUNDS {
                std::hint::black_box(Library::for_completion(&root));
            }
            let cached = t.elapsed().as_secs_f64() / f64::from(ROUNDS);

            println!(
                "{n:>4} | {:>7.1} µs | {:>7.1} µs | {:>7.3} µs | {:>7.2} ms → {:>6.3} ms   \
                 ({bytes} bytes)",
                read * 1e6,
                stat * 1e6,
                cached * 1e6,
                read * (n as f64 + 1.0) * 1e3,
                cached * (n as f64 + 1.0) * 1e3,
            );
            // The cache is dropped between sizes, so the next `n` measures its own file rather
            // than answering with this one's.
            Library::forget_completion_cache();
            let _ = fs::remove_dir_all(&root);
        }
    }
}
