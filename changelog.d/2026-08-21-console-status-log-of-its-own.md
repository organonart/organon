### The console's status messages get a home of their own, and the conversation carries the conversation

Organon Console. Every line the console writes about a session goes to a **status log** —
`organon-console/src/status_log.rs`, a bounded panel that opens out of the status band. The
conversation keeps only genuine exceptions. The band gains one small mark that is quiet when
nothing is wrong, hovered for the last few lines and clicked to open the log. `/trace on` still
works and now means *"keep the log open"* rather than *"narrate into my conversation"*.

🚨 **The leak this closes is structural, not a matter of taste.** Until now a line the console had
to say faced exactly two futures: **James's transcript, or nothing.** So chrome kept coming back —
somebody judges a line important enough to show, and the only way to show it is the flow — and the
escape hatch made it worse rather than better, because `/trace on` widened the *scrollback*. James:
*"I like the idea that there is a status log somehow, but it should not be present normally. … it
should not feel like part of the conversational flow. When everything is moving right, I generally
don't care about this stuff unless there is some exception or problem."* 📌 The fix is that nothing
is classified show-or-hide when it is **written**, only when it is **drawn**: `StatusLog::push`
refuses and classifies nothing, and `Remark::always` — one flag, unchanged — is read by the
conversation and by the indicator, so the two cannot come to disagree about what is exceptional.

🚨 **An indicator that is silent when healthy is only worth having if it reliably lights when things
are not**, which is the same defect this tree has found three times over (`organon-swap` reading
`failed` while swap was active; a lighting renderer printing `ambient is OFF` while running
`--ambient glow`). So the attention state is **derived from the log's contents on every call** —
there is no cached flag to drift — and the only carried state is a high-water mark that cannot
disagree with the lines about which of them exist. Both directions are mutation-checked: ignoring
the exception flag fails five tests (*"machinery lit the indicator — it will stop being read"*),
never lighting fails seven (*"an exception left the indicator dark — the badge is a lie"*), and
dropping the acknowledgement fails one (*"opening the log did not clear the indicator"*).

⚠️ **It clears by being read, not by ageing**, and the alternatives are recorded because both are
tempting. A badge that never clears is a badge nobody reads — the silent failure by another road.
One that ages out goes quiet precisely because somebody stepped away, which is the case it exists
for. Opening the log is the one event that is evidence a human looked; the next exception lights it
again, because it has not been.

⚠️ **`/trace` may no longer touch the transcript, so `element_seen` lost its mode parameter** — the
`— turn complete` caption on a *successful* turn is now never drawn. A click on the band's mark that
also put a caption under every reply would be this change's own leak arriving by a new route. Four
lines were reclassified out of the conversation on the same test: the child's `stdout:`/`stderr:`
chatter (the console did not write it and a genuinely broken stream still sets `failure`), the
revoked-session-allow confirmation (the band's marker vanishes on the same frame), and `/help`'s
verb table — which now writes to the log **and opens it**, so nineteen true, routine lines appear in
the surface built to hold them instead of in the middle of a conversation. The empty-transcript hint
is gone entirely rather than reclassified.

⚠️ **The band's width budget gained a fixed item, deliberately.** What sat there before was a *line*
of the log, excluded from `strip_right_reserve` because truncating into whatever slack was left was
its whole behaviour. An indicator must not truncate — an ellipsis where it should be loses the one
item the band is least allowed to lose — so it is measured, in the monospace face it is drawn in,
and the flexible reading gives way to it. That is the overlap `#125` fixed (`◆ What are we working
on?ession $1.18`), respected by a new item rather than rediscovered.

📌 **Nothing here has been seen on screen.** 802 console-lib tests, 594 core, every edition green —
which says it compiles and behaves, and says nothing about whether the console now *reads* as calm.
That is James's call and only his.
