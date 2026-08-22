### A skill for the gap between "it builds here" and "a stranger can install it"

`.claude/skills/create-installer/` is new: a method for taking a Windows build to a
machine that has never built it, written from a worked example on the sibling
workstation repo where an installer was built, handed to a second machine, and failed in
forty seconds.

The governing idea is one question, and it is the one this machine is structurally unable
to answer about itself: **what does this product take from the machine it was built on
that a stranger's machine will not have?** A machine that builds a thing is configured
*by* building it, and from the inside a supplied dependency and an absent one look
identical — the program works. Every expensive bug in the worked example had that shape,
and none of them was a mistake in the code.

🚨 **The failure worth naming, because no layer of the product can report it.** ONNX
Runtime 1.23 is built with MSVC 14.4x and, against a Visual C++ runtime older than 14.40,
does not fail to *link* — every import resolves, and it then access-violates inside
`msvcp140.dll` during static initialisation. The process dies at `0xC0000142` in the
**loader**, before `main()`. There is nothing to print to and nothing to log with. The
installer installed perfectly, the program would not start, and every layer that should
have said so was silent. A prerequisite check is the only layer that can cover a loader
failure; a log covers everything after it; **neither substitutes for the other**, and the
skill is organised around that order.

⚠️ **It is written against what this repo already has, not against a greenfield project.**
`native/bundle.ps1`, `native/deploy.ps1` and `native/bundler.toml` are real, they predate
the skill, and an installer sits *downstream* of them rather than replacing them — the
skill says so at the top and says explicitly not to graft installer concerns onto the two
scripts that are the inner loop of every native change on Windows. It also names the
organon-specific questions an installer would have to answer that the worked example
cannot: which of the several binaries is being shipped at all, that the GPL forced on the
root crate by `vst3-sys` follows the binary to whoever receives it, that the galleries land
where `dirs::data_dir()` puts them rather than where a script says, that the visual lives
*inside* the `.vst3` bundle in a directory named after the target, and that `F:\vst3` is a
developer's choice and not an install destination.

📌 **One trap in the skill is already this repo's own, which is why it is cited rather than
re-taught.** "`cargo build` writes the same path whatever features it was given" is not
hypothetical here: cargo features unify across a package's targets and `EDITION` is a
compile-time `const` from which the IPC namespace derives, so `target/release/organon.exe`
built with `--features console-edition` and the same path built without it are different
products at one path. The corresponding rule — *verify the artifact, never the command that
produced it* — is worked through with the five refusals a packaging build should make. The
`.ps1` encoding gate in `.github/workflows/ci.yml` gets the same treatment: the skill points
at it, and notes that its file list is **hardcoded**, so a new script has to be added to it
by name or it is unchecked.

The last stage is the one that is not a technique. Keep a machine that is not the
developer's, and **write down which claims are verified and which are reasoned** — because
making that machine a build host is what destroys its value for the check it was bought
for. In the worked example the prerequisite check can no longer be exercised on the machine
whose failure motivated it: Visual Studio keeps `msvcp140.dll` current in System32
regardless, so the condition is gone. The ledger is the deliverable, not the machine. That
is the same line `CLAUDE.md` already draws under "What can and can't be verified where", and
the skill's own organon-specific claims are marked to it — including the SmartScreen
behaviour of an unsigned installer, which is reasoned here and has not been run.
