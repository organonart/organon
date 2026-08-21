### One PRD, because there is one product — and the agent is what a layout cannot omit

`doc/organon_prd.md` is new and `doc/organon_mind_prd.md` is absorbed into it. No code moved; this
is the *"ratify the words first"* step `doc/organon_is_the_product.md` §5 puts ahead of everything
else, and #111 still owns the restructure that makes the tree match.

🚨 **Three PRDs for three products was the same category error as three binaries, made one level up
where it is more expensive.** A PRD is explicitly designed to be handed to a fresh session with the
instruction *"implement this"*, so two peer PRDs mean the next session correctly implements a
*separate product* from an authoritative document. Nothing was thrown away: the reverse-engineering
frame and the IDA parallels, the lens table, the analytics, the settled decisions and the
mech-interp glossary all survive, and **FR-1 … FR-32 keep their numbers verbatim** so existing issue
references still resolve. What did not survive is one claim — that Organon Mind is a separate
product with its own posture. ⚠️ That file's last substantive commit was titled *"Organon — one
engine, three instruments"*, which is exactly the framing now superseded.

🚨 **The correction the PRD exists to make: Organon is not the visualizer.** The music-synced
generative-math visualizer is *one thing Organon hosts* — built in, and conceptually a module like
any other. Organon is the renderer, the layout system, the command vocabulary, the agent surface
and the module system. 📌 The test for any description is whether it would still be true if the
visualizer were deleted, and the code has already made that demotion: the one-live-viewport limit
is attributed in source to *that renderer's* shared jitter phase rather than to viewports being
singular, and the region content word is `3d` rather than `world` precisely so the vocabulary would
not bake in today's only answer.

🚨 **Agent-first is stated as an invariant rather than a feature, because that is what it already
is.** Any command whose *result* would leave no `agent` region is refused, and *"nothing holding
`agent`"* is one of the eight refusals a saved layout is rejected on — a console with no agent
region is a window with nothing to talk to, and the verb that would fix it is typed at an agent.
⚠️ **And the non-goal that looks reversed by this is not**: an agent as *operator* — same verbs a
person has, bounded by approval, outranked by a hand — is the product; an agent as *character* is
still out. That is consistent with what was already settled, since building an agent *into* Organon
was superseded by giving an external agent a plain local command surface to drive.

📌 **`doc/organon_modules_plan.md` §12 names a third unit of extension: a skill.** §4's two kinds
are still right about *modules* and incomplete about *extension*, because a resident agent can be
extended by text. Its trust profile is genuinely different rather than a degree apart: a skill
contains no code and crosses no boundary of its own — it **steers an agent that already holds your
permissions** — so the control is neither source audit nor a protocol but **the approval**, which
this tree already made a real boundary by serving its MCP endpoint in-process and routing every
tool the agent calls, shell commands included, through one card. ⚠️ A skill can tell an agent what
to want; it cannot get it past the card. What that does not make it is harmless — a person who
approves fluently is the whole attack surface, and *"what changed in this skill since the one you
last read"* is §11.4's diff argument at a lower price, because a skill is only text.

⚠️ **The identity claim was spelled five different ways across the tree** — `README.md`,
`doc/equations_into_light.md`, `doc/how_organon_works.md` twice, and `CLAUDE.md`'s naming section —
which is how it came to be stale in five places at once. §1.1 of the PRD is now the single source
the others quote; `README.md` gains a pointer to it and keeps its current text, which accurately
describes what ships today and is rewritten when #111 lands. 📌 Those three docs were missing from
the move list in `doc/organon_is_the_product.md` §5 **and** from #111's, and both lists now say so.

✏️ **`doc/organon_is_the_product.md`'s status block said it was an unratified proposal while #110
and #111 both described the position as already ratified.** Two documents disagreeing about whether
a decision had been taken is the drift this tree spends its refusals preventing, so it is resolved
rather than left for a reader to arbitrate: ratified as words, not executed as code, with §5's
ordering untouched.
