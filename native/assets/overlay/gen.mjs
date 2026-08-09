// Offline TeX → PNG formula renderer for the #135 capture overlay (Phase 2).
//
// MathJax (tex2svg) → SVG → @resvg/resvg-js → PNG, transparent background, white
// default glyphs with per-variable colours baked in via \textcolor. Output PNGs are
// committed to ../../src/overlay/ and bundled into the Rust binary via include_bytes!.
//
//   cd native/assets/overlay && npm i && node gen.mjs
//
// Per-variable colours MUST match overlay_meta.rs's Symbol colours (formula↔readout).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mathjax } from 'mathjax-full/js/mathjax.js';
import { TeX } from 'mathjax-full/js/input/tex.js';
import { SVG } from 'mathjax-full/js/output/svg.js';
import { liteAdaptor } from 'mathjax-full/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from 'mathjax-full/js/handlers/html.js';
import { AllPackages } from 'mathjax-full/js/input/tex/AllPackages.js';
import { Resvg } from '@resvg/resvg-js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = path.resolve(__dirname, '../../src/overlay');
fs.mkdirSync(OUT_DIR, { recursive: true });

// Shared symbol palette (keep in sync with overlay_meta.rs Symbol colours).
const C = {
  u: '#e879c0', // pink
  v: '#b07cf0', // purple
  th: '#4cc9f0', // theta  — cyan
  ph: '#67e8a3', // phi    — green
  ka: '#f6a64c', // kappa  — orange
  ta: '#f0688f', // tau    — rose
};
const col = (hex, body) => `\\textcolor{${hex}}{${body}}`;

// One multi-line TeX string per flagship generator. `\textcolor` colours the live
// variables so the formula matches its readout panel.
const FORMULAS = {
  original: String.raw`\begin{array}{l}
    \mathbf{p}_n = R(${col(C.th, '\\theta_n')})\,\mathbf{p}_{n-1} + \mathbf{t} \\[4pt]
    ${col(C.th, '\\theta_n')} = n\,\dot\theta\,t
  \end{array}`,

  frenet: String.raw`\begin{array}{l}
    \mathbf{T}' = ${col(C.ka, '\\kappa')}\,\mathbf{N} \\[2pt]
    \mathbf{N}' = -${col(C.ka, '\\kappa')}\,\mathbf{T} + ${col(C.ta, '\\tau')}\,\mathbf{B} \\[2pt]
    \mathbf{B}' = -${col(C.ta, '\\tau')}\,\mathbf{N}
  \end{array}`,

  dna: String.raw`\begin{array}{l}
    x = r\cos${col(C.th, '\\theta')} \\[2pt]
    y = r\sin${col(C.th, '\\theta')} \\[2pt]
    z = \dfrac{p}{2\pi}\,${col(C.th, '\\theta')}
  \end{array}`,

  harmonic: String.raw`\begin{array}{l}
    r(${col(C.th, '\\theta')},${col(C.ph, '\\phi')}) = Y_\ell^{\,m}(${col(C.th, '\\theta')},${col(C.ph, '\\phi')}) \\[3pt]
    \mathbf{x} = r\,\hat{\mathbf{r}}(${col(C.th, '\\theta')},${col(C.ph, '\\phi')})
  \end{array}`,

  // Catenoid↔helicoid associate family (the live bend the generator animates).
  minimal: String.raw`\begin{array}{l}
    x = \cosh ${col(C.v, 'v')}\,\cos ${col(C.u, 'u')} \\[2pt]
    y = \cosh ${col(C.v, 'v')}\,\sin ${col(C.u, 'u')} \\[2pt]
    z = ${col(C.v, 'v')} \qquad H = 0
  \end{array}`,

  // --- Non-flagship generators (#135 P4) ---

  // Lorenz strange attractor.
  attractor: String.raw`\begin{array}{l}
    \dot x = ${col(C.ka, '\\sigma')}(y - x) \\[2pt]
    \dot y = x(${col(C.ta, '\\rho')} - z) - y \\[2pt]
    \dot z = xy - ${col(C.u, '\\beta')} z
  \end{array}`,

  // L-system production rule + branch angle.
  lsystem: String.raw`\begin{array}{l}
    F \rightarrow F[+F]F[-F]F \\[3pt]
    \text{turn } = \pm\,${col(C.th, '\\delta')}
  \end{array}`,

  // Divergence-free curl-noise flow.
  curlnoise: String.raw`\mathbf{v} = \nabla\times\boldsymbol{${col(C.ph, '\\psi')}}(\mathbf{x}),\quad \nabla\cdot\mathbf{v} = 0`,

  // Circularly polarized wave.
  polarization: String.raw`\mathbf{E} = E_0\big(\hat{\mathbf{x}}\cos ${col(C.th, '\\omega')} t + \hat{\mathbf{y}}\sin ${col(C.th, '\\omega')} t\big)`,

  // Maxwell's curl equations.
  maxwell: String.raw`\begin{array}{l}
    \nabla\times${col(C.ka, '\\mathbf{E}')} = -\,\partial_t ${col(C.th, '\\mathbf{B}')} \\[3pt]
    \nabla\times${col(C.th, '\\mathbf{B}')} = \mu_0\varepsilon_0\,\partial_t ${col(C.ka, '\\mathbf{E}')}
  \end{array}`,

  // Phyllotaxis: golden-angle spiral.
  phyllotaxis: String.raw`\begin{array}{l}
    ${col(C.th, '\\theta_n')} = n\,\psi,\quad \psi = 137.5^\circ \\[3pt]
    r_n = c\sqrt{n}
  \end{array}`,

  // Mandelbulb iteration (spherical power).
  mandelbulb: String.raw`\mathbf{z} \;\mapsto\; \mathbf{z}^{\,${col(C.u, 'p')}} + \mathbf{c}`,

  // Kaleidoscopic IFS: fold then scale.
  kifs: String.raw`\mathbf{z} \;\mapsto\; ${col(C.v, 's')}\,\mathcal{F}(\mathbf{z}) - \mathbf{c}`,

  // Boids: steering = weighted sum of the three rules.
  boids: String.raw`\mathbf{a} = ${col(C.u, 'w_s')}\mathbf{S} + ${col(C.ph, 'w_a')}\mathbf{A} + ${col(C.th, 'w_c')}\mathbf{C}`,

  // Aperiodic tiling: golden-ratio inflation.
  tessellation: String.raw`${col(C.ka, '\\varphi')} = \dfrac{1+\sqrt5}{2},\quad A_{n+1} = ${col(C.ka, '\\varphi')}^{2} A_n`,

  // Synchrotron: the Lienard-Wiechert field of a relativistic charge orbiting a
  // circle (velocity 1/R^2 + radiation 1/R terms), evaluated at the retarded time.
  synchrotron: String.raw`\begin{array}{c}
    \mathbf{E} = \dfrac{q}{4\pi\varepsilon_0}\!\left[\dfrac{\hat{\mathbf{n}}-\boldsymbol{${col(C.u, '\\beta')}}}{${col(C.ph, '\\gamma')}^{2}\,${col(C.ka, '\\kappa')}^{3} R^{2}} + \dfrac{\hat{\mathbf{n}}\times[(\hat{\mathbf{n}}-\boldsymbol{${col(C.u, '\\beta')}})\times\dot{\boldsymbol{${col(C.u, '\\beta')}}}]}{c\,${col(C.ka, '\\kappa')}^{3} R}\right]_{t'} \\[8pt]
    ${col(C.ka, '\\kappa')} = 1 - \hat{\mathbf{n}}\cdot\boldsymbol{${col(C.u, '\\beta')}},\quad \mathbf{r}_s(t') = R(\cos${col(C.th, '\\theta')},\,\sin${col(C.th, '\\theta')},\,0)
  \end{array}`,

  // Vector-field plotter (#173): the general field + the default bank entry
  // (the reel's parabolic swirl, lifted into 3-D by the z-lift term).
  vecfield: String.raw`\begin{array}{c}
    \mathbf{F}(${col(C.u, 'x')},\,${col(C.v, 'y')},\,${col(C.th, 'z')}) = \big(F_x,\;F_y,\;F_z\big) \\[6pt]
    \mathbf{F} = \big(${col(C.v, 'y')}^{2},\; -${col(C.u, 'x')}^{2},\; k\sin ${col(C.th, 'z')}\big)
  \end{array}`,

  // Axon Waveguide (#218): the step-index guiding condition (myelin n1 over
  // axoplasm n2) and the fibre V-number (normalized frequency) that sets how many
  // modes the fibre guides.
  axon: String.raw`\begin{array}{c}
    ${col(C.ka, 'n_1')} > ${col(C.th, 'n_2')} \quad\Rightarrow\quad \text{guided} \\[6pt]
    V = \dfrac{2\pi ${col(C.u, 'a')}}{\lambda}\sqrt{${col(C.ka, 'n_1')}^{2} - ${col(C.th, 'n_2')}^{2}}
  \end{array}`,

  // Rails (#187): the Gielis superformula cross-section swept along the
  // beat-parametrized rail (u = the beat clock; per-cell control values).
  rails: String.raw`\begin{array}{c}
    r(${col(C.th, '\\theta')},\,${col(C.u, 'u')}) = \left(\,\left|\cos\tfrac{${col(C.ph, 'm')}${col(C.th, '\\theta')}}{4}\right|^{n_2} + \left|\sin\tfrac{${col(C.ph, 'm')}${col(C.th, '\\theta')}}{4}\right|^{n_3}\right)^{-1/n_1} \\[8pt]
    ${col(C.ph, 'm')},\,n_i = \mathrm{CR}\!\big(h(\lfloor ${col(C.u, 'u')}/L \rfloor)\big),\qquad ${col(C.u, 'u')} = \text{beats}
  \end{array}`,
};

const adaptor = liteAdaptor();
RegisterHTMLHandler(adaptor);
const tex = new TeX({ packages: AllPackages });
const svg = new SVG({ fontCache: 'none' });
const doc = mathjax.document('', { InputJax: tex, OutputJax: svg });

const TARGET_H = 256; // px tall render; the Rust side scales the quad to the zone.

// Optional `ONLY=name` env filter: render just one formula (e.g. when adding a
// generator) so the other committed PNGs stay byte-for-byte unchanged.
const only = process.env.ONLY;
for (const [name, formula] of Object.entries(FORMULAS)) {
  if (only && name !== only) continue;
  const node = doc.convert(formula, { display: true });
  let svgStr = adaptor.innerHTML(node);
  // MathJax glyph paths use fill="currentColor"; default it to white so the
  // non-coloured parts read on a dark video. Coloured \textcolor parts override.
  svgStr = svgStr.replace('<svg ', '<svg color="#ffffff" ');

  const resvg = new Resvg(svgStr, {
    fitTo: { mode: 'height', value: TARGET_H },
    background: 'rgba(0,0,0,0)',
  });
  const png = resvg.render().asPng();
  const out = path.join(OUT_DIR, `formula_${name}.png`);
  fs.writeFileSync(out, png);
  const { width, height } = resvg.render(); // dims for the log
  console.log(`wrote ${path.relative(process.cwd(), out)}  (${png.length} bytes)`);
}

console.log('done — PNGs in', path.relative(process.cwd(), OUT_DIR));
