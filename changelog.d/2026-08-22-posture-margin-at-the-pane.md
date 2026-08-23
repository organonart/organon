### Posture moves the pane now, and the console stopped opening a window nobody asked for

Two changes to Organon Console, both of which are one symptom James could see and one mechanism
that was reaching almost nothing.

**`organon console posture desktop` answered `{"accepted": …}` and changed almost nothing.** The
verb worked, `Posture` stored, `Form::at` lerped, every one of its tests passed. What did not
happen is anybody reading the result: `Form::margin` had exactly **one** call site, the element
walk inside `conversation_view::scrollback` — three layers into one of the console's two
front-ends. So the desktop posture inset the transcript and left the composer, the command panel
and the status strip flush to the pane's edge, and a **terminal** tab, which never enters that
function at all, was untouched entirely. At a terminal tab *"changes almost nothing"* was
literally true.

`Form::content_margin` is now `Form::pane_margin(available_width)`, claimed **once**, by
`console_main.rs`'s `draw_active_pane` — the one place both front-ends pass through, and inside
the region walk's closure, so an undivided pane insets as a whole and a `/viewport` split insets
each region. ⚠️ `term_view` was given no new argument and must not be: it sizes itself from
`ui.available_rect_before_wrap()`, so an inset `Ui` narrows the glyph grid with nothing to plumb.
That is the argument for putting the margin on the *container* — a token every draw site has to
remember to read is a token the third draw site will not read.

⚠️ **`available_width` is a bound, not a hint, and it is new with the move.** Against a whole
window 90 points a side is a margin; against a third of one it is most of the region, and the
transcript this token used to inset was never the thing being divided. Each side is capped at a
quarter of the width, so content keeps at least half the pane at every posture and every
division. The test asserts that **property** rather than the arithmetic, so a different cap that
still keeps the promise passes.

📌 The `None` guarantee moved up with it and got stronger: at `t = 0` `pane_margin` answers
`None` and the pane is drawn straight into the `Ui` it was handed — no wrapping frame, nothing
that can move a row by a point — which now covers the terminal as well as the transcript.

⚠️ **Note what the tests did not catch, because it generalises.** Every posture test passed
throughout, including the one added the last time this token was wrong. They all assert what
`Form` *computes*; not one asserts that anything **reads** it. A token can be fully specified,
compiled, tested, and wired to one draw site out of five, and no test in this tree says so.
`grep -c "form\." native/src/console_main.rs` answering `0` is what said so — a coverage question
about call sites is not a question a unit test asks.

---

**The console opened a stray black window, and hiding it would have made the console mute.**
`organon-console` had no `#![windows_subsystem = "windows"]`, so Windows attached a console and
`start ""` showed it. Adding the attribute hides the window *and* sends every `eprintln!` in the
binary — the refusals, the device negotiation, the panic hook — into nothing.

🚨 That is not hypothetical here. This workstation's lighting renderer ran **unobservable for six
hours** for exactly that reason, with every indicator green throughout, because *"nobody is
reading the output"* and *"there is no output"* look identical from outside. The attribute and
`organon-console/src/log_file.rs` land together and neither is independently correct.

Output now goes to `%LOCALAPPDATA%\organon\console\console.log` — the same directory this
machine's other headless processes use — by moving the process's standard **handles** rather than
installing a logger. There are hundreds of `eprintln!` call sites in this binary and a framework
would capture none of them without editing every one; `SetStdHandle` before anything speaks
redirects all of them, **and the default panic hook**, with no call-site change.

📌 **Measured on this toolchain rather than assumed**, and it was worth the two minutes: a probe
that set both handles then ran `eprintln!`, `println!` and a panic printed *nothing* to the
terminal and all three to the file. Had Rust cached the handle instead of resolving it per write,
the log would have been empty and this change would have shipped the exact silence it exists to
close.

⚠️ **`--help` and `--version` are not redirected** — they return before it. A GUI-subsystem
process still *inherits* standard handles even though Windows declines to allocate it a console
(measured the same way: run from a shell it printed to that shell, and `> file` captured it), so
those two reach a real terminal in every case somebody types them. Redirecting them would answer
a question by writing the answer somewhere else. `--help` also now names the resolved log path,
quoted from `log_file::path()` rather than restated, because that is the only surface that
reaches somebody whose window never appeared.

⚠️ **A cap, not a rotation policy**: 4 MB checked at open, one generation kept as
`console.log.old`, so the pair is bounded at twice the cap with no scheduler and no cleanup pass.
`.log.old` rather than `with_extension("old")`, which answers `console.old` — a name that sorts
away from its live sibling and stops looking like a log at a glance.

⚠️ **Green and ready to try, not verified working.** Nobody has launched a build carrying the
attribute to confirm no window appears, that the log fills, or that a panic reaches it; and
nobody has looked at the desktop posture on a pane, a terminal tab, or a divided layout. The
mechanisms are each measured in isolation and the whole is compiled and unit-tested.
