# Organon Mind — PRD (absorbed)

> 🚨 **This document has been absorbed into `doc/organon_prd.md` and is no longer maintained.**
> Superseded 2026-08-21. It is kept as a stub rather than deleted, because it is cited by issues
> and by `MIND_ARCHITECTURE.md`, and a dangling path is worse than a redirect.

**Why it stopped being its own PRD.** `doc/organon_is_the_product.md` establishes that there is one
Organon application whose **layouts** replace what used to be the standalone visualizer, Organon
Mind and Organon Console. One product has one product definition: three PRDs for three products was
the same category error as three binaries, made one level up where it is more expensive. A PRD is
explicitly designed to be handed to a fresh session with the instruction *"implement this"*, so two
peer PRDs mean the next session correctly implements a *separate product* from an authoritative
document.

⚠️ **Nothing was thrown away.** This document's content was absorbed rather than summarized:

| Was here | Is now |
|---|---|
| §1 Vision, §5.1 posture-vs-Organon | `doc/organon_prd.md` §6.2 — the Mind layout |
| §1.2 the reverse-engineering frame, the IDA parallels, where the analogy breaks, the terminology | §6.2, largely verbatim — including *never "disassembler"* |
| §1.1 the four-stage trajectory | §1.4, generalized from Mind's arc to the product's |
| §4 design principles | §4 — the ones that generalize (honesty and provenance, linked views, progressive disclosure, reproducibility, performance, honest interventions, no anthropomorphizing) are now product-wide |
| §5.2 the dock shell, §5.4 savable workspaces | §5.1 — regions and saved layouts, which is what that shell became |
| §5.5 the colour language | §5.4 |
| §6 lenses, §7 analytics, §8 the subject | §6.2 |
| §9 functional requirements FR-1 … FR-32 | §8, **with the numbers preserved verbatim** so existing issue references still resolve |
| §10 verification bar | §9 |
| §11 non-goals | §10 |
| §13 settled decisions | §11, attributed and dated |
| §14 glossary, including the mech-interp terms | §13 |

📌 **What did not survive is one claim: that Organon Mind is a separate product with its own
posture** — *"the analytical, scientific sibling of Organon (the VST visualizer)"*. It is an
arrangement of the one application. ⚠️ Note that this file's last substantive commit was titled
*"Organon — one engine, three instruments"*, which is exactly the framing now superseded; that is
why the absorption is a rewrite of the frame rather than a move of the text.

**Where to go now**

- **`doc/organon_prd.md`** — the product definition. §6.2 is the Mind layout.
- **`MIND_ARCHITECTURE.md`** — the living state: what exists right now, plus the honesty ledger.
- **`doc/organon_is_the_product.md`** — the decision this absorption follows from.
- **`doc/watching_a_mind_think.md`** — the public statement of the honesty stance, unchanged.
- **#111** — the crate restructure that makes the code match. Not started; the words go first.
