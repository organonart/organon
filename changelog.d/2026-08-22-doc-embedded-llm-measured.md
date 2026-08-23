### The LLM runtime hard-links CUDA, which nothing had noticed

`doc/shipping-windows.md` predicted that adding `embedded-llm` would raise the Visual C++
floor from 14.0 to 14.20, and marked it reasoned rather than measured. It has now been
built and inspected. The prediction holds exactly and by the named mechanism —
`VCRUNTIME140_1.dll`'s single imported symbol is `__CxxFrameHandler4` — and it was the
small half of the answer.

🚨 **`organic-math-mind-runtime.exe` statically imports `cublas64_13.dll`**, with real
calls (`cublasSgemm_v2`, `cublasGemmEx`, `cublasStrsmBatched`). That DLL is not a Windows
component and is in no Visual C++ redistributable; on the workstation it exists in exactly
one place, inside the CUDA 13.3 toolkit, and is reachable only because that installer put
its `bin` on PATH. So a Windows build of the runtime **cannot start at all** on a machine
without an NVIDIA GPU and a CUDA runtime — and it fails at loader time, `0xC0000142`,
before `main()`, with no window and no log line.

This is deliberate rather than accidental: `native/Cargo.toml`'s Windows target block adds
`"cuda"` to `llama-cpp-4` so a `.gguf` runs on the RTX 5090 instead of the CPU, which is
the right call for the workstation. What had never been evaluated is what that means for
an artifact that leaves it.

📌 **The clearest instance yet of this document's governing idea**: a build host with no
CUDA toolkit produces a CPU-only binary carrying no `cublas` import whatsoever. Same
command, same commit, two different products — the dependency is supplied by the machine
and is invisible from inside it.

⚠️ Shipping it self-contained would cost about **690 MB before any model**, because cuBLAS
pulls cuBLASLt: 173 MB of runtime, 53 MB of `cublas64_13.dll`, and 464 MB of
`cublasLt64_13.dll`. And organon-two — the machine named as the installer target — has an
RTX 2080 Ti on a 2021 driver, so whether it can load a CUDA 13 cuBLAS at all is now the
first question blocking anything that carries this runtime. The ledger records it as
unchecked.
