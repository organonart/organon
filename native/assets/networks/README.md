# Neural Network demo gallery

Load-ready JSON networks for the Organon **Neural Network** generator (#226). One
button in the plugin editor — **"Load Network (JSON)…"** — ingests any of these;
the visual **auto-detects** which of the three formats it is and you pick the
matching topology in the generator card.

> **Honesty (the issue's line):** these are procedurally **synthesized** to look
> meaningful and beautiful, not scraped from a real organism or a trained
> checkpoint. They render *connectivity + activity*, not a neural simulation; the
> ANN/attention 3-D layouts are *imposed* (units/positions, not cells). Drop a
> genuine C. elegans graph or a real attention dump in the same schema anytime —
> the loaders don't care where the numbers came from.

## Where these get installed

`native/deploy.sh` copies every `*.json` here into the app-support store —
`~/Library/Application Support/OrganicMath/networks/` (next to `presets.json` +
`clips/`). The plugin's **"Load Network (JSON)…"** dialog opens at that dir, so the
gallery is one click away. They are **repo files, not embedded in the `.vst3`** —
so **re-run `./deploy.sh` whenever you add or regenerate a file** (the copy is
idempotent). No Mac? The files still live here in the repo; just browse to this
folder in the dialog.

## How to load

1. In the editor's **Neural Network** card, click **"Load Network (JSON)…"** (it
   opens at the installed gallery) and pick a file below.
2. Set **topology** to match: `Connectome (loaded)` / `MLP (loaded weights)` /
   `Attention (transformer)`.
3. Best look: **Swept Tubes** surface + **Glass** or **Chrome** material + HDR.

## The gallery

### Connectomes — `topology = Connectome (loaded)`
| File | What it is |
|---|---|
| `connectome-celegans-worm.json` | A *C. elegans*-flavoured nervous system: amphid sensory tuft → **nerve ring** → head ganglia, a two-line **ventral cord** of motor neurons with commissures, tail ganglia. Node glow = degree (hubs shine). |
| `connectome-modular-cortex.json` | A modular "cortex": 7 communities (lobes) on a sphere, dense intra-lobe wiring + a sparse **rich-club** of hubs between them — textbook small-world. |
| `connectome-torus-weave.json` | A woven **torus lattice** (pure geometry); node scalar is a travelling wave so it shimmers like knitted fabric. |

### MLPs — `topology = MLP (loaded weights)`
Edges are the **signed weights** (warm +, cool −; thickness = magnitude), nodes lit
by a **live forward pass**. Weights are smooth signed bases, so the colour field
flows instead of looking like noise.
| File | Shape |
|---|---|
| `mlp-autoencoder.json` | `16→10→4→10→16` — the bottleneck **hourglass/diamond**. |
| `mlp-classifier-funnel.json` | `24→14→8→4` — a wide sensory layer **tapering** to a few classes (relu). |
| `mlp-deep-tower.json` | `10×6` equal layers — a rhythmic, block-structured **tower**. |

### Attention — `topology = Attention (transformer)`  *(also works with no file — synthetic fallback)*
Causal attention graphs; select a **(layer, head)** or turn up **head sweep /beat**
to cycle them, and **reveal /beat** to grow the attended set token-by-token.
| File | Contents |
|---|---|
| `attention-head-gallery.json` | 3 layers × 4 heads × 40 tokens — a gallery of recognizable head archetypes: **BOS-sink**, **previous-token**, **induction**, **local/positional**, **delimiter**, **broad**. Sweep the heads to tour them. |
| `attention-sentence-ring.json` | A short 18-token "sentence", 2×2 heads — gorgeous as a **ring** of attention chords. |

## Regenerating / adding your own

```bash
python3 generate.py       # deterministic; rewrites every file above
```

Pure stdlib, fixed seeds. Edit the archetypes / sizes in `generate.py` and re-run.
The schemas (all accepted by the loaders in `native/src/math.rs`):

- **Connectome** — `{"nodes":[{"id",..,"pos":[x,y,z],"scalar"?}], "edges":[{"src","dst","weight"?}]}`
  (every node needs a `pos`, else the whole graph is force-laid-out).
- **MLP** — `{"type":"mlp","layers":[..],"weights":[[..],..],"biases"?,"activation":"tanh|relu|sigmoid|identity","input"?}`
  (`weights[l]` is the flat `[out×in]` matrix for layer *l→l+1*).
- **Attention** — `{"type":"attention","tokens","layers","heads","attention":[L][H][T][T]}`
  (causal: row *i* should be 0 for *j > i*; rows are renormalized on load).

⚠️ Auto-detect uses substring sniffing: a **connectome** file must not contain
`"weights"`, `"layers"`, or `"attention"`; an **MLP** must not contain
`"attention"` (checked first). The shipped files respect this — a Rust regression
test (`shipped_network_gallery_loads`) keeps them honest.
