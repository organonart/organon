### Two SessionStart hooks had never run on Windows, and exited 0 to say so

`context-budget-check.sh` and `structure-drift-check.sh` are both wired in
`.claude/settings.json`. Neither had ever produced a single byte on this project's Windows
workstation. Both are Python inside, and every call was spelled
`python3 … 2>/dev/null || exit 0` — the fail-safe posture every hook here uses, and the
right one for a hiccup. It is the wrong one for a **missing interpreter**, because the
result is a hook that exits 0 having printed nothing, which is byte-for-byte what "nothing
to report" looks like.

🚨 **The Microsoft Store stub is why `command -v` cannot be the test.** Windows ships
`%LOCALAPPDATA%\Microsoft\WindowsApps\python3.exe`: a stub that is on PATH, is executable,
and — with no Store Python installed — prints *"Python was not found; run without arguments
to install from the Microsoft Store…"* to **stderr** and exits **49**. So locating an
interpreter proves nothing at all. `.claude/hooks/python-runner.sh` now resolves one by
**running** candidates — `python3`, `python`, `py -3`, then `wsl.exe -e python3` — and
demanding a sentinel back on stdout. With none reachable, each hook prints a named refusal
and still exits 0. They are reports, never gates; the guarantee is now *a number, or a line
telling you why there is no number* — never silence.

⚠️ **`CLAUDE.md` asserted the opposite in as many words**: *"the budget line **always**
prints, because a number you only see when it is already bad is how the injected core
doubled without anyone noticing."* It did not print, and the sentence describes exactly what
happened in the gap — `ARCHITECTURE.md` went from 202,276 bytes when organon#1 was filed to
**219,695**, and the hook now measures the real injected cost at **220,439 B ≈ 55.1k tokens,
110% of the 200 KB budget**. The doc is corrected rather than deleted: the claim is true
again, and now says what it rests on. (`CONSOLE_ARCHITECTURE.md` is 474,552 bytes and is
**not** in that figure — it is read on demand, never injected, which is the whole design.)

⚠️ **Which `bash` runs the loaders turned out to be part of the measurement.** The budget's
Python found the wired `load-*.sh` hooks *and ran them* with
`subprocess.run(["bash", script])`, counting stdout. That is correct while the interpreter
is a local one and quietly wrong the moment it is reached through WSL, because `"bash"` then
means WSL's bash — and a loader resolves its own root with `git rev-parse`, which Linux git
cannot do against a Windows-made worktree (its `.git` file holds `C:/…`). Measured in
exactly such a worktree: `load-architecture-doc.sh` emits **220,439 bytes under Git Bash and
0 under WSL bash**. Dispatching the whole script to WSL would have re-created the original
defect one layer in — a budget of 0 B, reported with complete confidence. So the loader loop
moved out to the shell, where it runs in the same bash, cwd and environment the harness
itself uses at SessionStart. Parsing `settings.json` and computing the session-over-session
deltas stay in Python; a shell rewrite of those would be a second implementation, and second
implementations rot.

⚠️ **Proving the failure branch exposed a second silent hole, one layer out: a wired loader
that prints nothing is a broken loader, not a zero budget.** With `git` off PATH,
`load-architecture-doc.sh` takes its own `|| exit 0` path and emits nothing — and the
arithmetic dutifully reported `injected each session 0 B ≈ 0.0k tokens (0% of budget)` as
settled fact. Every loader exits 0 on its failure paths *by design* (silence beats a dangling
"authoritative architecture" header with no document behind it), which is right for the
loader and is precisely why the budget must not read silence as a measurement. All-zero is
now a diagnosis naming the loaders and pointing at `git rev-parse`; a partial zero appends a
warning to the real total.

📌 **Env vars do not cross into WSL by themselves**, which is the one thing that makes the
fallback backend workable at all. `NOW=… wsl.exe -e python3` leaves `NOW` unset on the far
side — verified. `WSLENV` is the only forwarding mechanism and it takes **names, not
values**, so a multi-line JSON blob crosses intact with no quoting involved. For the same
family of reasons every program travels on **stdin**, never in argv: `wsl.exe` re-quotes its
arguments, and a heredoc cannot be re-quoted.

This is organon#1 Tier 1, and it is the second instance of a class that issue already
names — `status-week-check.sh` is *"wired but silently inert… the shape of failure that is
hardest to notice, because everything looks configured."* That one is inert for a different
and deliberate reason (`STATUS.md` does not exist yet) and is untouched here; #1 Tier 1 owns
it, along with the `ARCHITECTURE.md` trim that the 110% figure above now argues for out loud
every single session.
