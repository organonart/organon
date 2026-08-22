//! Build script — the one thing cargo cannot work out on Windows: where CUDA lives.
//!
//! **Everything here is a no-op unless the `embedded-llm` feature is on AND the target
//! is Windows.** A default `cargo build`, and every build on macOS or Linux, reaches the
//! first `return` below and emits nothing. That is deliberate: this file exists to fix a
//! single platform's link line, not to become a general hook.
//!
//! ## Why it is needed (organon#658 Tier 5 follow-up, measured 2026-08-21)
//!
//! `llama-cpp-sys-4` 0.4.2 links `ggml-cuda` **statically** and emits
//! `cargo:rustc-link-lib=static=ggml-cuda` for it — but on Windows it never emits the
//! CUDA **runtime**, **driver** or **cuBLAS** import libraries that `ggml-cuda.lib`'s
//! own symbols resolve against, nor a search path to find them. So a
//! `--features embedded-llm` build compiles all 184 of ggml-cuda's `.cu` files across
//! seven GPU architectures — twenty-odd minutes — and then dies at the *final* link
//! with **81 unresolved externals**:
//!
//! ```text
//! softmax.cu.obj : error LNK2019: unresolved external symbol cudaLaunchCooperativeKernel
//! solve_tri.cu.obj : error LNK2019: unresolved external symbol cublasStrsmBatched
//! fatal error LNK1120: 81 unresolved externals
//! ```
//!
//! Every one of the 81 is `cudart` (`cudaMalloc`, `__cudaRegisterFatBinary`, …), the
//! CUDA driver API (`cuMemCreate`, `cuDeviceGet`, …) or cuBLAS (`cublasGemmEx`, …).
//! The four `rustc-link-lib` lines below are the whole fix.
//!
//! ⚠️ **Do NOT do this with `RUSTFLAGS` instead.** Two reasons, both paid for here:
//! `RUSTFLAGS` is split on **whitespace**, and the default CUDA location is
//! `C:\Program Files\NVIDIA GPU Computing Toolkit\…` — so `-L native=<that>` reaches
//! rustc as two arguments and fails with *"multiple input filenames provided"*. A build
//! script directive is passed through intact, spaces and all. And `RUSTFLAGS` **replaces**
//! `.cargo/config.toml`'s `target.*.rustflags`, which on this workspace silently drops
//! the `/STACK:33554432` link argument that file documents as required for every MSVC
//! binary. Setting it also re-fingerprints every crate in the graph, forcing a full
//! rebuild of llama.cpp; this file does not.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");

    // `CARGO_FEATURE_<NAME>` is set by cargo for each enabled feature of THIS package,
    // which is what we want to branch on — not the host's own cfg.
    if std::env::var_os("CARGO_FEATURE_EMBEDDED_LLM").is_none() {
        return;
    }
    // The TARGET's OS, not the build host's: a cross-compile to Windows still needs this.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let Some(root) = cuda_root() else {
        // A warning, never a hard error. The link will fail a few minutes later with the
        // LNK2019 wall quoted above, and a reader who has seen this line knows why —
        // which is strictly better than failing here and being wrong about a layout we
        // did not anticipate.
        println!(
            "cargo:warning=embedded-llm on Windows needs the CUDA toolkit, and it was not \
             found. Set CUDA_PATH to the toolkit root (the directory holding lib/x64/cudart.lib). \
             Without it the link ends in ~81 unresolved cudart/cuBLAS externals."
        );
        return;
    };

    let libdir = root.join("lib").join("x64");
    if !libdir.join("cudart.lib").exists() {
        println!(
            "cargo:warning=CUDA root {} has no lib/x64/cudart.lib — the embedded-llm link \
             will fail with unresolved cudart/cuBLAS symbols.",
            root.display()
        );
    }
    // Passed through verbatim by cargo, so a path containing spaces is fine here.
    println!("cargo:rustc-link-search=native={}", libdir.display());

    // `dylib`, not `static`: these are import libraries for cudart64_*.dll / cublas64_*.dll,
    // which ship with the driver and toolkit and are found on PATH at run time. Asking for
    // `static` would demand cudart_static.lib and pull in a different, larger link.
    //
    // `cuda` is the DRIVER API (the `cu*` symbols — cuMemCreate, cuDeviceGet). It is a
    // separate library from the runtime `cudart` (`cuda*`), and ggml-cuda's virtual-memory
    // pool uses both; dropping either leaves a partial wall of unresolved externals that
    // looks like a different problem.
    for lib in ["cudart", "cublas", "cublasLt", "cuda"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
}

/// Locate the CUDA toolkit root: the installer's `CUDA_PATH` first, then the highest
/// `CUDA_PATH_V<major>_<minor>` it also sets, then the highest version under the default
/// install location.
///
/// -- **Version order is computed NUMERICALLY, never lexicographically.** These directories
/// are named `v9.0`, `v12.3`, `v13.3`, and compared as *strings* `"v9.0" > "v13.3"` because
/// `'9'` beats `'1'` -- so the obvious `sort()` then `pop()` over the paths selects the
/// OLDEST toolkit on any box with a single-digit version installed, which is exactly
/// backwards. (This shipped that way for one review round; the comment above it even
/// claimed the highest would "sort to the front", which both contradicted the `pop()` below
/// it and was wrong about the ordering.) `std::env::vars_os()` likewise has **no specified
/// iteration order**, so "the first `CUDA_PATH_V*` that exists" is not a choice at all.
/// Both paths parse `(major, minor)` and take the max.
///
/// This is a fallback of a fallback -- NVIDIA's installer sets `CUDA_PATH` -- but a wrong
/// toolkit here does not fail loudly: it links against the wrong CUDA and surfaces later
/// as a runtime error a long way from the cause.
fn cuda_root() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("CUDA_PATH") {
        let p = std::path::PathBuf::from(p);
        if usable(&p) {
            return Some(p);
        }
    }

    // `CUDA_PATH_V12_3` -> (12, 3); highest wins.
    let from_env = std::env::vars_os()
        .filter_map(|(k, v)| {
            let k = k.to_string_lossy().into_owned();
            let tail = k.strip_prefix("CUDA_PATH_V")?;
            let ver = parse_version(tail, '_');
            let p = std::path::PathBuf::from(v);
            if usable(&p) {
                Some((ver, p))
            } else {
                None
            }
        })
        .max_by_key(|(ver, _)| *ver)
        .map(|(_, p)| p);
    if from_env.is_some() {
        return from_env;
    }

    // `.../CUDA/v13.3` -> (13, 3); highest wins.
    let base = std::path::Path::new(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA");
    std::fs::read_dir(base)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| usable(p))
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let ver = parse_version(name.strip_prefix('v').unwrap_or(&name), '.');
            Some((ver, p))
        })
        .max_by_key(|(ver, _)| *ver)
        .map(|(_, p)| p)
}

/// A toolkit root counts only if it actually holds the import libraries we link against.
/// Checking this here is what lets every candidate above be filtered the same way.
fn usable(p: &std::path::Path) -> bool {
    p.join("lib").join("x64").is_dir()
}

/// `"12_3"` or `"12.3"` -> `(12, 3)`. Anything unparseable sorts lowest instead of
/// panicking: a stray directory called `vNext` should lose to a real version, not stop
/// a build.
fn parse_version(s: &str, sep: char) -> (u32, u32) {
    let mut it = s.split(sep);
    let major = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    let minor = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    (major, minor)
}
