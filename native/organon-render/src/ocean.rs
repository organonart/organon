//! FFT (Tessendorf) ocean surface synthesis (#102, Part B).
//!
//! A statistical wind-wave ocean. CPU-side: a Phillips-spectrum field `h0(k)`
//! (seeded, built once), evolved in time and **inverse-FFT'd each frame** into a
//! tiling height field + horizontal (choppy) displacement + foam (from the
//! displacement Jacobian). The result is packed into an `Rgba16Float` tile
//! `(normal.x, normal.z, height, foam)` that the terrain water shader samples,
//! repeated across the world — so swell/chop/glitter carry the correct distribution
//! of energy across scales instead of a couple of hand-tuned ripples.
//!
//! The pure math (FFT round-trip, spectrum determinism + energy) is unit-tested; the
//! GPU upload is a thin wrapper. Compiled only by the visual binary (via `#[path]`).

/// Tile resolution (a power of two for radix-2 FFT). 128² is a good
/// detail/perf balance; the per-frame cost is 3 inverse 2-D FFTs of this size.
pub const N: usize = 128;
const G: f32 = 9.81; // gravity (m/s²) for the deep-water dispersion ω=√(g|k|)
/// Linear height gain applied after the inverse FFT. The raw Phillips field comes out
/// sub-unit (RMS ≈ 0.18 at amplitude 1), i.e. a near-flat mirror — this lifts it to
/// world scale so the waves (and their slopes → reflection/glitter/foam) actually read.
/// `amplitude` is the user multiplier on top of this.
const AMP_GAIN: f32 = 18.0;
const FMT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Editor-driven ocean parameters (the wind sea state + look).
#[derive(Clone, Copy, PartialEq)]
pub struct OceanParams {
    pub wind_speed: f32,   // drives the Phillips largest-wave length L = V²/g
    pub wind_dir_deg: f32, // wind/wave heading
    pub amplitude: f32,    // overall wave-height scale
    pub choppiness: f32,   // lateral displacement (sharp crests) + foam source
    pub tile_size: f32,    // world units of one FFT tile (wave scale)
}

impl Default for OceanParams {
    fn default() -> Self {
        OceanParams {
            wind_speed: 14.0,
            wind_dir_deg: 45.0,
            amplitude: 1.0,
            choppiness: 1.0,
            tile_size: 600.0,
        }
    }
}

// --- tiny complex -----------------------------------------------------------
#[derive(Clone, Copy, Default)]
struct C {
    re: f32,
    im: f32,
}
impl C {
    #[inline]
    fn new(re: f32, im: f32) -> C {
        C { re, im }
    }
    #[inline]
    fn add(self, o: C) -> C {
        C { re: self.re + o.re, im: self.im + o.im }
    }
    #[inline]
    fn sub(self, o: C) -> C {
        C { re: self.re - o.re, im: self.im - o.im }
    }
    #[inline]
    fn mul(self, o: C) -> C {
        C { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
    #[inline]
    fn conj(self) -> C {
        C { re: self.re, im: -self.im }
    }
    #[inline]
    fn scale(self, s: f32) -> C {
        C { re: self.re * s, im: self.im * s }
    }
}

/// In-place iterative radix-2 Cooley–Tukey FFT (`inverse` divides by n).
fn fft_1d(a: &mut [C], inverse: bool) {
    let n = a.len();
    // bit-reversal permutation
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = (if inverse { 2.0 } else { -2.0 }) * std::f32::consts::PI / len as f32;
        let wlen = C::new(ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let mut w = C::new(1.0, 0.0);
            for k in 0..len / 2 {
                let u = a[i + k];
                let v = a[i + k + len / 2].mul(w);
                a[i + k] = u.add(v);
                a[i + k + len / 2] = u.sub(v);
                w = w.mul(wlen);
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let inv = 1.0 / n as f32;
        for x in a.iter_mut() {
            *x = x.scale(inv);
        }
    }
}

/// 2-D FFT: rows then columns (separable). `grid` is row-major `n×n`.
fn fft_2d(grid: &mut [C], n: usize, inverse: bool) {
    let mut line = vec![C::default(); n];
    for r in 0..n {
        line.copy_from_slice(&grid[r * n..r * n + n]);
        fft_1d(&mut line, inverse);
        grid[r * n..r * n + n].copy_from_slice(&line);
    }
    for c in 0..n {
        for r in 0..n {
            line[r] = grid[r * n + c];
        }
        fft_1d(&mut line, inverse);
        for r in 0..n {
            grid[r * n + c] = line[r];
        }
    }
}

// --- deterministic Gaussian RNG (xorshift + Box–Muller) ---------------------
struct Rng(u32);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        // xorshift32
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        // (0,1)
        (x as f32 / u32::MAX as f32).clamp(1e-7, 1.0 - 1e-7)
    }
    fn gauss(&mut self) -> f32 {
        let u1 = self.next_f32();
        let u2 = self.next_f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

/// Phillips spectrum P(k): energy ∝ 1/k⁴, aligned with the wind, with a small-wave
/// cutoff. `amp` is the global scale `A`.
fn phillips(kx: f32, kz: f32, wind_speed: f32, wdir: (f32, f32), amp: f32) -> f32 {
    let k2 = kx * kx + kz * kz;
    if k2 < 1e-8 {
        return 0.0;
    }
    let k4 = k2 * k2;
    let l = (wind_speed * wind_speed / G).max(1e-3); // largest wave from the wind
    let khat = (kx / k2.sqrt(), kz / k2.sqrt());
    let kdotw = khat.0 * wdir.0 + khat.1 * wdir.1;
    let small = l * 0.001; // suppress very small wavelengths
    amp * (-1.0 / (k2 * l * l)).exp() / k4 * (kdotw * kdotw) * (-k2 * small * small).exp()
}

/// Spatial-frequency vector for grid index `m` (centred so DC is at N/2).
#[inline]
fn k_of(m: usize, tile: f32) -> f32 {
    std::f32::consts::TAU * (m as f32 - (N as f32) / 2.0) / tile
}

/// The CPU side of the ocean: the seeded spectrum + per-frame field synthesis. The
/// GPU texture lives in [`Ocean`]; this is split out so it is unit-testable headless.
struct Spectrum {
    params: OceanParams,
    h0: Vec<C>,    // N*N base spectrum
    omega: Vec<f32>, // N*N dispersion
}

impl Spectrum {
    fn new(params: OceanParams, seed: u32) -> Self {
        let mut s = Spectrum { params, h0: vec![C::default(); N * N], omega: vec![0.0; N * N] };
        s.rebuild(seed);
        s
    }

    fn rebuild(&mut self, seed: u32) {
        let mut rng = Rng(seed | 1);
        let wd = self.params.wind_dir_deg.to_radians();
        let wdir = (wd.cos(), wd.sin());
        let inv_sqrt2 = 1.0 / std::f32::consts::SQRT_2;
        for m in 0..N {
            for n in 0..N {
                let kx = k_of(m, self.params.tile_size);
                let kz = k_of(n, self.params.tile_size);
                // `amplitude` is a linear post-FFT gain (applied in `evaluate`), not a
                // spectrum scale, so the look is intuitive and needs no rebuild.
                let p = phillips(kx, kz, self.params.wind_speed, wdir, 1.0);
                let h0 = inv_sqrt2 * p.max(0.0).sqrt();
                let i = m * N + n;
                self.h0[i] = C::new(rng.gauss() * h0, rng.gauss() * h0);
                self.omega[i] = (G * (kx * kx + kz * kz).sqrt()).sqrt();
            }
        }
    }

    /// Synthesise the field at time `t`: fills `height`, `dispx`, `dispz` (length
    /// N*N, row-major). `dispx/dispz` already carry the choppiness scale λ.
    fn evaluate(&self, t: f32, height: &mut [f32], dispx: &mut [f32], dispz: &mut [f32]) {
        let mut hk = vec![C::default(); N * N];
        let mut dx = vec![C::default(); N * N];
        let mut dz = vec![C::default(); N * N];
        let lambda = self.params.choppiness;
        for m in 0..N {
            for n in 0..N {
                let i = m * N + n;
                // mirror index for −k (the conjugate-symmetry partner)
                let mi = ((N - m) % N) * N + ((N - n) % N);
                let w = self.omega[i] * t;
                let (s, c) = w.sin_cos();
                let ep = C::new(c, s); // e^{iωt}
                let em = C::new(c, -s); // e^{−iωt}
                // ĥ(k,t) = h0(k)·e^{iωt} + conj(h0(−k))·e^{−iωt}
                let h = self.h0[i].mul(ep).add(self.h0[mi].conj().mul(em));
                hk[i] = h;
                let kx = k_of(m, self.params.tile_size);
                let kz = k_of(n, self.params.tile_size);
                let kmag = (kx * kx + kz * kz).sqrt().max(1e-6);
                // horizontal displacement spectrum: D = −i (k/|k|) ĥ
                dx[i] = C::new(0.0, -kx / kmag).mul(h);
                dz[i] = C::new(0.0, -kz / kmag).mul(h);
            }
        }
        fft_2d(&mut hk, N, true);
        fft_2d(&mut dx, N, true);
        fft_2d(&mut dz, N, true);
        // Centred-k convention → multiply by (−1)^(m+n) to undo the fftshift; the
        // linear amplitude gain lifts the near-flat raw field to world scale.
        let gain = self.params.amplitude.max(0.0) * AMP_GAIN;
        for m in 0..N {
            for n in 0..N {
                let i = m * N + n;
                let sign = if (m + n) & 1 == 0 { gain } else { -gain };
                height[i] = hk[i].re * sign;
                dispx[i] = dx[i].re * sign * lambda;
                dispz[i] = dz[i].re * sign * lambda;
            }
        }
    }
}

/// The FFT ocean: the CPU spectrum + an `Rgba16Float` tile texture updated each
/// frame. The terrain water shader binds `view()`/`sampler()` and tiles it.
pub struct Ocean {
    spec: Spectrum,
    seed: u32,
    // scratch fields (reused each frame)
    height: Vec<f32>,
    dispx: Vec<f32>,
    dispz: Vec<f32>,
    f16: Vec<half::f16>, // packed RGBA tile
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

impl Ocean {
    pub fn new(device: &wgpu::Device, params: OceanParams, seed: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ocean-tile"),
            size: wgpu::Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FMT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ocean-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Ocean {
            spec: Spectrum::new(params, seed),
            seed,
            height: vec![0.0; N * N],
            dispx: vec![0.0; N * N],
            dispz: vec![0.0; N * N],
            f16: vec![half::f16::ZERO; N * N * 4],
            texture,
            view,
            sampler,
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Rebuild the seeded spectrum when the wind sea-state changes (cheap; only on a
    /// param change — the per-frame `update` reuses it).
    pub fn set_params(&mut self, params: OceanParams) {
        if params != self.spec.params {
            self.spec.params = params;
            self.spec.rebuild(self.seed);
        }
    }

    /// Synthesise the surface at `time` and upload the tile. Packs
    /// `(normal.x, normal.z, height, foam)`; the shader reconstructs `normal.y` and
    /// tiles the texture across the world.
    pub fn update(&mut self, queue: &wgpu::Queue, time: f32) {
        self.spec.evaluate(time, &mut self.height, &mut self.dispx, &mut self.dispz);
        let dxw = self.spec.params.tile_size / N as f32; // world spacing of a texel
        let wrap = |v: i32| ((v % N as i32 + N as i32) % N as i32) as usize;
        for m in 0..N {
            for n in 0..N {
                let i = m * N + n;
                // Surface normal from central differences of the height field.
                let hl = self.height[m * N + wrap(n as i32 - 1)];
                let hr = self.height[m * N + wrap(n as i32 + 1)];
                let hd = self.height[wrap(m as i32 - 1) * N + n];
                let hu = self.height[wrap(m as i32 + 1) * N + n];
                let dhdx = (hr - hl) / (2.0 * dxw);
                let dhdz = (hu - hd) / (2.0 * dxw);
                let nx = -dhdx;
                let nz = -dhdz;
                let nlen = (nx * nx + nz * nz + 1.0).sqrt();
                // Foam from the displacement Jacobian (folding crests → J < 1).
                let exl = self.dispx[m * N + wrap(n as i32 - 1)];
                let exr = self.dispx[m * N + wrap(n as i32 + 1)];
                let ezd = self.dispz[wrap(m as i32 - 1) * N + n];
                let ezu = self.dispz[wrap(m as i32 + 1) * N + n];
                let exu = self.dispx[wrap(m as i32 + 1) * N + n];
                let ezr = self.dispz[m * N + wrap(n as i32 + 1)];
                let jxx = (exr - exl) / (2.0 * dxw);
                let jzz = (ezu - ezd) / (2.0 * dxw);
                let jxz = (exu - exl) / (2.0 * dxw); // ∂Dx/∂z approximated
                let jzx = (ezr - ezd) / (2.0 * dxw); // ∂Dz/∂x approximated
                let jac = (1.0 + jxx) * (1.0 + jzz) - jxz * jzx;
                let foam = (1.0 - jac).clamp(0.0, 2.0);
                let o = i * 4;
                self.f16[o] = half::f16::from_f32(nx / nlen);
                self.f16[o + 1] = half::f16::from_f32(nz / nlen);
                self.f16[o + 2] = half::f16::from_f32(self.height[i]);
                self.f16[o + 3] = half::f16::from_f32(foam);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&self.f16),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((N * 4 * 2) as u32), // 4 ch × 2 bytes
                rows_per_image: Some(N as u32),
            },
            wgpu::Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_round_trips() {
        // ifft(fft(x)) ≈ x for a small deterministic signal.
        let n = 16;
        let mut a: Vec<C> = (0..n).map(|i| C::new((i as f32 * 0.7).sin(), (i as f32 * 0.3).cos())).collect();
        let orig = a.clone();
        fft_1d(&mut a, false);
        fft_1d(&mut a, true);
        for i in 0..n {
            assert!((a[i].re - orig[i].re).abs() < 1e-4, "re {i}");
            assert!((a[i].im - orig[i].im).abs() < 1e-4, "im {i}");
        }
    }

    #[test]
    fn fft_2d_round_trips() {
        let n = 8;
        let mut g: Vec<C> = (0..n * n).map(|i| C::new((i as f32 * 0.13).sin(), 0.0)).collect();
        let orig = g.clone();
        fft_2d(&mut g, n, false);
        fft_2d(&mut g, n, true);
        for i in 0..n * n {
            assert!((g[i].re - orig[i].re).abs() < 1e-3, "re {i}");
        }
    }

    #[test]
    fn phillips_is_aligned_and_falls_off() {
        let wdir = (1.0, 0.0);
        // More energy along the wind than across it.
        let along = phillips(0.05, 0.0, 14.0, wdir, 1.0);
        let across = phillips(0.0, 0.05, 14.0, wdir, 1.0);
        assert!(along > across, "along {along} across {across}");
        // Energy concentrates at low k (∝1/k⁴): small k ≫ large k.
        let low = phillips(0.02, 0.0, 14.0, wdir, 1.0);
        let high = phillips(0.4, 0.0, 14.0, wdir, 1.0);
        assert!(low > high, "low {low} high {high}");
        // Stronger wind pushes energy to larger waves (lower k) → more low-k energy.
        let calm = phillips(0.03, 0.0, 6.0, wdir, 1.0);
        let storm = phillips(0.03, 0.0, 20.0, wdir, 1.0);
        assert!(storm > calm, "storm {storm} calm {calm}");
    }

    #[test]
    fn field_has_visible_world_scale_waves() {
        // Regression for the "invisible flat-mirror ocean" bug: with the amplitude
        // gain the surface must have real slope (else it just mirrors the sky and
        // reads as no waves at all).
        let s = Spectrum::new(OceanParams::default(), 0x0cea_1234);
        let (mut h, mut dx, mut dz) = (vec![0.0; N * N], vec![0.0; N * N], vec![0.0; N * N]);
        s.evaluate(3.0, &mut h, &mut dx, &mut dz);
        let rms = (h.iter().map(|v| v * v).sum::<f32>() / (N * N) as f32).sqrt();
        let dxw = OceanParams::default().tile_size / N as f32;
        let mut slope_max = 0.0f32;
        for m in 0..N {
            for n in 1..N - 1 {
                slope_max = slope_max.max(((h[m * N + n + 1] - h[m * N + n - 1]) / (2.0 * dxw)).abs());
            }
        }
        assert!(rms > 1.0, "ocean too flat to see: RMS {rms}");
        assert!(slope_max > 0.3, "ocean slope too shallow (near-flat mirror): {slope_max}");
    }

    #[test]
    fn field_is_deterministic_and_bounded() {
        let p = OceanParams::default();
        let s = Spectrum::new(p, 1234);
        let (mut h1, mut dx1, mut dz1) = (vec![0.0; N * N], vec![0.0; N * N], vec![0.0; N * N]);
        let (mut h2, mut dx2, mut dz2) = (vec![0.0; N * N], vec![0.0; N * N], vec![0.0; N * N]);
        s.evaluate(2.0, &mut h1, &mut dx1, &mut dz1);
        // Same spectrum + time → identical field (deterministic).
        let s2 = Spectrum::new(p, 1234);
        s2.evaluate(2.0, &mut h2, &mut dx2, &mut dz2);
        for i in 0..N * N {
            assert!((h1[i] - h2[i]).abs() < 1e-6, "nondeterministic at {i}");
            assert!(h1[i].is_finite() && dx1[i].is_finite() && dz1[i].is_finite());
        }
        // A different seed gives a different surface (the field actually varies).
        let s3 = Spectrum::new(p, 9999);
        s3.evaluate(2.0, &mut h2, &mut dx2, &mut dz2);
        let diff: f32 = (0..N * N).map(|i| (h1[i] - h2[i]).abs()).sum();
        assert!(diff > 1e-3, "seed had no effect: {diff}");
    }
}
