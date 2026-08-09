//! Write a valid `Shared` snapshot to the real IPC file: the default look
//! (Original generator, Instanced path) with the RT master toggle + args-driven
//! extras. Usage: cargo run --example rt_ipc_poke -- <rt:0|1> [debug 0-3] [nodes]
use organic_math_native::ipc;

fn main() {
    let rt: f32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(1.0);
    let dbg: f32 = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let nodes: f32 = std::env::args().nth(3).and_then(|a| a.parse().ok()).unwrap_or(0.0);
    let mut s = ipc::Shared::default();
    s.rt[0] = rt;
    s.rt[1] = dbg;
    if nodes > 0.0 {
        // Crank the field: loop_count w is the big axis on the default grid.
        s.loop_count = [1.0, 1.0, nodes.sqrt().ceil(), nodes.sqrt().ceil()];
    }
    if let Some(sm) = std::env::args().nth(4).and_then(|a| a.parse().ok()) {
        s.surface_mode = sm;
    }
    if let Some(g) = std::env::args().nth(5).and_then(|a| a.parse().ok()) {
        s.generator = g;
    }
    if std::env::args().any(|a| a == "gtao") {
        // AO on with the screen-space source — the warm-GTAO starting state.
        s.ssao = [1.0, 1.0, 1.0, 0.5];
        s.rt = [1.0, 0.0, 1.0, 0.15, 1.0, 1.0, 0.0, 0.0];
        s.rt2 = [0.0, 1.0, 0.4, 2.0, 1.0, 0.0, 2.0, 0.0];
    }
    if std::env::args().any(|a| a == "gionly") {
        // RT GI ALONE — nothing else drives the depth prepass (the #207
        // review case: SSGI off, no SSAO/shadows/reflections/AO).
        s.rt3 = [1.0, 1.0, 2.0, 2.0, 1.0, 0.0, 0.0, 0.0];
    }
    if std::env::args().any(|a| a == "rtgi") {
        // Full RT stack + Tier 4 GI: AO on, shadows, reflections, RT AO, RT GI.
        s.ssao = [1.0, 1.0, 1.0, 0.5];
        s.rt = [1.0, 0.0, 1.0, 0.15, 1.0, 1.0, 0.0, 0.0];
        s.rt2 = [1.0, 1.0, 0.4, 2.0, 1.0, 1.0, 2.0, 0.0];
        s.rt3 = [1.0, 1.0, 2.0, 2.0, 1.0, 0.0, 0.0, 0.0];
    }
    if std::env::args().any(|a| a == "rtao") {
        // Tier-3 repro: AO master on + RT source + the whole RT stack.
        s.ssao = [1.0, 1.0, 1.0, 0.5];
        s.rt = [1.0, 0.0, 1.0, 0.15, 1.0, 1.0, 0.0, 0.0];
        s.rt2 = [1.0, 1.0, 0.4, 2.0, 1.0, 1.0, 2.0, 0.0];
    }
    if std::env::args().nth(6).as_deref() == Some("heavy") {
        // Every heavy pass at once: MSAA 4x, SSAO, SSR, probe GI, TAA + motion
        // blur, SSGI, cast shadows, VXGI (+spec) — the full-stack frame.
        s.msaa = 4;
        s.ssao = [1.0, 1.0, 1.0, 0.5];
        s.ssr = [1.0, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        s.gi = [1.0, 1.0, 0.0, 0.0];
        s.temporal = [1.0, 1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        s.ssgi = [1.0, 1.0, 0.0, 0.0];
        s.shadow = [1.0, 1.0, 0.0, 0.0];
        s.vxgi = [1.0, 1.0, 0.0, 0.0];
        s.vxgi_spec = [1.0, 0.0, 0.0, 0.0];
    }
    let mut w = ipc::Writer::create().expect("ipc writer");
    w.write(s);
    println!(
        "wrote Shared (rt={} debug={} loops={:?}) to {:?}",
        rt,
        dbg,
        s.loop_count,
        ipc::ipc_path()
    );
}
