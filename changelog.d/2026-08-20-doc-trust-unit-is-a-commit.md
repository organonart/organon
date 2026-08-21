### The unit of trust is a commit, and git is the distribution mechanism

`doc/organon_modules_plan.md` §11, extending §10. A proposal, not a change — nothing is
scheduled and §10's ordering stands.

**It costs nothing to adopt, because it is what a linked module already is.** Cargo consumes
git repositories natively and this workspace already does it three times. Hosting is genuinely
open — GitHub, a machine on a desk, a company VPN — because git is distributable by nature and
cargo does not care which remote a URL names.

🚨 **And §10 does not merely permit it, it requires it.** §10 establishes that a linked module
has no boundary and that *"auditing the source is the only control."* If source audit is the
only control, source distribution **is** the security model — a linked module shipped as a
binary is one whose only defence has been removed. §9.5's registry question largely dissolves
with it: a URL is the identity and a commit hash is a content address, which names bytes rather
than naming a name that points at bytes.

🚨 **The unit is a COMMIT, not a repo, and this workspace is currently on the wrong side of
it.** A repo says where bytes live; a commit says which bytes. Tags move, branches move, and
force-push rewrites history. ⚠️ Measured from `native/Cargo.lock` rather than from the
manifest: **three** crates resolve from `nih-plug.git` with **no `rev`, no `tag`, no `branch`**
— `nih_plug`, `nih_plug_xtask`, and 🚨 **`nih_plug_derive`, a proc-macro crate**
(`proc-macro = true`, reached through `#[derive(Params)]` at `native/src/params.rs:4060`). All
three are pinned by the lock at `f36931f7…`, so today's builds are safe, but **`cargo update`
moves them silently** and **a lockfile does not protect a consumer**: cargo ignores a
dependency's lockfile, so the pin is a property of building *this* repo rather than of the
dependency. `baseview` shows the right shape, pinned at `237d323c…`.

🚨 **That the middle one is a proc macro is the whole of §11.6 sitting in this tree** — code
fetched from a moving reference and executed at compile time with the builder's privileges,
which is exactly where "complete visibility into all code" is true and insufficient.

⚠️ **`nih_plug_egui` is not a floater, though it looks like one.** It is declared as a git
dependency and then overridden by a `[patch]` block to a vendored in-tree copy, so its lock
entry has no `source` field and it never resolves via git. ✏️ An earlier draft named it as one;
review caught it, and the lesson is worth the correction: **a manifest line is not where a
dependency's identity is settled — the lock is**, and a `[patch]` makes the two disagree on
purpose.

📌 **The affordance that makes this worth doing is one no package manager ships.** Trust is
renewed at every update, not granted once — the code that was audited is not the code that
arrived. Git is the only mechanism where the console can say *"this module changed 14 files
since the commit you last trusted; here they are."* `git diff <last-trusted>..<candidate>` is
one command. npm, PyPI and crates.io could all offer it and do not, because their unit is a
tarball. It does not ask anyone to read a module — only to read a diff, which is tractable and
is exactly where an attack has to appear. Forking is the companion: an unacceptable update is
answered by pinning the previous commit or carrying a fork with full history, which a binary
cannot offer at all.

⚠️ **Two honest counters are recorded rather than glossed.** First, **visibility is not
review** — npm, PyPI and crates.io are all fully source-visible and compromised regularly;
nobody reads their dependencies, so a model resting on *"you could read it"* rests on something
almost nobody does. Second, **the build is the surface visibility does not cover**: `build.rs`
runs at build time with your privileges and proc macros execute during compilation, so a linked
module can take a machine before a line of its code runs in the application — and `build.rs` is
the file nobody opens. The review target for a linked module is therefore `build.rs`, any proc
macro, and the dependency tree; ⚠️ this workspace configures neither `cargo deny` nor
`cargo vet`.

**Source is required for linked modules and optional for hosted ones**, which falls out of
§10's table rather than being a new rule — and that hosted modules need not be source is a
feature, since it is what lets the ecosystem include work written in other languages or by
people you have no relationship with, without pretending you audited it.

Finally, what git does and does not supply for identity: **signed commits and tags** give an
author identity that survives across repos and hosting moves, and §10's tiers are about people,
so that is the half worth adopting. ⚠️ **Revocation is the half git has no answer for** — a
signature stays valid, a commit stays fetchable, a fork keeps the history — so it must be
designed rather than inherited, under §10's standing constraint that a layout referencing a
module you have stopped trusting must not fail to open.

🚨 **And §11.9's own recommendation was tested and turned out to be wrong**, which is recorded
rather than quietly fixed because it is the section's thesis arriving as evidence. The draft
said pinning the floaters *"changes no bytes in any build today."* Measured: adding
`rev = "f36931f7…"` to both declarations and re-resolving with `cargo metadata` takes `nih_plug`
from **one package entry to two**, and `nih_plug_derive` likewise — the lock ends up carrying
both `nih-plug.git#f36931f7…` and `nih-plug.git?rev=f36931f7…#f36931f7…`, the same commit and,
to cargo, two different sources.

⚠️ The cause names itself: `vendor/nih_plug_egui` declares `nih_plug` **unpinned on purpose**,
its `Cargo.toml:19` explaining that the *"workspace already resolves [it], so cargo unifies it
to a SINGLE `nih_plug`"* and warning that otherwise *"options become different types across the
boundary."* 📌 So the rule is sharper than *pin the commit*: **a rev pin must be applied at every
declaration of the same git source in the graph — vendored and patched crates included — or
cargo stops unifying and silently duplicates the dependency.** A half-applied pin is worse than
none. The whole check needed no compiler: edit, re-resolve, diff the lock.
