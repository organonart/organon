### A little mute button on the viewport

🚨 **The console does not ask a module to be quiet; it turns the module down.** `organon_module
::input`'s refusal table has no audio in either direction and says out loud that the absence is
*promised, not enforced* — a separate process can open WASAPI itself, and Ascent does. A `Mute`
verb there would be the console **asking**, so a producer that ignored it could not be silenced:
a control that works only while nobody minds. Naming the child process to Windows' own mixer
needs no grant and cannot be declined.

📌 **The music itself needed nothing** — Ascent's sound already plays. Only the quieting needed
building.

⚠️ **`ModuleProcess::pid` is new and its doc restricts it**: the only legitimate shape is *naming*
a process to an OS facility that already governs it from outside. `OpenProcess` appears nowhere.
It answers `None` once the process has ended, because a pid is reusable the moment a process is
reaped — handing it out afterwards is handing out somebody else's process.

⚠️ **`windows` joins `windows-sys`, on that note's own test.** It chose `windows-sys` because
"two functions do not justify the larger crate"; this is a COM subsystem five interfaces deep,
and `windows 0.62` is **already in `Cargo.lock`** via wgpu's Windows tail — so it resolves to a
crate that was going to be compiled anyway, the same argument `windows-sys` was admitted under.

🚨 **The mute is re-asserted every three seconds, not set once.** A process that has not yet
played anything **has no audio session to mute** — Windows creates one lazily — so a mute issued
before the first note finds nothing and would stay unapplied for ever under change detection.
That is the lighting renderer's lesson on this workstation, reached from the other end and
reusing its period. It is also why `set_process_muted` answering `false` is not an error.

⚠️ **Hidden while playing and unattended; shown whenever muted.** A viewport is the one place
that must stay clean — and *silence is indistinguishable from a module with nothing to say*, so
the control is the only thing that can attribute the quiet to a hand. A rectangle too small gets
none.

⚠️ **A departed producer is forgotten**, or it comes back silent with nothing on screen to say
why. That line was written after the test for it, which is the wrong order: the test passed while
nothing called `forget`, which is the "declared but unwired" defect this tree keeps finding.

⚠️ **The COM path has no test at all** — it needs a real endpoint, a real session and a real
producer making a sound. Nobody has muted anything.
