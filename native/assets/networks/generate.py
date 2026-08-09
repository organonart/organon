#!/usr/bin/env python3
"""Generate a gallery of beautiful, load-ready network JSON files for the Organon
Neural Network generator (#226).

Three loadable formats (auto-detected by the visual's sidecar loader), one button
("Load Network (JSON)…"):

  * Connectome  {nodes:[{id,pos:[x,y,z],scalar}], edges:[{src,dst,weight}]}
                -> topology "Connectome (loaded)"
  * MLP         {layers,weights,biases,activation,input}
                -> topology "MLP (loaded weights)"
  * Attention   {type:"attention",tokens,layers,heads,attention:[L][H][T][T]}
                -> topology "Attention (transformer)"

These are procedurally SYNTHESIZED to look meaningful and beautiful under the
renderer (structured, not random noise). They are NOT scraped from real organisms
or trained checkpoints — the honesty line from the issue holds: this renders
connectivity + activity, not a neural simulation; the graph shapes are plausible,
the 3-D layouts (for ANN/attention) are imposed. Drop a genuine C. elegans graph
or a real attention dump in the same schema anytime.

Deterministic: fixed seeds, pure stdlib. Re-run to regenerate:
    python3 generate.py
"""

import json
import math
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))


def write(name, obj):
    path = os.path.join(HERE, name)
    with open(path, "w") as f:
        json.dump(obj, f, separators=(",", ":"))
    print(f"  {name:36s} {os.path.getsize(path)/1024:7.1f} KB")


def r3(x):
    return round(float(x), 4)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------
def normalize_scalars(nodes, deg):
    m = max(deg) or 1
    for n in nodes:
        n["scalar"] = r3(deg[n["id"]] / m)


def fib_sphere(n, radius=1.0):
    """n roughly-even points on a sphere (golden-angle spiral)."""
    pts = []
    ga = math.pi * (3.0 - math.sqrt(5.0))
    for i in range(n):
        y = 1.0 - (i / max(1, n - 1)) * 2.0
        r = math.sqrt(max(0.0, 1.0 - y * y))
        t = ga * i
        pts.append((radius * math.cos(t) * r, radius * y, radius * math.sin(t) * r))
    return pts


# ---------------------------------------------------------------------------
# CONNECTOME 1 — a C. elegans-flavoured worm nervous system
# nerve ring + head/tail ganglia + a ventral cord of motor neurons.
# ---------------------------------------------------------------------------
def worm_connectome():
    rng = random.Random(226_001)
    nodes, edges = [], {}

    def add_node(pos):
        i = len(nodes)
        nodes.append({"id": i, "pos": [r3(pos[0]), r3(pos[1]), r3(pos[2])]})
        return i

    def add_edge(a, b, w=1.0):
        if a == b:
            return
        k = (min(a, b), max(a, b))
        edges[k] = max(edges.get(k, 0.0), w)

    # body runs along Z in [-1, 1]; a gentle sinusoidal bend for life.
    def body(z, ang, rad, dv=0.0):
        bend = 0.18 * math.sin(z * 2.4)
        return (rad * math.cos(ang) + bend, rad * math.sin(ang) + dv, z)

    # --- amphid sensory tuft at the nose (z ~ 1.0) ---
    sensory = [add_node(body(1.0 + 0.04 * rng.random(),
                             rng.uniform(0, math.tau), 0.05 + 0.04 * rng.random()))
               for _ in range(10)]

    # --- nerve ring: a clean circle just behind the nose ---
    ring = []
    RN = 34
    for k in range(RN):
        a = k / RN * math.tau
        ring.append(add_node(body(0.72, a, 0.22)))
    for k in range(RN):  # ring neighbours + a few chords
        add_edge(ring[k], ring[(k + 1) % RN], 1.0)
        if k % 3 == 0:
            add_edge(ring[k], ring[(k + RN // 2) % RN], 0.6)

    # --- head ganglia: two dense clusters flanking the ring ---
    head = []
    for _ in range(46):
        a = rng.uniform(0, math.tau)
        head.append(add_node(body(rng.uniform(0.78, 0.94), a, rng.uniform(0.04, 0.16))))
    for _ in range(150):  # dense internal wiring
        add_edge(rng.choice(head), rng.choice(head), rng.uniform(0.4, 1.0))
    for h in head:  # head projects into the ring
        if rng.random() < 0.5:
            add_edge(h, rng.choice(ring), 0.8)
    for s in sensory:  # sensory dendrites reach the ring
        add_edge(s, rng.choice(ring), 1.0)
        add_edge(s, rng.choice(head), 0.5)

    # --- ventral cord: two parallel motor lines running down the body ---
    cordL, cordR = [], []
    CN = 40
    for k in range(CN):
        z = 0.6 - (k / (CN - 1)) * 1.5  # 0.6 -> -0.9
        cordL.append(add_node(body(z, -math.pi / 2, 0.10, dv=-0.05)))
        cordR.append(add_node(body(z, -math.pi / 2, 0.10, dv=+0.05)))
    for k in range(CN - 1):  # chain each cord
        add_edge(cordL[k], cordL[k + 1], 1.0)
        add_edge(cordR[k], cordR[k + 1], 1.0)
    for k in range(CN):  # commissures (cross-links) every few segments
        if k % 4 == 0:
            add_edge(cordL[k], cordR[k], 0.7)
    for k in range(0, CN, 6):  # long processes from the ring into the cord
        add_edge(rng.choice(ring), cordL[k], 0.5)
        add_edge(rng.choice(ring), cordR[k], 0.5)

    # --- tail ganglia ---
    tail = [add_node(body(rng.uniform(-1.0, -0.9), rng.uniform(0, math.tau),
                          rng.uniform(0.04, 0.14))) for _ in range(24)]
    for _ in range(70):
        add_edge(rng.choice(tail), rng.choice(tail), rng.uniform(0.4, 1.0))
    for t in tail:  # tail hooks onto the end of the cord
        if rng.random() < 0.5:
            add_edge(t, cordL[-1] if rng.random() < 0.5 else cordR[-1], 0.6)

    deg = [0.0] * len(nodes)
    edge_list = []
    for (a, b), w in edges.items():
        edge_list.append({"src": a, "dst": b, "weight": r3(w)})
        deg[a] += w
        deg[b] += w
    normalize_scalars(nodes, deg)
    return {"nodes": nodes, "edges": edge_list}


# ---------------------------------------------------------------------------
# CONNECTOME 2 — a modular "cortex": communities as lobes on a sphere,
# dense intra-community wiring + a sparse rich-club of hubs between them.
# ---------------------------------------------------------------------------
def modular_cortex():
    rng = random.Random(226_002)
    K = 7               # communities (lobes)
    per = 26            # neurons per community
    centers = fib_sphere(K, radius=1.0)
    nodes, deg = [], []
    hubs, members = [], []

    def add(pos):
        i = len(nodes)
        nodes.append({"id": i, "pos": [r3(pos[0]), r3(pos[1]), r3(pos[2])]})
        deg.append(0.0)
        return i

    for c in centers:
        grp = []
        for _ in range(per):
            p = (c[0] + rng.gauss(0, 0.22),
                 c[1] + rng.gauss(0, 0.22),
                 c[2] + rng.gauss(0, 0.22))
            grp.append(add(p))
        members.append(grp)
        hubs.append(grp[0])  # first of each lobe is its rich-club hub

    edges = {}

    def add_edge(a, b, w):
        if a == b:
            return
        k = (min(a, b), max(a, b))
        edges[k] = max(edges.get(k, 0.0), w)

    for grp in members:                      # dense intra-lobe
        for i in range(len(grp)):
            for j in range(i + 1, len(grp)):
                if rng.random() < 0.28:
                    add_edge(grp[i], grp[j], rng.uniform(0.4, 1.0))
            add_edge(grp[i], grp[0], 0.8)     # everyone touches its hub
    for i in range(K):                        # rich-club: hubs fully interlinked
        for j in range(i + 1, K):
            add_edge(hubs[i], hubs[j], 1.0)
    for _ in range(24):                       # a few stray long-range fibres
        add_edge(rng.choice(nodes)["id"], rng.choice(nodes)["id"], 0.3)

    edge_list = []
    for (a, b), w in edges.items():
        edge_list.append({"src": a, "dst": b, "weight": r3(w)})
        deg[a] += w
        deg[b] += w
    m = max(deg) or 1
    for n in nodes:
        n["scalar"] = r3(deg[n["id"]] / m)
    return {"nodes": nodes, "edges": edge_list}


# ---------------------------------------------------------------------------
# CONNECTOME 3 — a woven torus lattice (pure geometry), scalar = a travelling
# wave so it shimmers as a knitted surface.
# ---------------------------------------------------------------------------
def torus_lattice():
    U, V = 28, 12
    R, r = 1.0, 0.42
    nodes, edges = [], {}

    def idx(u, v):
        return (u % U) * V + (v % V)

    for u in range(U):
        for v in range(V):
            a = u / U * math.tau
            b = v / V * math.tau
            x = (R + r * math.cos(b)) * math.cos(a)
            y = (R + r * math.cos(b)) * math.sin(a)
            z = r * math.sin(b)
            wave = 0.5 + 0.5 * math.sin(3 * a + 2 * b)
            nodes.append({"id": idx(u, v),
                          "pos": [r3(x), r3(y), r3(z)],
                          "scalar": r3(wave)})

    def add_edge(a, b, w=1.0):
        k = (min(a, b), max(a, b))
        edges[k] = w

    for u in range(U):
        for v in range(V):
            add_edge(idx(u, v), idx(u + 1, v), 1.0)      # around the tube ring
            add_edge(idx(u, v), idx(u, v + 1), 0.8)      # around the tube
            add_edge(idx(u, v), idx(u + 1, v + 1), 0.4)  # a diagonal weave
    edge_list = [{"src": a, "dst": b, "weight": r3(w)} for (a, b), w in edges.items()]
    return {"nodes": nodes, "edges": edge_list}


# ---------------------------------------------------------------------------
# MLP helpers — build [out x in] row-major matrices from smooth signed bases so
# the signed-weight edge colours form flowing patterns, not noise.
# ---------------------------------------------------------------------------
def smooth_matrix(n_out, n_in, freq, seed, sparsity=0.0, gain=1.0):
    rng = random.Random(seed)
    W = []
    for j in range(n_out):
        for i in range(n_in):
            fi = i / max(1, n_in - 1)
            fj = j / max(1, n_out - 1)
            v = math.cos(math.pi * freq * fi * (fj + 0.5)) * math.cos(math.pi * (fj - fi))
            v *= gain * (0.75 + 0.25 * rng.random())
            if abs(v) < sparsity:
                v = 0.0
            W.append(r3(v))
    return W  # flat [out x in]


def mlp_file(layers, freqs, activation="tanh", sparsity=0.12, seed=0, inp=None):
    weights, biases = [], []
    for l in range(len(layers) - 1):
        weights.append(smooth_matrix(layers[l + 1], layers[l], freqs[l],
                                     seed + l * 17, sparsity))
        biases.append([r3(0.15 * math.sin(k)) for k in range(layers[l + 1])])
    if inp is None:
        inp = [r3(math.sin(0.7 * k) * 0.9) for k in range(layers[0])]
    return {"type": "mlp", "layers": layers, "weights": weights,
            "biases": biases, "activation": activation, "input": inp}


# ---------------------------------------------------------------------------
# ATTENTION helpers — synthesize causal, per-row-normalized attention with
# recognizable head "personalities" (real transformer head archetypes).
# ---------------------------------------------------------------------------
def softmax_causal(logits_row, i):
    row = logits_row[: i + 1]
    m = max(row)
    ex = [math.exp(x - m) for x in row]
    s = sum(ex) or 1.0
    return [x / s for x in ex] + [0.0] * (len(logits_row) - i - 1)


def head_logits(kind, i, T, rng):
    """Return a length-T logit row for query i, for a named head archetype."""
    lg = [-1e9] * T  # masked (future) stays -inf
    for j in range(i + 1):
        x = 0.0
        if kind == "bos_sink":
            x = 3.0 if j == 0 else -0.4 * (i - j)
        elif kind == "previous":            # attend the immediately previous token
            x = 4.0 if j == i - 1 else (1.0 if j == 0 else -1.2 * (i - j))
        elif kind == "induction":           # attend a fixed span back (copy-ish)
            span = 4
            x = 3.2 if (i - j) == span else (1.2 if j == 0 else -0.9 * (i - j))
        elif kind == "local":               # soft Gaussian window around i
            x = -((i - j) ** 2) / 6.0
        elif kind == "delimiter":           # attend "punctuation" positions
            x = 2.6 if (j % 7 == 0) else -0.5 * (i - j)
        elif kind == "broad":               # gentle, near-uniform triangular fill
            x = 0.3 * math.cos(0.4 * (i - j))
        else:
            x = -0.5 * (i - j)
        x += 0.15 * (rng.random() - 0.5)    # a whisper of jitter
        lg[j] = x
    return lg


def attention_file(T, layer_heads, seed=0, tokens=None):
    """layer_heads: list of layers, each a list of head-kind strings."""
    rng = random.Random(seed)
    att = []
    for heads in layer_heads:
        layer = []
        for kind in heads:
            head = []
            for i in range(T):
                head.append([r3(v) for v in softmax_causal(head_logits(kind, i, T, rng), i)])
            layer.append(head)
        att.append(layer)
    obj = {"type": "attention",
           "layers": len(layer_heads),
           "heads": len(layer_heads[0]),
           "attention": att}
    obj["tokens"] = tokens if tokens is not None else T
    return obj


# ---------------------------------------------------------------------------
def main():
    print("connectomes:")
    write("connectome-celegans-worm.json", worm_connectome())
    write("connectome-modular-cortex.json", modular_cortex())
    write("connectome-torus-weave.json", torus_lattice())

    print("mlps:")
    # Autoencoder — the hourglass/bottleneck diamond.
    write("mlp-autoencoder.json",
          mlp_file([16, 10, 4, 10, 16], [2, 3, 3, 2], activation="tanh", seed=51))
    # Classifier funnel — wide sensory layer tapering to a few classes.
    write("mlp-classifier-funnel.json",
          mlp_file([24, 14, 8, 4], [3, 4, 2], activation="relu", seed=52))
    # Deep tower — many equal layers, rhythmic block structure.
    write("mlp-deep-tower.json",
          mlp_file([10, 10, 10, 10, 10, 10], [2, 3, 4, 3, 2], activation="tanh", seed=53))

    print("attention:")
    # The head gallery: 3 layers x 4 heads, each a distinct archetype. Sweep heads.
    gallery = [
        ["bos_sink", "previous", "local", "broad"],
        ["induction", "delimiter", "previous", "bos_sink"],
        ["local", "induction", "broad", "delimiter"],
    ]
    write("attention-head-gallery.json", attention_file(40, gallery, seed=71))
    # A single "sentence": fewer tokens, 2x2, gorgeous as a ring of chords.
    sentence = [["bos_sink", "previous"], ["induction", "local"]]
    write("attention-sentence-ring.json", attention_file(18, sentence, seed=72))


if __name__ == "__main__":
    main()
