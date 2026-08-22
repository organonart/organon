### The status log drops down from the top, the entry box stops moving, and the band stops spelling itself out

Organon Console. The status log `#127` built is kept whole — the data model, the derived
attention, the clear-by-reading rule — and its **surface** moves. A permanent one-line status
line now sits at the **top** of the pane, coloured green / amber / red from the log's own
contents; clicking it drops the log down over the page, Quake-console style. The log's rows
became a trace: a fixed-width local clock, a mark, and the text, all in the mono face. And the
status band stopped saying `default` and `allow all` at rest.

🚨 **THE ENTRY BOX NEVER MOVES, and that is why this change exists.** `#127` hung the log
immediately above the band, in a bottom-up column, so opening it pushed the composer up the
screen by nine rows. James: *"its positioning isn't right. It should not be displacing the entry
box. The entry box should never move. So put the entry box back where it was and put the status
log at the top, sort of like a Quake console drop-down."* The composer's column is now exactly
what it was before `#127`, and everything the log draws is on the far side of it: the status line
is the first child of the top-down remainder and is always one row, and the drop-down is an
`egui::Area` — a **layer, not a child** — so it takes no layout space at all and cannot displace
anything by construction.

⚠️ **The invariant is measured, not asserted, because a prose invariant is what got us here.**
`composer_box` publishes its rect through `conversation_view::composer_rect`, and
`the_entry_box_never_moves_when_the_status_log_opens` runs the real `draw` at 700 / 460 / 360 pt
and compares the rect closed against open against closed again — `assert_eq`, not "about the
same", because a share-of-the-pane bound is exactly the kind that holds at 700 and fails at 360.
Putting the drop-down back in the column fails it naming both rects.

🚨 **The summary is three states and every one of them is derived.** `Health::Ok` (no exceptions
at all), `Warning` (exceptions, all read), `Attention` (something nobody has looked at) — on
`Theme::ok` / `asking` / `bad`, no palette invented. ⚠️ **The middle state is what the
clear-by-reading rule costs and is the reason there are three**: once the reader has looked,
"you have unread exceptions" is false, but "nothing has gone wrong this session" is *also* false,
and collapsing back to green would be the log telling a comfortable lie about a session that
broke twenty minutes ago. All three mutations were **run**, not asserted: ignoring the exception
flag fails five tests (*"machinery lit the status line — it will stop being read"*), forcing the
health to `Ok` fails four (*"an exception left the status line green — the summary is a lie"*),
and collapsing `Warning` fails two (*"a session that broke is not 'all clear'"*).

⚠️ **Timestamps are local wall clock, `HH:MM:SS`, and the choice is against elapsed-since-start.**
What a reader does with a status line is correlate it against something *outside* the console —
a terminal they were watching, a file's mtime, their own memory of "it was about ten past" — and
every one of those is wall clock; elapsed reads well within a session and is useless the moment
the question leaves it. A session that spans midnight is answered on the **header**, which names
one date or `2026-08-21 → 2026-08-22`, so the rows stay eight characters wide and stay a column.
A row **truncates**, never wraps and has no horizontal scrollbar: a wrapped trace line's second
row has no timestamp and no mark, which breaks the alignment that makes the surface readable at
all, and a scrollbar puts every long line behind a gesture when the identifying half of a console
line is its beginning. The whole text is on the row's hover.

🚨 **The band's resting state is marks, and its abnormal state is still words.** James: *"we don't
want to show words like `default` and `allow all` at all times. That would be a sort of verbose
form of the interface. We should have either icons or some other way of not having to show all
those characters."* So the permission plate draws one mark — `◈` you are being asked, `×` you are
not — and the mode's name and consequence move to its hover; `you allowed everything — the console
is not asking` (48 characters) becomes `×` plus `allowing all`. ⚠️ **The persistent-warning
invariant is unchanged, which is the half a "make it compact" change would quietly lose:** the
mark is unconditional and an abnormal mode still carries two words, permanently and uncloseable.
Only `default` — the state where there is nothing to warn about — is a mark and nothing else. The
status log's band indicator is **gone**: the status line at the top is the one door, and two
doors to one surface is duplication on the band James asked to say less.

🚨 **The overlap is fixed structurally rather than shortened away, and the first attempt to test
it was wrong.** `#125` gave the *reading* a width budget; the left group's own items still had
none, which is why James photographed `allow all` painted over `you allowed everything…`.
`strip_box` now measures the right-hand fixed set **first** and allocates the remainder to a
sub-`Ui`, so nothing in the left half can be drawn outside a rect sized before any of it existed;
`band_marks_reserve` bounds the model plate against the marks, `band_word` **drops** an optional
word that does not fit rather than eliding it to `not a…`, and `BAND_LEFT_FLOOR` drops the
telemetry chips — the lowest-priority segment — before the identity is squeezed. ⚠️ **The band's
height cannot see an overlap**: `Ui::horizontal` does not wrap, so an overflowing left group stays
exactly one row tall and runs under the chips. A height assertion was written first and **passed
against deliberately broken code**; `band_group_rects` publishes the two halves' rects and
`the_bands_two_halves_never_overlap_however_narrow_it_gets` checks them at 260 / 380 / 520 / 900
pt — and found a real defect on its first run.

📌 **One dependency, `chrono`, held to `clock` + `std`.** ⚠️ std has no timezone at all —
`SystemTime` is UTC seconds and nothing in the standard library turns that into the wall clock a
reader compares against. The alternatives were unsafe FFI (`GetLocalTime` / `localtime_r`) inside
a UI crate that builds on three platforms, or timestamping in UTC and asking the reader to do
arithmetic; a reader doing arithmetic on a status line is a reader who stops reading it.
`LogTime` is plain integers rather than a `DateTime` so a test can pin a stamp — including the
midnight case, which nobody can wait for — and `LogTime::now` is the crate's only clock read.

⚠️ **Nothing here has been seen on screen.** 813 tests pass in `organon-console`, the four
root-crate console legs are green, and every geometric claim above is a measurement taken headless
by egui — but the colours, the drop-down's proportions, whether the marks read at a glance and
whether twelve rows is the right ceiling are all things only a running console can answer.
