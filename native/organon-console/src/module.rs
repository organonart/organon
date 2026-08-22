//! **The module registry — what "approve a repo" means, written down as data.**
//!
//! `doc/organon_module_viewport.md` §3 is the specification this is built against, and
//! `doc/organon_modules_plan.md` §11 is where its central rule comes from. This tier is T3a:
//! the **two files and the types that decide them**, and nothing that starts a process. No
//! clone, no `cargo build`, no launch — those are T3b, and they want these types to exist
//! first. Everything here is a pure function of a string, a path or a value, so every
//! decision is a headless test.
//!
//! # 🚨 Two files, two authors, and never one file
//!
//! §3.1, and it is the constraint the whole module is shaped around:
//!
//! | | [`ModuleManifest`] | [`ApprovedModule`] |
//! |---|---|---|
//! | Lives | `organon-module.toml`, in the module's own repo | `<store root>/modules.json`, beside `harnesses.json` |
//! | Written by | the module's author | Organon, at a person's instruction |
//! | Says | *"I am a viewport producer called `ascent`; I need these things"* | *"this URL at this commit is approved, and holds these grants"* |
//! | Trust | **data, never instruction** | the console's own record |
//!
//! **A manifest must never be able to grant itself anything**, and here that is structural
//! rather than a convention someone has to remember. Three things make it so, and it takes all
//! three:
//!
//! 1. The two sets of grant names are two *types* — [`Requested`] and [`Granted`] — with no
//!    conversion between them except [`Requested::grant`], which takes the names a person
//!    chose and refuses one that was never asked for.
//! 2. [`ApprovedModule::approve`] takes those **names**, not a finished [`Granted`], and
//!    derives the grant from the manifest it is approving. ⚠️ **This is the part an earlier
//!    version got wrong**: taking a `Granted` stopped a manifest granting *itself* anything
//!    and still let a caller pair manifest A with a `Granted` computed from manifest B's
//!    requests. Taking names leaves no pair to mismatch.
//! 3. There is no `From<ModuleManifest> for ApprovedModule`, and [`tests`] carries a
//!    coherence tripwire that stops the crate compiling if one is added.
//!
//! ⚠️ **Where "structural" stops, stated rather than left to be assumed.** It is a property of
//! the approval *step*. [`ApprovedModule`]'s fields are public — as `SavedLayout`'s and
//! `HarnessSpec`'s are — and `modules.json` is a file a person can edit, so a record can be
//! *assembled* holding grants no manifest requested. Nothing here can catch that, because a
//! manifest lives in a repo and [`ModuleRegistry::load`] has a file and no repository.
//! [`ApprovedModule::check`] validates everything a file can be wrong about **on its own**;
//! grants are not one of those things, and a check that pretended otherwise would be the
//! security property people believe they have that §3.4 warns about, one layer down.
//!
//! # 🚨 The unit is a commit
//!
//! §3.2, from the modules plan §11.3: a repo says *where the bytes live*, a commit says
//! *which bytes*. Tags move, branches move, force-push rewrites history. So an approval
//! record carries a URL **and a commit hash**, and a record naming only a branch is
//! **refused by name on load** ([`ModuleFault::NoCommit`]) rather than quietly accepted —
//! a reference that names only a branch is a reference that has not decided what it trusts.
//!
//! ⚠️ **A hash is 40 **or 64** hex characters.** Git is migrating to SHA-256 and a 64-char
//! object name is not an error; [`is_commit_hash`] accepts both and nothing here assumes 40.
//!
//! # 🚨 The commit that was BUILT is a different field from the commit that was APPROVED
//!
//! §3.4's cheap corollary, taken. [`ApprovedModule::commit`] is what a person approved;
//! [`BuildRecord::commit`] is what a build actually consumed, beside a `dirty` flag for the
//! case where the tree was not a commit at all. They are normally equal, and **the record is
//! a lie exactly when they silently are not** — so [`BuildRecord::names_approved_bytes`] is
//! the one place that compares them, and [`ModuleRegistry::vacancy`] uses it rather than
//! trusting the mere presence of a build.
//!
//! Nothing builds yet, so [`ApprovedModule::built`] is an `Option` that [`ApprovedModule::approve`]
//! always leaves empty. The *vocabulary* has to exist now because §4.6 needs to say
//! "approved, not built" in a rectangle.
//!
//! # The four states, and why this file carries two of them
//!
//! §4.6 names four things a region's rectangle must be able to say. [`ModuleState`] carries
//! the two that are reachable without a process: **not approved**, and **approved, not
//! built**. The other two — *launched, not yet producing* and *died / stopped producing* —
//! need something running, and nothing in this tier starts anything.
//!
//! 🚨 That omission is `region.rs`'s standing rule, applied: *"an unreachable arm is an
//! untested branch pretending to be a design."* The design document records the other two
//! as coming; the code does not carry dead arms for them.
//!
//! [`ModuleRegistry::vacancy`] answers `None` when there is nothing standing in the way,
//! which is §1.14's vacancy rule read the right way round: a rectangle draws a **sentence**
//! instead of a picture only when it cannot draw the picture. When the producer works, the
//! picture is what it says.
//!
//! # 🚨 The registry ships EMPTY, and here that is stronger than scope
//!
//! [`crate::layout`] ships an empty library because naming presets nobody asked for is not
//! scope. This file ships empty for a harder reason: **an approval seeded in code is a grant
//! Organon wrote on your behalf**, which is precisely what §3.1 forbids. So there is no
//! `builtin()` here, and there must never be one — every entry in `modules.json` got there
//! because somebody approved it.
//!
//! # The failure posture
//!
//! [`crate::layout`]'s, verbatim: **a corrupt library costs you your modules, never your
//! console.** [`ModuleRegistry::load`] is total — it never errors and never panics — but it
//! does not answer with silence either: every record it drops comes back as a
//! [`ModuleFault`] with a sentence, because §3.2 and §3.5 both say *refused by name*. A
//! whole file that will not parse is one fault rather than none, which is the one place this
//! is deliberately louder than `layouts.json`: a modules file that evaporates silently looks
//! identical to one you never wrote.
//!
//! Writing follows [`crate::prefs::Preferences::save`] exactly — temp file in the *same*
//! directory, then rename, plain UTF-8, never a BOM.
//!
//! # What T4 consumes, and the measurement it must not re-learn
//!
//! §4.2 makes producer names a **second, dynamic vocabulary** — `3d ascent` beside `3d`.
//! §1.15 already measured what a candidate walk over a stored library costs: it runs on the
//! **draw** path and asks n + 1 times per call, so a hundred entries is 10.1 ms against a
//! 16.7 ms frame. [`ModuleRegistry::for_completion`] is that measurement's answer, moved here
//! rather than left for T4 to rediscover — the same 200 ms TTL, the same store-root key, and
//! the same invalidation on write.
//!
//! ⚠️ **The ring itself is NOT built here.** No `NarrowFn`, no registry entry, no content
//! word: T4 owns the vocabulary. What this owes T4 is an API a cache sits over, and that is
//! all it provides.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::SessionLog;

/// Organon's own record of what has been approved, at the store root beside
/// `harnesses.json`, `layouts.json` and `preferences.json`.
pub const MODULES_FILE: &str = "modules.json";

/// What a module's repo calls the file that describes it, at the repo root.
///
/// Named here rather than in whatever eventually clones a repo, so the manifest's name and
/// the type that parses it cannot drift apart.
pub const MANIFEST_FILE: &str = "organon-module.toml";

/// The producer an omitted qualifier means — Organon's own `World`.
///
/// 🚨 **Reserved, and refused to a module** ([`check_producer_name`]). §4.2 settles that
/// `3d` with no producer means Organon, so a module calling itself `organon` would be a
/// module that can impersonate the one producer the console wrote. T4 reads this constant
/// for the same decision; a second spelling of it is how the two would come to disagree.
pub const DEFAULT_PRODUCER: &str = "organon";

/// The one kind of module this build hosts.
///
/// §8 keeps a module contributing a *panel*, a *command* or a *harness* as separate designs
/// with separate grants. A manifest declaring anything else is refused by name rather than
/// half-understood.
pub const VIEWPORT_KIND: &str = "viewport";

/// The longest a producer name may be. [`crate::layout::MAX_NAME`]'s ceiling, for its reason:
/// a name is typed at a prompt and echoed in refusals that list every other one.
pub const MAX_PRODUCER: usize = 64;

/// The verb that turns a repo into an approval record.
///
/// ⚠️ **Not registered yet — T3b builds it**, and this constant exists so that when it is,
/// the sentence in the rectangle and the verb a person types come from one string. A
/// refusal that names a verb which is spelled differently in the command table is a refusal
/// that cannot be acted on.
pub const APPROVE_VERB: &str = "console module approve";

/// The verb that turns an approval record into something launchable. Same caveat as
/// [`APPROVE_VERB`]: T3b registers it, this is where its spelling lives.
pub const BUILD_VERB: &str = "console module build";

/// How much of a commit hash a sentence in a rectangle shows.
///
/// The record keeps the whole hash — that is the point of §3.2 — but forty characters in a
/// rectangle is a wall a person reads past. Twelve is long enough that a collision is not a
/// practical concern in one repository and short enough to sit in a line of prose.
const SHORT_COMMIT: usize = 12;

// ---------------------------------------------------------------------------------------
// Grants: two types, because one type would be a permission system where the request and
// the answer come from the same party.
// ---------------------------------------------------------------------------------------

/// What a manifest **asks for**. Data written by someone else; it takes effect nowhere.
///
/// The only route out of this type is [`Requested::grant`], which takes the names a person
/// chose. There is deliberately no `grant_all`, no `Into<Granted>` and no `Default`-shaped
/// shortcut: a call that turns every request into a grant with no argument is the shape §3.1
/// forbids, whatever it is named.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Requested(BTreeSet<String>);

/// What a person **granted**. Organon's own record, and the only thing an approval carries.
///
/// 📌 `CLAUDE.md` invariant 4 arriving where it always applies: [`Granted::none`] is what a
/// module gets unless somebody says otherwise, so a new capability defaults to inert.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Granted(BTreeSet<String>);

impl Requested {
    /// A request set, for a manifest a test writes by hand.
    pub fn of(names: &[&str]) -> Self {
        Requested(names.iter().map(|s| s.to_string()).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    /// The names asked for, in a stable order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Grant exactly `chosen` of what was requested.
    ///
    /// 🚨 **The only bridge between the two types, and the one site of the rule.** The argument
    /// is what a person picked, never what the manifest asked for.
    ///
    /// 📌 [`ApprovedModule::approve`] calls this on the manifest it is approving rather than
    /// accepting a [`Granted`] from its caller, which is what stops one manifest's grants
    /// being attached to another's approval. So a [`Granted`] built by hand here is a
    /// **preview** — a surface can use it to show what a selection would grant, and nothing
    /// consumes it but the stored record.
    ///
    /// ⚠️ **A name that was not requested is refused** ([`ModuleFault::NotRequested`]) rather
    /// than granted anyway. Granting something nobody asked for is the console inventing a
    /// permission on a module's behalf — the mirror image of the failure this type split
    /// exists to prevent, and it would just as quietly widen what a module may do.
    pub fn grant(&self, chosen: &[&str]) -> Result<Granted, ModuleFault> {
        let mut granted = BTreeSet::new();
        for name in chosen {
            if !self.0.contains(*name) {
                return Err(ModuleFault::NotRequested { grant: (*name).to_string() });
            }
            granted.insert((*name).to_string());
        }
        Ok(Granted(granted))
    }
}

impl Granted {
    /// Nothing granted — what an approval carries unless a person said otherwise.
    pub fn none() -> Self {
        Granted(BTreeSet::new())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    /// The names granted, in a stable order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

// ---------------------------------------------------------------------------------------
// Names and hashes
// ---------------------------------------------------------------------------------------

/// Is this a name a module may call itself?
///
/// Five rules, each a fact rather than a taste:
///
/// * **Empty** — a producer with no name cannot be asked for.
/// * **Longer than [`MAX_PRODUCER`]** — every refusal that lists the approved modules
///   becomes unreadable.
/// * **Whitespace** — a console op crosses the sidecar as one whitespace-delimited line
///   ([`crate::layout::check_name`]'s measurement, and the same wire), so a two-word producer
///   would arrive truncated: a command that appears to work and names something else.
/// * **A control character** — it corrupts the line it travels on.
/// * **[`DEFAULT_PRODUCER`]** — reserved for Organon's own `World`, because `3d` with no
///   qualifier already means it.
pub fn check_producer_name(name: &str) -> Result<(), ModuleFault> {
    let bad = |why: &'static str| {
        Err(ModuleFault::BadProducer { producer: name.to_string(), why })
    };
    if name.is_empty() {
        return bad("a module needs a name to be asked for by");
    }
    if name.chars().count() > MAX_PRODUCER {
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
    if name == DEFAULT_PRODUCER {
        return bad("`organon` is the producer a viewport means when no producer is named");
    }
    Ok(())
}

/// Is this a git object name?
///
/// ⚠️ **40 or 64 hex characters, and 64 is not an error.** Git is migrating to SHA-256, so a
/// check that assumed 40 would refuse a perfectly good reference from a repository that had
/// moved — and it would refuse it as though the reference were malformed, which is the least
/// diagnosable way to be wrong about it.
///
/// Case is accepted either way and the value is stored exactly as given: folding it would
/// make the record disagree with what the person approved, for no gain.
pub fn is_commit_hash(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------------------
// The manifest — data written by someone else
// ---------------------------------------------------------------------------------------

/// What a module's repo declares about itself: `organon-module.toml` at its root.
///
/// 🚨 **Everything here is a request or a declaration of identity. Nothing here takes
/// effect.** The field that looks most like power — [`ModuleManifest::requests`] — is a
/// [`Requested`], which no approval record can hold.
///
/// **Unknown fields are tolerated and dropped**, and unlike [`crate::layout::SavedLayout`]
/// that is the *correct* posture rather than a shortcut: Organon only ever **reads** this
/// file, exactly as it only ever reads `harnesses.json`, so there is no rewrite that could
/// strip a newer console's fields. Not keeping them is also what makes "a manifest cannot
/// grant itself" true by construction — there is no bag of unparsed keys for a grant to hide
/// in. A manifest written for a newer Organon therefore loads in an older one rather than
/// failing, which is [`crate::harness`]'s forward-compat discipline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleManifest {
    /// The identity: the producer name a viewport will ask for. See [`check_producer_name`].
    pub producer: String,
    /// A human-readable name. Defaults to [`ModuleManifest::producer`] when absent, because a
    /// missing display name is a cosmetic gap and not a reason to refuse a repo.
    pub name: String,
    /// What kind of producer this is. [`VIEWPORT_KIND`] is the only one this build hosts.
    pub kind: String,
    /// What the module asks to be allowed to do. **A request, never a grant.**
    pub requests: Requested,
}

impl ModuleManifest {
    /// Read a manifest out of the text of an `organon-module.toml`.
    ///
    /// A **string**, not a path, deliberately: a manifest only reaches Organon after
    /// something has fetched a repo, and fetching is T3b. Taking text keeps every decision in
    /// this file headless and keeps the parsing testable against a manifest nobody has to
    /// write to disk.
    ///
    /// Refuses on shape rather than on taste: the producer name must be usable
    /// ([`check_producer_name`]) and the kind must be one this build hosts. Both are checked
    /// here, at the boundary where data written by someone else arrives, so that nothing
    /// downstream has to wonder whether it did.
    pub fn parse(text: &str) -> Result<Self, ModuleFault> {
        let mut manifest: ModuleManifest = toml::from_str(text)
            .map_err(|e| ModuleFault::Unreadable { what: MANIFEST_FILE, why: e.to_string() })?;
        check_producer_name(&manifest.producer)?;
        if manifest.kind != VIEWPORT_KIND {
            return Err(ModuleFault::UnknownKind {
                producer: manifest.producer,
                kind: manifest.kind,
            });
        }
        if manifest.name.is_empty() {
            manifest.name = manifest.producer.clone();
        }
        Ok(manifest)
    }
}

// ---------------------------------------------------------------------------------------
// The approval record — Organon's own
// ---------------------------------------------------------------------------------------

/// Where a module's bytes come from, as the approval step is given it.
///
/// A struct rather than three loose arguments so that [`ApprovedModule::approve`]'s signature
/// reads as *"this source, these grants"* — the grants unmistakably a second thing, supplied
/// by a second party.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleSource {
    /// Where the bytes live. A git URL — GitHub, a machine on the desk, a company VPN;
    /// modules plan §11.1 is explicit that hosting is open.
    pub url: String,
    /// **Which** bytes. See [`is_commit_hash`].
    pub commit: String,
    /// The branch or tag the commit was taken from, if a person named one.
    ///
    /// ⚠️ **Provenance, never identity.** §3.2: tags move, branches move. This says where the
    /// hash was read from on the day; the hash is what is trusted.
    pub reference: Option<String>,
}

/// What a build actually consumed.
///
/// §3.4: the console builds from source at a pinned commit, so the record must describe the
/// binary that exists rather than the intention that produced it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildRecord {
    /// The commit the working tree was at when the build ran.
    pub commit: String,
    /// **The tree was not a commit.** Uncommitted changes were present, so no hash names the
    /// bytes that were compiled — §3.4's *"record that rather than recording a commit as
    /// though it named the bytes"*.
    pub dirty: bool,
    /// Fields a newer console wrote. [`ApprovedModule::extra`]'s reason.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl BuildRecord {
    /// Does this build name the bytes that were approved?
    ///
    /// 🚨 **The one place the two commits are compared**, so that "they must be the same value
    /// or the record describes a binary that does not exist" is a function rather than a
    /// sentence in a document. A dirty tree fails it whatever the hash says: the hash names a
    /// commit, and a dirty tree is not that commit.
    pub fn names_approved_bytes(&self, approved_commit: &str) -> bool {
        !self.dirty && self.commit == approved_commit
    }
}

/// One approved module: **this URL at this commit is approved, and holds these grants.**
///
/// 🚨 **[`ApprovedModule::approve`] is the only way one comes into being**, and there is no
/// `From<ModuleManifest>` for it. See this module's header, and
/// [`tests::a_manifest_cannot_grant_itself`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApprovedModule {
    /// The producer name a viewport asks for. Taken from the manifest — identity is the one
    /// thing a manifest genuinely supplies.
    pub producer: String,
    /// A human-readable name, for a menu and for a sentence.
    pub name: String,
    /// Where the bytes live.
    pub url: String,
    /// **The approved commit.** Required; a record without one is refused on load
    /// ([`ModuleFault::NoCommit`]).
    ///
    /// `#[serde(default)]` at the container is what makes that refusal possible: a record
    /// missing this key still *parses*, so it can be refused by name rather than taking every
    /// other module in the file down with it.
    pub commit: String,
    /// The branch or tag the commit was read from. Provenance; see [`ModuleSource::reference`].
    pub reference: Option<String>,
    /// What a person granted. **Never what the manifest asked for.**
    pub granted: Granted,
    /// What was built, if anything has been. `None` is the whole of today: nothing in this
    /// tier builds.
    pub built: Option<BuildRecord>,
    /// Every field this build does not know, kept so a rewrite does not delete it.
    ///
    /// 🚨 [`crate::layout::SavedLayout::extra`]'s reason exactly: `harnesses.json` can
    /// tolerate-and-drop because nothing writes it back, and this file **is** written back —
    /// by every approval and every revocation. Without this, a console one version behind
    /// would silently strip a newer one's fields off every module in the file, including the
    /// ones it never touched.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ApprovedModule {
    /// Approve `source`, for the module `manifest` says it is, granting exactly `grant`.
    ///
    /// 🚨 **The manifest supplies identity. The caller supplies grants.** That split is the
    /// whole of §3.1, and the shape of this signature is what makes it structural rather than
    /// something a call site has to remember.
    ///
    /// ⚠️ **`grant` is a list of NAMES, not a [`Granted`], and that is the fix for a real
    /// hole.** An earlier version of this took a finished [`Granted`] as its third argument.
    /// That stopped a manifest granting *itself* anything — [`Requested::grant`] is still the
    /// only way to build one — but it did not stop a caller pairing manifest **A** with a
    /// [`Granted`] computed from manifest **B**'s requests, which is the same failure one step
    /// out: a grant answering a request nobody made of *this* module. Taking names means there
    /// is no pair left to mismatch. The [`Granted`] is derived **here**, from
    /// `manifest.requests`, so a grant is always an answer to the request being approved and a
    /// name that module never asked for is refused ([`ModuleFault::NotRequested`]).
    ///
    /// ⚠️ **What this does NOT reach, said plainly rather than implied away.** Like every other
    /// record type in this crate, [`ApprovedModule`]'s fields are public and `modules.json` is
    /// a file a person can edit, so a record can be *assembled* carrying anything. Nothing in
    /// the registry can check that against a manifest, because the manifest lives in a repo the
    /// console may not even have cloned — [`ModuleRegistry::load`] has a file and no
    /// repository. **The approval step is structural; the stored record is checked only for
    /// what a file can be wrong about on its own**, and its grants are not one of those things.
    ///
    /// 📌 [`ApprovedModule::built`] is always `None` here. Approving is not building; §3.4's
    /// two commits stay two fields, and the second one is filled by whatever eventually
    /// builds (T3b), never by this.
    pub fn approve(
        manifest: &ModuleManifest,
        source: ModuleSource,
        grant: &[&str],
    ) -> Result<Self, ModuleFault> {
        check_producer_name(&manifest.producer)?;
        if source.url.is_empty() {
            return Err(ModuleFault::NoUrl { producer: manifest.producer.clone() });
        }
        if source.commit.is_empty() {
            return Err(ModuleFault::NoCommit {
                producer: manifest.producer.clone(),
                reference: source.reference.clone(),
            });
        }
        if !is_commit_hash(&source.commit) {
            return Err(ModuleFault::BadCommit {
                producer: manifest.producer.clone(),
                commit: source.commit,
            });
        }
        Ok(ApprovedModule {
            producer: manifest.producer.clone(),
            name: if manifest.name.is_empty() {
                manifest.producer.clone()
            } else {
                manifest.name.clone()
            },
            url: source.url,
            commit: source.commit,
            reference: source.reference,
            // 🚨 Derived from **this** manifest's requests. That is the whole reason the
            // argument is a list of names and not a `Granted` — see the doc above.
            granted: manifest.requests.grant(grant)?,
            built: None,
            extra: BTreeMap::new(),
        })
    }

    /// The approved commit, cut to [`SHORT_COMMIT`] for a sentence a person reads.
    ///
    /// ⚠️ `get`, not an index: the loader refuses a commit that is not hex, but this is a
    /// public accessor on a public struct and a caller can hand it anything. A sentence in a
    /// rectangle must not be able to panic on a character boundary.
    pub fn short_commit(&self) -> &str {
        let n = self.commit.len().min(SHORT_COMMIT);
        self.commit.get(..n).unwrap_or(&self.commit)
    }

    /// Is there a build that names the bytes this record approved? See
    /// [`BuildRecord::names_approved_bytes`].
    pub fn is_built(&self) -> bool {
        self.built.as_ref().map_or(false, |b| b.names_approved_bytes(&self.commit))
    }

    /// Every rule [`ModuleRegistry::load`] applies to one record, in one place.
    ///
    /// Shared by the loader and by [`ModuleRegistry::upsert`] so that a record cannot enter
    /// the registry by a route that checks less than the file does.
    fn check(&self, at: usize) -> Result<(), ModuleFault> {
        if self.producer.is_empty() {
            return Err(ModuleFault::NoProducer { at });
        }
        check_producer_name(&self.producer)?;
        if self.url.is_empty() {
            return Err(ModuleFault::NoUrl { producer: self.producer.clone() });
        }
        if self.commit.is_empty() {
            return Err(ModuleFault::NoCommit {
                producer: self.producer.clone(),
                reference: self.reference.clone(),
            });
        }
        if !is_commit_hash(&self.commit) {
            return Err(ModuleFault::BadCommit {
                producer: self.producer.clone(),
                commit: self.commit.clone(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------

/// Why something was refused. **Every one of these is spoken**, never counted: §3.2 and §3.5
/// both say *refused by name*, and a module that silently is not there looks exactly like a
/// module that was never approved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleFault {
    /// A record naming a branch — or naming nothing — but no commit. §3.2's headline.
    NoCommit { producer: String, reference: Option<String> },
    /// A `commit` that is not a git object name. See [`is_commit_hash`].
    BadCommit { producer: String, commit: String },
    /// A record with no producer name at all, at this position in the file.
    NoProducer { at: usize },
    /// A producer name nothing can ask for. `why` comes from [`check_producer_name`].
    BadProducer { producer: String, why: &'static str },
    /// A record with no URL: nowhere to fetch the bytes from.
    NoUrl { producer: String },
    /// Two records claiming one producer name. The **first** is kept, because a viewport
    /// asking for a name must get one answer and the file's own order is the only thing that
    /// distinguishes them.
    DuplicateProducer { producer: String },
    /// A manifest declaring a kind this build does not host. See [`VIEWPORT_KIND`].
    UnknownKind { producer: String, kind: String },
    /// A grant that was never requested — [`Requested::grant`]'s refusal.
    NotRequested { grant: String },
    /// A whole file that could not be read as what it claims to be. `what` names the file so
    /// the sentence says which of the two it was.
    Unreadable { what: &'static str, why: String },
}

impl ModuleFault {
    /// The one sentence this refusal is said in.
    ///
    /// 🚨 **One place, per `CONSOLE_ARCHITECTURE.md`'s rule that the console does not paste
    /// prose into its own UI.** These are refusals and answers — the permitted kind — and
    /// each is generated from the values that caused it rather than written twice.
    pub fn sentence(&self) -> String {
        match self {
            ModuleFault::NoCommit { producer, reference: Some(r) } => format!(
                "{producer} names the branch `{r}` and no commit — a branch moves, so a \
                 reference that names only one has not decided what it trusts"
            ),
            ModuleFault::NoCommit { producer, reference: None } => format!(
                "{producer} names no commit — an approval records which bytes it trusts, \
                 not only where they live"
            ),
            ModuleFault::BadCommit { producer, commit } => format!(
                "{producer}'s commit `{commit}` is not a git object name — 40 or 64 hex \
                 characters"
            ),
            ModuleFault::NoProducer { at } => {
                format!("the module at position {at} names no producer, so nothing can ask for it")
            }
            ModuleFault::BadProducer { producer, why } => {
                format!("`{producer}` cannot be a producer name: {why}")
            }
            ModuleFault::NoUrl { producer } => {
                format!("{producer} names no URL, so there is nowhere to fetch it from")
            }
            ModuleFault::DuplicateProducer { producer } => format!(
                "{producer} is named twice — the first was kept, because a viewport asking \
                 for it must get one answer"
            ),
            ModuleFault::UnknownKind { producer, kind } if kind.is_empty() => format!(
                "{producer}'s manifest declares no kind — this build hosts one kind of \
                 module, `{VIEWPORT_KIND}`"
            ),
            ModuleFault::UnknownKind { producer, kind } => format!(
                "{producer}'s manifest declares kind `{kind}` — this build hosts one kind of \
                 module, `{VIEWPORT_KIND}`"
            ),
            ModuleFault::NotRequested { grant } => format!(
                "`{grant}` was never requested — a grant answers a request, and granting one \
                 nobody asked for widens what a module may do"
            ),
            ModuleFault::Unreadable { what, why } => {
                format!("{what} could not be read: {why}")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// What the rectangle says
// ---------------------------------------------------------------------------------------

/// What a region's rectangle says **instead of a picture**.
///
/// §4.6 names four states. This carries the two that are reachable with no process running;
/// see this module's header for why the other two are absent rather than stubbed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleState {
    /// Nothing in `modules.json` answers to this producer name — including the case where a
    /// person revoked it, which §3.5 requires a layout to survive: a revoked module is a
    /// region whose producer declines to run, never a layout that will not open.
    NotApproved { producer: String },
    /// Approved at a commit, and no build names those bytes.
    ///
    /// ⚠️ **This covers a drifted build as well as a missing one.** A record whose
    /// [`BuildRecord`] names a different commit, or was taken from a dirty tree, describes a
    /// binary that is not the approved one — and the thing to do about it is the same:
    /// build it.
    ApprovedNotBuilt { producer: String, commit: String },
}

impl ModuleState {
    /// The sentence the rectangle carries — the module, the fact, and the verb.
    pub fn sentence(&self) -> String {
        match self {
            ModuleState::NotApproved { producer } => format!(
                "{producer} is not an approved module — `{APPROVE_VERB}` approves one"
            ),
            ModuleState::ApprovedNotBuilt { producer, commit } => format!(
                "{producer} is approved at {commit}, and nothing built from it — \
                 `{BUILD_VERB}` builds it"
            ),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------------------

/// Everything that has been approved, in file order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModuleRegistry {
    pub modules: Vec<ApprovedModule>,
    /// Top-level fields this build does not know. [`ApprovedModule::extra`]'s reason.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// What reading `modules.json` produced: what is approved, and what was refused getting there.
///
/// 🚨 **Two halves, because a total read that reports nothing is indistinguishable from an
/// empty file.** [`crate::layout::Library::load`] can be silent — a layout that will not load
/// is refused later, by name, when somebody asks for it. A module record that will not load
/// is never asked for again: the viewport just says "not approved", which is a true sentence
/// about a false situation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Load {
    pub registry: ModuleRegistry,
    /// In the order the file offended, each with a [`ModuleFault::sentence`].
    pub refused: Vec<ModuleFault>,
}

/// Distinguishes concurrent temp files written by this process — [`crate::prefs`]'s counter,
/// for its reason: the pid separates processes, this separates two writes inside one.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long [`ModuleRegistry::for_completion`] may answer from memory.
///
/// [`crate::layout`]'s ceiling and its argument, inherited rather than re-derived: above one
/// frame the ~12 draws between two keystrokes collapse into one read, and 200 ms is under the
/// ~250 ms at which a delay stops feeling immediate — so a file edited by hand or by a second
/// console is in the ring before anybody could notice it was not. This console's own writes do
/// not wait for it at all; [`ModuleRegistry::save`] drops the cache outright.
const RING_TTL: Duration = Duration::from_millis(200);

struct RingCache {
    root: PathBuf,
    registry: Arc<ModuleRegistry>,
    taken: Instant,
}

/// ⚠️ **Poisoning is absorbed, never unwrapped** — [`crate::layout`]'s rule: a panic elsewhere
/// while this lock was held must not turn a completion ring into a second panic.
fn ring_cache() -> &'static Mutex<Option<RingCache>> {
    static CACHE: OnceLock<Mutex<Option<RingCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

impl ModuleRegistry {
    /// The store root, from [`SessionLog::store_root`] — reused, never re-derived.
    /// [`crate::prefs::Preferences::store_root`]'s rule and its reason in full: two resolvers
    /// that *can* disagree eventually do.
    pub fn store_root() -> Option<PathBuf> {
        SessionLog::store_root()
    }

    /// Read `<store_root>/modules.json`.
    ///
    /// **Total** — it never errors and never panics, so a corrupt library costs you your
    /// modules and never your console. **Not silent** — every record it drops comes back in
    /// [`Load::refused`] with a sentence.
    ///
    /// A **missing** file is neither: it is an empty registry with nothing refused, which for
    /// a fresh install is also the correct answer. Only a file that exists and will not be
    /// read produces [`ModuleFault::Unreadable`].
    pub fn load(store_root: &Path) -> Load {
        let path = store_root.join(MODULES_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Load::default(),
            Err(e) => {
                return Load {
                    registry: ModuleRegistry::default(),
                    refused: vec![ModuleFault::Unreadable {
                        what: MODULES_FILE,
                        why: e.to_string(),
                    }],
                }
            }
        };
        Self::from_json(&text)
    }

    /// [`ModuleRegistry::load`]'s decisions over text a test can write. The whole of the
    /// loader's judgement lives here so that none of it needs a filesystem to exercise.
    pub fn from_json(text: &str) -> Load {
        let parsed: ModuleRegistry = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                return Load {
                    registry: ModuleRegistry::default(),
                    refused: vec![ModuleFault::Unreadable {
                        what: MODULES_FILE,
                        why: e.to_string(),
                    }],
                }
            }
        };
        let mut registry = ModuleRegistry { modules: Vec::new(), extra: parsed.extra };
        let mut refused = Vec::new();
        for (at, module) in parsed.modules.into_iter().enumerate() {
            if let Err(fault) = module.check(at) {
                refused.push(fault);
                continue;
            }
            if registry.modules.iter().any(|m| m.producer == module.producer) {
                refused.push(ModuleFault::DuplicateProducer { producer: module.producer });
                continue;
            }
            registry.modules.push(module);
        }
        Load { registry, refused }
    }

    /// [`ModuleRegistry::load`] against the real store. A platform with no data directory
    /// yields an empty registry, for the same reason every other absence does.
    pub fn load_default() -> Load {
        Self::store_root().map(|r| Self::load(&r)).unwrap_or_default()
    }

    /// [`ModuleRegistry::load`] for the **draw path** — the same approved set, at a hundredth of
    /// the cost, because this one is asked while somebody is typing *and* while the console is
    /// painting.
    ///
    /// ✏️ **Named "for a completion ring" until T4, and it has a second caller now.**
    /// `console_main.rs`'s region walk asks [`ModuleRegistry::vacancy`] once per frame for every
    /// region holding a hosted producer, to decide whether the rectangle draws a picture or a
    /// sentence. That is the same requirement the ring has — a bounded-staleness read on a path
    /// that runs at frame rate — so it is the same cache rather than a second one. The name is
    /// left alone because the constant it keys, [`RING_TTL`], is §1.15's and renaming a
    /// measurement's vocabulary is how the measurement stops being findable.
    ///
    /// 🚨 **The measurement is §1.15's and it is inherited rather than re-taken.** A candidate
    /// walk over a stored library runs on the **draw** path and asks n + 1 times per call; at
    /// a hundred entries that measured 10.1 ms against a 16.7 ms frame. T4's producer ring is
    /// the same shape over this file, so the cache belongs here rather than in the ring — a
    /// ring built over an uncached read would have to learn the same thing again, and would
    /// learn it as a dropped frame while somebody was typing.
    ///
    /// ⚠️ **The refusals are not cached, and are not returned.** They are for a person to read
    /// once, when the file is loaded; repeating them per frame would be the ambient prose
    /// `CONSOLE_ARCHITECTURE.md` spent a whole tier removing. A surface that wants them calls
    /// [`ModuleRegistry::load`].
    pub fn for_completion(store_root: &Path) -> Arc<Self> {
        let mut slot = ring_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = slot.as_ref() {
            // The root is part of the key: a test store and the real one are two registries,
            // and a cache that answered one for the other would be a wrong ring, not a slow
            // one.
            if cached.root == store_root && cached.taken.elapsed() < RING_TTL {
                return Arc::clone(&cached.registry);
            }
        }
        let registry = Arc::new(Self::load(store_root).registry);
        *slot = Some(RingCache {
            root: store_root.to_path_buf(),
            registry: Arc::clone(&registry),
            taken: Instant::now(),
        });
        registry
    }

    /// Drop what [`ModuleRegistry::for_completion`] is holding. Called by
    /// [`ModuleRegistry::save`] — the one path a write takes, which is what makes "an approval
    /// and a revocation both invalidate it" a property of the writer rather than of two call
    /// sites remembering.
    fn forget_completion_cache() {
        *ring_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Write `<store_root>/modules.json`, replacing any existing file **atomically**.
    ///
    /// [`crate::prefs::Preferences::save`]'s mechanism and its caveats in full: the temp file
    /// goes in the *same* directory because a rename is only atomic within one volume,
    /// `std::fs::rename` replaces an existing destination on Windows too, and the bytes are
    /// plain UTF-8 because `serde_json` refuses a BOM outright and a total reader would turn
    /// that refusal into silence.
    ///
    /// 📌 **There is no `save_over(builtin)` here and there is no `builtin()` to pass it.**
    /// [`crate::layout`] needs one because a preset shipped in code is a suggestion; an
    /// *approval* shipped in code would be a grant Organon wrote on somebody's behalf, which
    /// §3.1 forbids outright. Every line in this file got there because a person approved it.
    pub fn save(&self, store_root: &Path) -> io::Result<()> {
        fs::create_dir_all(store_root)?;
        let mut json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        json.push('\n');

        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = store_root.join(format!("{MODULES_FILE}.tmp-{}-{seq}", std::process::id()));
        fs::write(&temp, json.as_bytes())?;
        match fs::rename(&temp, store_root.join(MODULES_FILE)) {
            Ok(()) => {
                Self::forget_completion_cache();
                Ok(())
            }
            Err(e) => {
                // One stranded temp per failed write would accumulate forever, and each looks
                // like a torn write to anyone reading the directory.
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    /// [`ModuleRegistry::save`] to the real store. No data directory is an **error** here,
    /// mirroring [`crate::prefs::Preferences::save_default`]: a caller asking to persist an
    /// approval has nowhere to fall back to.
    pub fn save_default(&self) -> io::Result<()> {
        let root = Self::store_root()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no platform data directory"))?;
        self.save(&root)
    }

    /// The module approved under this exact producer name.
    ///
    /// ⚠️ **Exact, never approximated** — [`crate::region::Region::resolve`]'s rule, and here
    /// an approximation would run somebody else's code. §4.2 says the same thing from the
    /// other end: `3d` with a typo must be refused by name, never fall back to Organon's
    /// `World`, because a person would get a picture and it would be the wrong one.
    pub fn get(&self, producer: &str) -> Option<&ApprovedModule> {
        self.modules.iter().find(|m| m.producer == producer)
    }

    /// Every approved producer name, in file order.
    ///
    /// 📌 **The vocabulary T4 turns into a ring**, handed over as a borrow of what is already
    /// in memory rather than as a read. Building the ring is T4's; not making T4 pay for a
    /// file read per candidate is this file's.
    pub fn producers(&self) -> Vec<&str> {
        self.modules.iter().map(|m| m.producer.as_str()).collect()
    }

    /// Store `module`, replacing any approval of the same producer. `true` if something was
    /// replaced — which a caller says out loud, because replacing an approval is a change of
    /// what somebody trusts and it happens under one word they typed.
    ///
    /// Refuses exactly what [`ModuleRegistry::load`] refuses, so a record cannot enter the
    /// registry through a route that checks less than the file does.
    pub fn upsert(&mut self, module: ApprovedModule) -> Result<bool, ModuleFault> {
        module.check(self.modules.len())?;
        match self.modules.iter_mut().find(|m| m.producer == module.producer) {
            Some(slot) => {
                *slot = module;
                Ok(true)
            }
            None => {
                self.modules.push(module);
                Ok(false)
            }
        }
    }

    /// Revoke: take an approval out. `false` if nothing answered to that name, so the caller
    /// refuses by name rather than reporting a revocation that did not happen.
    ///
    /// 📌 §3.5's rule is satisfied *above* this function rather than inside it: a layout that
    /// names a revoked module still opens, because what a revoked producer produces is
    /// [`ModuleState::NotApproved`] — a sentence in a rectangle — and never a load failure.
    pub fn revoke(&mut self, producer: &str) -> bool {
        let before = self.modules.len();
        self.modules.retain(|m| m.producer != producer);
        self.modules.len() != before
    }

    /// What stands in the way of `producer` drawing, or `None` if nothing does.
    ///
    /// 🚨 **`None` is the answer when the picture is what the rectangle says.** §1.14's vacancy
    /// rule read the right way round: a region draws a sentence *instead of* content, so the
    /// working case has no sentence — it has a producer. That also keeps this total over two
    /// arms rather than needing a third one meaning "fine", which nothing would ever draw.
    pub fn vacancy(&self, producer: &str) -> Option<ModuleState> {
        match self.get(producer) {
            None => Some(ModuleState::NotApproved { producer: producer.to_string() }),
            Some(m) if m.is_built() => None,
            Some(m) => Some(ModuleState::ApprovedNotBuilt {
                producer: m.producer.clone(),
                commit: m.short_commit().to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🚨 **A coherence tripwire, and the only reason it exists.**
    ///
    /// Rust forbids two `impl From<ModuleManifest> for ApprovedModule`. This one is compiled
    /// **only in a test build** and is never called; adding a real conversion anywhere in the
    /// crate makes this file stop compiling with *"conflicting implementations of trait
    /// `From<ModuleManifest>`"*, which is the loudest available way to say that a manifest
    /// must not be able to become an approval on its own.
    ///
    /// ⚠️ Do not "fix" the conflict by deleting this impl. The conflict **is** the test.
    impl From<ModuleManifest> for ApprovedModule {
        fn from(_: ModuleManifest) -> Self {
            panic!(
                "a manifest cannot grant itself — this impl is a coherence tripwire, never a \
                 conversion"
            )
        }
    }

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA256: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn manifest(producer: &str) -> ModuleManifest {
        ModuleManifest {
            producer: producer.into(),
            name: "Ascent".into(),
            kind: VIEWPORT_KIND.into(),
            requests: Requested::of(&["audio"]),
        }
    }

    fn source() -> ModuleSource {
        ModuleSource {
            url: "https://github.com/organonart/ascent".into(),
            commit: HASH.into(),
            reference: Some("main".into()),
        }
    }

    /// A private directory per test. ⚠️ **Never the real store** — a test that wrote
    /// `%APPDATA%\OrganonShell\modules.json` would overwrite the approvals of whoever ran
    /// `cargo test`.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("organon-console-modules-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    // -----------------------------------------------------------------------------------
    // §3.1 — the manifest requests, the record grants
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **a manifest cannot grant itself anything.** A manifest that spells every key
    /// an approval record uses — its own grants, its own commit, its own build — reaches an
    /// approval carrying none of them, because the only grant an approval holds is the one
    /// passed as a separate argument.
    ///
    /// The tripwire impl above is the other half: this test pins the behaviour, and coherence
    /// pins the absence of a conversion that would route around it.
    #[test]
    fn a_manifest_cannot_grant_itself() {
        let hostile = format!(
            r#"
            producer = "ascent"
            name = "Ascent"
            kind = "viewport"
            requests = ["audio", "input"]

            # Every spelling of the approval record's own vocabulary. All of it is unknown to
            # the manifest type and all of it is dropped.
            granted = ["audio", "input"]
            grants = ["audio"]
            url = "https://example.invalid/not-the-approved-url"
            commit = "{HASH}"
            built = {{ commit = "{HASH}", dirty = false }}
            "#
        );
        let m = ModuleManifest::parse(&hostile).expect("a manifest with extra keys still parses");
        assert_eq!(m.requests.len(), 2, "what it ASKED for survives, because that is a request");

        let approved = ApprovedModule::approve(&m, source(), &[]).unwrap();
        assert!(approved.granted.is_empty(), "the manifest's `granted` key granted nothing");
        assert!(
            !approved.granted.contains("audio"),
            "nor did its `requests`: a request is not a grant"
        );
        assert_eq!(approved.url, source().url, "the URL is the one the approval was given");
        assert!(approved.built.is_none(), "approving is not building");
    }

    /// CONTRACT: a grant reaches an approval only through a person's choice, and only for
    /// something that was asked for.
    #[test]
    fn a_grant_answers_a_request_and_nothing_else() {
        let m = manifest("ascent");
        let granted = m.requests.grant(&["audio"]).unwrap();
        assert!(granted.contains("audio"));

        let overreach = m.requests.grant(&["filesystem"]).unwrap_err();
        assert_eq!(overreach, ModuleFault::NotRequested { grant: "filesystem".into() });
        assert!(overreach.sentence().contains("never requested"));
    }

    /// CONTRACT: **an approval grants only what the manifest being approved asked for.**
    ///
    /// 🚨 The hole this closes is one step out from `a_manifest_cannot_grant_itself`, and it
    /// is the one an automated review found: `approve` used to take a finished [`Granted`], so
    /// nothing stopped a caller pairing manifest **A** with grants computed from manifest
    /// **B**'s requests — a grant answering a request nobody made of *this* module. `approve`
    /// now takes names and derives the grant from the manifest in its own hand, which is what
    /// makes the pairing unrepresentable rather than merely discouraged.
    ///
    /// Both directions are checked: a name the *other* manifest requested is still refused
    /// here, and a name this one requested is granted.
    #[test]
    fn an_approval_grants_only_what_this_manifest_asked_for() {
        // Two modules asking for different things.
        let quiet = ModuleManifest { requests: Requested::of(&["input"]), ..manifest("ascent") };
        let loud = ModuleManifest { requests: Requested::of(&["audio"]), ..manifest("wobble") };
        assert!(loud.requests.contains("audio"), "the other module really does want audio");

        // The grant the OTHER manifest would have justified is refused for this one.
        let err = ApprovedModule::approve(&quiet, source(), &["audio"]).unwrap_err();
        assert_eq!(err, ModuleFault::NotRequested { grant: "audio".into() });
        assert!(err.sentence().contains("audio"), "{}", err.sentence());

        // …and this manifest's own request is granted, so the guard refuses rather than
        // refusing everything.
        let ok = ApprovedModule::approve(&quiet, source(), &["input"]).unwrap();
        assert!(ok.granted.contains("input"));
        assert!(!ok.granted.contains("audio"));
    }

    /// CONTRACT: `CLAUDE.md` invariant 4 — a new capability defaults to inert.
    #[test]
    fn nothing_is_granted_by_default() {
        assert!(Granted::none().is_empty());
        assert!(Granted::default().is_empty());
        assert!(ApprovedModule::default().granted.is_empty());
        let approved =
            ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap();
        assert_eq!(approved.granted.len(), 0);
    }

    // -----------------------------------------------------------------------------------
    // §3.2 — the unit is a commit
    // -----------------------------------------------------------------------------------

    /// CONTRACT: a record naming a branch and no commit is **refused by name**, not accepted.
    #[test]
    fn a_branch_only_reference_is_refused_by_name() {
        let file = r#"{ "modules": [
            { "producer": "ascent", "url": "https://example.invalid/a", "reference": "main" }
        ] }"#;
        let load = ModuleRegistry::from_json(file);
        assert!(load.registry.modules.is_empty(), "it is not in the approved set");
        assert_eq!(
            load.refused,
            vec![ModuleFault::NoCommit {
                producer: "ascent".into(),
                reference: Some("main".into())
            }]
        );
        let said = load.refused[0].sentence();
        assert!(said.contains("ascent"), "it names the module: {said}");
        assert!(said.contains("main"), "and the branch it was going to trust: {said}");
    }

    /// CONTRACT: approval refuses the same thing at the other door, so a record cannot enter
    /// the registry by a route that checks less than the file does.
    #[test]
    fn approving_a_branch_with_no_commit_is_refused() {
        let no_commit =
            ModuleSource { commit: String::new(), ..source() };
        let err = ApprovedModule::approve(&manifest("ascent"), no_commit, &[])
            .unwrap_err();
        assert!(matches!(err, ModuleFault::NoCommit { .. }));
    }

    /// CONTRACT: **64 hex characters is a commit, not an error.** Git is migrating to SHA-256.
    #[test]
    fn a_sha256_object_name_is_a_commit() {
        assert!(is_commit_hash(HASH), "40 hex");
        assert!(is_commit_hash(SHA256), "64 hex — SHA-256, and not an error");
        assert!(!is_commit_hash(&HASH[..39]), "39 is not a git object name");
        assert!(!is_commit_hash(&format!("{HASH}0")), "nor is 41");
        assert!(!is_commit_hash("main"), "nor is a branch");
        assert!(
            !is_commit_hash(&"g".repeat(40)),
            "nor is 40 characters that are not hex"
        );

        let sha256_source = ModuleSource { commit: SHA256.into(), ..source() };
        let approved =
            ApprovedModule::approve(&manifest("ascent"), sha256_source, &[]).unwrap();
        assert_eq!(approved.commit, SHA256);
        assert_eq!(approved.short_commit().len(), SHORT_COMMIT);
    }

    /// CONTRACT: a malformed hash is refused as a malformed hash, naming the value.
    #[test]
    fn a_commit_that_is_not_an_object_name_is_refused() {
        let bad = ModuleSource { commit: "deadbeef".into(), ..source() };
        let err =
            ApprovedModule::approve(&manifest("ascent"), bad, &[]).unwrap_err();
        assert_eq!(
            err,
            ModuleFault::BadCommit { producer: "ascent".into(), commit: "deadbeef".into() }
        );
        assert!(err.sentence().contains("40 or 64"));
    }

    // -----------------------------------------------------------------------------------
    // §3.4 — approved and built are two fields
    // -----------------------------------------------------------------------------------

    /// CONTRACT: the commit that was approved and the commit that was built are **separate
    /// values that round-trip separately**, and a build that names other bytes does not make
    /// the module built.
    #[test]
    fn the_approved_commit_and_the_built_commit_are_two_fields() {
        let other = "fedcba9876543210fedcba9876543210fedcba98";
        let mut approved =
            ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap();
        approved.built = Some(BuildRecord {
            commit: other.into(),
            dirty: false,
            extra: BTreeMap::new(),
        });

        // The two values survive a round trip through the file without collapsing into one.
        let json = serde_json::to_string(&approved).unwrap();
        let back: ApprovedModule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.commit, HASH, "the approved commit");
        assert_eq!(back.built.as_ref().unwrap().commit, other, "the built commit, still other");
        assert_ne!(back.commit, back.built.as_ref().unwrap().commit);

        // And a build of other bytes is not a build of these.
        assert!(!back.is_built(), "a drifted build does not describe the approved binary");
        assert!(!back.built.as_ref().unwrap().names_approved_bytes(&back.commit));
    }

    /// CONTRACT: a dirty tree is recorded as such, and no hash it carries names the bytes.
    #[test]
    fn a_dirty_tree_is_not_a_commit() {
        let build = BuildRecord { commit: HASH.into(), dirty: true, extra: BTreeMap::new() };
        assert!(
            !build.names_approved_bytes(HASH),
            "the hash matches and the bytes still are not it"
        );
        let clean = BuildRecord { dirty: false, ..build };
        assert!(clean.names_approved_bytes(HASH));
    }

    // -----------------------------------------------------------------------------------
    // §4.6 — the two reachable states
    // -----------------------------------------------------------------------------------

    /// CONTRACT: an unapproved producer says so, and names the verb that approves it.
    #[test]
    fn an_unapproved_producer_names_itself_and_the_verb() {
        let registry = ModuleRegistry::default();
        let state = registry.vacancy("ascent").expect("nothing draws it");
        assert_eq!(state, ModuleState::NotApproved { producer: "ascent".into() });
        let said = state.sentence();
        assert!(said.contains("ascent"), "{said}");
        assert!(said.contains("not an approved module"), "{said}");
        assert!(said.contains(APPROVE_VERB), "{said}");
    }

    /// CONTRACT: an approved-but-unbuilt producer says so, and names the module, the commit
    /// and the verb that builds it.
    #[test]
    fn an_approved_but_unbuilt_producer_names_the_commit_and_the_verb() {
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        let state = registry.vacancy("ascent").expect("nothing draws it yet");
        assert_eq!(
            state,
            ModuleState::ApprovedNotBuilt {
                producer: "ascent".into(),
                commit: HASH[..SHORT_COMMIT].into()
            }
        );
        let said = state.sentence();
        assert!(said.contains("ascent"), "{said}");
        assert!(said.contains(&HASH[..SHORT_COMMIT]), "{said}");
        assert!(said.contains(BUILD_VERB), "{said}");
    }

    /// CONTRACT: when a build names the approved bytes there is **no sentence** — the picture
    /// is what the rectangle says. This is the arm that keeps [`ModuleRegistry::vacancy`]
    /// total without a third variant nothing would ever draw.
    #[test]
    fn a_built_module_has_nothing_to_say_instead_of_a_picture() {
        let mut registry = ModuleRegistry::default();
        let mut module =
            ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap();
        module.built =
            Some(BuildRecord { commit: HASH.into(), dirty: false, extra: BTreeMap::new() });
        registry.upsert(module).unwrap();
        assert_eq!(registry.vacancy("ascent"), None);
    }

    // -----------------------------------------------------------------------------------
    // §3.5 — revocation
    // -----------------------------------------------------------------------------------

    /// CONTRACT: a revoked module becomes a sentence, never a failure. §3.5's requirement,
    /// which is what makes a layout naming it still open.
    #[test]
    fn a_revoked_module_is_a_sentence_not_a_failure() {
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        assert!(registry.revoke("ascent"));
        assert!(!registry.revoke("ascent"), "revoking twice reports that nothing was there");
        assert_eq!(
            registry.vacancy("ascent"),
            Some(ModuleState::NotApproved { producer: "ascent".into() })
        );
    }

    // -----------------------------------------------------------------------------------
    // Names
    // -----------------------------------------------------------------------------------

    /// CONTRACT: `organon` is reserved, because `3d` with no producer already means it.
    #[test]
    fn a_module_may_not_call_itself_organon() {
        let err = check_producer_name(DEFAULT_PRODUCER).unwrap_err();
        assert!(matches!(err, ModuleFault::BadProducer { .. }));
        assert!(err.sentence().contains("no producer is named"), "{}", err.sentence());

        let toml = format!(
            "producer = \"{DEFAULT_PRODUCER}\"\nkind = \"{VIEWPORT_KIND}\"\nrequests = []\n"
        );
        assert!(ModuleManifest::parse(&toml).is_err(), "and a manifest cannot claim it either");
    }

    /// CONTRACT: a producer name is a word that crosses a whitespace-delimited line.
    #[test]
    fn a_producer_name_must_survive_the_wire_it_travels_on() {
        assert!(check_producer_name("ascent").is_ok());
        assert!(check_producer_name("").is_err(), "empty");
        assert!(check_producer_name("two words").is_err(), "whitespace would truncate it");
        assert!(check_producer_name("as\tcent").is_err(), "a tab is whitespace too");
        assert!(check_producer_name("as\u{7}cent").is_err(), "a control character");
        assert!(check_producer_name(&"a".repeat(MAX_PRODUCER)).is_ok(), "64 is the ceiling");
        assert!(check_producer_name(&"a".repeat(MAX_PRODUCER + 1)).is_err(), "65 is over it");
    }

    // -----------------------------------------------------------------------------------
    // The manifest as a file
    // -----------------------------------------------------------------------------------

    /// CONTRACT: a manifest written for a newer Organon loads in an older one rather than
    /// failing — [`crate::harness`]'s forward-compat discipline, and the reason every field
    /// carries a default.
    #[test]
    fn a_manifest_from_a_newer_organon_still_loads() {
        let newer = r#"
            producer = "ascent"
            kind = "viewport"
            protocol_version = 4
            [audio]
            channels = 2
        "#;
        let m = ModuleManifest::parse(newer).unwrap();
        assert_eq!(m.producer, "ascent");
        assert_eq!(m.name, "ascent", "a missing display name falls back to the producer name");
        assert!(m.requests.is_empty(), "and asking for nothing is the default");
    }

    /// CONTRACT: this build hosts one kind of module and says which when it meets another.
    /// §8 keeps a panel-producing or command-producing module a separate design with a
    /// separate grant.
    #[test]
    fn a_kind_this_build_does_not_host_is_refused_by_name() {
        let panel = "producer = \"ascent\"\nkind = \"panel\"\n";
        let err = ModuleManifest::parse(panel).unwrap_err();
        assert_eq!(
            err,
            ModuleFault::UnknownKind { producer: "ascent".into(), kind: "panel".into() }
        );
        assert!(err.sentence().contains("viewport"), "{}", err.sentence());

        let none = "producer = \"ascent\"\n";
        let err = ModuleManifest::parse(none).unwrap_err();
        assert!(err.sentence().contains("declares no kind"), "{}", err.sentence());
    }

    /// CONTRACT: text that is not TOML is refused as a manifest, naming the file.
    #[test]
    fn a_manifest_that_is_not_toml_is_refused_naming_the_file() {
        let err = ModuleManifest::parse("{ not toml").unwrap_err();
        match &err {
            ModuleFault::Unreadable { what, .. } => assert_eq!(*what, MANIFEST_FILE),
            other => panic!("expected Unreadable, got {other:?}"),
        }
        assert!(err.sentence().contains(MANIFEST_FILE), "{}", err.sentence());
    }

    // -----------------------------------------------------------------------------------
    // The file
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **a corrupt library costs you your modules, never your console.** The read is
    /// total, and it is not silent about it.
    #[test]
    fn a_corrupt_file_costs_the_modules_and_not_the_console() {
        let root = temp_root("corrupt");
        fs::write(root.join(MODULES_FILE), b"{ this is not json").unwrap();

        let load = ModuleRegistry::load(&root);
        assert!(load.registry.modules.is_empty(), "no modules — and no panic, and no error");
        assert_eq!(load.refused.len(), 1, "and it says so rather than evaporating");
        assert!(load.refused[0].sentence().contains(MODULES_FILE));

        // …and the surfaces that a person is actually looking at keep answering.
        assert!(load.registry.producers().is_empty());
        assert_eq!(
            load.registry.vacancy("ascent"),
            Some(ModuleState::NotApproved { producer: "ascent".into() })
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a fresh install is not an error, and is not a refusal either.
    #[test]
    fn a_missing_file_is_nothing_approved_and_nothing_refused() {
        let root = temp_root("missing");
        let load = ModuleRegistry::load(&root);
        assert_eq!(load, Load::default());
        assert!(load.refused.is_empty(), "absence is not a fault");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the registry ships empty. §3.1 — an approval seeded in code would be a grant
    /// Organon wrote on somebody's behalf.
    #[test]
    fn the_registry_ships_empty() {
        assert!(ModuleRegistry::default().modules.is_empty());
        let root = temp_root("empty");
        assert!(ModuleRegistry::load(&root).registry.modules.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: what was approved is what loads back, and one bad record does not take the
    /// good ones with it.
    #[test]
    fn approvals_round_trip_and_one_bad_record_costs_only_itself() {
        let root = temp_root("round-trip");
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        registry.save(&root).unwrap();
        assert_eq!(ModuleRegistry::load(&root).registry, registry);

        // Hand-edit a second, commitless record in beside the good one.
        let text = fs::read_to_string(root.join(MODULES_FILE)).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["modules"].as_array_mut().unwrap().push(serde_json::json!({
            "producer": "wobble",
            "url": "https://example.invalid/w",
            "reference": "main"
        }));
        fs::write(root.join(MODULES_FILE), serde_json::to_string(&value).unwrap()).unwrap();

        let load = ModuleRegistry::load(&root);
        assert_eq!(load.registry.producers(), vec!["ascent"], "the good one survives");
        assert_eq!(load.refused.len(), 1, "and only the bad one is refused");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a newer console's fields survive an older console rewriting the file. This
    /// file is **written back** by every approval and every revocation, so tolerating-and-
    /// dropping would quietly strip them.
    #[test]
    fn a_newer_consoles_fields_survive_a_rewrite() {
        let root = temp_root("forward-compat");
        let written = format!(
            r#"{{
              "modules": [
                {{
                  "producer": "ascent",
                  "url": "https://example.invalid/a",
                  "commit": "{HASH}",
                  "sandbox": "seccomp"
                }}
              ],
              "protocol": 7
            }}"#
        );
        fs::write(root.join(MODULES_FILE), written).unwrap();

        let mut registry = ModuleRegistry::load(&root).registry;
        assert!(!registry.revoke("nothing-by-this-name"), "nothing answered to that name");
        registry.save(&root).unwrap();

        let text = fs::read_to_string(root.join(MODULES_FILE)).unwrap();
        assert!(text.contains("\"sandbox\""), "a module's unknown field survived: {text}");
        assert!(text.contains("\"protocol\""), "and a top-level one: {text}");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a producer named twice gets one answer, and the file is told which.
    #[test]
    fn one_producer_name_gets_one_answer() {
        let file = format!(
            r#"{{ "modules": [
              {{ "producer": "ascent", "url": "https://a.invalid", "commit": "{HASH}" }},
              {{ "producer": "ascent", "url": "https://b.invalid", "commit": "{HASH}" }}
            ] }}"#
        );
        let load = ModuleRegistry::from_json(&file);
        assert_eq!(load.registry.modules.len(), 1);
        assert_eq!(load.registry.get("ascent").unwrap().url, "https://a.invalid", "the first");
        assert_eq!(
            load.refused,
            vec![ModuleFault::DuplicateProducer { producer: "ascent".into() }]
        );
    }

    /// CONTRACT: a write cannot destroy what is stored — the temp file is renamed over the
    /// target, and it goes in the same directory because a rename is only atomic within one
    /// volume.
    #[test]
    fn a_write_leaves_no_temp_file_behind() {
        let root = temp_root("atomic");
        let registry = ModuleRegistry::default();
        registry.save(&root).unwrap();
        registry.save(&root).unwrap();
        let strays: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != MODULES_FILE)
            .collect();
        assert!(strays.is_empty(), "left behind: {strays:?}");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: never a BOM. `serde_json` refuses one outright and a total reader turns that
    /// refusal into silence — this machine has that exact failure on record for
    /// `harnesses.json`.
    #[test]
    fn a_bom_is_never_written() {
        let root = temp_root("bom");
        ModuleRegistry::default().save(&root).unwrap();
        let bytes = fs::read(root.join(MODULES_FILE)).unwrap();
        assert_ne!(&bytes[..3], b"\xEF\xBB\xBF".as_slice(), "a BOM here is silently unreadable");
        assert_eq!(bytes[0], b'{', "the first byte is the JSON itself");
        let _ = fs::remove_dir_all(&root);
    }

    // -----------------------------------------------------------------------------------
    // What T4 consumes
    // -----------------------------------------------------------------------------------

    /// CONTRACT: the producer vocabulary is a borrow of what is already in memory, so a ring
    /// built over it costs no file read per candidate. §1.15's measurement, not re-learned.
    #[test]
    fn the_producer_vocabulary_is_read_from_memory() {
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        registry
            .upsert(ApprovedModule::approve(&manifest("wobble"), source(), &[]).unwrap())
            .unwrap();
        assert_eq!(registry.producers(), vec!["ascent", "wobble"], "in file order");
    }

    /// CONTRACT: the completion read is cached, and **this console's own write is not made to
    /// wait for the TTL** — a module just approved is in the ring on the next frame.
    #[test]
    fn a_write_puts_its_module_in_the_ring_immediately() {
        let root = temp_root("ring");
        // Prime the cache with an empty store.
        assert!(ModuleRegistry::for_completion(&root).modules.is_empty());

        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        registry.save(&root).unwrap();

        assert_eq!(
            ModuleRegistry::for_completion(&root).producers(),
            vec!["ascent"],
            "the save forgot the cache, so the ring is not 200 ms behind the file"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the cache is keyed by store root, so a test store and the real one are two
    /// registries rather than one wrong one.
    #[test]
    fn the_completion_cache_is_keyed_by_store_root() {
        let a = temp_root("ring-a");
        let b = temp_root("ring-b");
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        registry.save(&a).unwrap();

        assert_eq!(ModuleRegistry::for_completion(&a).producers(), vec!["ascent"]);
        assert!(
            ModuleRegistry::for_completion(&b).modules.is_empty(),
            "a second root is a second registry, not a cache hit"
        );
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    /// CONTRACT: an exact name, never an approximation — approving a typo would run somebody
    /// else's code.
    #[test]
    fn a_producer_is_found_by_an_exact_name() {
        let mut registry = ModuleRegistry::default();
        registry
            .upsert(ApprovedModule::approve(&manifest("ascent"), source(), &[]).unwrap())
            .unwrap();
        assert!(registry.get("ascent").is_some());
        assert!(registry.get("Ascent").is_none(), "case is not folded");
        assert!(registry.get("ascen").is_none());
    }
}
