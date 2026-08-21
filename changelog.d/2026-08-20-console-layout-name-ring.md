### Organon Console: `/layout load ` completes to the layouts you have actually saved

The name slot was free text on every action, so bringing an arrangement back meant remembering
what you had called it and typing it exactly. It is a ring now: `/layout load ` and
`/layout delete ` offer the layouts in the library, each option carrying what choosing it holds
(`left agent, right panel`), and a name that is not in it is refused **in the composer** — while
the words are still there to be edited — rather than after a round trip through the sidecar.
James asked for exactly this: *"When we do layout load, we should populate the list of choices
with the saved layout names."*

🚨 **`save` is not narrowed, and that asymmetry is the whole design.** `load` and `delete` name
something that must already exist, so the library is their value space. `save` takes a name a
person is **inventing** — offering the existing names there would read as a closed list, and the
same validator that makes the ring useful would then refuse every new name in the world, which is
the one thing `save` is for. So the hook reads the *action* word and answers "no opinion" for
`save` **before the library is touched at all**; the declared `ArgKind::Text` answers as usual.
That is why the narrowing is chosen by the action rather than declared on the argument, and it is
pinned by a test that types `/layout save nope` and expects it to run.

📌 **The deferral this closes said "nobody has measured it", so the measurement is what closed
it — and the measurement changed the design.** `CONSOLE_ARCHITECTURE.md` §1.15 recorded the ring
as possible and deliberately unbuilt because it is a `layouts.json` read on the candidate path.
Two facts came out of measuring it (`layout::tests::library_read_cost`, kept in the tree so the
numbers can be re-taken rather than believed). First, it is not one read: `value_candidates` asks
the ring once and then calls `settled` per candidate, and `settled` reaches the same hook again —
**n + 1** reads for a library of n. Second, the walk runs while the composer band is **drawn**, so
it is per *frame*, not per keystroke. Measured in release on organon-one, medians of three runs:
24.2 µs to read and parse a one-layout library, 24.5 µs at ten, **100.2 µs at a hundred** — which
is 10.1 ms per call against a 16.7 ms frame, spent while somebody is typing. ⚠️ A first run taken
while other builds were on the machine put n=1 *above* n=10 and n=100 at 16.4 ms; an n=1 that is
not the cheapest row is the tell that a run measured contention rather than the file.

⚠️ **So it is cached, and what invalidates it is stated rather than hoped.** `Library::save_over`
is the one path every write takes — which is why `delete` (a `remove` and then a rewrite) needs no
second call site — and it forgets the cache outright, so a layout you just saved is in the ring on
the very next frame and one you just deleted is gone from it. Everything else (a hand-edited file,
a second console) is covered by a 200 ms TTL. That is the answer to the standing objection beside
`console.layout.list`, which re-reads per call because *"a cached copy would fight a hand-edited
one and win silently"*: this one can only win for a fifth of a second, and the two **reads** —
the listing and the load path itself — still do not use it at all.

⚠️ **An empty library says so in the sentence that already existed.** With nothing saved,
`/layout load ` answers `Ring::Empty` carrying *"nothing has been saved yet — `console layout save
<name>` writes the console's current arrangement to the layout library"* — the same string the
`console.layout.list` read answers an empty library with, moved into `layout` so that two surfaces
read one sentence instead of each keeping its own. A band with nothing in it is indistinguishable
from a broken one. ⚠️ And a platform with **no data directory** answers "no opinion" rather than
"empty": telling somebody whose layouts are merely unreachable that they have saved nothing is
the mistake the listing already refuses to make.

⚠️ Two properties of the names survive into the ring, both of them things a list can quietly get
wrong. A **comma** is a legal layout name character, and each option stays its own label all the
way to the popup row — the names are never joined, so `a,b` is one option and not two. And
matching stays **exact**: `Desk` and `desk` are two layouts and the ring shows both, because
folding them would offer a name that then fails to load.
