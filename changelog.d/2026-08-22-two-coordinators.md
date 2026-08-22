### Two coordinators is the same failure as two workers, one layer up

- The coordination skill gains what a night of running it taught about running it badly. Two
  coordinator sessions ruled on one tier for hours without telling anyone which decisions were
  whose; a worker collapsed them into "the coordinator", misattributed a ruling, and caught it
  itself. ⚠️ **The failure is not disagreement — it is that a worker cannot tell a ruling from a
  suggestion when two sources speak in the same register.** So: check the running sessions before
  popping a card, quote the session title a ruling came from, and tell workers to **refuse to
  arbitrate** between coordinators rather than picking.

  🚨 **Never publish your own `sessionId`.** `list_sessions` excludes the current session, so a
  coordinator cannot read its own id and anything it publishes is an inference — and the obvious
  candidate, the UUID in its own working directory, is not it. Measured: a coordinator published
  that UUID, was told once by a worker that the address did not resolve, answered by pointing at its
  title instead of fixing the fact, then published the same bad id to a second party. Give the
  title; rely on *reply on the channel the message arrived on*.

  📌 And the finding worth more than either apology: **a rule you can state fluently is not a rule
  you are applying.** One coordinator popped a duplicate card while its own skill forbade it; the
  other catalogued "an inference in the register of an observation" five times while committing five
  more. In both cases the tell was that they were *teaching* the rule at the time.

  ⚠️ Also recorded: a peer can no more *receive* a user instruction than lift one, so two agents
  cannot settle a remit between themselves — do it in practice if the work demands it, then tell the
  user and mark it provisional.
