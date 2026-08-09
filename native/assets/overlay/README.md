# Overlay formula assets (#135 Phase 2)

Pre-rendered TeX formula images for the capture overlay. White default glyphs on a
transparent background, with per-variable colours baked in (`\textcolor`) so the
formula matches its live readout panel. Drawn as a textured quad by `src/overlay.rs`.

## Regenerate

```bash
cd native/assets/overlay
npm_config_cache="$HOME/.npm-cache-organic" npm install   # ~/.npm is root-owned; use the project cache
node gen.mjs
```

Output → `../../src/overlay/formula_<gen>.png` (committed; bundled via `include_bytes!`).
Pipeline: MathJax (tex2svg) → SVG → `@resvg/resvg-js` → PNG. No LaTeX / system deps.

**Per-variable colours must stay in sync** with `Symbol` colours in
`native/src/overlay_meta.rs` (the `C` palette in `gen.mjs` is the source of truth).
`node_modules/` is gitignored; the committed PNGs are what the build uses.
