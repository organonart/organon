# Shipping a Windows binary — what the target machine must already have

> **What this document is.** The measured answer to the one question a build machine
> cannot answer about itself: *what does Organon take from this box that a stranger's
> box will not have?* A machine that builds a thing is configured **by** building it,
> and from the inside a supplied dependency and an absent one look identical — the
> program works.
>
> Every claim below is marked **measured** or **reasoned**, and the ledger at the
> bottom says which is which without you having to reconstruct it from the prose.
> `CLAUDE.md`'s "What can and can't be verified where" draws the same line for CI;
> this is that discipline pointed at a different machine.

---

## The measurement

| | |
|---|---|
| Console artifact | `native/target/release/organon-console.exe`, 29,370,368 bytes |
| Console built from | `dc7196c`, `cargo build --release --features console-edition --bin organon-console` |
| Runtime artifact | `native/target/release/organic-math-mind-runtime.exe`, 173,302,784 bytes |
| Runtime built from | `7217414`, `cargo build --release --features embedded-llm --bin organic-math-mind-runtime` |
| Toolchain | MSVC 14.44.35207 (x64), `dumpbin` 14.44.35228.0, CUDA 13.3, cmake 4.4.2 |
| Host | organon-one, Windows 11 Pro 10.0.26200 |
| Date | 2026-08-22 |

This is the **first time any organon binary has been inspected this way**. Nothing
before it establishes a baseline, so a number that disagrees with this one is not
necessarily a regression — it may simply be the first honest look at a different
target.

---

## Loader-time: the Visual C++ runtime

`organon-console.exe` imports **nine symbols** from `VCRUNTIME140.dll`:

```
_CxxThrowException        __CxxFrameHandler3        __C_specific_handler
__current_exception       __current_exception_context
memcmp   memmove   memset   memcpy
```

Every one of those has been present in that DLL since the **Visual C++ 2015
redistributable (14.0)**, so that is the floor. Write the reason down next to the
number, because a floor without a reason gets "simplified" into a presence check by
the next person: the floor is 14.0 **because `__CxxFrameHandler4` is absent**. That
symbol is the VS 2019 exception path, and a binary that imported it would need 14.20
or newer.

⚠️ **Do not copy a floor from another product.** The `create-installer` skill's worked
example cites 14.40; that is ONNX Runtime's number, and organon does not use ONNX
Runtime. Deriving it from what organon actually links moved the floor down by seven
years, and the whole value of stage 1 is that it is derived.

**Two absences carry as much information as the imports.**

- **`MSVCP140.dll` is not imported.** That is the C++ standard library, so nothing
  C++ is statically linked into this build. It is Rust and the Windows API.
- **`VCRUNTIME140_1.dll` is not imported.** That is the companion added in VS 2019 for
  the newer exception handler, and its absence is the same fact as `__CxxFrameHandler4`
  seen from the other side.

🚨 **Both absences end the moment `embedded-llm` is added.** llama.cpp is C++, so a
build carrying that feature will link the C++ standard library, and the floor moves
from VC++ 2015 to VS 2019 16.0+ (14.20) — plausibly further. That is a real,
measurable cost of bundling the LLM runtime, and it is measurable *before* deciding:
build the runtime and run `dumpbin /dependents` over it. **That has now been done** —
the prediction holds, and it was the small half of the answer. See
[the `embedded-llm` section](#measured-the-embedded-llm-runtime-is-a-different-animal).

### The Universal CRT is not a prerequisite

The binary also imports six `api-ms-win-crt-*` apisets (`math`, `string`, `runtime`,
`stdio`, `locale`, `heap`). These are the **Universal CRT, an operating-system
component on Windows 10 and later** — they resolve to `ucrtbase.dll` in System32 and
need no redistributable on any supported target. They look alarming in a dependency
list and are the least of the problem.

---

## Runtime: the GPU, which `dumpbin` structurally cannot see

⚠️ **`/dependents` lists static imports only, and Organon's graphics dependency is not
one.** `d3d12.dll`, `vulkan-1.dll`, `dxcompiler.dll` and `d3dcompiler_47.dll` all
appear as **strings inside the binary** and none of them appears in the import table:
wgpu resolves its backends with `LoadLibrary` at runtime. `dxgi.dll` and `opengl32.dll`
*are* statically imported, which makes the list look complete when it is not.

So the prerequisites fall into **two classes that fail differently and need different
answers**, and neither substitutes for the other:

| | Loader-time | Runtime |
|---|---|---|
| What | `VCRUNTIME140.dll` | GPU adapter + driver |
| When | before `main()` | after `main()` |
| Symptom | `0xC0000142` / `STATUS_DLL_INIT_FAILED` — no window, no log line, no message | wgpu adapter selection fails |
| Who can report it | **only an installer's prerequisite check** | the product itself |

The left column is why a plain zip is the *weaker* of the two shipping options: an
installer can check before anything runs, and a zip can only fail.

---

## Whether the product can say anything

**`organon-console.exe` is a console-subsystem binary** — measured: no
`windows_subsystem` attribute exists anywhere in the crate. That has one good half and
one bad half, and they are not separable without a decision:

- **Good.** Run from a terminal, it prints. A runtime failure in the right-hand column
  above can reach a person.
- **Bad.** Launched from a Start-menu shortcut or Explorer, Windows allocates a console
  window it never asked for. `FreeConsole` does not fix this — it closes the window
  only after it has appeared.

📌 **This wants deciding before any shortcut exists**, because the resolution is not a
toggle: it is GUI subsystem gated on `not(test)`, plus
`AttachConsole(ATTACH_PARENT_PROCESS)`, plus reopening `CONOUT$` onto the standard
handles — skip that last step and `GetStdHandle` returns null after a *successful*
attach, and the program looks like it has nothing to say.

---

## The version and edition gate

`organon-console.exe --version` prints **`Organon Console 0.1.0`** and exits 0 without
opening a window. That is a usable build gate: a packaging script can ask the artifact
what it is rather than trusting the command that produced it, which matters because
`cargo build` writes the same path whatever features it was given.

⚠️ **It proves the version, not the edition.** The name it prints is a hardcoded
literal in `console_main.rs`, and `organon-core/src/edition.rs` states the same string
independently for `EDITION.name()` — two copies that cannot see each other. What
actually guarantees the edition is `required-features = ["console-edition"]` on the bin
target, which makes a wrong-edition `organon-console.exe` unbuildable rather than
merely unlikely. Use that as the guarantee and `--version` as the version pair.

---

## Measured: the `embedded-llm` runtime is a different animal

`organic-math-mind-runtime.exe`, built at `7217414` with
`cargo build --release --features embedded-llm --bin organic-math-mind-runtime`
(25 minutes — llama.cpp compiles from source and needs cmake), is **173,302,784 bytes**,
roughly six times the Console, and imports four things the Console does not:

| Import | What it is | Effect on the floor |
|---|---|---|
| `MSVCP140.dll` | the C++ standard library | none by itself |
| `VCRUNTIME140_1.dll` | `__CxxFrameHandler4` — the VS 2019 exception path | **14.0 → 14.20** |
| `VCOMP140.DLL` | the OpenMP runtime (`_vcomp_*`, `omp_get_thread_num`) | none — same redistributable |
| `cublas64_13.dll` | **NVIDIA cuBLAS** | not a redistributable question at all |

The Visual C++ prediction holds exactly, and by the mechanism that was named rather than
by coincidence: `VCRUNTIME140_1.dll`'s single imported symbol is `__CxxFrameHandler4`.

### 🚨 The Windows LLM runtime hard-links CUDA

`cublas64_13.dll` is a **static import** with real calls — `cublasSgemm_v2`,
`cublasGemmEx`, `cublasStrsmBatched`, `cublasSetMathMode`. It is not a Windows component
and it is in no Visual C++ redistributable. On this machine it exists in exactly one
place, `C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.3\bin\x64\`, put there by
the CUDA toolkit and reachable only because that installer added its `bin` to PATH.

This is deliberate and `native/Cargo.toml` says so: the Windows target block adds `"cuda"`
to `llama-cpp-4` "so a `.gguf` on the RTX 5090 runs on the GPU instead of the CPU". That
is the right call for the workstation. What nothing had evaluated is what it means for an
artifact that leaves it:

- **A machine with no NVIDIA GPU cannot run this binary at all**, and it fails at *loader*
  time — `0xC0000142`, before `main()`, no window, no log line, nothing to read. It is the
  left-hand column of the two-classes table above in its purest form, and only a
  prerequisite check can cover it.
- 📌 **The artifact's requirements depend on the machine that built it.** A build host with
  no CUDA toolkit produces a CPU-only binary carrying no `cublas` import whatsoever. Same
  command, same commit, two different products — this document's governing idea arriving
  in its most literal available form.

⚠️ **organon-two is the machine named for receiving an installer, and it has an RTX 2080 Ti
on a 2021 driver.** Whether a driver of that vintage can load a CUDA 13 cuBLAS is
**unverified here** and is the first thing to establish before treating that box as a
target for anything carrying this runtime. Do not assume the GPU being NVIDIA is
sufficient; the driver is a separate gate from the card.

### What shipping it would cost

Redistributing cuBLAS is permitted under NVIDIA's CUDA EULA redistributable list, so this
is a size question rather than a licensing one. The size is not small, because cuBLAS
itself depends on cuBLASLt:

| | |
|---|---|
| `organic-math-mind-runtime.exe` | 173,302,784 bytes |
| `cublas64_13.dll` | 52,697,712 bytes |
| `cublasLt64_13.dll` (required by cuBLAS) | 463,655,536 bytes |
| **total, before any model** | **~690 MB** |

⚠️ **A `.gguf` is not in that figure and cannot be.** The runtime loads a model an installer
cannot carry, so it becomes a download-or-locate decision — and anything downloaded rather
than installed has no `[Files]` entry, so it must be named explicitly in
`[UninstallDelete]` or gigabytes stay behind that nobody can account for.

📌 **Separately from all of this: the Organon Console has no route to launch the runtime.**
`mind_runtime_path()` has one caller, inside `editor_ui` — the plugin and standalone
editor. `console_main.rs` and `organon-console/src/` contain no reference to it. Bundling
the runtime with a Console installer today would install ~690 MB that nothing on the
machine can start.

---

## Who produces the artifact — nobody, on Windows

Nothing in CI builds this binary on the platform it ships to. `.github/workflows/ci.yml`
builds the Console on Linux (leg 2) and macOS (leg 5), and for Windows runs only
`cargo check --target x86_64-pc-windows-msvc --features console-edition` on a Linux
runner (leg 3); leg 4, the real Windows job, builds the **default** edition.

📌 **That is a considered trade, not an oversight**, and the workflow header sets out the
arithmetic rather than leaving it to look like a missing step: `windows-latest` bills at
2x the Linux per-minute rate and is slower on the I/O-heavy work cargo does, so one
Windows leg costs roughly 4x one Linux leg. The choice recorded there is one Windows job,
default edition only, with the Console's Windows compile coverage coming from the 1x
cross-check.

⚠️ **Worth re-reading now that something ships.** That trade was made when no artifact
left the machine, and "compile coverage" is a different claim from "this binary has been
produced on this platform" — `cargo check` does not link, and cannot produce a release
artifact by construction. Changing it is a spend decision, and the workflow's own rule is
to ask before adding a runner, so this document records the gap rather than closing it.

---

## Ledger

**Measured on organon-one, 2026-08-22, at `dc7196c`:**

- the import table of `organon-console.exe` and the nine `VCRUNTIME140.dll` symbols
- the absence of `MSVCP140.dll` and `VCRUNTIME140_1.dll`
- the four GPU DLL names present as strings and absent from the import table
- the absence of any `windows_subsystem` attribute
- `--version` output and its exit code

**Measured on organon-one, 2026-08-22, at `7217414` (the `embedded-llm` runtime):**

- the import table of `organic-math-mind-runtime.exe`, and that it adds `MSVCP140.dll`,
  `VCRUNTIME140_1.dll`, `VCOMP140.DLL` and `cublas64_13.dll`
- that `VCRUNTIME140_1.dll`'s only imported symbol is `__CxxFrameHandler4` — so the floor
  moves to 14.20 by exactly the mechanism predicted, not by coincidence
- that `cublas64_13.dll` exists on this machine only inside the CUDA 13.3 toolkit
  directory, and not in System32
- the on-disk sizes of the runtime, `cublas64_13.dll` and `cublasLt64_13.dll`

**Measured on organon-one, 2026-08-22 (the installer, `native/installer/`):**

- it compiles, installs silently, and exits 0; the install tree holds exactly the
  expected files plus Inno's uninstaller
- installing over an existing `%APPDATA%\OrganicMath` left all 21 gallery files
  untouched — `onlyifdoesntexist` behaves as intended
- uninstall exits 0 and the install directory is **removed**, so `dirifempty` is not
  defeated; galleries, the models folder and `OrganonShell` are left behind, and no
  uninstall entry remains
- four of `build.ps1`'s five refusals were provoked and observed to fire; the
  Inno-Setup-absent one was not

**Reasoned, not run:**

- that the UCRT needs no redistributable on Windows 10+
- that a build host without a CUDA toolkit yields a CPU-only runtime with no `cublas`
  import — it follows from `llama-cpp-sys-4`'s build.rs turning every backend off unless
  its feature is on, but no such build has been produced here to confirm it
- SmartScreen behaviour on an unsigned artifact; there is no code-signing certificate
  and nobody in this fleet has observed the warning

**Not done at all:**

- 🚨 **Whether organon-two's 2021 driver can load a CUDA 13 cuBLAS.** That box is the
  named installer target and has an RTX 2080 Ti; the card being NVIDIA is not the
  question, the driver is, and nobody has checked. Anything carrying the LLM runtime is
  blocked on this.
- 🚨 **No organon binary has ever been handed to a machine that did not build it.**
  Everything here is a property of the artifact, not of a successful install. The
  second machine is the deliverable that closes this section, and until it runs, the
  loader-time column above is a prediction.
