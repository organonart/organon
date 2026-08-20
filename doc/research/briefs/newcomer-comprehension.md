---
id: newcomer-comprehension
title: Can a capable stranger land a change here?
one_line: A cold read that uses the model as the instrument and measures the documentation.
scoreable: partial
ground_truth: the tree at the dispatched commit — every stall point resolves to a real file or a real gap
cadence: every release, and after any documentation restructuring
models: at least three, from different labs
---

## Question

You have never seen this project. You have a terminal, this repository, and no one to ask.

Work through the four tasks below **in order**, reading only what you would actually reach
for. At each step, record where you went, what you read, and — the part that matters —
**where you stalled, and what you had to guess.**

1. **Orient.** In five minutes of reading, what is this, what does it produce, and how do
   you run it? Say what you concluded and from which files.
2. **Add a parameter.** A new user-facing control that reaches the visual output. Name
   every file you would have to touch, in order, and what you would write in each. The
   project says this is a chain, not a line, and that following only part of it produces a
   parameter that exists and does nothing. Find the chain yourself before checking whether
   a document already lists it — then say whether the document's list is complete.
3. **Add a generator.** A new motion algorithm that shows up in the UI, in the CLI, and in
   the generated reference documentation. Same output: the ordered file list, and the step
   you are least sure about.
4. **Verify your work.** You have no GPU. What exactly can you prove, what can you not
   prove, and what would you have to say honestly in the pull request?

## Scope

Read like a newcomer, not like an auditor. **Reach for what a reasonable person reaches
for** — `README.md`, then whatever it points at — and record the order. Do not
pre-emptively read all five architecture documents; if you needed one and did not know it
existed, that is the finding.

Note as you go: every moment you had to search for something a document should have told
you, every term used before it is defined, and every point where two documents sent you to
different places.

## Method

Keep a **trace**. It is the primary artifact of this brief, and it is more valuable than
your conclusions:

```
step · what I wanted · where I looked · found it? · what I did next
```

Three things to be honest about, because they are what this brief actually measures:

- **Where you guessed.** If you produced a plausible file list partly from pattern-matching
  on how Rust projects usually look rather than from this repository, say so and mark it.
  A confident wrong answer here is precisely the failure this brief exists to catch.
- **What you already knew.** If you have seen this project or its algorithm before, say so
  in `## What I could not determine`. It changes how the result should be read.
- **Where the documentation was right.** The point is not to find fault. If a document
  answered a question cleanly, record that too — it is how anyone can tell whether a later
  restructuring helped or hurt.

## Deliverable

Beyond the standard output contract, include:

- The full trace table for all four tasks.
- **Stall points**, ranked by how long you were stuck: what you were looking for, where
  you eventually found it, and where you first looked.
- Your file lists for tasks 2 and 3, each marked `traced` (you found it in the tree) or
  `guessed` (it seemed likely).
- **"The document I needed and did not know existed."** If there wasn't one, say so.

Mark a claim `verified` only when you opened the file. Everything from the shape of the
project rather than its contents is `inferred` at best.

---

**How this one is read.** The subject is the documentation; the model is the instrument.
The signal is in **the spread**: a stall that every model hits is a documentation defect
and gets a fix. A stall only one model hits is usually about that model and gets recorded,
not acted on. That distinction is the whole reason the same brief goes to several models,
and it is why a single run of this brief tells you much less than three.
