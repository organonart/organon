### Organon Console — a command runs the moment nothing is left to resolve, unless it cannot be taken back

James: *"when we reach the end of a tab completion hierarchy … it automatically executes and
we don't press enter. I would limit this so that if there are any things that would be
irreversible or dangerous, it should not do that, but should instead display a final
completion that says something like press enter."* `Palette::autorun` — the machinery that
does the firing — was already there and already correct; it was off behind
`ORGANON_PALETTE_AUTORUN=1`, because being *certain* what a line means is not by itself a
reason to run it. This adds the term that was missing and turns it on.
`CONSOLE_ARCHITECTURE.md` §1.9 owns the rules.

🚨 **THE RULE, and it is recoverability rather than severity: a verb may run without an Enter
when the console can be put back the way it was.** A setting has an inverse — another value of
the same verb — and a read changes nothing; both fire. **A verb that puts a new element into
the transcript does not**, because the transcript only ever grows and there is no verb that
takes an element back out of it. Nothing in this vocabulary formats a disk, so a severity
scale would have had one rung and said nothing; what a hand needs protecting from here is the
edit it cannot undo.

| Runs unasked | Completes, then waits for Enter |
|---|---|
| `background`, `rig`, `theme`, `posture`, `screen`, `portal`, `camera`, `camera.read`, `help` | `block`, `patch`, `surface`, `organon` |

⚠️ **`help` is the one that looks like it belongs on the right and does not, and the pair
`help`/`surface` is what the rule has to get right.** Both are view-lane, both take no
arguments, both are reached the same way — and `/help` writes through `note` (the capped
diagnostic log) while `/surface` calls `Transcript::push`. That is a difference in the code
rather than a judgement call. A rule spelled *"view-lane verbs are dangerous"* or
*"argument-less verbs are dangerous"* would have got one of the two wrong.

🚨 **The declaration is `command::Reversal`, on `CommandSpec` and on `registry::Entry`** — one
per verb, in the place that verb is declared, never a list in a renderer. Both types carry it
because neither covers the whole vocabulary on its own: `surface`, `help` and `organon` have
no `CommandSpec` at all. ⚠️ **It has no `Default`, deliberately.** A default would let a verb
added later answer this question by not answering it, and the quiet answer is the one that
runs; adding a `CommandSpec` is now a compile error until it says which it is.
`Candidate::fires` is derived from it inside the same `Registry::resolve` call that derives
`Candidate::completes`, so neither can drift from what Enter would actually do, and a name the
table cannot find answers `false`.

📌 **The MCP catalog deliberately does not restate it.** An agent's tool call never reaches
this rule: the question at that door is *"may this agent act on my behalf"*, which
`start_approvals` answers with a real prompt per call — a stronger mechanism than an Enter
key, not a weaker one. Emitting the flag as a tool annotation would be a second claim about
the same verb with nothing reading it. It sits on the shared spec so both doors can read one
fact when the approval model wants it.

**What "stop and ask" looks like is the `Enter runs` marker that already existed.** A verb on
the right of the table still *completes* — `/su` becomes `/surface` under the hand — and then
the compact row says `Enter runs`, because `Palette::runnable` holds. No second phrasing was
invented: the marker added when `/surface` drew an empty panel turns out to be exactly the
"final completion that says press enter" the request asks for.

🚨 **A command does not run on the frame its last character landed.** `palette_autorun` now
takes `edited` — whether the composer changed on *this* frame, read before `palette_complete`
rewrites it — and refuses while it is true, so the earliest a fire can happen is the first
frame in which nothing was typed. The completed line is therefore **drawn at least once**
before it disappears, and §1.9's one-frame caret window becomes a window in which a keystroke
**cancels** the fire rather than racing it. ⚠️ **A settled frame has to be made to happen**:
egui repaints on input, so a deferred fire explicitly `request_repaint`s — without that, a
command would run whenever something else next moved the mouse, which is worse than either
extreme. It is requested only when a fire is pending, never unconditionally.

⚠️ **Measured while pinning that, and it is completion's price rather than autorun's**: typing
`h` on `/` completes to `/help`, and a character arriving on the very next frame lands at the
caret index the completion has not yet moved — `/hxelp`, not `/helpx`. The wait does not fix
that and nothing here claims to; what it fixes is that `/hxelp` then runs nothing at all.

⚠️ **`Palette::autorun` still obeys `completion_held`**, and the stake is now real rather than
conditional: backspacing `/theme dark` to `/theme dar` leaves one candidate that completes
*and* is recoverable, so without the insertion-only latch the keystroke trying to erase the
command would execute it.

**On by default. `ORGANON_PALETTE_AUTORUN=0` is the escape hatch** and restores the
Enter-for-everything console for a session. ⚠️ **`=1` still means ON** — the variable's
existing spelling keeps its existing meaning, so nobody's shell profile quietly came to mean
the opposite of what they wrote, which is the trap that renaming or inverting it would have
set. `conversation_view::autorun_enabled` is that rule as a pure function of the value, so a
test pins it without writing to the process environment.

⚠️ **The honesty ledger's most load-bearing line is now this one.** Auto-execute has still
never been used by a human; until today that was a claim about code nobody ran, and it is now
what happens to James the first time he types a slash. The recoverability rule is a *design*
argument supported by tests, not evidence: it bounds the cost of being wrong at one command,
and says nothing about how often, or about whether a command running under the hand feels like
help. `CONSOLE_ARCHITECTURE.md` §3 records the three things a first real use has to settle.
