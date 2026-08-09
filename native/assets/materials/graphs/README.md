# Procedural material graphs (`material.json`) — #472 Tier 4

A **material graph** is a declarative JSON description of a procedural material (the
#472 Tier 2/3 noise layers + derived maps). It is human- *and* agent-authorable:
write one by hand (or have the CLI agent write it), then **Load Material Graph…** in
the plugin's Material card to apply it. Loading turns materials + procedural on and
sets the params, so it renders immediately and is captured by presets like any look.

`native/deploy.sh` installs the `*.json` here into
`~/Library/Application Support/OrganicMath/materials/`, where the load dialog opens.

## Schema

All fields are optional — a partial graph takes sensible defaults, so an agent can
write just the layers it cares about.

```jsonc
{
  "version": 1,
  "name": "my_material",
  "projection": "triplanar",     // triplanar | world_planar | object_planar
  "scale": 1.0,                   // sampling tiling over the geometry
  "bake_res": 512,                // 256 | 512 | 1024
  "layers": [                     // baked in order; today the engine uses the first 2
    {
      "enabled": true,
      "noise": "fbm",             // value perlin simplex fbm turbulence ridged worley
                                  //   cells gabor curl domain_warp checker stripes
                                  //   hex brick veins
      "channel": "height",        // albedo roughness metallic height ao emissive
      "blend": "normal",          // normal add multiply overlay screen min max height
                                  //   (the first layer per channel is the base/replace)
      "scale": 6,                 // noise tiles (snaps to whole tiles → seamless)
      "rotation": 0.0,
      "offset": [0.0, 0.0],
      "octaves": 5,               // fbm/turbulence/ridged
      "lacunarity": 2.0,
      "gain": 0.5,
      "warp": 0.0,                // domain-warp amount
      "contrast": 1.0,
      "gamma": 1.0,
      "remap": [0.0, 1.0],        // input remap [low, high]
      "invert": false,
      "seed": 0,
      "gradient_low":  [0.04, 0.04, 0.05],  // albedo gradient stops (linear RGB)
      "gradient_high": [0.80, 0.76, 0.70]
    }
  ],
  "derive": {                     // the correlation principle — maps that agree
    "normal": true,               // derive a normal map (Sobel of height / albedo)
    "ao": true,                   // derive AO (cavity of height)
    "normal_from_albedo": false,  // source the normal from albedo luminance instead
    "normal_strength": 1.0,
    "ao_strength": 1.0,
    "ao_radius": 2.0
  }
}
```

## The correlation recipe

The materials read as real because the maps **agree**. The reliable pattern:

1. Bake a **height** layer (e.g. `fbm`, or `worley`/`cells` for cracked stone).
2. Optionally composite a second height layer with a `blend` (`min` carves cracks).
3. Bake an **albedo** layer (its own noise + `gradient_low`/`gradient_high`).
4. Turn on **`derive.normal` + `derive.ao`** — the normal and AO are computed from
   the height, so crevices are dark, occluded, and correctly lit for free.

See `nacre.json`, `weathered_stone.json`, `brick.json` for worked examples.
