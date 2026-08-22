### The coordinator pattern has a name in the catalogue, and a half a worker can actually read

- **`coordinate-sessions` is now the implementation of a named pattern rather than a loose set
  of habits.** `doc/r&d/conversational_agent_control_surfaces.md` gains a section **VII ·
  Delegation** and **pattern 15 · Coordinated Sessions**, in the same GoF-derived template as
  the other fourteen — Intent through Failure Signature, with the skill named as its Known Use.
  The pattern was not invented for this entry: the catalogue has carried it since it was
  written, in the list of things it does not cover.

  📌 **It is recorded as closed rather than deleted from that list.** *"Multi-agent
  choreography — delegation between agents, and what a receipt means when the actor was itself
  an agent"* is struck through and annotated, because the gap was named for months before it was
  filled, and it was filled by a working practice being written down rather than by a design.
  What is still open is choreography deeper than one level: a worker that is itself a
  coordinator, verifying an artifact assembled from reports it never saw.

  The answer to the receipt question is the pattern's spine, and it is the one line worth
  carrying: **a receipt written by an agent is a claim, not evidence.** Pattern 7's receipt
  assumed a tool with a return code, not an actor that can be sincerely wrong about its own
  work — so the coordinator reads the diff, the branch and the count, and treats the report as a
  pointer to where to look.

- 🚨 **The skill's rules now reach the far side, which they did not before.** A skill under
  `.claude/skills/` is loaded by the session whose project directory holds it; a worker in its
  own worktree — or in another repository — has the *files* and not the *skill*. So every rule
  in `SKILL.md` reached a worker only by being **retyped into a brief by hand**, which made the
  whole contract rememberable rather than checkable, and the measured cost is already on the
  record: the verification bar circulated in briefs for months in a six-command form with a hole
  in it, while `CONTRIBUTING.md`'s copy was right the whole time.

  `.claude/skills/coordinate-sessions/BRIEF.md` is the worker-readable half — the bar, the
  process rules, the report-back contract, in the second person — and a brief now **cites it by
  command** instead of quoting it. That is `SKILL.md`'s own first rule ("as a *command that
  prints it*, never a description") finally applied to `SKILL.md` itself.

  ⚠️ This is the correction `CONSOLE_ARCHITECTURE.md` §1.20 already made one layer down, when it
  moved the reserved-key set **into the mapped header** rather than leaving it a `pub const`
  only linking modules could see. A hosted module deliberately does not link its host, so from
  the far side the set was rememberable, not checkable — *exactly the kind of promise that
  drifts*. The same sentence describes a worker and a coordinator.

- **The bar is duplicated on purpose, and the agreement is tested** — `organon-module`'s
  `ROW_ALIGNMENT` move applied to prose. Neither copy can go: a contributor must find the bar in
  `CONTRIBUTING.md` where the rest of the process lives, and a worker needs a file it can
  `git show` out of a checkout it already has. So `.claude/hooks/bar-agreement-check.sh` diffs
  the two command blocks on every Stop and refuses if they have forked — the ninth hook, and the
  first that watches two files for *agreement* rather than one file for freshness or coherence.

  📌 **Duplication a check pins is a copy; duplication nothing pins is a fork waiting to
  happen**, and this one has already forked once. The hook anchors on the first command rather
  than on a heading, so either file's surrounding prose can be rewritten freely — the two are
  addressed to different readers and *should* read differently. What must never differ is which
  commands you are told to run.

- ⚠️ **One self-contradiction in the skill is resolved rather than carried.** The brief template
  asked for "the verification bar, verbatim, **with the baseline numbers** so a drop is
  visible", while the section immediately below it said 🚨 *do not put expected counts in a
  brief*. Both had good reasons and the second wins: a count is stale within hours — leg 7 moved
  324 → 332 in about three hours across three merges — and a worker handed one has to decide
  whether it found a regression or an out-of-date brief. The cheap wrong answer is to assume the
  brief. It is told to measure its own baseline, by stashing rather than by remembering.
