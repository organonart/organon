# Doc images

Illustrations for the documents in `doc/`. Referenced by relative path from the doc that owns
them — `![…](images/<name>.png)` — never hotlinked.

⚠️ **There is no Git LFS in this repository** (`git lfs env` is empty; `site/og.png` and
`site/endcard.png` are the only other binaries in the tree). Everything here lands as a plain
blob in history and can never be made smaller retroactively, so **resize before committing** —
under ~1 MB each, and prefer PNG for flat/diagrammatic art, JPEG for photographic renders.

Name after the owning document plus the plate: `pbr-text-<plate>.png` for
`doc/pbr_text_engine.md`.

⚠️ **Third-party reference imagery does not belong here.** Look references are cited by name in
the prose (see `doc/pbr_text_engine.md` §11) rather than committed — this repository is public
and someone else's work is not ours to redistribute.

## Expected plates for `doc/pbr_text_engine.md`

| File | Plate |
|---|---|
| `pbr-text-spec-sheet.png` | The spec sheet — hero render, exploded cell, material ladder |
| `pbr-text-before-after.png` | Before/after — one grid, one seam, flat ANSI left / PBR right |
| `pbr-text-resolve-arc.png` | The arc — scatter → settle → accumulate → resolved |
