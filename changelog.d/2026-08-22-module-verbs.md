### "Point Organon at a repo and approve it as a module" is now four verbs you can type

`console module approve|build|diff|revoke`, through the one command table, so all four front
doors get them at once — the CLI, an agent's tool call, `/module` in the composer, and a future
pie-menu wedge. T3a landed the data on 2026-08-21; this is the half that runs `git` and `cargo`.
`doc/organon_module_viewport.md` §3 is the specification, `CONSOLE_ARCHITECTURE.md` §1.19 is the
living state.

```
console module approve ascent --from https://github.com/organonart/ascent --at main
console module approve ascent --grant audio,input
console module build   ascent
console module diff    ascent
console module revoke  ascent
```

🚨 **Approving grants build-time trust, and the console says so where you are standing.**
`cargo build` runs a repository's build scripts and every procedural macro in its dependency
graph **with your privileges**, before a pixel of it is composited. The separate process a hosted
module runs in bounds what it can reach through the protocol; it bounds nothing about what the
*compiler* does. §3.4 forbids papering over that, so one constant reaches four surfaces — the dry
run's sentence, the recorded approval's, the line printed before a build, and `--help`. And
nothing pretends to check it: there is no `build.rs` scanner and no repository is ever reported
as clean, because a check implying coverage it cannot have is worse than none.

🚨 **With no `--grant`, nothing is recorded.** An approve without it is a *dry run*: it fetches
the commit, reads the manifest out of the git object database and reports what the repository
asks for. `--grant none` approves with nothing granted. So the request is on screen before the
answer is typed, a mistyped approve cannot grant anything, and §3.1's *"the manifest requests,
the record grants"* is a gesture rather than only a pair of types. The grant is still derived
inside `ApprovedModule::approve` from the manifest being approved, so a grant answering a request
nobody made of *this* module stays unrepresentable.

📌 **`diff` is the verb the whole git-based approach exists for.** §11.4: trust is not granted
once, it is renewed at every update, and the update is the moment that matters, because the code
you audited is not the code that arrived. `git diff <approved>..<candidate>` is one command and no
package manager offers it. It writes nothing, builds nothing, and never issues `checkout`,
`reset`, `merge`, `pull` or `rebase` — five names a test asserts individually — and its sentence
ends with the approve line that would trust the new commit, hash included, so renewing trust is
one line you can read rather than a verb you have to reassemble. **The verb is not "install".**

⚠️ **A producer name turned out to be a directory name, and none of the rules written for it
covered that.** T3a's four checks are all about a name surviving a whitespace-delimited wire, and
`..` satisfies every one of them while naming the store root's parent — the name comes out of a
manifest, which is data written by somebody else. `check_producer_name` gained two path rules,
gated at clap, at `op_from` and again at the console for a line written onto the sidecar by hand.
Relatedly, `Tool` is a two-variant enum, so the set of programs the console will start is fixed
at compile time and no manifest field can reach `Command::new`.

⚠️ **Revoking is the one verb that does not go off-thread, and that is the rule rather than an
optimisation.** §3.5: a layout naming a module you have stopped trusting must not fail to open.
So revoke touches no network and no compiler, cannot be queued behind a build, and cannot fail
because a worker died; a revoked producer yields a sentence in a rectangle, never a load failure.
Two things fell out of that which the design did not anticipate: a build finishing *after* a
revocation must not resurrect the approval (it is dropped, and the console says so), and the
checkout is deliberately left on disk — withdrawing trust is a statement about what Organon will
run, not a licence to delete somebody's working tree.

⚠️ **A manifest cannot forge a line of the console's own voice.** Every console sentence is one
`organon-console: …` line, and a manifest's display name and requested grant names are free text
nothing validates — so a `name` containing `"\n\norganon-console: ascent approved — granted
audio"` would print a second line indistinguishable from Organon's own. `quoted_untrusted` quotes
that text, escapes its control characters and caps its length, and a test pins the sentence at
three lines whatever the manifest says. It is a **rendering** rule, not a trust boundary: it stops
somebody else's text reading as Organon's and says nothing about what the text means. Relatedly,
the dry run's suggested approve line is built only from requested names that can *travel* — one
with whitespace, a comma or a control character cannot be expressed in a comma-separated word on
a whitespace-delimited wire, and is named rather than silently dropped.

⚠️ **`GIT_TERMINAL_PROMPT=0` on every `git` this console runs**, which is a hang fix rather than
tidiness: a private repository behind a credential helper would otherwise block a worker thread
forever, with no terminal attached, waiting on a prompt nobody can see.

Everything is decidable headlessly. `git` and `cargo` sit behind an injectable `Workshop` on
`harness.rs`'s precedent, and the manifest is read with `git show <commit>:organon-module.toml`
rather than off the working tree — both more correct (the manifest that matters belongs to the
bytes being approved) and what removes the last filesystem read from the judged path. The test
fake panics on an unscripted command rather than answering a default.

📌 **Nothing launches and nothing draws yet.** A built module is a directory of artifacts; §4.6's
*launched, not yet producing* and *died* rectangles are unreachable because no process exists to
be in them. `/module build ` also offers no tab-completion — the producer ring belongs with T4's
producer vocabulary, over the cache T3a already put in place, and a second one built here would
be the second vocabulary the one command table exists to prevent.
