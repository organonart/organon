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
force-push rewrites history. ⚠️ Measured: `nih_plug` and `nih_plug_egui` are declared with **no
`rev`, no `tag`, no `branch`** — the plugin's entire framework floats on a default branch —
while `baseview` three lines below shows the right shape. `Cargo.lock` is committed and pins
all three entries to `f36931f7…`, so today's builds are safe, but **`cargo update` moves it
silently** and **a lockfile does not protect a consumer**: cargo ignores a dependency's
lockfile, so the pin is a property of building *this* repo rather than of the dependency. The
moment `organon-core` is published, the float is somebody else's.

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
