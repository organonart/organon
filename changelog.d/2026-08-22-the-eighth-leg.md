### The verification bar had a second hole, one crate over, in the crate both repositories depend on

🚨 **Nothing in the seven-leg bar ran `organon-module`.** Legs 1–2 cover `organon-console` and
`organon-core`; legs 3–7 cover the root crate. `organon-module` — **82 tests** — was never executed
by any of them.

⚠️ **That is leg 7's failure exactly, reached one crate over, and after the same class had already
been found and closed once.** A change landing in the contract crate could report *"the bar is
green"* in perfect good faith with none of its own tests run — and this is the crate a module's
own repository pins, so it is the one where a silent gap travels furthest.

📌 Found the way the first hole was: by a worker whose new tests were entirely in that target, and
who said so rather than reporting the seven as green. Independently corroborated during tonight's
post-merge gate, where `organon-module` had to be run as an *extra* precisely because no leg
covered it.

The eighth leg is `cargo test -p organon-module --all-features` — the wider net rather than
`--features wgpu`, and safe under `CARGO_PROFILE_TEST_OPT_LEVEL=0` because the two timing-shaped
staleness tests in that crate are `#[ignore]`d and never run.

⚠️ **Both published copies move together, and the hook that pins them was mutation-tested rather
than trusted.** Changing one copy alone produces `‼️ The verification bar has forked.` with a diff
naming which file a worker is handed. That check is the only reason a bar duplicated on purpose is
a copy rather than a fork — and this bar has already forked once, circulating in a six-command form
for months while `CONTRIBUTING.md` was right.
