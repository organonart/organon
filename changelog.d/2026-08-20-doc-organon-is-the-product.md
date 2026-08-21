### Organon is the product, and trust is the axis the modules plan was missing

Two documents, both proposals rather than changes. Nothing was renamed, no code was touched,
and neither should be executed before #98's tiers land.

**`doc/organon_is_the_product.md`** — a panel stack and a live 3D viewport in one window,
assembled by typing, is what forces the question. A window full of panels beside an instrument
is the standalone editor; a window full of conversation is the console; a window built around
model visualisation is Mind. Those stop being three programs and become **three arrangements of
one program**, where the arrangement is data.

🚨 **What that dissolves is the compile-time `Edition`.** `Full | Mind | Console` exists to
answer *"which product is this binary?"*, and a layout answers it better at runtime — because a
runtime answer can be switched, saved, shared and extended, and a cargo feature cannot. The
edition system was right for what it was; what changed is that the thing it approximates at
build time is now expressible.

🚨 **What it does NOT dissolve is the plugin, and that is not a matter of effort.** A VST3/CLAP
inside a DAW has a host-owned window, a host-controlled lifetime, an audio thread with hard
real-time constraints and a saved-session identity that outlives any decision here. So the
shape is one Organon application whose layouts replace the three standalone front-ends, and one
plugin artifact that stays exactly where it is. ⚠️ Invariant 1 is untouched and must stay so:
renaming a product changes words a person reads, while changing a class ID orphans the device
in every saved DAW session. They are not the same kind of edit.

📌 The note also promotes **saved layouts** from a deferred convenience to the unit of product
identity — with the ordering left alone, because you cannot usefully save an arrangement of
things that do not exist yet — and records what a layout must be able to refuse. A saved layout
is an assignment that arrives all at once, from a file, possibly written by somebody else; one
that cannot be drawn must say so and leave the current layout standing, never half-apply.

**`doc/organon_modules_plan.md` §10** — an amendment, because §9.5 deferred *"who owns the
module registry"* and in doing so hid a question that is not about a registry at all.

🚨 **The process boundary is the trust boundary**, which makes trust the same decision as §4's
two module kinds rather than a policy layered over them. A **linked** module is a cargo
dependency: your address space, your filesystem, your GPU, with source audit as the only
control. A **hosted** module is a separate process, so what it may touch is what the protocol
exposes — enforceable rather than promised. A trust tier therefore selects a module's *kind*,
not a policy applied to it.

⚠️ **The failure mode to design against is social.** Tiers make it easy to promote a module as
a favour, and a system where *"I know them"* is spelled the same as *"grant full address-space
access"* will drift upward until the tiers mean nothing. ⚠️ And **the protocol is the permission
set** — a hosted module can do exactly what the protocol allows, so every verb added to it is a
grant, which is worth writing down before the first one rather than after the tenth.

The amendment also notes that **#6 (`organon-remote`)** — filed as "the console on a phone" — is
really the collaboration primitive both trust tiers and peer-to-peer distribution land on, and
that §9.5 now has a prerequisite: an index of names is not a trust model, and a registry that
implies one without having one is worse than no registry.
