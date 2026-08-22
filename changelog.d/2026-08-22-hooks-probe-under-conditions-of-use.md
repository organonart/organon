### The two always-print hooks had stopped printing again, one layer in from where that was fixed

🚨 **`context-budget-check.sh` and `structure-drift-check.sh` were both refusing to report on the
Windows workstation, and the reason they gave named the wrong cause.** They said *"python ran and
returned nothing from the arithmetic pass"*. Python ran and returned plenty: it died with
`UnicodeEncodeError: 'charmap' codec can't encode character '\u2248'` — stdout on Windows defaults
to **cp1252**, and every one of these hooks prints `≈` or an emoji.

⚠️ **This is the same defect `python-runner.sh` was written to fix, arriving from one step further
in.** That file's own thesis is that *locating* an interpreter proves nothing and only **running**
one does — which is right, and is why it probes with a sentinel instead of `command -v`. But the
sentinel was `print("PY_RUNNER_OK")`, pure ASCII. **An interpreter that starts is not the same
thing as one that can print what these programs print**, so the probe certified a backend that
then died on the first real line of output. Real Python 3.13 is installed here, works, and passed.

📌 **The fix is to probe under the conditions of use.** The sentinel now carries `≈`, and both the
probe and `py_run` set `PYTHONIOENCODING=utf-8` / `PYTHONUTF8=1` — so the property the programs
depend on is the property the probe tests, and a backend that cannot do the job is rejected rather
than certified. Mutation-tested: strip the environment from the probe alone and native Python is
**refused**, falling through to the WSL backend, which is the correct degradation rather than a
failure.

🚨 **And the hook that could not report was concealing exactly the condition it exists to police.**
With it printing again, the first line out is **222,645 B injected each session — 111% of the
200,000 B budget**, up 2,206 B since 2026-08-20. `CLAUDE.md` records the previous silent stretch
letting `ARCHITECTURE.md` reach 110% unnoticed; it did not stop, because nothing was watching. What
to do about it is a decision about doc structure and is filed separately — the point here is that
it is visible again.

⚠️ Worth keeping as the general shape, because this tree keeps paying for it: **a check that
reports its own failure is only better than silence if the reason it gives is true.** *"python
returned nothing"* sent a reader to look for a missing interpreter, which was present and healthy
the whole time — the third instance this month of an error message that costs more than no message
because it names the wrong cause.

⚠️ **And the first cut of this fix set those variables on `wsl.exe` itself, which sets them on the
Windows side and nowhere else** — caught in review, against this file's own ⚠️ note that *"env vars
do not cross into WSL by themselves; `WSLENV` is the only forwarding mechanism, and it takes
NAMES"*. It was harmless, because WSL's `python3` already defaults to UTF-8 — and that is precisely
what made it worth fixing rather than deleting: a prefix that reads like it fixes the WSL path,
does nothing, and contradicts the paragraph six lines above it is a trap for whoever edits next.
The names are now forwarded and it is proven rather than argued — the Linux interpreter reports
`ioenc= utf-8 utf8= 1 stdout= utf-8` through `py_run`, against `ioenc= None` without the
forwarding.
