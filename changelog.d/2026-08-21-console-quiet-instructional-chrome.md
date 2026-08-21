### Organon Console: the instructional chrome goes, the band stops narrating, and one segment stops painting over another

#117 removed the *receipts*. What survived was everything shaped like a **caption** — text
whose only reader is somebody who does not already know how this console works. James, on the
first thing the new build showed him: `message the agent — Enter sends, Shift+Enter for a new
line`; and, circling the block at the head of the transcript, *"it should not feel like part of
the conversational flow. … When everything is moving right, I generally don't care about this
stuff unless there is some exception or problem."* One rule, which is #117's own rule pointed at
a wider class:

> **Delete text whose only reader is a stranger. Keep text carrying a fact he cannot already
> have — a failure, a refusal, or a state no pixel states.**

**Gone:** the composer's live hint (an empty box under a conversation reads as *ready*); the
command panel's permanent `Tab completes - Enter runs`; `— turn complete` after every successful
turn. **Shortened:** the dead composer still says `not running`, because a *disabled* box with
no hint reads as broken; a `Declared` panel says `no controls yet` where it said *"this panel is
named in Organon's editor but has not been transplanted into the console yet"* — one sentence
about the console's own construction, repeated down a column twenty-four times.

**Moved to `/trace on`, not discarded:** the session's spend and the last turn's duration
(harness telemetry — Claude Desktop tells you which model you are talking to, not what the turn
cost); the band's echo of the agent's closing line, which is already a few pixels above it in
the transcript; the two `/theme edit` receipts and the editor-closed explanation. ⚠️ Every one
of these still enters `pane.log` and still returns whole under `/trace on` — *"I like the idea
that there is a status log somehow, but it should not be present normally"* is a surface nobody
has built yet, and dropping the entries at the source would make building it harder.

🚨 **The approvals audit could not simply be hidden, and the rule it earned is the interesting
part.** It reports §7's withholding guarantee — whether the permission handler is absent from
the model's own tool list — and a *silent* failure of that is the class of defect this tree
exists to prevent. It is now **loud when anomalous, quiet when the property holds**, decided by
`ExposureAudit::confirms_withholding` rather than by reading its own summary string: a handler
the model can call, or an init that reported no tools at all, are both always seen; a served
name the model cannot see is **not** an anomaly (that is ordinary deferred MCP loading, and
folding it in would make every cold start read as a fault). stderr keeps the line
unconditionally — the screen is where a repeated confirmation costs something, a launch log is
where an audit trail lives. ⚠️ It had **two** render sites, the scrollback head and the band's
slot, both fed from one `Remark` — which is why one predicate fixed both, and why a per-site fix
would have left it on screen.

🚨 **And the status band had no width budget at all**: `◈ What are we working on?ession $1.18 ·
last turn 5.1s`, the echo's tail painted *under* the chips. egui's ordinary idiom with an
unbounded left-hand item — `Label::truncate` truncates to `available_width`, which for the first
child is *everything*, so the right-aligned group added after it gets a zero-width rect and lays
out leftwards over what is already there. **Truncating "to what is left" is only a bound when
something has already been taken.** `strip_right_reserve` now measures the fixed items first —
the ring, and the chip run laid out through the painter — and `reading_room` hands the flexible
reading the remainder, floored at nought, inside an allocation of exactly that width. Fixed
rather than hidden: both offending segments are trace-only now, so the overlap would have
survived into `/trace on` and returned the moment anything else joined that band.

Three visibility carriers, each spelled next to the thing it decides and none re-derived at a
draw site: `Remark::seen` (a log line), `StatusReading::seen_text` (the band's standing),
`StripContent::chips_seen` (a dim chip), plus `element_seen` for a transcript element. ⚠️
`narration` is set where the value is *built* — two of the seven status readings are echoes and
both produce the same `Standing` as live conditions do, so nothing downstream could tell them
apart. `2 remembered decisions` stays on the quiet band: it reports how far the console has
delegated its own authority, which is the mode marker's class rather than the spend's.

**Judged and kept**, so the next sweep does not re-litigate them: `> add | remove` (the words you
would type, not a description of the control); `Enter runs` (it appears **only** where the panel
would otherwise be blank, and a blank panel reads as a broken one); the theme editor's key ring
and button hovers; `/help`'s body; the child's own `stdout:`/`stderr:` lines; and the
standing-allow revocation.

🚨 **Nothing here has been seen on a screen.** The rules hold and the arithmetic is pinned — the
audit is loud in both anomalous arms, a failed turn keeps its caption, the reading never asks
for width the fixed items need. Whether a band holding a model chip and a mode plate reads as
calm or as broken is James's to judge.
