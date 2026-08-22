### Modules — what "approve a repo" means, written down as data

`doc/organon_module_viewport.md` §3 asks what it means to point Organon at a repository and approve
it as a module. `native/organon-console/src/module.rs` is the first slice of the answer — **the data
model and its two files, and nothing that spawns a process.** No clone, no `git diff`, no
`cargo build`, no launch: those are the next tier, and they want these types to exist first. Every
decision in the file is a pure function of a string, a path or a value, which is exactly what makes
this the right thing to build first — all of it is decidable headlessly, and 30 tests decide it.

🚨 **Two files, two authors, and never one file.** `organon-module.toml` lives in the module's own
repo and is written by its author; `modules.json` lives beside `harnesses.json`, `layouts.json` and
`preferences.json` in the console's store and is written by Organon at a person's instruction. The
first **requests**; the second **grants**. A manifest that could grant itself anything would be a
permission system where the request and the answer come from the same party, so here the two sets of
grant names are two *types* — `Requested` and `Granted` — with no conversion between them except
`Requested::grant`, which takes the names a person chose and refuses one that was never asked for.
`ApprovedModule::approve` is the only constructor of an approval record, and it takes `Granted` as a
**separate argument** from the manifest.

⚠️ **There is no `From<ModuleManifest> for ApprovedModule`, and the crate will not compile if one is
added.** A `#[cfg(test)]` `From` impl that never runs sits beside the tests as a coherence tripwire:
a second one anywhere in the crate is `E0119: conflicting implementations`. The behavioural half is
`a_manifest_cannot_grant_itself`, which parses a hostile manifest spelling every key the approval
record uses — `granted`, `grants`, `url`, `commit`, `built` — and asserts the approval carries none
of them. The manifest type deliberately keeps **no** bag of unknown fields, unlike a saved layout:
Organon only ever reads that file, so tolerate-and-drop is the correct posture *and* it means there
is nowhere for a grant to hide.

🚨 **The unit of trust is a commit.** `doc/organon_modules_plan.md` §11.3: a repo says where the
bytes live, a commit says which bytes — tags move, branches move, force-push rewrites history. So a
record carries a URL **and** a commit hash, and one naming only a branch is refused **by name** on
load rather than quietly accepted. The branch survives as provenance (*which ref the hash was read
from on the day*) and is never identity.

⚠️ **40 or 64 hex characters, and 64 is not an error.** Git is migrating to SHA-256; a check that
assumed 40 would refuse a perfectly good reference from a repository that had moved, and refuse it as
though the reference were malformed — the least diagnosable way to be wrong about it.

🚨 **The commit that was built is a second field from the commit that was approved.** §3.4's cheap
corollary, taken: `ApprovedModule::commit` is what a person approved and `BuildRecord` carries what a
build actually consumed, plus an explicit `dirty` flag for the case where the tree was not a commit
at all. They are normally equal and **the record is a lie exactly when they silently are not**, so
one function compares them — and a dirty tree fails it whatever its hash says, because a hash names a
commit and a dirty tree is not that commit. Nothing builds yet, so `built` is an `Option` that
`approve` always leaves empty; the *vocabulary* has to exist now because the rectangle has to be able
to say "approved, not built".

**Two of §4.6's four states, and the other two are absent rather than stubbed.** `ModuleState`
carries `NotApproved` and `ApprovedNotBuilt` — the two reachable with no process running. *Launched,
not yet producing* and *died / stopped producing* need something running, and this tier starts
nothing; `region.rs`'s standing rule is that **an unreachable arm is an untested branch pretending to
be a design**. The design document records the other two as coming; the code does not carry dead arms
for them. ⚠️ `ApprovedNotBuilt` covers a **drifted** build as well as a missing one, because a build
naming other bytes describes a binary that is not the approved one and the thing to do about it is
the same.

`ModuleRegistry::vacancy` answers `Option<ModuleState>`, and **`None` is the working case** — the
vacancy rule read the right way round: a region draws a *sentence* instead of a picture only when it
cannot draw the picture, so the healthy state has no sentence, it has a producer. That also keeps the
enum total over two arms rather than needing a third meaning "fine" that nothing would ever draw.

⚠️ **`APPROVE_VERB` and `BUILD_VERB` are constants here and neither is registered yet.** The
sentences name the verb that fixes them, which is what the four-state table asks for; the next tier
registers those verbs, and it must register *these strings* rather than re-spelling them. A refusal
naming a verb the command table spells differently is a refusal nobody can act on.

🚨 **The registry ships empty, and here that is stronger than "not scope".** The saved-layout library
ships empty because naming presets nobody asked for is a taste call. This one ships empty because
**an approval seeded in code is a grant Organon wrote on your behalf** — the very thing the two-file
split forbids. So there is no `builtin()` and no `save_over` taking one.

**Total, but not silent.** A corrupt library costs you your modules and never your console, exactly
as a corrupt `layouts.json` costs you your layouts — but with one deliberate difference. A layout
that will not load is refused *later*, by name, when somebody asks for it; a module record that will
not load is **never asked for again**, because the viewport just says "not approved", which is a true
sentence about a false situation. So the read returns what was approved **and** what was refused
getting there, each with its own sentence. A *missing* file is neither: empty registry, nothing
refused, which for a fresh install is also the correct answer. Writing is the preferences file's
mechanism verbatim — temp file in the same directory then rename, plain UTF-8, never a BOM — and
unknown fields survive a rewrite, because unlike `harnesses.json` this file *is* written back, by
every approval and every revocation.

📌 **What the producer-qualifier tier gets, and the measurement it must not re-learn.** Producer
names will be a second, dynamic vocabulary (`3d ascent` beside `3d`). Two things are owed to that
here and nothing more: the producer list is a borrow of what is already in memory, and the completion
read is cached on the saved-layout ring's own terms — 200 ms TTL, keyed by store root, dropped
outright by any write. That cache exists because the candidate walk runs on the **draw** path and
asks n + 1 times per call, which measured 10.1 ms for a hundred entries against a 16.7 ms frame. A
ring built over an uncached read would learn that again, as a dropped frame while somebody was
typing. ⚠️ The ring itself is **not** built here, and neither is a `Producer` enum, a trait or a
viewport arm — there is one producer today and inventing an abstraction over it is the branch this
tier declines to write. `organon` is reserved as a producer name, though, and refused to a module: a
viewport with no qualifier already means it, so a module able to claim it would be a module able to
impersonate the one producer the console wrote.

**The one dependency the crate gained** is `toml`, for parsing a manifest and nothing else. A
module's repo describes itself in a file the console does not get to choose the format of. It is
already resolved in this workspace's lock, so nothing new downloads, and it brings no `nih_plug`
edge — the crate's standing acceptance test is unaffected.
