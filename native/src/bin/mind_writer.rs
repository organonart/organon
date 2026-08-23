//! Synthetic activation-ring writer (#367 Tier 2 — the visible mind, watched).
//!
//! Emits fake per-token model-activation frames into the mind ring
//! (`$TMPDIR/organic-math-mind.bin`) so the whole Live-streaming visual path can be
//! exercised with ZERO inference. This proves the ring protocol + the visual's
//! per-token node-glow before any real model is embedded (the embedded llama.cpp
//! runtime is the next PR — it replaces this bin as the ring's writer).
//!
//! Run it beside the visual, then in the plugin's Mind card set topology = "Live"
//! and select the Neural Network generator + "Connectome (loaded)" topology to watch a
//! smooth, moving "thought" pattern light the architecture skeleton per token.
//!
//! Usage: `organic-math-mind-writer [n_layers] [n_heads]` (defaults 24 × 16).

use organic_math_native::mind_ring::{
    write_snippet, MindFrame, MindRingWriter, MR_MAX_HEADS, MR_MAX_LAYERS, MR_TOPK,
};
use std::time::{Duration, Instant};

/// A plausible next-token vocabulary for the fake top-k bars (the widget shows real
/// text, so give it real words). `MR_TOPK` entries.
const CANDIDATES: [&str; MR_TOPK] = ["the", " a", " to", " of", " and", " is", " in", "\n"];

/// Synthesize a **plausible** next-token distribution over `CANDIDATES` — a moving
/// "winner" and a breathing sharpness so the model looks sometimes decisive (one bar
/// dominates, low entropy / high confidence) and sometimes torn (flat bars). Returns
/// the descending-sorted (prob, index) list, the normalized softmax `entropy` [0,1],
/// and the top `confidence` [0,1]. Fake but honest in shape — it IS a softmax.
fn synth_topk(t: f32) -> (Vec<(f32, usize)>, f32, f32) {
    // Winner drifts across the vocabulary; sharpness breathes 0.2 (flat) → 2.8 (peaked).
    let winner = (0.5 + 0.5 * (t * 0.31).sin()) * (MR_TOPK as f32 - 1.0);
    let sharp = 1.5 + 1.3 * (t * 0.7).sin();
    let mut logits = [0f32; MR_TOPK];
    for (i, l) in logits.iter_mut().enumerate() {
        let d = i as f32 - winner;
        *l = (-0.5 * d * d + 0.4 * (t * 1.3 + i as f32).sin()) * sharp;
    }
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = probs.iter().sum();
    if sum > 0.0 {
        for p in &mut probs {
            *p /= sum;
        }
    }
    let entropy: f32 = probs.iter().filter(|&&p| p > 0.0).map(|&p| -p * p.ln()).sum();
    let entropy_n = (entropy / (MR_TOPK as f32).ln()).clamp(0.0, 1.0);
    let confidence = probs.iter().cloned().fold(0.0f32, f32::max);
    let mut ranked: Vec<(f32, usize)> = probs.iter().cloned().zip(0..MR_TOPK).collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    (ranked, entropy_n, confidence)
}

fn main() {
    // `--ns <name>` before anything else touches `ipc` (#191 T1). It is not a second
    // mechanism: it sets `$ORGANON_IPC_NS`, which `ipc::namespace()` resolves ONCE per
    // process into a `OnceLock`, so it has to happen before the first path is composed.
    //
    // It exists because the env var alone is a foot-gun for the case this tier is about.
    // Launching a base runtime and then a fine-tune runtime from the SAME shell means
    // setting one variable twice, and a PowerShell `$env:` assignment persists for the
    // session — forget the second one and both runtimes write the same ring, the newer
    // frames overwrite the older, and the "difference" between two models is a model
    // against itself with nothing to report the collision. A flag is per-launch, and it
    // shows up in the command line you can scroll back to.
    //
    // Unlike the env var it REFUSES a bad name instead of falling back. `$ORGANON_IPC_NS`
    // falls back on purpose — a visual spawned with a junk namespace must still come up.
    // A name typed on a command line is a mistake worth stopping for.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--ns" | "-n" => {
                let Some(raw) = argv.get(i + 1) else {
                    eprintln!("mind-writer: --ns wants a namespace, e.g. --ns mind-base");
                    std::process::exit(2);
                };
                match organic_math_native::ipc::sanitize_ns(raw) {
                    Some(ns) => std::env::set_var("ORGANON_IPC_NS", &ns),
                    None => {
                        eprintln!(
                            "mind-writer: '{raw}' is not a usable IPC namespace — ASCII \
                             letters, digits, '-' and '_', 1..=64 characters."
                        );
                        std::process::exit(2);
                    }
                }
                i += 2;
            }
            "-h" | "--help" => {
                println!(
                    "usage: organic-math-mind-writer [--ns <namespace>] [n_layers] [n_heads]\n\
                     \n\
                     Streams synthetic activation frames into $TMPDIR/<namespace>-mind.bin.\n\
                     --ns forks the IPC namespace so a second writer (or runtime) can run\n\
                     beside this one on its own ring; it sets $ORGANON_IPC_NS for this\n\
                     process only. Defaults: 24 layers, 16 heads."
                );
                return;
            }
            other => {
                positional.push(other);
                i += 1;
            }
        }
    }
    let mut positional = positional.into_iter();
    let n_layers = positional
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(24)
        .clamp(1, MR_MAX_LAYERS as u32);
    let n_heads = positional
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(16)
        .clamp(0, MR_MAX_HEADS as u32);

    let path = organic_math_native::ipc::mind_ring_path();
    let mut writer = match MindRingWriter::create() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("mind-writer: failed to create ring at {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    // Say the namespace out loud. Two writers are otherwise indistinguishable in two
    // terminals, and the failure they hide — both on one ring — looks like a working
    // demo right up until the two traces are identical.
    println!(
        "mind-writer: namespace '{}' — read it with MindRingReader::open_ns(\"{}\")",
        organic_math_native::ipc::namespace(),
        organic_math_native::ipc::namespace()
    );
    println!(
        "mind-writer: streaming {n_layers} layers × {n_heads} heads into {} (Ctrl-C to stop)",
        path.display()
    );

    let start = Instant::now();
    let mut token_index: u32 = 0;
    let mut last_report = Instant::now();
    // ~20 frames/sec — a plausible token cadence for a small model.
    let period = Duration::from_millis(50);

    loop {
        let t = start.elapsed().as_secs_f32();
        let mut f = MindFrame::default();
        f.token_index = token_index;
        f.n_layers = n_layers;
        f.n_heads = n_heads;

        // Smooth, bounded, time+layer+head-varying values — a moving "thought" front
        // rolling down the layer stack. Never NaN, always in ~[0, 1].
        for l in 0..n_layers as usize {
            let lp = l as f32 * 0.3;
            let tok = token_index as f32 * 0.1;
            // A travelling wave along depth: the residual norm crests at a moving layer.
            f.layer_norm[l] = 0.5 + 0.5 * (t * 1.7 + lp + tok).sin();
            // MLP activity, offset in phase so it doesn't track the norm exactly.
            f.mlp_act[l] = 0.5 + 0.5 * (t * 2.3 - lp * 0.7 + tok * 1.3).sin();
            for h in 0..n_heads as usize {
                let hp = h as f32 * 0.5;
                // Per-head summary: a slower ring pattern so heads shimmer around each layer.
                f.head_summ[l * MR_MAX_HEADS + h] =
                    0.5 + 0.5 * (t * 1.1 + lp + hp + tok * 0.6).sin();
            }
        }

        // #482 Tier 3 — a plausible rising context / KV fuel gauge (a fake ~20-token
        // prompt, then one token per frame, capped at the window so it fills and holds).
        const CTX_TOTAL: u32 = 4096;
        f.ctx_total = CTX_TOTAL;
        f.ctx_used = (20 + token_index).min(CTX_TOTAL);

        // #482 Tier 2 — the honest per-token stats (a plausible softmax over real words).
        let (ranked, entropy_n, confidence) = synth_topk(t);
        f.entropy = entropy_n;
        f.confidence = confidence;
        f.topk_count = MR_TOPK as u32;
        for (rank, &(prob, idx)) in ranked.iter().enumerate() {
            f.topk_id[rank] = idx as u32;
            f.topk_prob[rank] = prob;
            write_snippet(&mut f.topk_text[rank], CANDIDATES[idx]);
        }

        writer.write_frame(&f);
        token_index = token_index.wrapping_add(1);

        if last_report.elapsed() >= Duration::from_secs(1) {
            println!("mind-writer: token {token_index}, t = {t:.1}s");
            last_report = Instant::now();
        }

        std::thread::sleep(period);
    }
}
