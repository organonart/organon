### The eighth leg was right about the bar and overclaimed about the gate

🚨 The paragraph added with leg 8 said **"Nothing ran `organon-module` at all"**. That is false, and
it was merged into both pinned copies of the bar.

**CI had been running those tests the whole time** — in release, on three platforms. Features unify
across a workspace build, and the root crate depends on `organon-module` with `features = ["wgpu"]`,
so `cargo test --workspace` compiles it with `wgpu` on and executes all 84. Verified with
`cargo tree --workspace -e features`, which lists `organon-module feature "wgpu"` alongside
`default`; `-p organon-module` alone shows only `default`, which is what makes the wrong reading so
easy to reach.

📌 **The true claim is narrower and still worth the leg**: *no leg of the bar* ran that crate, so a
worker following the documented sequence saw eight green legs with none of its own tests among them.
That is a real hole and leg 8 closes it. What it never meant is that the code was unguarded.

⚠️ Worth keeping as the shape: **"the bar had a hole" and "nothing checked this" are different
claims, and the second is the one that gets repeated.** The finding was reported accurately by the
worker who made it and by the PR that landed it — the overclaim entered in the *summary sentence*,
where a precise statement was compressed into a punchier one. Compression is where an accurate
finding turns into a false one.

📌 Caught by the same worker re-reading the trunk, who nearly filed the opposite correction first:
`organon-module`'s `gpu.rs` is `#[cfg(feature = "wgpu")]` and CI does not pass `--all-features`, so
the plausible reading was that five tests ran in neither CI nor `--workspace`. One `cargo tree` call
settled it against that reading. Checking the cheap thing first applies hardest when you think you
have found something.
