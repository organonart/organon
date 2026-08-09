# Per-PR acceptance checks

**This directory is where a PR carries its own verification.** The standing suite in
`../scenes/` guards what already worked; the scenes *here* answer a different question:
**does the thing this PR added actually do what the PR claims?**

Files here are `*.scene`, same format as the standing suite, and they run only when the
deploy session passes `--pr`:

```bash
cd native && ./verify.sh --pr     # standing suite + this PR's checks
```

## Why these need no golden

A golden is a picture of a known-good past. A PR that just *added* a capability has no
known-good past to compare against — which is why goldens alone can't verify new work.

So these scenes assert **between two frames captured in the same run**:

```
#!expect same-as      <other-scene>    # must be pixel-identical
#!expect differs-from <other-scene>    # must visibly change
```

Both frames come off the same GPU seconds apart, so the comparison is exact, needs no
committed baseline, and **works the first time it runs** — on a feature that did not
exist yesterday. A scene may carry several `#!expect` lines, and may reference a scene
declared later; assertions are evaluated in a second pass after every frame is
captured, so ordering is never load-bearing.

## The pattern that matters most: bracket the claim

Invariant #6 says new capability must be **default-inert** — "dispersion 0 = today's
glass; palette `Native` = current". That is what lets tiers land one at a time over
weeks without changing anything until someone turns them on. Today it is prose that
nobody checks. Two scenes make it mechanical:

| Scene | Assertion | Catches |
|---|---|---|
| `<feature>-inert` | `same-as <baseline>` | the feature leaks at its default — every older preset silently changed |
| `<feature>-active` | `differs-from <baseline>` | the feature is wired but dead — the param reaches nothing |

You need **both**. `same-as` alone passes trivially if the feature never does anything;
`differs-from` alone says nothing about whether merging it was safe. Together they say
"off costs nothing, on does something", which is the whole contract of an inert tier.

`../scenes/06-fx-inert.scene` and `07-fx-active.scene` are a worked example against
Surface FX (#27), and they are promoted — a permanent invariant rather than one PR's.

## Writing them (cloud session)

You are writing these **for a session that cannot ask you what you meant**, so make each
scene state its claim in `#!desc` and its reasoning in comments.

1. **Name them for the issue**: `580-t2-dispersion.scene`, not `test2.scene`.
2. **Make each scene self-contained** — set generator / surface / material explicitly.
   `verify.sh` runs `organon release` between scenes, which drops parameter holds but
   **not** the mode selectors.
3. **Freeze the clock** on anything being compared: zero `rot_mod_*` (the rotation
   *speed*), the `rot_amp_*` / `trans_amp_*` / `scale_amp` oscillators, and `cam_path`
   / `cam_speed` / `cam_kick`. An A/B between two moving frames measures the animation,
   not the feature.
4. **Change one thing.** The baseline and the variant should differ only in the lines
   under test — otherwise a failure doesn't tell you which edit caused it.
5. **Say what a failure would mean**, in a comment. Some of these encode a *claim* from
   the issue rather than established behaviour; when one fails, the next session needs
   to know whether to suspect the code or the scene.

## The protocol

1. **Cloud session** builds the tier, and writes its acceptance scenes here as part of
   the same PR. The PR body says which scenes cover which claim.
2. **Deploy session** runs `./verify.sh --pr`, and posts `target/verify/report.md` to
   the PR — the frames and diffs are the evidence.
3. **On merge**, do one of two things with every scene in this directory, and never a
   third:
   - **Promote** it to `../scenes/` if it guards a lasting invariant (inertness checks
     usually qualify) and give it a golden.
   - **Delete** it if it only ever meant something during review.

   Left alone, this directory silently becomes a second, unmaintained suite that
   everyone learns to ignore. The merge is the moment to decide, because it is the last
   moment anyone remembers what the scene was for.

## What this still cannot do

It compares frames. It cannot tell you a **new** look is the *right* look — only that
it is or isn't the same as something else. The first time a feature renders, a human
still has to say "yes, that's what I meant." These scenes make everything *after* that
moment mechanical.
