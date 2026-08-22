### The activation tap says MEASURED — the #1 honesty gap is closed, and the test we published for it was wrong

`MIND_ARCHITECTURE.md`'s honesty ledger has carried "**proxy** — *labeled*, **pending
verification**" against the per-layer glow since #522 T1 shipped in PR #528, because the
ledger records what is *confirmed* rather than what is *implemented* and **nobody had ever
run `organic-math-mind-runtime`**. It has now been run, on organon-one (RTX 5090, CUDA
13.3, `gemma-4-12B-it-QAT-Q4_0.gguf`, 48L×16H, every layer GPU-offloaded). It printed:

```text
mind-runtime: activation tap MEASURED — real per-layer tensors (#522 T1) (48 layers requested)
```

Frames carry `flags=0x6`/`0x7` — `FLAG_RESID_MEASURED` + `FLAG_MLP_MEASURED` — so the #482
dashboard's provenance glyphs for `layer_norm` and `mlp_act` read `=`. 📌 **It was the
Windows/CUDA path that got there first, not Metal.** The tap is `llama-cpp-4`'s safe
`cb_eval` API, so this is evidence about the API rather than about one backend; Metal is
still unrun, and `head_summ` stays a labeled proxy (#522 Tier 2's flash-attention trade).

⚠️ **The ledger's own acceptance test was wrong, and that correction is worth more than the
flag.** It predicted the measured profile would "rise monotonically with depth instead of
showing the proxy's travelling sine". Real residual norms do not: they climb to a
**mid-late peak and then fall**, collapsing sharply at the final layer (L47 ≈ 0.07–0.11
against a peak of 1.5 at L22–L25). `mlp_act` is more lopsided still — layers 0–1 hold the
maximum and the rest of the stack sits **two orders of magnitude** below. Anyone checking
the glow against that sentence would have concluded the tap had failed. New §3.1 replaces
it with three properties that are each decisive on their own, all verified against four
consecutive tokens of a real generation: the proxy's ceiling is **1.0** so it can never
produce the measured path's **exact 1.5000** (seen in 4/4 frames); the proxy's floors are
0.135 and 0.100 so it cannot produce the observed 0.0057 (**160 of 192** `mlp_act` samples
sit below its floor); and the proxy *travels* by construction while the observed argmax
stayed pinned at L25 with **Pearson r 0.94–0.97** between consecutive tokens.

### `--features embedded-llm` could not link on Windows at all, and now can

New **`native/build.rs`** — the workspace's first, and a no-op unless BOTH `embedded-llm`
and a Windows target, so a default build and every macOS/Linux build are unchanged.

`llama-cpp-sys-4` 0.4.2 emits `cargo:rustc-link-lib=static=ggml-cuda` but on Windows never
emits the CUDA **runtime**, **driver** or **cuBLAS** import libraries its symbols resolve
against, nor a search path. So the build compiled all **184** of ggml-cuda's `.cu` files
across **seven** GPU architectures — about twenty minutes — and then died at the *final*
link with **81 unresolved externals** (`cudaLaunchCooperativeKernel`, `cublasStrsmBatched`,
`cuMemCreate`…). Four `rustc-link-lib` lines are the whole fix; `cuda` (the driver API) is
separate from `cudart` (the runtime) and dropping either leaves a partial wall that reads
as a different problem.

⚠️ **Do not do this with `RUSTFLAGS` instead** — tried first, and wrong three ways.
`RUSTFLAGS` is split on **whitespace** and the toolkit lives under `C:\Program Files\…`, so
`-L native=<that>` reaches rustc as two arguments and fails with *"multiple input filenames
provided"*. It **replaces** `.cargo/config.toml`'s `target.*.rustflags`, silently dropping
the `/STACK:33554432` link argument that file documents as required for every MSVC binary.
And it re-fingerprints the whole graph, forcing llama.cpp to rebuild from scratch — which
it did, costing a second full CUDA compile. A build-script directive has none of those
problems and is passed through spaces intact.

🚨 **`CMAKE_GENERATOR=Ninja` remains required and is now confirmed, not assumed.** The live
command line is `ninja -j 32 -j32 install`: the `cmake` crate appends cargo's `NUM_JOBS`
whatever the generator is, MSBuild has no `-j` switch at all, and that is `MSB1001`.
Lowering the number cannot help — `NUM_JOBS=1` fails identically, because the switch itself
is what MSBuild rejects.

⚠️ **The toolkit-discovery fallback shipped picking the OLDEST CUDA, and the comment above
it claimed the opposite** — caught in review. `cuda_root()` did `found.sort()` then
`found.pop()` over `PathBuf`s named `v9.0`, `v12.3`, `v13.3`; `Ord` on those is
**lexicographic**, and `"v9.0" > "v13.3"` because `'9'` beats `'1'`, so `pop()` returned
v9.0. A stray non-version directory (`vNext`) beat every real version outright. The
`CUDA_PATH_V*` loop had the same shape of bug from the other side: it took the first match
from `std::env::vars_os()`, whose iteration order is **unspecified**, so with two toolkits
installed the winner was not a choice at all. Both now parse `(major, minor)` and take the
numeric max, with unparseable names sorting lowest instead of winning.

📌 The comment is the part worth dwelling on. It read *"newest version last so the highest
sorts to the front"* — which contradicted the `pop()` on the line below it **and** was
wrong about the ordering, so it would have talked the next reader out of checking. This is
a fallback of a fallback (NVIDIA's installer sets `CUDA_PATH`), and that is exactly why it
had to be fixed rather than noted: it fails **silently**, linking the wrong toolkit and
surfacing much later as a runtime error a long way from its cause.

### `deploy.ps1 -WithLlm` said "Needs cmake", which was true and badly incomplete

That one sentence was the whole prerequisite list for a switch that also needs MSVC `cl.exe`
(not on a normal shell's PATH), Ninja **with** `CMAKE_GENERATOR=Ninja`, libclang for
bindgen, and the CUDA toolkit — because `Cargo.toml`'s Windows table pins `llama-cpp-4` to
`features = ["cuda"]`, so there is no CPU-only path. Every missing piece fails inside a
*build script*, so the error names a C++ tool and never the switch that pulled it in. All
five are now listed, with the `MSB1001` mechanism and a note that
`CMAKE_CUDA_ARCHITECTURES` cuts the seven-architecture default down for a local build.

📌 **Not verified: the Mind card's own launch path.** What ran here is the terminal binary
driven through its stdin REPL and its OpenAI-compatible endpoint. The plugin launching the
runtime as a child, and the visual reading the ring live, were not exercised.

⚠️ One harness detail that looks like a bug and is not: the REPL's `gen` passes the prompt
**raw**, while `format_chat` is wired only to the HTTP path — so `gen` against an instruct
model returns a 0-character reply as it emits `<eos>` immediately. The tap still fires (the
frame comes from a real forward pass), but a multi-token profile needs
`POST /v1/chat/completions`.
