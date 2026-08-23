//! Embedded llama.cpp runtime (#367 Tier 2b — the REAL visible mind).
//!
//! The real counterpart to the synthetic `organic-math-mind-writer`: this bin loads
//! the `.gguf` the editor picked, runs **live inference** on a typed prompt, and taps
//! per-token activations into the same mind ring (`mind_ring::MindRingWriter`) the
//! visual's live-glow path already reads — so the connectome node-glow lights
//! from a model that is actually generating, no shader change.
//!
//! **Feature-gated, default OFF.** The whole llama.cpp dependency (and the C++ build it
//! drags in) is behind the `embedded-llm` cargo feature; this target's
//! `required-features = ["embedded-llm"]` means the default build never compiles it.
//! Build + run it on the Mac beside the visual:
//! ```text
//! cargo build --release --features embedded-llm
//! ./target/release/organic-math-mind-runtime         # or launched from the Mind card
//! ```
//! then in the plugin's Mind card: Load a `.gguf`, type a prompt, press **Generate**.
//! The **Specimen** view lights per token on its own while frames are arriving — there
//! is no "Live" mode to switch on (#520; `mind[2]` selects *geometry*: Specimen or the
//! #507 embedding Galaxy).
//!
//! ## OpenAI-compatible server (#367 Tier 2c — the LM Studio drop-in for #317)
//! Besides the one-shot Mind-card path, this runtime also listens on
//! `http://127.0.0.1:<port>/v1/chat/completions` (default port 1234, override with
//! `ORGANIC_MATH_LLM_PORT`). It speaks the same minimal OpenAI wire shape the #317
//! Performer agent (`agent.rs`) already POSTs to LM Studio / Ollama — so pointing the
//! agent's `organic-math-agent.txt` endpoint at this port makes the embedded model the
//! Performer's inference backend, **and every agent turn taps the activation ring** (the
//! generation flows through the same `generate_core` tap). Both paths apply the model's
//! built-in chat template + a repetition penalty, so instruct models answer instead of
//! drifting into repetition loops.
//!
//! ## The activation tap — REAL per-layer internals, with an honest fallback (#522 T1)
//!
//! **This block previously documented a constraint that no longer exists**, and saying
//! so plainly is the point of rewriting it rather than deleting it. It read: the safe
//! API exposes logits and (in embedding mode) the final hidden state but *not* per-layer
//! residual/MLP norms; reading those needs llama.cpp's ggml eval callback
//! (`ggml_backend_sched_set_eval_callback`), "exposed only in the raw `llama-cpp-sys-2`
//! layer — an unsafe FFI tap." That was true of `llama-cpp-2` 0.1.150, which this file
//! was written against. **It is not true of `llama-cpp-4` 0.4.2**, which ships the same
//! `cb_eval` tap as a *safe* API. Four separate issues had scoped that work as an unsafe
//! FFI project (#409 T1, #507 T2–T5, #423 T4, #482's provenance flip); it was a crate
//! bump.
//!
//! So the frame is now filled two ways, and **which one is in force is recorded in the
//! frame itself** rather than left for a reader to assume:
//!
//! - **Measured (the normal path).** A [`TensorCapture`] is attached over a strided set
//!   of layers before the context is built. After each `decode`, `l_out-{N}` gives that
//!   layer's residual and `ffn_out-{N}` its FFN output; their **L2 norms**, normalized
//!   across the captured layers, become `layer_norm` / `mlp_act`. Real numbers out of
//!   the real forward pass. The #482 dashboard's `Provenance` glyphs read `=`.
//! - **Proxy (the fallback).** If capture yields nothing — an older model whose graph
//!   node names don't match, a backend that doesn't fire the callback, or the feature
//!   off — the runtime falls back to the *original* shaping: softmax **entropy** as a
//!   global effort signal and top-token **confidence**, spread across the real
//!   layer/head counts as a travelling front. A faithful proxy for *effort*, never a
//!   readout of internals, and the glyphs stay `?`.
//!
//! The fallback is not a formality: nothing about whether the callback fires **on
//! Metal** is verifiable in a GPU-less, model-less sandbox, so the proxy path stays
//! intact and tested rather than deleted on the assumption that the tap always works.
//!
//! **`head_summ` remains a proxy in Tier 1.** Per-head attention weights require the
//! attention matrix to actually materialize in the graph, which means flash-attention
//! **off** — that is #522 Tier 2's trade to make, using the `mind[7]` `full_attn` dial
//! that already exists. Tier 1 does not silently mark it measured.
//!
//! Same `MindFrame` layout, same offsets, same widgets: the only visible change is which
//! provenance glyph each widget carries.
//!
//! **#482 Tier 2 — the MEASURED headliners.** The `entropy`, `confidence`, and the
//! **top-k** next-token candidates (id + prob + decoded text) written per frame are the
//! *actual* temperature-softmax over the real logits — not the proxy. These feed the
//! dashboard's top-k bars and the entropy/confidence heartbeat, so those two widgets are
//! honest even before the `cb_eval` per-layer tap lands (that only upgrades the layer /
//! head heat, which stays a proxy until then).
//!
//! ## Two runtimes at once (#191 Tier 1 — the base model beside its fine-tune)
//!
//! Nothing here needed building: `$ORGANON_IPC_NS` already forks every `$TMPDIR` mmap
//! and sidecar through `ipc::ns_file`, which is the same mechanism that lets an Organon
//! session and an Organon Mind session coexist. Give each runtime its own namespace and
//! each writes its own `<ns>-mind.bin`; a reader names the ring it means with
//! `MindRingReader::open_ns("mind-base")` rather than opening whichever one its own
//! process resolved to.
//!
//! ```text
//! ORGANON_IPC_NS=mind-base  ORGANIC_MATH_LLM_PORT=1234 organic-math-mind-runtime
//! ORGANON_IPC_NS=mind-tuned ORGANIC_MATH_LLM_PORT=1235 organic-math-mind-runtime
//! ```
//!
//! ⚠️ **The HTTP port is NOT namespaced and must be set per runtime.** It is a TCP
//! listener, not a `$TMPDIR` path, so the fork cannot reach it: the second runtime finds
//! 1234 taken, prints `could not bind …` and comes up with its OpenAI-compatible server
//! **off**. The ring still fills — the Mind-card prompt path is unaffected — so a
//! fan-out over HTTP would quietly reach one model twice. Set the port explicitly.
//!
//! ⚠️ **Two runtimes cost two models of VRAM.** `gemma-4-12B-it-QAT-Q4_0` twice is
//! ~13-17 GB with context, and on organon-one the local TTS reclaims ~21 GB the moment
//! Vera speaks. That is an operating constraint on the demo, not a design flaw.

// The default build (no `embedded-llm`) skips this whole target via
// `required-features`, so this arm is only reached by an explicit no-feature `rustc`
// (e.g. an IDE) — keep it well-formed regardless.
#[cfg(not(feature = "embedded-llm"))]
fn main() {
    eprintln!(
        "organic-math-mind-runtime: built without the `embedded-llm` feature — nothing to do.\n\
         Rebuild with:  cargo build --release --features embedded-llm"
    );
}

#[cfg(feature = "embedded-llm")]
fn main() {
    runtime::run();
}

#[cfg(feature = "embedded-llm")]
mod runtime {
    use organic_math_native::mind_ring::{
        write_snippet, MindFrame, MindRingWriter, MR_MAX_HEADS, MR_MAX_LAYERS, MR_TOK_LEN, MR_TOPK,
    };
    use organic_math_native::{gguf, ipc, mind_log};

    use std::io::{BufRead, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::num::NonZeroU32;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};

    use llama_cpp_4::context::params::{LlamaContextParams, LlamaFlashAttnType};
    use llama_cpp_4::context::{CapturedTensor, TensorCapture};
    use llama_cpp_4::llama_backend::LlamaBackend;
    use llama_cpp_4::llama_batch::LlamaBatch;
    use llama_cpp_4::model::params::LlamaModelParams;
    use llama_cpp_4::model::{AddBos, LlamaChatMessage, LlamaModel};
    // `Special` is only consumed by the deprecated (but String-returning) `token_to_str`.
    #[allow(deprecated)]
    use llama_cpp_4::model::Special;
    use llama_cpp_4::token::LlamaToken;

    // The frame's flag bits now live in `mind_ring` beside the field they describe —
    // this file used to redeclare `FLAG_EOG` locally with a comment saying it "matches
    // the synthetic writer," which is a hand-kept invariant the compiler could not check.
    use organic_math_native::mind_ring::{FLAG_EOG, FLAG_MLP_MEASURED, FLAG_RESID_MEASURED};
    /// Hard cap on generated tokens per prompt (a runaway guard; EOS stops earlier).
    const MAX_GEN_TOKENS: usize = 512;
    /// #522 Tier 1 — `llama-cpp-4`'s `meta_val_str` takes a caller-supplied buffer size
    /// where `llama-cpp-2`'s allocated internally. Ample for the short metadata strings
    /// we read (`general.architecture` and friends).
    const META_BUF: usize = 512;
    /// The same, for `tokenizer.chat_template` — a multi-KB Jinja program on modern
    /// instruct models, so it needs its own, much larger, budget. Too small returns a
    /// buffer error, which `render_jinja_template` would silently treat as "no template"
    /// and fall through to the built-in path.
    const TEMPLATE_BUF: usize = 128 * 1024;
    /// Repetition penalty applied to recently-emitted tokens' logits before sampling, and
    /// how many recent tokens it covers. Without this, instruct models fed off-template
    /// (or the honest low-temp path) fall into `0.0.0.0…` style loops.
    const REPEAT_PENALTY: f32 = 1.3;
    const REPEAT_LAST_N: usize = 64;
    /// Default OpenAI-compatible server port (LM Studio's default, so the agent's stock
    /// `organic-math-agent.txt` endpoint works unchanged). Override: `ORGANIC_MATH_LLM_PORT`.
    const DEFAULT_HTTP_PORT: u16 = 1234;
    /// Context window for the HTTP/agent path — big enough for the #317 capability catalog
    /// system prompt. The one-shot Mind-card path keeps the smaller dial default.
    const SERVER_MIN_CTX: u32 = 8192;

    /// Runtime dials read from the reserved `Shared.mind[4..8]` slots each loop.
    #[derive(Clone, Copy)]
    struct Dials {
        temperature: f32,
        context_len: u32,
        rate_cap: f32, // tokens/sec, 0 = uncapped
        full_attn: bool,
    }

    impl Dials {
        /// Decode the dials from a `Shared` snapshot, applying sane fallbacks so an
        /// all-zero (never-stamped) snapshot still yields a usable configuration.
        fn from_shared(s: &ipc::Shared) -> Dials {
            // Detect a never-stamped snapshot (the whole dial block is exactly zero, which
            // `process()` never produces — it stamps a positive context length). Only then
            // do we substitute defaults; otherwise a deliberate temperature of 0.0 (greedy)
            // is honored instead of being silently bumped to 0.8.
            let unstamped = s.mind[4] == 0.0 && s.mind[5] == 0.0 && s.mind[6] == 0.0;
            if unstamped {
                return Dials { temperature: 0.8, context_len: 2048, rate_cap: 0.0, full_attn: false };
            }
            let temperature = s.mind[4].clamp(0.0, 2.0);
            let context_len = if s.mind[5] >= 1.0 { s.mind[5] as u32 } else { 2048 };
            let rate_cap = s.mind[6].max(0.0);
            let full_attn = s.mind[7] > 0.5;
            Dials { temperature, context_len, rate_cap, full_attn }
        }
    }

    // =======================================================================
    // stdin command REPL (#367 Tier 2 — the in-plugin terminal)
    // =======================================================================
    //
    // The plugin spawns this runtime as a child with a piped stdin and sends one command
    // per line. This is ADDITIVE to the IPC-counter path (`model_gen`/`prompt_gen`): those
    // are stamped by the audio-thread `process()`, so they only fire while the Ableton track
    // is processing audio. The stdin REPL lets a user load / generate / tune dials WITHOUT
    // the transport running, and gives the Mind card a live console. Both paths drive the
    // same `load_model` / `generate_core` internals and the same in-memory dials.

    /// Per-field dial overrides typed via the stdin REPL, layered on top of the
    /// `Shared`-derived `Dials` each loop. `None` = defer to the Shared snapshot (so the
    /// IPC dials keep working); `Some` = the REPL value wins until changed.
    #[derive(Default, Clone, Copy)]
    struct DialOverrides {
        temperature: Option<f32>,
        context_len: Option<u32>,
        rate_cap: Option<f32>,
        full_attn: Option<bool>,
    }

    impl DialOverrides {
        /// Fold the overrides onto a `Shared`-derived base, so an unset field defers to IPC.
        fn apply(&self, base: Dials) -> Dials {
            Dials {
                temperature: self.temperature.unwrap_or(base.temperature),
                context_len: self.context_len.unwrap_or(base.context_len),
                rate_cap: self.rate_cap.unwrap_or(base.rate_cap),
                full_attn: self.full_attn.unwrap_or(base.full_attn),
            }
        }
    }

    /// Cross-thread generation control for the REPL. `generate_core` runs synchronously in
    /// the main loop (it blocks the poll), so a `stop`/`quit` typed mid-generation can't be
    /// dispatched until the run returns — the stdin reader thread instead flips `abort` the
    /// moment it reads such a line, and `generate_core` polls it each token to break early.
    /// `busy` is raised for the duration of a run so `status` can report it.
    #[derive(Default)]
    struct GenControl {
        abort: AtomicBool,
        busy: AtomicBool,
    }

    /// RAII guard that clears the `busy` flag on every exit path of `generate_core`.
    struct BusyGuard<'a>(&'a AtomicBool);
    impl Drop for BusyGuard<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
        }
    }

    /// Result of dispatching one stdin command.
    enum Cmd {
        Continue,
        Quit,
    }

    /// Execute one line from the stdin REPL (#367 Tier 2 — the in-plugin terminal). Drives
    /// the SAME `load_model` / `generate_core` internals as the IPC-counter path, so a user
    /// can load / generate / tune dials with the transport stopped. `overrides` layers typed
    /// dial values over the `Shared`-derived `base_dials` (so IPC dials still apply when
    /// unset). Returns `Cmd::Quit` on `quit`/`exit` (or when the parent closes stdin).
    #[allow(clippy::too_many_arguments)]
    fn dispatch_command(
        line: &str,
        backend: &LlamaBackend,
        reader: &ipc::Reader,
        model_gen: u32,
        model: &mut Option<LlamaModel>,
        header: &mut Option<gguf::GgufHeader>,
        overrides: &mut DialOverrides,
        base_dials: Dials,
        ctl: &GenControl,
        writer: &mut MindRingWriter,
    ) -> Cmd {
        let mut parts = line.trim().splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
        let arg = parts.next().unwrap_or("").trim();
        match cmd.as_str() {
            "" => {}
            "help" => println!(
                "mind-runtime: commands — gen [text] | load [path] | temp <f> | ctx <n> | \
                 rate <f> | fullattn <0|1> | stop | status | help | quit"
            ),
            "quit" | "exit" => return Cmd::Quit,
            "stop" => {
                // The reader thread already flipped `abort` (that's what actually breaks a
                // running generation); this is the console-visible confirmation.
                ctl.abort.store(true, Ordering::Relaxed);
                println!("mind-runtime: stop requested.");
            }
            "status" => {
                let path = std::fs::read_to_string(ipc::model_sidecar_path()).unwrap_or_default();
                let path = path.trim();
                let dims = header
                    .as_ref()
                    .map(|h| format!("{}L×{}H", h.n_layers, h.n_heads))
                    .unwrap_or_else(|| "—".into());
                let d = overrides.apply(base_dials);
                println!(
                    "mind-runtime: status — model={} [{}] loaded={} busy={} | \
                     temp={:.2} ctx={} rate={:.1} fullattn={}",
                    if path.is_empty() { "none" } else { path },
                    dims,
                    model.is_some(),
                    ctl.busy.load(Ordering::Relaxed),
                    d.temperature,
                    d.context_len,
                    d.rate_cap,
                    d.full_attn,
                );
            }
            "load" => {
                if !arg.is_empty() {
                    if let Err(e) = std::fs::write(ipc::model_sidecar_path(), arg.as_bytes()) {
                        eprintln!("mind-runtime: could not write model sidecar: {e}");
                        return Cmd::Continue;
                    }
                }
                load_model(backend, model, header);
            }
            "gen" | "generate" => {
                if !arg.is_empty() {
                    if let Err(e) = std::fs::write(ipc::mind_prompt_path(), arg.as_bytes()) {
                        eprintln!("mind-runtime: could not write prompt sidecar: {e}");
                        return Cmd::Continue;
                    }
                }
                let prompt = read_prompt();
                if prompt.trim().is_empty() {
                    eprintln!(
                        "mind-runtime: empty prompt — type `gen <text>` or set one in the Mind card."
                    );
                    return Cmd::Continue;
                }
                match (model.as_ref(), header.as_ref()) {
                    (Some(m), Some(h)) => {
                        let d = overrides.apply(base_dials);
                        println!("mind-runtime: generating…");
                        match generate_core(backend, reader, model_gen, m, h, &prompt, d, ctl, writer)
                        {
                            GenOutcome::Done(text) => log_reply(&text, d.full_attn),
                            GenOutcome::Failed(e) => {
                                eprintln!("mind-runtime: generation failed: {e}")
                            }
                            GenOutcome::Aborted => {
                                println!("mind-runtime: generation aborted (model changed).")
                            }
                        }
                    }
                    _ => eprintln!("mind-runtime: no model loaded — `load <path>` first."),
                }
            }
            "temp" => match arg.parse::<f32>() {
                Ok(v) => {
                    let v = v.clamp(0.0, 2.0);
                    overrides.temperature = Some(v);
                    println!("mind-runtime: temp = {v:.2}");
                }
                Err(_) => eprintln!("mind-runtime: usage: temp <0.0..2.0>"),
            },
            "ctx" => match arg.parse::<u32>() {
                Ok(v) => {
                    let v = v.clamp(64, 32768);
                    overrides.context_len = Some(v);
                    println!("mind-runtime: ctx = {v}");
                }
                Err(_) => eprintln!("mind-runtime: usage: ctx <tokens>"),
            },
            "rate" => match arg.parse::<f32>() {
                Ok(v) => {
                    let v = v.max(0.0);
                    overrides.rate_cap = Some(v);
                    println!("mind-runtime: rate = {v:.1} tok/s (0 = max)");
                }
                Err(_) => eprintln!("mind-runtime: usage: rate <tokens/sec, 0 = max>"),
            },
            "fullattn" => {
                if matches!(arg, "1" | "on" | "true" | "yes") {
                    overrides.full_attn = Some(true);
                    println!("mind-runtime: fullattn = true");
                } else if matches!(arg, "0" | "off" | "false" | "no") {
                    overrides.full_attn = Some(false);
                    println!("mind-runtime: fullattn = false");
                } else {
                    eprintln!("mind-runtime: usage: fullattn <0|1>");
                }
            }
            other => eprintln!("mind-runtime: unknown command '{other}' — try: help"),
        }
        Cmd::Continue
    }

    pub fn run() {
        let backend = match LlamaBackend::init() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("mind-runtime: failed to init llama backend: {e}");
                std::process::exit(1);
            }
        };

        let mut writer = match MindRingWriter::create() {
            Ok(w) => w,
            Err(e) => {
                eprintln!(
                    "mind-runtime: failed to create mind ring at {}: {e}",
                    ipc::mind_ring_path().display()
                );
                std::process::exit(1);
            }
        };

        // #191 T1 — say which ring this runtime owns, and say it on the happy path.
        //
        // Two runtimes — a base model and its own fine-tune — are two processes with two
        // `$ORGANON_IPC_NS` values writing two rings, which is the whole of "two
        // runtimes, two rings": the namespace fork already carries it, nothing new is
        // invented. What the fork does NOT do is make the two processes distinguishable
        // in two terminals, and the failure that hides is the expensive one — both
        // runtimes on one namespace overwrite each other's frames in a single ring, and
        // a difference lens then reads a model against itself. There is no error for
        // that; it looks like a working demo whose two traces agree perfectly.
        println!(
            "mind-runtime: namespace '{}' -> ring {}",
            ipc::namespace(),
            ipc::mind_ring_path().display()
        );

        let reader = ipc::Reader::open();

        // Optional OpenAI-compatible HTTP server (the LM Studio drop-in for #317). Bind
        // non-blocking so the single generation thread also polls the IPC each loop.
        let port: u16 = std::env::var("ORGANIC_MATH_LLM_PORT")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(DEFAULT_HTTP_PORT);
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                println!(
                    "mind-runtime: OpenAI-compatible server on \
                     http://127.0.0.1:{port}/v1/chat/completions \
                     (point organic-math-agent.txt here to replace LM Studio)"
                );
                Some(l)
            }
            Err(e) => {
                eprintln!(
                    "mind-runtime: could not bind 127.0.0.1:{port} ({e}) — HTTP server off \
                     (set ORGANIC_MATH_LLM_PORT or stop the other server). \
                     The Mind-card prompt path still works."
                );
                None
            }
        };

        println!(
            "mind-runtime: watching {} for model_gen/prompt_gen (Ctrl-C to stop)",
            ipc::ipc_path().display()
        );

        // Loaded model + the header (for n_layers/n_heads → frame layout), and the
        // generation counters we edge-detect.
        let mut model: Option<LlamaModel> = None;
        let mut header: Option<gguf::GgufHeader> = None;
        let mut last_model_gen: u32 = 0;
        let mut last_prompt_gen: u32 = 0;
        let mut seeded = false;
        // Throttle the "no model loaded" notice so an un-serviceable prompt edge (which we
        // deliberately don't consume, so it runs once a model loads) doesn't spam the log.
        let mut warned_no_model = false;

        // Load an already-selected model straight from the sidecar at startup, so the
        // HTTP/agent path works even when the plugin's `process()` is idle (transport
        // stopped) and never bumps `model_gen`.
        if std::fs::read_to_string(ipc::model_sidecar_path())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            load_model(&backend, &mut model, &mut header);
        }

        // stdin command REPL (#367 Tier 2). One reader thread pumps each line over an mpsc
        // channel; the main loop drains it non-blockingly each iteration. The thread also
        // pre-flips `ctl.abort` for `stop`/`quit` so those interrupt a running generation
        // (which blocks this loop). On EOF the thread drops the sender → the main loop's
        // `try_recv()` sees `Disconnected` and exits cleanly (no busy-spin).
        let ctl = Arc::new(GenControl::default());
        let mut overrides = DialOverrides::default();
        let (tx, rx) = mpsc::channel::<String>();
        {
            let ctl = ctl.clone();
            std::thread::spawn(move || {
                let stdin = std::io::stdin();
                for line in stdin.lock().lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    let first = line
                        .trim()
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if first == "stop" || first == "quit" {
                        ctl.abort.store(true, Ordering::Relaxed);
                    }
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                // Falling out of the loop (stdin EOF / closed pipe) drops `tx`, which makes
                // the main loop's `try_recv()` return `Disconnected` → clean shutdown.
            });
        }
        println!("mind-runtime: stdin REPL ready — type `help` for commands.");

        loop {
            let shared = reader.read();
            let base_dials = Dials::from_shared(&shared);
            let last_dials = overrides.apply(base_dials);
            let model_gen = shared.mind[1] as u32;
            let prompt_gen = shared.mind[3] as u32;

            // Drain any pending stdin commands (non-blocking — never stalls the poll loop).
            loop {
                match rx.try_recv() {
                    Ok(line) => match dispatch_command(
                        &line, &backend, &reader, model_gen, &mut model, &mut header,
                        &mut overrides, base_dials, &ctl, &mut writer,
                    ) {
                        Cmd::Continue => {}
                        Cmd::Quit => {
                            println!("mind-runtime: exiting.");
                            return;
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // Parent closed our stdin → shut down cleanly.
                        println!("mind-runtime: stdin closed — exiting.");
                        return;
                    }
                }
            }

            // On first read, adopt the current counters WITHOUT firing (so we don't
            // replay a stale prompt / reload on startup — the startup load above already
            // handled an already-selected model).
            if !seeded {
                last_model_gen = model_gen;
                last_prompt_gen = prompt_gen;
                seeded = true;
            }

            // (Re)load the model when the editor picks a new .gguf.
            if model_gen != last_model_gen {
                last_model_gen = model_gen;
                warned_no_model = false; // a fresh load may service a pending prompt
                if model_gen > 0 {
                    load_model(&backend, &mut model, &mut header);
                } else {
                    model = None;
                    header = None;
                }
            }

            // Run a completion when the editor presses "Generate" (one-shot Mind card).
            // Consume the edge only when we can actually service it — otherwise a Generate
            // pressed before the model finished loading would be dropped, forcing a re-press.
            if prompt_gen != last_prompt_gen {
                match (&model, &header) {
                    (Some(m), Some(h)) => {
                        let prompt = read_prompt();
                        if prompt.trim().is_empty() {
                            eprintln!("mind-runtime: empty prompt, skipping");
                            last_prompt_gen = prompt_gen; // nothing to run — consume it
                        } else {
                            // Consume the edge unless a mid-run model swap aborted the run —
                            // then leave it unconsumed so the prompt re-runs on the new model.
                            match generate_core(
                                &backend, &reader, model_gen, m, h, &prompt, last_dials, &ctl,
                                &mut writer,
                            ) {
                                GenOutcome::Done(_) => {
                                    last_prompt_gen = prompt_gen;
                                }
                                GenOutcome::Failed(e) => {
                                    eprintln!("mind-runtime: generation failed: {e}");
                                    last_prompt_gen = prompt_gen; // deterministic — don't retry
                                }
                                GenOutcome::Aborted => {} // unconsumed → re-runs on reload
                            }
                        }
                    }
                    _ => {
                        // No model yet — leave the edge unconsumed so the prompt runs as
                        // soon as one loads. Warn once to avoid spamming the poll loop.
                        if !warned_no_model {
                            eprintln!(
                                "mind-runtime: prompt received but no model loaded — \
                                 it will run once a model is loaded."
                            );
                            warned_no_model = true;
                        }
                    }
                }
            }

            // Serve one pending HTTP request, if any (non-blocking). Lazy-load the model
            // from the sidecar if the editor never bumped model_gen.
            if let Some(listener) = &listener {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if model.is_none() {
                            load_model(&backend, &mut model, &mut header);
                        }
                        serve_http(
                            stream, &backend, &reader, model_gen, &model, &header, last_dials,
                            &ctl, &mut writer,
                        );
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => eprintln!("mind-runtime: accept error: {e}"),
                }
            }

            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Read the model path from the sidecar, load the weights (Metal on macOS, CPU
    /// elsewhere), and parse the GGUF header for the ring frame layout.
    fn load_model(
        backend: &LlamaBackend,
        model: &mut Option<LlamaModel>,
        header: &mut Option<gguf::GgufHeader>,
    ) {
        // Drop any previously-loaded model up front: this is a reload edge, and if we
        // can't read the new selection we must NOT keep running against the old weights
        // while the editor believes a new specimen was picked.
        *model = None;
        *header = None;
        let path = match std::fs::read_to_string(ipc::model_sidecar_path()) {
            Ok(s) => PathBuf::from(s.trim().to_string()),
            Err(e) => {
                eprintln!("mind-runtime: no model sidecar: {e}");
                return;
            }
        };
        // Preferred (cheap, weight-free): the GGUF header gives the exact declared
        // n_layers/n_heads for the frame layout. NOT fatal if it fails — we fall back to
        // dims read from the loaded model itself so Generate still works.
        let parsed = match gguf::parse_file(&path) {
            // Only trust a parsed header that actually declares usable dims; a parse that
            // succeeds but reports zero layers/heads is as useless as a parse failure, so
            // fall through to deriving dims from the loaded model instead of discarding it.
            Ok(h) if h.n_layers > 0 && h.n_heads > 0 => {
                println!(
                    "mind-runtime: loading {} ({} layers × {} heads)…",
                    path.display(),
                    h.n_layers,
                    h.n_heads
                );
                Some(h)
            }
            Ok(_) => {
                eprintln!(
                    "mind-runtime: GGUF header declared zero layers/heads; will derive dims from the loaded model."
                );
                None
            }
            Err(e) => {
                eprintln!(
                    "mind-runtime: GGUF header parse failed ({e}); will derive dims from the loaded model."
                );
                None
            }
        };
        // Offload every layer to the GPU wherever a GGML GPU backend was actually compiled
        // in — Metal on macOS, **CUDA on Windows** (organon#658 Tier 5) — and stay on the CPU
        // where none was. `1000` is llama.cpp's idiom for "all of them": any count at or above
        // the model's layer count offloads the whole stack.
        //
        // ⚠️ This condition is the mirror of `Cargo.toml`'s target-gated `llama-cpp-4` feature
        // blocks and has to stay that way. Both are keyed on `target_os`, which is what keeps
        // them honest; a third backend (Vulkan, HIP) is an edit in *both* places. Asking for
        // offload without the backend is not fatal — llama.cpp falls back to the CPU — but it
        // would make the log below claim a GPU that never ran a layer, and on this box the
        // whole point of the tier is that the 5090 does the work.
        let gpu_backend = cfg!(any(target_os = "macos", target_os = "windows"));
        let n_gpu_layers = if gpu_backend { 1000 } else { 0 };
        println!(
            "mind-runtime: GPU offload {}.",
            if gpu_backend { "on (all layers)" } else { "off — CPU-only build" }
        );
        let model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        match LlamaModel::load_from_file(backend, &path, &model_params) {
            Ok(m) => {
                // Dims: prefer the parsed header; else derive from the loaded model so
                // `generate` (which needs n_layers/n_heads) has a usable layout.
                let hdr = parsed.unwrap_or_else(|| {
                    let arch = m.meta_val_str("general.architecture", META_BUF).unwrap_or_default();
                    let mut h = gguf::GgufHeader { arch, ..Default::default() };
                    h.n_layers = m.n_layer().max(0) as u32;
                    h.n_heads = m.n_head().max(0) as u32;
                    h.n_heads_kv = m.n_head_kv().max(0) as u32;
                    h.n_embd = m.n_embd() as u32;
                    println!(
                        "mind-runtime: derived dims from model ({} layers × {} heads).",
                        h.n_layers, h.n_heads
                    );
                    h
                });
                // Only report success when BOTH weights AND usable dims are present.
                if hdr.n_layers == 0 || hdr.n_heads == 0 {
                    eprintln!(
                        "mind-runtime: model loaded but dims are unusable ({} layers × {} heads); treating as failed.",
                        hdr.n_layers, hdr.n_heads
                    );
                    *model = None;
                    *header = None;
                    return;
                }
                println!("mind-runtime: model loaded.");
                mind_log::append(
                    mind_log::MindEvent::Model,
                    "runtime",
                    &format!("embedded runtime loaded {}", path.display()),
                );
                *model = Some(m);
                *header = Some(hdr);
            }
            Err(e) => {
                eprintln!("mind-runtime: model weight load failed: {e}");
                *model = None;
                *header = None;
            }
        }
    }

    fn read_prompt() -> String {
        std::fs::read_to_string(ipc::mind_prompt_path()).unwrap_or_default()
    }

    /// Outcome of one generation run: a normal completion (with the decoded reply, possibly
    /// empty), a deterministic setup failure (→ HTTP 500 / logged, not retried), or an abort
    /// because a new model was selected mid-run (→ the prompt edge is left unconsumed so it
    /// re-runs on the freshly-loaded specimen).
    enum GenOutcome {
        Done(String),
        Aborted,
        Failed(String),
    }

    /// Run one templated prompt → completion, streaming a `MindFrame` per generated token
    /// into the ring and the decoded text into the reply sidecar. Shared by the one-shot
    /// Mind-card path and the HTTP/agent server. `prompt` is expected to already carry the
    /// model's chat template (see `format_chat`). `model_gen` is the `Shared.mind[1]` value
    /// this run started under; `reader` is polled each token so a new model selected mid-run
    /// aborts the (now-stale) generation.
    fn generate_core(
        backend: &LlamaBackend,
        reader: &ipc::Reader,
        model_gen: u32,
        model: &LlamaModel,
        header: &gguf::GgufHeader,
        prompt: &str,
        dials: Dials,
        ctl: &GenControl,
        writer: &mut MindRingWriter,
    ) -> GenOutcome {
        // Fresh reply sidecar for this run (the editor polls + shows it live).
        let _ = std::fs::write(ipc::mind_reply_path(), b"");

        // Raise `busy` for the duration (so `status` can report it) and clear any stale
        // `abort` left by an earlier `stop`/`quit` line, so this fresh run starts clean.
        // The reader thread flips `abort` the instant a `stop`/`quit` is typed mid-run.
        ctl.abort.store(false, Ordering::Relaxed);
        ctl.busy.store(true, Ordering::Relaxed);
        let _busy = BusyGuard(&ctl.busy);

        // A fresh context per completion (clean KV cache). Context length from the dial,
        // but clamped to the model's trained maximum — requesting a larger window than the
        // model supports (e.g. the server path's 8192 floor on a 2048-ctx model) fails
        // context creation / decoding.
        let ctx_len = dials.context_len.clamp(64, model.n_ctx_train().max(64));
        let ctx_size = NonZeroU32::new(ctx_len);
        // Full-attention dial (mind[7]): ON = flash-attention DISABLED, so the full
        // attention matrix materializes. Default (full_attn = false) = flash-attention
        // enabled. #522 Tier 2 is what actually consumes this (per-head `head_summ`
        // needs the attention matrix to exist); Tier 1 only carries the dial across the
        // crate bump. `llama-cpp-4` exposes this as a safe enum, so the raw
        // `llama-cpp-sys-2` dependency that used to supply the constants is gone.
        let flash_policy = if dials.full_attn {
            LlamaFlashAttnType::Disabled
        } else {
            LlamaFlashAttnType::Enabled
        };
        // #522 Tier 1 — THE TAP.
        //
        // ⚠️ `capture` MUST be declared before `ctx` and must not move. The crate's docs
        // say the `&mut` in `with_tensor_capture` enforces the outlives-relationship at
        // compile time; reading the source, it does not — the signature is
        // `fn with_tensor_capture(self, &mut TensorCapture) -> Self` with no lifetime on
        // the return, and the reference is immediately cast to a raw `cb_eval_user_data`
        // pointer. So the borrow ends at that call and nothing stops you from declaring
        // the capture *after* the context, which would compile and be undefined
        // behaviour. Declaration order here is load-bearing: locals drop in reverse, so
        // `ctx` dies first and the pointer is never dangling. (It is also why the
        // `capture.clear()` / `read_capture(&capture, …)` calls below are accepted by the
        // borrow checker at all — and they are safe because llama.cpp only writes through
        // that pointer *inside* `decode`, which has returned by the time we read.)
        //
        // `for_names`, not `for_layers`: the filter is a single exclusive enum, and its
        // `Layers` variant only matches names that strip-prefix `l_out-`, so it can
        // never see an FFN node. We need both the layer residual AND the FFN output, so
        // the name list is generated explicitly.
        let tap_layers = tap_layer_set(
            header.n_layers as usize,
            (header.n_layers as usize).clamp(1, MR_MAX_LAYERS),
        );
        let tap_names = tap_tensor_names(&tap_layers);
        let name_refs: Vec<&str> = tap_names.iter().map(String::as_str).collect();
        let mut capture = TensorCapture::for_names(&name_refs);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(ctx_size)
            .with_flash_attn_type(flash_policy)
            .with_tensor_capture(&mut capture);
        let mut ctx = match model.new_context(backend, ctx_params) {
            Ok(c) => c,
            Err(e) => {
                return GenOutcome::Failed(format!("context creation failed: {e}"));
            }
        };

        let tokens = match model.str_to_token(prompt, AddBos::Always) {
            Ok(t) => t,
            Err(e) => {
                return GenOutcome::Failed(format!("tokenize failed: {e}"));
            }
        };

        // Frame layout from the GGUF header, clamped to the ring caps. If the model
        // exceeds the caps we simply report the capped count (the proxy is depth-shaped,
        // not per-source-layer; a true cb_eval tap would down-sample source layers here).
        let n_layers = (header.n_layers as usize).clamp(1, MR_MAX_LAYERS) as u32;
        let n_heads = (header.n_heads as usize).min(MR_MAX_HEADS) as u32;

        // Prime the KV cache with the prompt; only the last token needs logits.
        let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
        let last = tokens.len() as i32 - 1;
        for (i, tok) in tokens.iter().enumerate() {
            let want_logits = i as i32 == last;
            if let Err(e) = batch.add(*tok, i as i32, &[0], want_logits) {
                return GenOutcome::Failed(format!("prompt batch add failed: {e}"));
            }
        }
        if let Err(e) = ctx.decode(&mut batch) {
            return GenOutcome::Failed(format!("prompt decode failed: {e}"));
        }

        let min_period = if dials.rate_cap > 0.0 {
            Some(Duration::from_secs_f32(1.0 / dials.rate_cap))
        } else {
            None
        };

        let mut n_cur = batch.n_tokens();
        let mut token_index: u32 = 0;
        let mut reply = String::new();
        // #522 Tier 1 — this token's tap readings, kept alive across the `fill_frame`
        // call, plus a one-shot "which path did we take" report.
        let mut tap_reading: Option<TapReading> = None;
        let mut tap_reported = false;

        // Recent-token window for the repetition penalty. Seeded with the prompt tail so
        // the model doesn't just parrot its own prompt back.
        let mut recent: std::collections::VecDeque<LlamaToken> =
            tokens.iter().rev().take(REPEAT_LAST_N).rev().copied().collect();

        for _ in 0..MAX_GEN_TOKENS {
            // Abort if the editor selected a new model mid-generation — don't keep
            // inferring on the now-stale weights while shared memory already reflects
            // the new pick. The caller leaves the prompt edge unconsumed so it re-runs.
            if reader.read().mind[1] as u32 != model_gen {
                eprintln!("mind-runtime: model changed mid-generation — aborting to reload.");
                return GenOutcome::Aborted;
            }

            // A `stop`/`quit` typed at the REPL flips `ctl.abort`. Break (not `Aborted`) so
            // the partial reply is returned as `Done` and the prompt edge is CONSUMED — a
            // user-requested stop must not replay, unlike a mid-run model swap above.
            if ctl.abort.load(Ordering::Relaxed) {
                println!("mind-runtime: generation stopped at token {token_index}.");
                break;
            }

            let step_start = Instant::now();
            // Logits for the just-decoded last position → measured entropy / confidence /
            // top-k + the chosen token.
            let logits_index = batch.n_tokens() - 1;
            let sampled = sample_from_logits(
                &ctx,
                logits_index,
                dials.temperature,
                recent.iter().copied(),
                REPEAT_PENALTY,
            );
            let next = sampled.chosen;

            // Track the emitted token for the penalty window.
            recent.push_back(next);
            while recent.len() > REPEAT_LAST_N {
                recent.pop_front();
            }

            // Decode the top-k candidates' text once so the dashboard can label the bars
            // ("the / a / this…"). `token_to_str` is deprecated in favour of the
            // decoder-based `token_to_piece`, but is enough for a short snippet here.
            let topk: Vec<(u32, f32, String)> = sampled
                .topk
                .iter()
                .map(|&(tok, prob)| {
                    #[allow(deprecated)]
                    let text = model.token_to_str(tok, Special::Tokenize).unwrap_or_default();
                    (tok.0 as u32, prob, text)
                })
                .collect();

            let is_eog = model.is_eog_token(next);
            // Publish the activation frame BEFORE breaking on EOS so the final token's
            // "effort" still lights the network, with the end-of-generation flag set.
            let f = fill_frame(
                token_index,
                n_layers,
                n_heads,
                sampled.entropy,
                sampled.confidence,
                &topk,
                // #482 Tier 3 — the KV fuel gauge: `n_cur` is the current cache occupancy
                // (prompt + generated so far), `ctx_len` the active window.
                n_cur.max(0) as u32,
                ctx_len,
                if is_eog { FLAG_EOG } else { 0 },
                // #522 Tier 1 — this token's real per-layer readings, if the tap fired.
                // `any()` gates it: an all-miss capture must fall back to the proxy
                // rather than render the stack as silent.
                {
                    let r = read_capture(&capture, &tap_layers);
                    if r.any() {
                        tap_reading = Some(r);
                    } else {
                        tap_reading = None;
                    }
                    tap_reading.as_ref()
                },
            );
            writer.write_frame(&f);
            // One report, the first time, so a Mac run says plainly which path it took —
            // the difference is otherwise only visible as a provenance glyph.
            if !tap_reported {
                tap_reported = true;
                eprintln!(
                    "mind-runtime: activation tap {} ({} layers requested)",
                    if f.flags & (FLAG_RESID_MEASURED | FLAG_MLP_MEASURED) != 0 {
                        "MEASURED — real per-layer tensors (#522 T1)"
                    } else {
                        "PROXY — capture returned nothing; entropy/confidence shaping"
                    },
                    tap_layers.len()
                );
            }

            if is_eog {
                break;
            }

            // Decode the token text → reply sidecar (append per token). `token_to_str`
            // is deprecated in favour of `token_to_piece` (which needs a caller-owned
            // `encoding_rs` decoder); the String-returning helper is enough here.
            #[allow(deprecated)]
            let piece = model.token_to_str(next, Special::Tokenize);
            if let Ok(piece) = piece {
                reply.push_str(&piece);
                let _ = std::fs::write(ipc::mind_reply_path(), reply.as_bytes());
            }

            // Feed the new token back in for the next step. A failure here is almost
            // always the KV cache filling past the context dial — log it so a truncated
            // reply isn't mistaken for the model stopping on its own.
            batch.clear();
            if let Err(e) = batch.add(next, n_cur, &[0], true) {
                eprintln!(
                    "mind-runtime: stopping at token {token_index} — batch add failed \
                     (context {} likely full; raise the context dial): {e}",
                    dials.context_len
                );
                break;
            }
            // #522 Tier 1 — drop the previous token's tensors before the next decode.
            // `TensorCapture` accumulates into a name-keyed map, so without this the
            // frame would keep reporting the first token's activations forever.
            capture.clear();
            if let Err(e) = ctx.decode(&mut batch) {
                eprintln!(
                    "mind-runtime: stopping at token {token_index} — decode failed \
                     (context {} likely exceeded; raise the context dial): {e}",
                    dials.context_len
                );
                break;
            }
            n_cur += 1;
            token_index = token_index.wrapping_add(1);

            // Throttle to the token-rate cap.
            if let Some(min_period) = min_period {
                let elapsed = step_start.elapsed();
                if elapsed < min_period {
                    std::thread::sleep(min_period - elapsed);
                }
            }
        }

        GenOutcome::Done(reply)
    }

    /// Log + print a completed reply (shared by both generation paths).
    fn log_reply(reply: &str, full_attn: bool) {
        mind_log::append(mind_log::MindEvent::Reply, "runtime", reply);
        println!(
            "mind-runtime: reply ({} chars, full-attn={}): {}",
            reply.len(),
            full_attn,
            reply.trim()
        );
    }

    /// The measured per-token signals from one position's logits (#482 Tier 2). All
    /// derived from the SAME temperature-softmax the sampler draws from, so the dashboard
    /// shows the model's actual next-token distribution — honest, not a proxy.
    struct Sampled {
        chosen: LlamaToken,
        /// Shannon entropy of the softmax, normalized by `ln(vocab)` → `[0,1]`.
        entropy: f32,
        /// Top token probability → `[0,1]`.
        confidence: f32,
        /// The `MR_TOPK` most-probable candidates `(token, prob)`, descending.
        topk: Vec<(LlamaToken, f32)>,
    }

    /// Sample the next token from position `i`'s logits with a temperature softmax, and
    /// return the chosen token plus the measured `entropy` / `confidence` / `top-k`.
    fn sample_from_logits(
        ctx: &llama_cpp_4::context::LlamaContext,
        i: i32,
        temperature: f32,
        recent: impl IntoIterator<Item = LlamaToken>,
        repeat_penalty: f32,
    ) -> Sampled {
        // Collect (id, logit) for the position. `candidates_ith` yields the full vocab;
        // `id()` is already a `LlamaToken`.
        let mut ids: Vec<llama_cpp_4::token::LlamaToken> = Vec::new();
        let mut logits: Vec<f32> = Vec::new();
        for c in ctx.candidates_ith(i) {
            ids.push(c.id());
            logits.push(c.logit());
        }
        if ids.is_empty() {
            return Sampled {
                chosen: llama_cpp_4::token::LlamaToken(0),
                entropy: 0.0,
                confidence: 0.0,
                topk: Vec::new(),
            };
        }

        // Repetition penalty: divide the logits of recently-emitted tokens (push them
        // down when positive, further down when negative) so the sampler can't lock into
        // a `0.0.0.0…` loop. The classic llama.cpp penalty law.
        if repeat_penalty > 1.0 {
            let recent_set: std::collections::HashSet<i32> =
                recent.into_iter().map(|t| t.0).collect();
            if !recent_set.is_empty() {
                for (k, id) in ids.iter().enumerate() {
                    if recent_set.contains(&id.0) {
                        logits[k] = if logits[k] > 0.0 {
                            logits[k] / repeat_penalty
                        } else {
                            logits[k] * repeat_penalty
                        };
                    }
                }
            }
        }

        let temp = temperature.max(1e-3);
        // Numerically-stable softmax over logits / temp.
        let max_logit = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut probs: Vec<f32> = logits
            .iter()
            .map(|&l| ((l - max_logit) / temp).exp())
            .collect();
        let sum: f32 = probs.iter().sum();
        let inv = if sum > 0.0 { 1.0 / sum } else { 0.0 };
        for p in &mut probs {
            *p *= inv;
        }

        // Shannon entropy (nats), normalized by ln(vocab) → [0,1].
        let mut entropy = 0.0f32;
        let mut top_p = 0.0f32;
        let mut top_idx = 0usize;
        for (k, &p) in probs.iter().enumerate() {
            if p > 0.0 {
                entropy -= p * p.ln();
            }
            if p > top_p {
                top_p = p;
                top_idx = k;
            }
        }
        let norm = (probs.len() as f32).max(2.0).ln();
        let entropy_n = (entropy / norm).clamp(0.0, 1.0);

        // At (or effectively at) temperature 0 the user is asking for deterministic
        // **greedy** decoding, so return the argmax directly instead of drawing from the
        // near-degenerate distribution. Above that, sample stochastically (cheap LCG, no
        // dep). `top_idx` is the argmax; the entropy/confidence signals above still use the
        // softmax so the activation tap stays meaningful either way.
        let choice = if temperature <= 1e-4 {
            top_idx
        } else {
            sample_index(&probs, top_idx)
        };

        // The MR_TOPK most-probable candidates (the "watch it weigh the/a/this…" bars).
        let topk = top_k_candidates(&probs, &ids, MR_TOPK);

        Sampled {
            chosen: ids[choice],
            entropy: entropy_n,
            confidence: top_p.clamp(0.0, 1.0),
            topk,
        }
    }

    /// The `k` highest-probability `(token, prob)` pairs, descending — a bounded
    /// selection over the full vocab (O(V·k), no full sort of ~100k candidates).
    fn top_k_candidates(
        probs: &[f32],
        ids: &[LlamaToken],
        k: usize,
    ) -> Vec<(LlamaToken, f32)> {
        let mut best: Vec<(LlamaToken, f32)> = Vec::with_capacity(k + 1);
        for (idx, &p) in probs.iter().enumerate() {
            if best.len() < k {
                best.push((ids[idx], p));
                best.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            } else if p > best[k - 1].1 {
                best[k - 1] = (ids[idx], p);
                let mut j = k - 1;
                while j > 0 && best[j].1 > best[j - 1].1 {
                    best.swap(j, j - 1);
                    j -= 1;
                }
            }
        }
        best
    }

    /// Weighted sample over a normalized `probs` vector; falls back to `fallback` (the
    /// argmax) if the draw underflows. Uses a time-seeded LCG (no rng dependency).
    fn sample_index(probs: &[f32], fallback: usize) -> usize {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            .wrapping_mul(2654435761)
            .wrapping_add(1);
        let r = (seed % 10_000_000) as f32 / 10_000_000.0;
        let mut acc = 0.0f32;
        for (k, &p) in probs.iter().enumerate() {
            acc += p;
            if r <= acc {
                return k;
            }
        }
        fallback
    }

    // =======================================================================
    // #522 Tier 1 — the real residual tap
    //
    // Everything in this section except `read_capture` is PURE: no llama types,
    // no GPU, no model. That is deliberate — the tap's behaviour on Metal cannot
    // be verified in a cloud session, so the parts that *can* be tested here are
    // separated from the one thin function that cannot.
    // =======================================================================

    /// Which of the model's layers to tap, mapped onto the frame's `slots`.
    ///
    /// The frame carries at most `MR_MAX_LAYERS` (64) layers. A model with **fewer**
    /// layers than that maps 1:1 — every slot is that layer's real value. A model with
    /// **more** is sampled at even intervals spanning the whole stack, *not* truncated
    /// to its first 64: showing the bottom 64 layers of an 80-layer model as if they
    /// were the model would misdescribe its shape. Sampling loses layers; truncation
    /// lies about depth.
    ///
    /// Every returned index is a layer we actually read, so no frame slot ever holds an
    /// interpolated or repeated value.
    fn tap_layer_set(model_layers: usize, slots: usize) -> Vec<usize> {
        let model_layers = model_layers.max(1);
        let slots = slots.clamp(1, MR_MAX_LAYERS);
        if model_layers <= slots {
            return (0..model_layers).collect();
        }
        // Even spread across [0, model_layers-1], endpoints included.
        (0..slots)
            .map(|i| {
                let t = i as f64 / (slots - 1).max(1) as f64;
                (t * (model_layers - 1) as f64).round() as usize
            })
            .collect()
    }

    /// The graph node names to capture for `layers`.
    ///
    /// `TensorCapture`'s filter is a **single exclusive enum**, and its `Layers` variant
    /// matches only names that strip-prefix `l_out-` — so it can never see an FFN node.
    /// Tier 1 needs the layer residual (`l_out-{N}` → `layer_norm`) *and* the FFN output
    /// (`ffn_out-{N}` → `mlp_act`), so the name list is generated explicitly and handed
    /// to `for_names`.
    fn tap_tensor_names(layers: &[usize]) -> Vec<String> {
        let mut v = Vec::with_capacity(layers.len() * 2);
        for &l in layers {
            v.push(format!("l_out-{l}"));
            v.push(format!("ffn_out-{l}"));
        }
        v
    }

    /// L2 norm of a captured row. `f64` accumulation so a wide (4096-dim) vector of
    /// small values doesn't lose the tail to `f32` rounding.
    fn l2_norm(v: &[f32]) -> f32 {
        let s: f64 = v.iter().map(|&x| (x as f64) * (x as f64)).sum();
        s.sqrt() as f32
    }

    /// One token's real per-layer readings, in frame-slot order. `None` entries are
    /// layers the capture didn't return (a graph whose node names don't match).
    #[derive(Default, Clone)]
    struct TapReading {
        resid: Vec<Option<f32>>,
        ffn: Vec<Option<f32>>,
    }

    impl TapReading {
        /// True if anything at all was captured. When false the caller falls back to the
        /// proxy — an all-miss capture must not read as "the model is silent."
        fn any(&self) -> bool {
            self.resid.iter().any(Option::is_some) || self.ffn.iter().any(Option::is_some)
        }
    }

    /// Normalize raw L2 norms into the `[0, 1.5]` band the visual's glow expects.
    ///
    /// **Scaled by the max across this token's captured layers, not by an absolute
    /// constant.** Residual norms grow monotonically with depth by construction (each
    /// block *adds* to the residual stream) and their absolute scale varies by orders of
    /// magnitude between models, so any fixed divisor would either clip or flatten. The
    /// visible quantity is therefore *relative* depth profile, which is what the glow
    /// has always meant. Missing layers stay at 0 rather than borrowing a neighbour's
    /// value — an unread layer is dark, not guessed.
    fn normalize_tap(vals: &[Option<f32>], out: &mut [f32; MR_MAX_LAYERS]) -> bool {
        let max = vals
            .iter()
            .filter_map(|v| *v)
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(0.0f32, f32::max);
        if max <= 0.0 {
            return false;
        }
        for (i, v) in vals.iter().enumerate().take(MR_MAX_LAYERS) {
            out[i] = match v {
                Some(x) if x.is_finite() => (x / max).clamp(0.0, 1.0) * 1.5,
                _ => 0.0,
            };
        }
        true
    }

    /// Pull this token's readings out of a `TensorCapture`.
    ///
    /// The one function here that touches llama types, kept as thin as possible for that
    /// reason. `token_embedding(i)` slices one token's row out of the flat buffer; we
    /// take the **last** row, because a prompt-priming decode carries every prompt token
    /// and only the final position is the one that produced this frame.
    fn read_capture(capture: &TensorCapture, layers: &[usize]) -> TapReading {
        let last_row = |t: &CapturedTensor| -> Option<f32> {
            let n = t.n_tokens();
            if n == 0 {
                return None;
            }
            t.token_embedding(n - 1).map(l2_norm)
        };
        TapReading {
            resid: layers
                .iter()
                .map(|&l| capture.get(&format!("l_out-{l}")).and_then(last_row))
                .collect(),
            ffn: layers
                .iter()
                .map(|&l| capture.get(&format!("ffn_out-{l}")).and_then(last_row))
                .collect(),
        }
    }

    /// Build a `MindFrame`, from the **real tap** when it produced anything and from the
    /// per-token proxy otherwise.
    ///
    /// `tap` is `Some` only when `TensorCapture` returned at least one layer. When it is:
    /// `layer_norm` and `mlp_act` are real L2 norms of `l_out-{N}` / `ffn_out-{N}`,
    /// normalized across the token's captured layers (#522 Tier 1), and the #482
    /// dashboard's provenance glyphs for those two widgets read `=`.
    ///
    /// When it is `None` — an older model whose graph node names don't match, a backend
    /// that never fires the callback, or a build without the feature — the ORIGINAL
    /// proxy fills the frame: `entropy`/`confidence` are per-token *global* signals
    /// shaped into a moving depth profile, so the network reads as "thinking" (busier
    /// when uncertain). That is a faithful proxy for **effort**, never a readout of
    /// internals, and the glyphs stay `?`. The fallback is kept live and tested rather
    /// than deleted, because whether the callback fires on Metal is not knowable here.
    ///
    /// `head_summ` stays a proxy either way: per-head attention needs the attention
    /// matrix to materialize (flash-attention off), which is #522 Tier 2's trade.
    fn fill_frame(
        token_index: u32,
        n_layers: u32,
        n_heads: u32,
        entropy: f32,
        confidence: f32,
        topk: &[(u32, f32, String)],
        ctx_used: u32,
        ctx_total: u32,
        flags: u32,
        tap: Option<&TapReading>,
    ) -> MindFrame {
        // Clamp to the ring caps here too (defense in depth) so we can never index past
        // the fixed frame arrays even if a caller passes a raw model dim.
        let n_layers = n_layers.min(MR_MAX_LAYERS as u32);
        let n_heads = n_heads.min(MR_MAX_HEADS as u32);
        let mut f = MindFrame::default();
        f.token_index = token_index;
        f.n_layers = n_layers;
        f.n_heads = n_heads;
        f.flags = flags;
        // #482 Tier 3 — the context / KV fuel gauge (measured counts).
        f.ctx_used = ctx_used;
        f.ctx_total = ctx_total;

        // #482 Tier 2 — the MEASURED next-token stats (the real softmax, not the proxy
        // activation shaping below). These are the honest headliners: the top-k bars and
        // the entropy/confidence heartbeat read straight from here.
        f.entropy = entropy.clamp(0.0, 1.0);
        f.confidence = confidence.clamp(0.0, 1.0);
        let n_topk = topk.len().min(MR_TOPK);
        f.topk_count = n_topk as u32;
        for (rank, (id, prob, text)) in topk.iter().take(MR_TOPK).enumerate() {
            f.topk_id[rank] = *id;
            f.topk_prob[rank] = prob.clamp(0.0, 1.0);
            let mut slot = [0u8; MR_TOK_LEN];
            write_snippet(&mut slot, text);
            f.topk_text[rank] = slot;
        }

        let tok = token_index as f32;

        // #522 Tier 1 — MEASURED: real L2 norms of this token's `l_out-{N}` and
        // `ffn_out-{N}`. A partial capture is still real for the layers it covers, so
        // each channel is taken independently: if the residual read and the FFN didn't,
        // `layer_norm` is measured and `mlp_act` falls through to the proxy below.
        let mut resid_measured = false;
        let mut ffn_measured = false;
        if let Some(t) = tap {
            resid_measured = normalize_tap(&t.resid, &mut f.layer_norm);
            ffn_measured = normalize_tap(&t.ffn, &mut f.mlp_act);
        }

        // PROXY (the fallback) — effort shaped across depth, for whichever channels the
        // tap didn't fill. A depth-travelling front so the pattern moves token-to-token;
        // entropy sets the overall energy (uncertain → the whole stack lights),
        // confidence adds a settled "answer" pulse to the MLP path.
        //
        // `head_summ` is proxy-filled unconditionally: per-head attention needs the
        // attention matrix to materialize, which is #522 Tier 2's flash-attention trade.
        for l in 0..n_layers as usize {
            let lp = l as f32 / (n_layers.max(1) as f32);
            if !resid_measured {
                let wave = 0.5 + 0.5 * (tok * 0.35 + lp * 6.0).sin();
                // Residual/hidden proxy: energy scaled by effort, modulated by the front.
                f.layer_norm[l] = ((0.30 + 0.70 * entropy) * (0.45 + 0.55 * wave)).clamp(0.0, 1.5);
            }
            if !ffn_measured {
                // MLP proxy: complementary phase, scaled by the model's confidence.
                let wave2 = 0.5 + 0.5 * (tok * 0.5 - lp * 4.5 + 1.7).sin();
                f.mlp_act[l] = ((0.25 + 0.75 * confidence) * (0.40 + 0.60 * wave2)).clamp(0.0, 1.5);
            }
            for h in 0..n_heads as usize {
                let hp = h as f32 / (n_heads.max(1) as f32);
                // Per-head shimmer around the layer front; energy from entropy.
                let hw = 0.5 + 0.5 * (tok * 0.25 + lp * 5.0 + hp * 7.0).sin();
                f.head_summ[l * MR_MAX_HEADS + h] =
                    ((0.20 + 0.80 * entropy) * hw).clamp(0.0, 1.0);
            }
        }
        // Tell the reader which it got, rather than making it infer. Bits 1 and 2 of
        // `flags` are the provenance the #482 dashboard reads to pick `=` over `?`.
        if resid_measured {
            f.flags |= FLAG_RESID_MEASURED;
        }
        if ffn_measured {
            f.flags |= FLAG_MLP_MEASURED;
        }
        f
    }

    // =======================================================================
    // Chat template
    // =======================================================================

    /// Format a `(role, content)` conversation into a prompt string, appending the
    /// assistant generation prefix. Tries, in order:
    ///   1. the model's **own** embedded chat template, and
    ///   2. llama.cpp's **built-in** template for the model family (by `general.architecture`)
    ///      — this rescues fine-tunes whose embedded Jinja is macro-heavy / tool-calling and
    ///      the built-in engine can't parse (e.g. this Gemma-QAT build), and
    ///   3. a last-resort generic template.
    /// This is what makes instruct models answer coherently instead of drifting into loops.
    fn format_chat(model: &LlamaModel, messages: &[serde_json::Value]) -> String {
        // 0. Render the model's OWN embedded Jinja template with a real Jinja engine
        //    (minijinja), passing the FULL message objects so assistant `tool_calls` and
        //    `tool`-role `tool_call_id`s survive into tool-calling templates — the way LM
        //    Studio / Ollama do it, and the only thing that handles exotic templates.
        if let Some(s) = render_jinja_template(model, messages) {
            return s;
        }
        // The built-in / generic fallbacks only understand plain role+content.
        let pairs = msg_pairs(messages);
        // 1. The model's own template via llama.cpp's built-in engine (handles the simple
        //    templates its C++ matcher recognizes, if minijinja somehow failed).
        // #522 Tier 1 — the one genuinely breaking part of the crate bump.
        // `llama-cpp-2` had a `LlamaChatTemplate` newtype resolved from the model;
        // `llama-cpp-4` drops it: `get_chat_template(buf_size) -> String` reads the
        // model's own template text, and `apply_chat_template` now takes
        // `Option<&str>` — `None` meaning "use the model's embedded template."
        // Passing `None` here is exactly step 1's intent and avoids a round trip
        // through the string.
        {
            let chat = to_llama_msgs(&pairs);
            if let Ok(s) = model.apply_chat_template(None, &chat, true) {
                if !s.trim().is_empty() {
                    return s;
                }
            }
        }
        // 2. A built-in family template, selected by architecture. Merge any system
        //    message into the first user turn first, since some families (Gemma) have no
        //    system role and their template errors on one.
        let arch = model
            .meta_val_str("general.architecture", META_BUF)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let family = if arch.contains("gemma") {
            "gemma"
        } else if arch.contains("phi") {
            "phi3"
        } else if arch.contains("llama") {
            "llama3"
        } else {
            "chatml"
        };
        let merged = merge_system_into_user(&pairs);
        // `LlamaChatTemplate::new(family)` is gone with the bump; a built-in family
        // template is now named by plain `&str`, which is what it always was underneath.
        {
            let chat = to_llama_msgs(&merged);
            if let Ok(s) = model.apply_chat_template(Some(family), &chat, true) {
                if !s.trim().is_empty() {
                    return s;
                }
            }
        }
        // 3. Last-resort generic ChatML-ish template.
        let mut out = String::new();
        for (r, c) in &merged {
            out.push_str(&format!("<|{r}|>\n{c}\n"));
        }
        out.push_str("<|assistant|>\n");
        out
    }

    /// Render the model's own embedded `tokenizer.chat_template` (Jinja2) with minijinja,
    /// exactly like LM Studio / Ollama. Returns `None` if the model has no template or the
    /// render fails (caller then falls back to the built-in / generic paths). `bos_token`
    /// is passed empty because `str_to_token(.., AddBos::Always)` adds the real BOS once.
    fn render_jinja_template(model: &LlamaModel, messages: &[serde_json::Value]) -> Option<String> {
        // Chat templates are large (multi-KB Jinja); size the buffer for one, not for
        // a short metadata string.
        let src = model.meta_val_str("tokenizer.chat_template", TEMPLATE_BUF).ok()?;
        if src.trim().is_empty() {
            return None;
        }
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        // Python string methods (`.startswith`, `.split`, `.strip`, …) HF templates use.
        env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
        // Common helpers HF templates call; stub so a render doesn't hard-fail.
        env.add_function(
            "raise_exception",
            |msg: String| -> Result<minijinja::Value, minijinja::Error> {
                Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, msg))
            },
        );
        env.add_function("strftime_now", |_fmt: String| -> Result<String, minijinja::Error> {
            Ok(String::new())
        });
        env.add_template("chat", &src).ok()?;
        let tmpl = env.get_template("chat").ok()?;
        // Pass the FULL message objects (role, content, tool_calls, tool_call_id, name, …)
        // so tool-calling templates render multi-turn tool history correctly.
        let ctx = minijinja::context! {
            messages => messages,
            add_generation_prompt => true,
            bos_token => "",
            eos_token => "",
        };
        match tmpl.render(ctx) {
            Ok(s) if !s.trim().is_empty() => Some(s),
            Ok(_) => None,
            Err(e) => {
                eprintln!("mind-runtime: minijinja template render failed ({e}); falling back.");
                None
            }
        }
    }

    /// Extract plain `(role, content)` pairs from OpenAI message objects, for the fallback
    /// templates (which don't understand tool-call structure). Content may be a string or
    /// an array of `{type:"text", text:…}` parts.
    fn msg_pairs(messages: &[serde_json::Value]) -> Vec<(String, String)> {
        messages
            .iter()
            .map(|m| {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
                let content = match m.get("content") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(serde_json::Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join(""),
                    _ => String::new(),
                };
                (role, content)
            })
            .collect()
    }

    /// Convert `(role, content)` pairs into llama.cpp chat messages (dropping any with
    /// null bytes in role/content).
    fn to_llama_msgs(msgs: &[(String, String)]) -> Vec<LlamaChatMessage> {
        msgs.iter()
            .filter_map(|(r, c)| LlamaChatMessage::new(r.clone(), c.clone()).ok())
            .collect()
    }

    /// Fold all `system` messages into the first `user` turn (or a leading user turn if
    /// there is none), so templates without a system role still carry the instructions.
    fn merge_system_into_user(msgs: &[(String, String)]) -> Vec<(String, String)> {
        let mut sys = String::new();
        let mut out: Vec<(String, String)> = Vec::new();
        for (r, c) in msgs {
            if r == "system" {
                if !sys.is_empty() {
                    sys.push_str("\n\n");
                }
                sys.push_str(c);
            } else {
                out.push((r.clone(), c.clone()));
            }
        }
        if !sys.is_empty() {
            if let Some(first_user) = out.iter_mut().find(|(r, _)| r == "user") {
                first_user.1 = format!("{sys}\n\n{}", first_user.1);
            } else {
                out.insert(0, ("user".to_string(), sys));
            }
        }
        out
    }

    // =======================================================================
    // OpenAI-compatible HTTP server (the #317 LM Studio drop-in)
    // =======================================================================

    /// Handle one HTTP connection: route `POST /v1/chat/completions` (run inference through
    /// `generate_core`, tapping the ring) and `GET /v1/models`. One request per connection
    /// (the agent sends `Connection: close`).
    fn serve_http(
        mut stream: TcpStream,
        backend: &LlamaBackend,
        reader: &ipc::Reader,
        model_gen: u32,
        model: &Option<LlamaModel>,
        header: &Option<gguf::GgufHeader>,
        dials: Dials,
        ctl: &GenControl,
        writer: &mut MindRingWriter,
    ) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        let Some((method, path, body)) = read_http_request(&mut stream) else {
            // Reply with a real status line rather than a bare close, so the agent's client
            // sees a clear error instead of an empty / unparseable payload.
            write_http(&mut stream, 400, "{\"error\":\"could not read HTTP request\"}");
            return;
        };

        if method == "POST" && path.starts_with("/v1/chat/completions") {
            let Some((messages, temperature)) = parse_chat_request(&body) else {
                write_http(&mut stream, 400, "{\"error\":\"malformed chat request\"}");
                return;
            };
            let (m, h) = match (model, header) {
                (Some(m), Some(h)) => (m, h),
                _ => {
                    write_http(
                        &mut stream,
                        503,
                        "{\"error\":\"no model loaded — pick a .gguf in the plugin's Mind card\"}",
                    );
                    return;
                }
            };
            // Honor the request temperature; give the agent path a roomy context for the
            // #317 capability-catalog system prompt.
            let mut d = dials;
            if let Some(t) = temperature {
                d.temperature = t;
            }
            d.context_len = d.context_len.max(SERVER_MIN_CTX);

            let prompt = format_chat(m, &messages);
            mind_log::append(mind_log::MindEvent::Prompt, "agent", &prompt);
            // A hard generation failure (context/tokenize/decode) must surface as HTTP 500,
            // not a 200 carrying an empty assistant message the agent would treat as a valid
            // (empty) reply.
            match generate_core(backend, reader, model_gen, m, h, &prompt, d, ctl, writer) {
                GenOutcome::Done(text) => {
                    log_reply(&text, d.full_attn);
                    write_http(&mut stream, 200, &build_completion_json(&h.arch, &text));
                }
                GenOutcome::Failed(e) => {
                    eprintln!("mind-runtime: agent generation failed: {e}");
                    let body = serde_json::json!({
                        "error": { "message": e, "type": "generation_error" }
                    })
                    .to_string();
                    write_http(&mut stream, 500, &body);
                }
                GenOutcome::Aborted => {
                    // A new model was selected mid-request — tell the client to retry.
                    write_http(
                        &mut stream,
                        503,
                        "{\"error\":\"model changed during generation — retry\"}",
                    );
                }
            }
        } else if method == "GET" && path.starts_with("/v1/models") {
            write_http(&mut stream, 200, &models_json(header));
        } else {
            write_http(&mut stream, 404, "{\"error\":\"not found\"}");
        }
    }

    /// Read an HTTP/1.1 request: headers up to the blank line, then `Content-Length` body
    /// bytes. Returns `(method, path, body)`. Localhost only, so no TLS / chunked-in.
    fn read_http_request(stream: &mut TcpStream) -> Option<(String, String, String)> {
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 8192];
        let header_end = loop {
            let n = stream.read(&mut tmp).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                break pos;
            }
            if buf.len() > 4_000_000 {
                return None; // runaway header guard
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut lines = head.lines();
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();

        let clen = head
            .lines()
            .find_map(|l| {
                let low = l.to_ascii_lowercase();
                low.strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);

        let body_start = header_end + 4;
        let mut body: Vec<u8> = buf[body_start..].to_vec();
        while body.len() < clen {
            let n = stream.read(&mut tmp).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(clen);
        Some((method, path, String::from_utf8_lossy(&body).to_string()))
    }

    /// Parse an OpenAI chat request body → `(messages, temperature)`. Messages are kept as
    /// their **full JSON objects** (so `tool_calls` on assistant turns and `tool_call_id`
    /// on tool results survive to the chat template); the fallback templates flatten them to
    /// role+content via `msg_pairs`.
    fn parse_chat_request(body: &str) -> Option<(Vec<serde_json::Value>, Option<f32>)> {
        let v: serde_json::Value = serde_json::from_str(body).ok()?;
        let arr = v.get("messages")?.as_array()?;
        if arr.is_empty() {
            return None;
        }
        let temperature = v.get("temperature").and_then(|t| t.as_f64()).map(|t| t as f32);
        Some((arr.clone(), temperature))
    }

    /// Build an OpenAI `chat.completion` response body carrying `content`.
    fn build_completion_json(model_name: &str, content: &str) -> String {
        serde_json::json!({
            "id": "chatcmpl-organon",
            "object": "chat.completion",
            "created": 0,
            "model": model_name,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        })
        .to_string()
    }

    /// Minimal `GET /v1/models` list (the loaded model, if any).
    fn models_json(header: &Option<gguf::GgufHeader>) -> String {
        let id = header
            .as_ref()
            .map(|h| h.arch.clone())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "organon-embedded".to_string());
        serde_json::json!({
            "object": "list",
            "data": [{ "id": id, "object": "model", "owned_by": "organon" }]
        })
        .to_string()
    }

    /// Write a minimal HTTP/1.1 JSON response and close.
    fn write_http(stream: &mut TcpStream, status: u16, body: &str) {
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "OK",
        };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    }

    /// First index of `needle` in `haystack`, if present.
    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fill_frame_is_bounded_and_shaped() {
            let topk = vec![
                (10u32, 0.6f32, "the".to_string()),
                (11, 0.3, " a".to_string()),
            ];
            let f = fill_frame(3, 24, 16, 0.7, 0.4, &topk, 137, 4096, 0, None);
            assert_eq!(f.n_layers, 24);
            assert_eq!(f.n_heads, 16);
            assert_eq!(f.token_index, 3);
            // #482 Tier 2 — the measured stats round-trip into the frame.
            assert!((f.entropy - 0.7).abs() < 1e-6);
            assert!((f.confidence - 0.4).abs() < 1e-6);
            assert_eq!(f.topk_count, 2);
            // #482 Tier 3 — the KV fuel gauge counts.
            assert_eq!(f.ctx_used, 137);
            assert_eq!(f.ctx_total, 4096);
            assert_eq!(f.topk_id[0], 10);
            assert!((f.topk_prob[0] - 0.6).abs() < 1e-6);
            assert_eq!(
                organic_math_native::mind_ring::snippet_str(&f.topk_text[0]),
                "the"
            );
            for l in 0..24usize {
                assert!(f.layer_norm[l] >= 0.0 && f.layer_norm[l] <= 1.5);
                assert!(f.mlp_act[l] >= 0.0 && f.mlp_act[l] <= 1.5);
                for h in 0..16usize {
                    let v = f.head_summ[l * MR_MAX_HEADS + h];
                    assert!(v >= 0.0 && v <= 1.0);
                }
            }
        }

        #[test]
        fn fill_frame_clamps_layer_count() {
            // A model larger than the ring caps still yields a valid, capped frame.
            let f = fill_frame(0, 999, 999, 1.0, 1.0, &[], 0, 0, FLAG_EOG, None);
            assert_eq!(f.n_layers, MR_MAX_LAYERS as u32);
            assert_eq!(f.n_heads, MR_MAX_HEADS as u32);
            assert_eq!(f.flags & FLAG_EOG, FLAG_EOG);
            assert_eq!(f.topk_count, 0);
        }

        // ── #522 Tier 1 — the tap's pure core ────────────────────────────────
        // Whether the callback fires on Metal is not testable here. The reduction
        // math is, and it is where a wrong answer would be invisible on screen.

        #[test]
        fn tap_layer_set_maps_one_to_one_when_it_fits() {
            let s = tap_layer_set(18, MR_MAX_LAYERS);
            assert_eq!(s, (0..18).collect::<Vec<_>>(), "every layer read for real");
        }

        #[test]
        fn tap_layer_set_spans_the_stack_rather_than_truncating() {
            // 80 layers into 64 slots: the point is that the LAST layer is still
            // represented. Truncation would stop at 63 and quietly redescribe an
            // 80-layer model as a 64-layer one.
            let s = tap_layer_set(80, 64);
            assert_eq!(s.len(), 64);
            assert_eq!(s[0], 0, "starts at the bottom of the stack");
            assert_eq!(s[63], 79, "reaches the top of the stack");
            assert!(s.windows(2).all(|w| w[0] <= w[1]), "monotonic");
            // Sampling, never interpolation: every entry is a real layer index.
            assert!(s.iter().all(|&l| l < 80));
        }

        #[test]
        fn tap_layer_set_is_never_empty() {
            assert_eq!(tap_layer_set(0, 64), vec![0]);
            assert_eq!(tap_layer_set(1, 64), vec![0]);
            assert_eq!(tap_layer_set(10, 0).len(), 1);
        }

        #[test]
        fn tap_names_cover_both_residual_and_ffn() {
            // The reason `for_names` is used instead of `for_layers`: the latter's
            // filter only matches `l_out-*`, so an FFN node could never arrive.
            let n = tap_tensor_names(&[0, 7]);
            assert_eq!(n, vec!["l_out-0", "ffn_out-0", "l_out-7", "ffn_out-7"]);
        }

        #[test]
        fn l2_norm_is_the_euclidean_length() {
            assert!((l2_norm(&[3.0, 4.0]) - 5.0).abs() < 1e-6);
            assert_eq!(l2_norm(&[]), 0.0);
            assert!((l2_norm(&[-1.0, -1.0, -1.0, -1.0]) - 2.0).abs() < 1e-6);
        }

        #[test]
        fn normalize_tap_scales_to_the_tokens_own_max() {
            // Relative depth profile, not an absolute scale: residual norms grow with
            // depth by construction and their magnitude varies wildly between models,
            // so a fixed divisor would clip or flatten.
            let vals = vec![Some(2.0), Some(4.0), Some(8.0)];
            let mut out = [0.0f32; MR_MAX_LAYERS];
            assert!(normalize_tap(&vals, &mut out));
            assert!((out[2] - 1.5).abs() < 1e-6, "max maps to the top of the band");
            assert!((out[1] - 0.75).abs() < 1e-6);
            assert!((out[0] - 0.375).abs() < 1e-6);
        }

        #[test]
        fn normalize_tap_leaves_unread_layers_dark_not_guessed() {
            let vals = vec![Some(1.0), None, Some(2.0)];
            let mut out = [0.0f32; MR_MAX_LAYERS];
            assert!(normalize_tap(&vals, &mut out));
            assert_eq!(out[1], 0.0, "a layer we didn't read is dark, not interpolated");
            assert!((out[2] - 1.5).abs() < 1e-6);
        }

        #[test]
        fn normalize_tap_reports_failure_on_nothing_usable() {
            let mut out = [0.0f32; MR_MAX_LAYERS];
            assert!(!normalize_tap(&[], &mut out));
            assert!(!normalize_tap(&[None, None], &mut out));
            assert!(!normalize_tap(&[Some(0.0), Some(0.0)], &mut out), "all-zero is not a signal");
            assert!(!normalize_tap(&[Some(f32::NAN)], &mut out), "NaN is not a signal");
            assert!(!normalize_tap(&[Some(f32::INFINITY)], &mut out));
        }

        #[test]
        fn tap_reading_any_gates_the_fallback() {
            assert!(!TapReading::default().any());
            let empty = TapReading { resid: vec![None; 4], ffn: vec![None; 4] };
            assert!(!empty.any(), "an all-miss capture must fall back to the proxy");
            let partial = TapReading { resid: vec![None, Some(1.0)], ffn: vec![None, None] };
            assert!(partial.any());
        }

        #[test]
        fn measured_tap_fills_the_frame_and_flags_it() {
            let tap = TapReading {
                resid: vec![Some(1.0), Some(2.0), Some(4.0)],
                ffn: vec![Some(4.0), Some(2.0), Some(1.0)],
            };
            let f = fill_frame(5, 3, 2, 0.9, 0.1, &[], 10, 100, 0, Some(&tap));
            assert_eq!(f.flags & FLAG_RESID_MEASURED, FLAG_RESID_MEASURED);
            assert_eq!(f.flags & FLAG_MLP_MEASURED, FLAG_MLP_MEASURED);
            // Real, monotonically-growing residual → a rising profile, not the
            // entropy-shaped sine the proxy would produce.
            assert!(f.layer_norm[0] < f.layer_norm[1]);
            assert!(f.layer_norm[1] < f.layer_norm[2]);
            // And the FFN channel is independent, here deliberately falling.
            assert!(f.mlp_act[0] > f.mlp_act[2]);
        }

        #[test]
        fn a_half_missing_tap_measures_one_channel_and_proxies_the_other() {
            // A graph that yields `l_out-*` but not `ffn_out-*` is real for the
            // residual. Marking the whole frame proxy would throw away true data;
            // marking it all measured would claim the FFN numbers are real.
            let tap = TapReading {
                resid: vec![Some(1.0), Some(2.0)],
                ffn: vec![None, None],
            };
            let f = fill_frame(0, 2, 2, 0.5, 0.5, &[], 0, 0, 0, Some(&tap));
            assert_eq!(f.flags & FLAG_RESID_MEASURED, FLAG_RESID_MEASURED);
            assert_eq!(f.flags & FLAG_MLP_MEASURED, 0, "FFN stays honestly a proxy");
            assert!(f.mlp_act[0] > 0.0, "and is still filled by the proxy");
        }

        #[test]
        fn no_tap_reproduces_the_proxy_exactly() {
            // The default build's behaviour must be untouched by this tier.
            let a = fill_frame(4, 12, 8, 0.6, 0.3, &[], 5, 50, 0, None);
            let b = fill_frame(4, 12, 8, 0.6, 0.3, &[], 5, 50, 0, Some(&TapReading::default()));
            assert_eq!(a.flags & (FLAG_RESID_MEASURED | FLAG_MLP_MEASURED), 0);
            assert_eq!(b.flags & (FLAG_RESID_MEASURED | FLAG_MLP_MEASURED), 0);
            assert_eq!(a.layer_norm, b.layer_norm, "an empty tap == no tap");
            assert_eq!(a.mlp_act, b.mlp_act);
        }

        #[test]
        fn head_summ_stays_a_proxy_even_with_a_full_tap() {
            // Tier 1 does not silently promote per-head attention: that needs the
            // attention matrix to materialize (flash-attention off), which is Tier 2.
            let tap = TapReading { resid: vec![Some(1.0); 4], ffn: vec![Some(1.0); 4] };
            let f = fill_frame(1, 4, 4, 0.8, 0.2, &[], 0, 0, 0, Some(&tap));
            assert!(f.head_summ[0] > 0.0, "still filled");
            assert_eq!(f.flags & 0x8, 0, "no head-measured bit is claimed");
        }

        #[test]
        fn dials_fall_back_from_zero_snapshot() {
            let d = Dials::from_shared(&ipc::Shared::default());
            assert!((d.temperature - 0.8).abs() < 1e-6);
            assert_eq!(d.context_len, 2048);
            assert_eq!(d.rate_cap, 0.0);
            assert!(!d.full_attn);
        }

        #[test]
        fn dials_honor_deliberate_zero_temperature() {
            // A stamped snapshot (positive context length) with temperature 0.0 must run
            // greedy, NOT be bumped to the 0.8 unstamped default.
            let mut s = ipc::Shared::default();
            s.mind[4] = 0.0; // temperature = 0 (greedy)
            s.mind[5] = 4096.0; // context length stamped → snapshot is not "unstamped"
            let d = Dials::from_shared(&s);
            assert_eq!(d.temperature, 0.0);
            assert_eq!(d.context_len, 4096);
        }

        #[test]
        fn parse_chat_request_reads_messages_and_temp() {
            let body = r#"{"model":"m","messages":[
                {"role":"system","content":"be brief"},
                {"role":"user","content":"hello"}],"temperature":0.4,"stream":false}"#;
            let (msgs, temp) = parse_chat_request(body).expect("parses");
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[0]["role"], "system");
            assert_eq!(msgs[0]["content"], "be brief");
            assert_eq!(msgs[1]["content"], "hello");
            assert!((temp.unwrap() - 0.4).abs() < 1e-6);
        }

        #[test]
        fn parse_chat_request_preserves_tool_fields() {
            // Assistant `tool_calls` + a `tool` result with `tool_call_id` must survive
            // verbatim so the model's own template can render tool history like LM Studio.
            let body = r#"{"messages":[
                {"role":"user","content":"go"},
                {"role":"assistant","content":"","tool_calls":[
                    {"id":"call_1","type":"function","function":{"name":"set","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":"ok"}]}"#;
            let (msgs, _) = parse_chat_request(body).expect("parses");
            assert_eq!(msgs.len(), 3);
            assert_eq!(msgs[1]["tool_calls"][0]["id"], "call_1");
            assert_eq!(msgs[2]["tool_call_id"], "call_1");
        }

        #[test]
        fn msg_pairs_flattens_array_content() {
            let body = r#"{"messages":[{"role":"user","content":[
                {"type":"text","text":"a"},{"type":"text","text":"b"}]}]}"#;
            let (msgs, temp) = parse_chat_request(body).expect("parses");
            let pairs = msg_pairs(&msgs);
            assert_eq!(pairs[0], ("user".to_string(), "ab".to_string()));
            assert!(temp.is_none());
        }

        #[test]
        fn parse_chat_request_rejects_junk_and_empty() {
            assert!(parse_chat_request("not json").is_none());
            assert!(parse_chat_request(r#"{"messages":[]}"#).is_none());
        }

        #[test]
        fn completion_json_roundtrips_content() {
            let s = build_completion_json("gemma", "hi there");
            let v: serde_json::Value = serde_json::from_str(&s).unwrap();
            assert_eq!(v["choices"][0]["message"]["content"], "hi there");
            assert_eq!(v["choices"][0]["message"]["role"], "assistant");
            assert_eq!(v["object"], "chat.completion");
        }

        #[test]
        fn merge_system_folds_into_first_user() {
            let msgs = vec![
                ("system".to_string(), "be brief".to_string()),
                ("user".to_string(), "hi".to_string()),
                ("assistant".to_string(), "hello".to_string()),
                ("user".to_string(), "more".to_string()),
            ];
            let out = merge_system_into_user(&msgs);
            assert_eq!(out.len(), 3); // system folded away
            assert_eq!(out[0], ("user".to_string(), "be brief\n\nhi".to_string()));
            assert_eq!(out[2].1, "more"); // later user untouched
        }

        #[test]
        fn merge_system_creates_user_when_none() {
            let msgs = vec![("system".to_string(), "rules".to_string())];
            let out = merge_system_into_user(&msgs);
            assert_eq!(out, vec![("user".to_string(), "rules".to_string())]);
        }

        #[test]
        fn find_subslice_locates_header_terminator() {
            assert_eq!(find_subslice(b"ab\r\n\r\ncd", b"\r\n\r\n"), Some(2));
            assert_eq!(find_subslice(b"abcd", b"\r\n\r\n"), None);
            assert_eq!(find_subslice(b"", b"x"), None);
        }
    }
}
