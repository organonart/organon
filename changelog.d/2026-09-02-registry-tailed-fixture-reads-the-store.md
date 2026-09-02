### The registry's positional-tail tests read the real module store, and went red on every clean runner

`main`'s test gate had been red since at least 2026-08-31 with two failures in
`organon-console`'s `registry::tests` — `a_trailing_open_optional_is_given_without_naming_it`
and `the_keyword_form_still_parses`, both from #209 — on Linux, macOS and Windows alike, and no
PR since had caused it. Both resolve `/viewport left 3d ascent` through the `tailed()` fixture
and were refused with *"`ascent` is not one of organon"*. On the workstation they passed, in
debug and in release, which is why nobody saw it: the set of legal producers was coming from
the **machine**.

🚨 **The fixture named its verb `console.viewport`, and that name is a key.** `Entry::from_spec`
attaches the dependent-ring hook by catalog name (`console_narrow`), on purpose — the pane
builds its own registry from the specs it is handed and could not have been given a builder
call. So the fixture got the shipped `viewport_options`, which reads
`%APPDATA%\OrganonShell\modules.json` through `ModuleRegistry::for_completion`, and `coerce`
asks that ring **before** it ever looks at the declared `ArgKind::Text`. The workstation's file
approves `ascent`, so the word was in the ring; a fresh runner has no file, so the ring was
`organon` alone and the word was refused. `dirs::data_dir()` answers on every platform, so the
hook never took its *"no data directory"* `None` branch and the declared `Text` never got to
accept the word. The tests were asserting a fact about a file in one person's roaming profile.

📌 **The suite already had the shape for this, one screen down.**
`the_producer_ring_is_offered_for_3d_and_refused_by_name_for_everything_else` and
`the_layout_verb_carries_the_ring_and_nothing_else_does` both assign a test-local hook to
`entries[0].narrow`, ringing over a registry the test owns (`registry_of(&["ascent"])`), and
say why in their own doc comments — *"over a registry this test owns rather than the store the
suite must never write to"*. `tailed()` was written without it. That hook is now a named
helper, `approved_ascent_hook`, shared by all of them, and `tailed()` installs it — so the two
tests ask exactly the question they were written to ask (may the trailing optional be given
without its name?) against a vocabulary that is the same on every machine. The assertions are
untouched.

⚠️ **The reproduction ran locally as well as on CI, and it is the discovery input that
decides.** With the fix reverted and the workstation's `modules.json` moved aside for one test
run, the two tests fail here with the CI sentence word for word; put the file back and they
pass again. The same reversal shows the third test in the group, `a_tail_given_twice_is_refused`,
had been passing on a clean runner **for the wrong reason** — its `Refused(_)` was the ring
refusing `ascent`, never the duplicate check. It now asserts the refusal names *given twice*,
which the hermetic fixture is what makes possible.

📌 **A test that reads a store the suite must never write to is machine-dependent in exactly
one direction: it passes for the person who wrote it.** The store is what made the feature
worth writing, so the author's machine is the one place the read cannot fail. Anything under
`tests` that resolves a verb with a dependent ring must install its own hook; the catalog name
is what wires the real one in, and that wiring is the feature, not the bug.
