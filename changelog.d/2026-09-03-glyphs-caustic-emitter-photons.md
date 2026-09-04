### Emitters as photon sources — a lit glyph throws its own caustic (organon#217 T8b)

T8 gave the three passes that shade a hit the emission buffer, and left a comment at
`rt_caustic`'s binding 5 saying what emission would mean for the photon pass and why it was
a tier of its own. This is that tier. `rt_caustic` now binds `emit_buf` read-only at exactly
the `@binding(6)` that comment reserved — and still does not **shade** with it. A photon's
transport is the *landing* surface's BSDF, so the shader has no `instance_emission` and its
deposit is still throughput × the receiver's albedo, both pinned by test. What it reads the
buffer for is the other end of the path: where a photon **starts**.

📌 **The gap this closes is a real one, not a nicety.** The tracer has no light list and no
next-event estimation toward an emitter — its NEE reaches the key and fill directions only —
so a lit tile is found by a cosine bounce happening to land on it. That converges for a tile
in plain view and essentially never converges for *lit tile → glass or lens → floor*, which
is the whole reason a light-tracing pass exists. Photons now leave the tiles as well as the
key light.

`cs_cdf` builds a per-frame inclusive CDF over the live emissive instances in emitted
**power** — Φ = π · A · L for world surface area A and radiance L = `emit.rgb * emit.w`, the
same product `cube.wgsl` adds, so a tile that looks twice as bright throws twice the photons
— in one workgroup, two passes over the live instances, no readback and no CPU round trip.
`cs_photon` then draws its source from the key light or from a tile in proportion to power,
and **every photon carries the same flux**, `(key_power + emitter_power) / N`: the population
splits, so each source deposits exactly its own power however the draw falls, and with
nothing emitting the expression reduces to the pre-T8b `key.w · π r² / N` term for term. The
emitter's hue rides the photon as a unit-luminance throughput (grey through its response at λ
when spectral transport is on), which is what keeps `flux` a scalar exactly as it was. A tile
is sampled by face area and then uniformly across the face; tube mode samples `cyl_mesh`'s
open wall. The deposit gate is unchanged and **shared** — a photon must have been redirected
by at least one specular event — so light going straight from a tile to the floor is still
direct light the tracer owns, and nothing is double-counted.

🚨 **There is no parameter, and there was never going to be one.** The renderer hands the
pass its emissive high-water mark — the glyph frame's instance count, and **0** on every
other frame — so the feature is on exactly while a ring is live and there is nothing to
leave switched on. At zero the CDF pass is not dispatched at all, *and* the source draw sits
inside an explicit `if (e_total > 0.0)` guard rather than being short-circuited by one: the
photon walk must consume the identical random stream it consumed before this change, or
every caustic in an ordinary Organon frame moves. ⚠️ A short-circuiting `&&` would read the
same and hide that intent, so a shader test holds the guarded line verbatim.

⚠️ **The count has to travel beside the buffer**, which is the one thing the design comment
did not anticipate: an all-zero emission buffer cannot say how long its live prefix is
without reading every entry of it, and the buffer is sized to the instance *capacity*. So
`PathTracer::trace` gained one `u32` argument and passes it through; `render.rs`'s call site
is the only place that knows the mark. Nothing else in `render.rs` moved.

Green and ready to try — **not looked at on a GPU**, and there is none here. What a session
must see is a lit ring in front of glass throwing its own colour onto the floor, and an
Organon scene with no ring looking exactly as it did.
