# The `organon` command

`organon` is a small command-line tool that talks to a running Organon. It is the fastest
way to explore the instrument — faster than hunting through cards — and it is the way to
script it or hand it to an agent.

`deploy.sh` installs it along with the plugin, with shell completions. To build it alone:

```bash
cd native
cargo build --release --bin organon
```

## The loop that matters

**See → act → see.** Read the state, change one thing, look at the result. Do not assume a
change did what you meant:

```bash
organon status                       # what's loaded right now
organon set metallic 0.9             # change one thing
organon snap -o /tmp/look.png        # look at it
```

## Finding things

The tool documents itself, and the live catalog is authoritative — if it ever disagrees
with these pages, believe the tool:

```bash
organon catalog --manual      # the whole vocabulary, every entry with a description
organon describe dna          # one generator, in prose
organon describe metallic     # one parameter: kind, range, current value, meaning
organon describe helix        # one recipe: exactly what it would change
organon recipes               # the built-in starting-points
```

`catalog` and `describe` work **with Organon not running** — the descriptions are compiled
into the binary. The same content is published as the
[reference](../reference/README.md), generated from that same source.

## Making something

You need no saved presets. Two ways in:

```bash
# Start from a recipe, then tweak
organon recipe nebula --dry-run     # see what it would do
organon recipe nebula               # apply it

# Or compose from scratch
organon generator dna               # name, unambiguous substring, or ordinal
organon surface swept
organon material glass
organon set ior 1.45 roughness 0.08 glow 0.2
```

Selectors accept a name, an unambiguous substring, or an ordinal — `organon generator 2`,
`organon generator dna` and `organon generator "DNA double helix"` are the same command.

Parameters are set in **raw units**, not normalized. Ranges are in
[the parameter reference](../reference/parameters.md) and in `describe`.

Negative values work as you would expect — `organon set exposure -3.0`.

## Reading state

```bash
organon status                # generator, surface, material, tempo, transport
organon get metallic          # one value
organon get --all             # every settable parameter
organon watch --ms 100        # stream JSON, one line per tick
```

Most commands take `--json` for machine consumption.

## Letting go

Your changes ride an **override lane** — they sit on top of the sliders rather than
replacing them. Two rules follow:

- **The human always wins.** If someone moves the physical slider for a parameter you are
  holding, your hold on *that* parameter is released. That is intended; do not fight it.
- **Let go when you are done.** `organon release <id>` drops one hold, `organon release`
  drops all of them and hands control back to the editor.

## Which commands need what

This trips people up, and the two failure modes look similar but are not:

| Commands | Need |
|---|---|
| `status`, `get`, `watch` | something **writing** the snapshot — the standalone, the plugin in a host, or Shell |
| `snap`, `record`, `set`, `generator`, `surface`, `material`, `release` | the **visual window** |
| `catalog`, `describe`, `recipes`, `docs` | nothing at all |

So `organon status` failing with only a visual running is **structural, not a timing
problem** — waiting will never fix it. Start the editor. Whereas `organon snap` timing out
usually *is* timing: the visual is still coming up, or its window is covered or unfocused.
Retry that one.

## Talking to the right instance

Organon, Organon Mind and Organon Shell each use their own IPC namespace, which is what
lets them run side by side without trampling each other. `ORGANON_IPC_NS` selects it, and
the CLI reads it from **its own** environment:

```bash
export ORGANON_IPC_NS=organon-mind    # now the CLI addresses Mind
```

Export the same value the app was launched with, or you will read one product's state
while trying to steer another's. The default is `organic-math`.

You can also use this to run two plain Organons at once — give each a distinct namespace at
launch.

## Regenerating the reference

```bash
organon docs            # rewrite doc/reference/ from the compiled descriptions
organon docs --check    # report drift, change nothing, exit non-zero if stale
```

This is for contributors: the reference pages are generated, a test fails the build if they
drift, and the fix is always to edit the Rust and re-run this — never to edit the Markdown.
