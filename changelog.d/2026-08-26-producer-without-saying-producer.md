### `/viewport left 3d ascent` — the trailing argument without naming it

⚠️ **Optional arguments in this catalog are keyword arguments**, so the only legal way to name a
hosted producer was `/viewport tl 3d producer ascent` — and the completer inserted that
`producer` for you, which is a word you then backspace out to type the thing you meant. Reported
by James, 2026-08-26.

`registry::positional_tail` is a narrow exception, and **the second of its two conditions came
from a failing test rather than from the argument**:

- **Exactly one optional.** With two, a bare word cannot say which it fills, and the cost of
  guessing wrong is a silently different command.
- **Its value space is open (`ArgKind::Text`).** `console.stack` also has exactly one optional —
  `region` — and the first cut made it positional too, which broke `region_line`'s
  `the_supplied_region_keyword_is_never_offered`. That failure was right: a region line edits
  *this* column and supplies that word itself, so offering the region vocabulary invites a
  second, contradicting one. A **closed** vocabulary is exactly the kind of word another surface
  may already be supplying; an open `Text` value cannot be mistaken for a keyword, because the
  keyword names are known and checked first.

📌 **The visible half is the ring**: after `/viewport left 3d ` the candidates are the approved
producers, not the word `producer`.

⚠️ **The keyword form still parses**, which is what makes this safe rather than a migration —
every stored layout and every MCP caller is untouched. Exactly one verb in the catalog meets both
conditions today: `console.viewport`. `console.module approve` has three optional `Text` args and
stays keyworded.

Both halves mutation-tested: widening the rule back to any non-`Bool` fails
`a_closed_vocabulary_tail_stays_a_keyword` **and** the pre-existing `region_line` test; dropping
the fallback in `parse_args` fails `a_trailing_open_optional_is_given_without_naming_it`.
