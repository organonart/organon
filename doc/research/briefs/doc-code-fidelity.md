---
id: doc-code-fidelity
title: Do the documents describe the code that actually exists?
one_line: A drift audit with ground truth in the tree — the one brief that scores the models as well as the repo.
scoreable: yes
ground_truth: the working tree at the dispatched commit; every claim resolves to a file, a line, or a command
cadence: every release, and on demand after any large refactor
models: at least three, from different labs
---

## Question

This project's documentation is unusually load-bearing. `ARCHITECTURE.md` is injected into
every AI session before anything else happens; `CLAUDE.md` is auto-loaded; agents act on
both without re-deriving them. Some pages under `doc/reference/` are generated from the
Rust and guarded by a test, so they cannot drift. **Everything else is prose, maintained
by discipline, and discipline is exactly the thing that fails quietly.**

So: **where has the documentation drifted from the code?**

Find, for the commit named in the fact pack:

1. **False claims** — a document asserts something the tree contradicts. Highest value.
2. **Stale counts and lists** — a number, table or file list that was right once. The
   documents warn against these repeatedly and count them as a known weakness; find out
   whether the warning is being obeyed.
3. **Orphaned references** — a path, module, function, binary or command that no longer
   exists under that name.
4. **Undocumented load-bearing behaviour** — something in the code that a newcomer must
   know and no document says. Rarer, and worth more than three stale counts.

## Scope

Everything checked in. Concentrate on the documents in the durable-docs table in the fact
pack, plus `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSING.md` and
`doc/guide/`.

Two exclusions, both deliberate — reporting either as a finding is a false positive:

- **`doc/reference/` is generated** by `organon docs` and a test
  (`generated_reference_is_current`) fails the build if it drifts. Do not audit it for
  drift; audit whether that guarantee holds, once, and move on.
- **Issue numbers below roughly #700, and some `doc/` paths, point into a private
  tracker.** They are unresolvable here by design and documented as such. An unresolvable
  `#N` is not a broken link.

## Method

Work from the code to the documents, not the other way around. Pick claims that are
**cheap to check and expensive to be wrong about** — the ones an agent would act on.

Good targets, in rough order of value:

- Counts of anything (crates, binaries, generators, files, modules, hooks, shaders,
  parameters, tests) stated in prose. The fact pack has measured several; check the rest.
- File-map sections that name modules — does each named file exist, at that path, and
  does what it is said to do?
- Build, test and deploy commands quoted in the documents. Do the flags exist? Do the
  feature names match the manifests? Does a named binary exist under that name?
- Any statement of the form "X is enforced by Y". Open Y. Does it enforce X?
- Cross-document contradictions: two documents making incompatible claims about the same
  thing. These are worth more than either being wrong alone.

For every finding, quote the document line and the contradicting source line, both with
`path:line`. **A finding without both halves is not a finding** — it will be scored as a
false positive when this report is adjudicated.

Calibrate deliberately: a report with eight findings that all hold up is worth more here
than one with thirty of which half are wrong. You are being measured on precision as well
as recall.

## Deliverable

Beyond the standard output contract, include:

- A findings table: `document:line` · what it claims · `source:line` · what is actually
  true · severity (**breaks an agent** / **misleads a reader** / **cosmetic**).
- A short section on **what you checked and found correct**, naming the areas. Absence of
  findings in an area you actually audited is a result; silence is not.
- Your own estimate of how many findings you missed, and why.

Every finding must be `verified`. If you cannot open both halves and quote them, do not
report it — put it in `## What I could not determine` instead.

---

**How this one is scored.** This brief is the eval leg with ground truth. Each claim is
adjudicated against the tree at the dispatched commit and lands in
`doc/research/FINDINGS.md` as `confirmed`, `refuted` or `open`. Two numbers come out of
that per model: **precision** (of the claims it made, how many held up) and **agreement**
(claims found by more than one model). A finding that only one model saw and that survives
adjudication is the most valuable output of this whole system; a confident claim that is
refuted is the most expensive.
