#!/usr/bin/env python3
"""Tests for `changelog.py`.

    python3 native/tools/test_changelog.py          # or: python3 -m unittest discover native/tools

Stdlib `unittest`, no dependencies, no cargo — this tool is prose plumbing and has no
business adding a workspace member or a compile to anybody's verification bar.

**Why these tests exist at all.** The step being replaced is a merge conflict: loud,
annoying, and impossible to miss. The step replacing it is a concatenation, and a
concatenation that silently drops a fragment is strictly worse than the conflict — the
entry simply never appears and nobody is told. So the bar here is not "does it work on the
happy path" but **"can a fragment go missing"**, which is why the malformed cases assert
that `release` wrote and deleted *nothing*, not merely that it complained.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import changelog

# A minimal CHANGELOG.md with the same shape as the real one: preamble, the `## Unreleased`
# anchor, and a later `## ` section that must survive untouched.
BASE_CHANGELOG = """# Changelog

Some preamble.

---

## Unreleased

<!-- Normally empty. New entries are one file each in `changelog.d/`; see `changelog.d/README.md`. Anything that does land here is absorbed by the next release. -->

## Before this repository

### The instrument

The first thing that existed was a plugin.
"""

# House style: markers, bold, inline code, a fenced block. This is the fragment the
# concatenation has to carry through byte-for-byte.
HOUSE_STYLE_FRAGMENT = """### The console command lane leaves the plugin crate

organon#49 Tier 5a. `ConsoleOp`, `PortalCmd` and the whole sidecar wire format move out of
`cli.rs` into `organon_core::console_ops`, **unchanged**.

🚨 **It was never plugin code.** What kept the console lane upstream was its address, not a
dependency — the same finding T4c-i made about `agent.rs`.

⚠️ The camera limits came with it, and that was not in the plan.

```bash
cargo test -p organon-core
```

📌 The name is the cheapest thing in this change.
"""


class Harness(unittest.TestCase):
    """A throwaway repo: `CHANGELOG.md` plus an empty `changelog.d/`."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        changelog.write_text(changelog.changelog_path(self.root), BASE_CHANGELOG)
        changelog.fragment_dir(self.root).mkdir()
        self.addCleanup(self._tmp.cleanup)

    def frag(self, name: str, body: str) -> Path:
        p = changelog.fragment_dir(self.root) / name
        changelog.write_text(p, body)
        return p

    def changelog_text(self) -> str:
        return changelog.read_text(changelog.changelog_path(self.root))

    def fragment_names(self) -> list[str]:
        return sorted(p.name for p in changelog.fragment_dir(self.root).iterdir())


class TestEmpty(Harness):
    def test_an_empty_directory_renders_nothing_and_is_valid(self) -> None:
        self.assertEqual(changelog.render(self.root), "")
        self.assertEqual(changelog.check(self.root), [])

    def test_a_missing_directory_is_not_an_error(self) -> None:
        changelog.fragment_dir(self.root).rmdir()
        self.assertEqual(changelog.render(self.root), "")
        self.assertEqual(changelog.check(self.root), [])
        self.assertEqual(changelog.fragment_paths(self.root), [])

    def test_releasing_nothing_refuses_rather_than_emitting_an_empty_section(self) -> None:
        with self.assertRaises(ValueError) as cm:
            changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.assertIn("nothing to release", str(cm.exception))
        self.assertEqual(self.changelog_text(), BASE_CHANGELOG)


class TestOneFragment(Harness):
    def test_render_is_the_fragment_body(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA sentence.\n")
        self.assertEqual(changelog.render(self.root), "### Alpha\n\nA sentence.")

    def test_release_inserts_it_under_a_version_heading(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA sentence.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        self.assertIn("## 0.1.0 — 2026-08-14", text)
        self.assertIn("### Alpha\n\nA sentence.", text)
        # The version heading is above the entry, and both are above the older history.
        self.assertLess(text.index("## 0.1.0"), text.index("### Alpha"))
        self.assertLess(text.index("### Alpha"), text.index("## Before this repository"))

    def test_release_clears_the_directory(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA sentence.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.assertEqual(self.fragment_names(), [])


class TestSeveralFragments(Harness):
    def seed(self) -> None:
        # Deliberately created oldest-first and named so that alphabetical order and
        # creation order disagree — filesystem order must not be what decides this.
        self.frag("2026-08-12-zulu.md", "### Zulu\n\nOldest day.\n")
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nNewest day, first slug.\n")
        self.frag("2026-08-14-yankee.md", "### Yankee\n\nNewest day, last slug.\n")
        self.frag("2026-08-13-mike.md", "### Mike\n\nMiddle day.\n")

    def test_order_is_filename_descending(self) -> None:
        self.seed()
        self.assertEqual(
            [p.name for p in changelog.fragment_paths(self.root)],
            [
                "2026-08-14-yankee.md",
                "2026-08-14-alpha.md",
                "2026-08-13-mike.md",
                "2026-08-12-zulu.md",
            ],
        )

    def test_render_emits_newest_day_first_reverse_slug_within_a_day(self) -> None:
        self.seed()
        out = changelog.render(self.root)
        self.assertLess(out.index("### Yankee"), out.index("### Alpha"))
        self.assertLess(out.index("### Alpha"), out.index("### Mike"))
        self.assertLess(out.index("### Mike"), out.index("### Zulu"))

    def test_every_fragment_reaches_the_changelog(self) -> None:
        self.seed()
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        for heading in ("### Yankee", "### Alpha", "### Mike", "### Zulu"):
            self.assertIn(heading, text)
        self.assertEqual(self.fragment_names(), [])

    def test_sections_are_separated_by_exactly_one_blank_line(self) -> None:
        self.seed()
        out = changelog.render(self.root)
        self.assertNotIn("\n\n\n", out)
        self.assertIn("Newest day, last slug.\n\n### Alpha", out)


class TestHouseStyleSurvives(Harness):
    def test_markers_and_fences_reach_the_changelog_verbatim(self) -> None:
        self.frag("2026-08-14-house.md", HOUSE_STYLE_FRAGMENT)
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        self.assertIn(HOUSE_STYLE_FRAGMENT.strip(), text)
        for marker in ("🚨", "⚠️", "📌"):
            self.assertIn(marker, text)
        self.assertIn("```bash\ncargo test -p organon-core\n```", text)

    def test_the_result_still_has_balanced_fences(self) -> None:
        # The property `.claude/hooks/doc-coherence.sh` checks on durable docs. A fenced
        # fragment must not leave CHANGELOG.md with an odd count.
        self.frag("2026-08-14-house.md", HOUSE_STYLE_FRAGMENT)
        self.frag("2026-08-13-second.md", "### Second\n\n```rust\nlet x = 1;\n```\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        fences = sum(1 for ln in self.changelog_text().splitlines() if ln.startswith("```"))
        self.assertEqual(fences % 2, 0)
        self.assertEqual(fences, 4)


class TestCheck(Harness):
    def test_readme_is_not_a_fragment_and_not_an_error(self) -> None:
        self.frag("README.md", "# How to record a change\n")
        self.assertEqual(changelog.check(self.root), [])
        self.assertEqual(changelog.render(self.root), "")

    def test_a_misnamed_file_is_reported_not_ignored(self) -> None:
        self.frag("notes.md", "### Something\n\nReal content.\n")
        findings = changelog.check(self.root)
        self.assertEqual(len(findings), 1)
        self.assertIn("notes.md", findings[0])
        self.assertIn("not a fragment name", findings[0])

    def test_an_uppercase_slug_is_rejected(self) -> None:
        self.frag("2026-08-14-Alpha.md", "### Alpha\n\nBody.\n")
        self.assertTrue(any("not a fragment name" in f for f in changelog.check(self.root)))

    def test_an_impossible_date_is_rejected(self) -> None:
        self.frag("2026-02-31-alpha.md", "### Alpha\n\nBody.\n")
        self.assertTrue(any("not a real date" in f for f in changelog.check(self.root)))

    def test_an_empty_fragment_is_rejected(self) -> None:
        self.frag("2026-08-14-alpha.md", "\n\n   \n")
        self.assertTrue(any("empty" in f for f in changelog.check(self.root)))

    def test_a_fragment_without_a_level_three_heading_is_rejected(self) -> None:
        self.frag("2026-08-14-alpha.md", "Just a paragraph, no heading.\n")
        self.assertTrue(any("must open with a `### ` heading" in f for f in changelog.check(self.root)))

    def test_a_level_two_heading_inside_a_fragment_is_rejected(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nBody.\n\n## Sneaky\n\nMore.\n")
        self.assertTrue(any("`## ` heading" in f for f in changelog.check(self.root)))

    def test_odd_fences_are_rejected(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\n```bash\ncargo test\n")
        self.assertTrue(any("odd" in f for f in changelog.check(self.root)))

    def test_a_directory_inside_changelog_d_is_reported(self) -> None:
        (changelog.fragment_dir(self.root) / "nested").mkdir()
        self.assertTrue(any("directories are not fragments" in f for f in changelog.check(self.root)))


class TestReleaseRefusesOnBadInput(Harness):
    def test_a_misnamed_file_blocks_the_release_and_nothing_is_touched(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nGood.\n")
        self.frag("notes.md", "### Something\n\nMis-named.\n")
        with self.assertRaises(ValueError) as cm:
            changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.assertIn("notes.md", str(cm.exception))
        # The valid fragment is still on disk and CHANGELOG.md is untouched — the whole
        # point: a partial release that swallowed one entry would be the silent failure.
        self.assertEqual(self.fragment_names(), ["2026-08-14-alpha.md", "notes.md"])
        self.assertEqual(self.changelog_text(), BASE_CHANGELOG)

    def test_a_missing_unreleased_anchor_is_an_error(self) -> None:
        changelog.write_text(changelog.changelog_path(self.root), "# Changelog\n\nNo anchor.\n")
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nGood.\n")
        with self.assertRaises(ValueError) as cm:
            changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.assertIn("## Unreleased", str(cm.exception))
        self.assertEqual(self.fragment_names(), ["2026-08-14-alpha.md"])


class TestTransition(Harness):
    """`## Unreleased` is the tool's second input, permanently — see changelog.py's header."""

    def with_stray_prose(self) -> None:
        text = BASE_CHANGELOG.replace(
            "## Before this repository",
            "### Written before the fragments existed\n\nAn entry from an older branch.\n\n"
            "## Before this repository",
        )
        changelog.write_text(changelog.changelog_path(self.root), text)

    def test_stray_unreleased_prose_is_absorbed_below_the_fragments(self) -> None:
        self.with_stray_prose()
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA fragment.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        self.assertIn("### Written before the fragments existed", text)
        # Fragments are newer, so they sit above — the file is newest-first.
        self.assertLess(text.index("### Alpha"), text.index("### Written before"))
        self.assertLess(text.index("## 0.1.0"), text.index("### Alpha"))

    def test_stray_prose_alone_still_releases(self) -> None:
        self.with_stray_prose()
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.assertIn("## 0.1.0 — 2026-08-14", self.changelog_text())
        self.assertIn("### Written before the fragments existed", self.changelog_text())

    def test_the_unreleased_heading_and_its_note_survive_the_release(self) -> None:
        self.with_stray_prose()
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA fragment.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        self.assertIn("## Unreleased", text)
        self.assertIn("changelog.d/README.md", text)
        self.assertLess(text.index("## Unreleased"), text.index("## 0.1.0"))
        # And the note is not duplicated by absorbing it as prose.
        self.assertEqual(text.count("Normally empty."), 1)

    def test_releasing_twice_finds_nothing_the_second_time(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA fragment.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        first = self.changelog_text()
        with self.assertRaises(ValueError):
            changelog.release(self.root, "0.1.1", date="2026-08-15")
        self.assertEqual(self.changelog_text(), first)

    def test_a_second_release_stacks_above_the_first(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nFirst release.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        self.frag("2026-08-15-bravo.md", "### Bravo\n\nSecond release.\n")
        changelog.release(self.root, "0.1.1", date="2026-08-15")
        text = self.changelog_text()
        self.assertLess(text.index("## 0.1.1"), text.index("## 0.1.0"))
        self.assertLess(text.index("### Bravo"), text.index("### Alpha"))
        self.assertIn("## Before this repository", text)


class TestPreamblePreserved(Harness):
    def test_everything_outside_the_unreleased_region_is_byte_identical(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nA fragment.\n")
        changelog.release(self.root, "0.1.0", date="2026-08-14")
        text = self.changelog_text()
        head = BASE_CHANGELOG.split("## Unreleased")[0]
        tail = "## Before this repository" + BASE_CHANGELOG.split("## Before this repository")[1]
        self.assertTrue(text.startswith(head))
        self.assertTrue(text.endswith(tail))


class TestNewFragment(Harness):
    def test_it_seeds_a_heading_that_check_accepts(self) -> None:
        path = changelog.new_fragment(self.root, "A thing happened", slug="my-branch", date="2026-08-14")
        self.assertEqual(path.name, "2026-08-14-my-branch.md")
        self.assertEqual(changelog.read_text(path), "### A thing happened\n\n")
        self.assertEqual(changelog.check(self.root), [])

    def test_a_second_fragment_on_the_same_branch_and_day_gets_a_suffix(self) -> None:
        a = changelog.new_fragment(self.root, "One", slug="my-branch", date="2026-08-14")
        b = changelog.new_fragment(self.root, "Two", slug="my-branch", date="2026-08-14")
        c = changelog.new_fragment(self.root, "Three", slug="my-branch", date="2026-08-14")
        self.assertEqual([a.name, b.name, c.name], [
            "2026-08-14-my-branch.md",
            "2026-08-14-my-branch-2.md",
            "2026-08-14-my-branch-3.md",
        ])
        self.assertEqual(changelog.check(self.root), [])

    def test_it_creates_the_directory_if_it_is_missing(self) -> None:
        changelog.fragment_dir(self.root).rmdir()
        changelog.new_fragment(self.root, "A thing", slug="my-branch", date="2026-08-14")
        self.assertTrue(changelog.fragment_dir(self.root).is_dir())

    def test_an_empty_title_still_produces_a_valid_fragment(self) -> None:
        changelog.new_fragment(self.root, "", slug="my-branch", date="2026-08-14")
        self.assertEqual(changelog.check(self.root), [])

    def test_it_refuses_rather_than_guessing_when_the_branch_is_unknown(self) -> None:
        original = changelog.current_branch
        changelog.current_branch = lambda root: None  # type: ignore[assignment]
        try:
            with self.assertRaises(ValueError) as cm:
                changelog.new_fragment(self.root, "A thing", date="2026-08-14")
            self.assertIn("--slug", str(cm.exception))
        finally:
            changelog.current_branch = original  # type: ignore[assignment]


class TestSlugify(unittest.TestCase):
    def test_branch_names_become_filename_safe(self) -> None:
        self.assertEqual(changelog.slugify("claude/practical-pare-f3bab5"), "claude-practical-pare-f3bab5")
        self.assertEqual(changelog.slugify("feat/Console_Ops"), "feat-console-ops")
        self.assertEqual(changelog.slugify("  Spaces  and  Punctuation!  "), "spaces-and-punctuation")

    def test_the_documented_collision_really_is_a_collision(self) -> None:
        # Pinning the residual case changelog.py's header admits to, so that a future
        # change to `slugify` cannot quietly claim to have fixed it without saying so.
        self.assertEqual(changelog.slugify("feat/foo"), changelog.slugify("feat-foo"))

    def test_it_is_truncated_and_never_ends_in_a_hyphen(self) -> None:
        out = changelog.slugify("x" * 200)
        self.assertEqual(len(out), 60)
        out = changelog.slugify("a" * 59 + "/b")
        self.assertFalse(out.endswith("-"))


class TestCli(Harness):
    def test_check_exits_zero_when_clean_and_one_when_not(self) -> None:
        self.frag("2026-08-14-alpha.md", "### Alpha\n\nBody.\n")
        self.assertEqual(changelog.main(["--repo", str(self.root), "check"]), 0)
        self.frag("bad.md", "### Bad\n\nBody.\n")
        self.assertEqual(changelog.main(["--repo", str(self.root), "check"]), 1)

    def test_release_exits_one_rather_than_raising(self) -> None:
        self.assertEqual(changelog.main(["--repo", str(self.root), "release", "0.1.0"]), 1)
        self.assertEqual(self.changelog_text(), BASE_CHANGELOG)

    def test_render_exits_zero_on_an_empty_directory(self) -> None:
        self.assertEqual(changelog.main(["--repo", str(self.root), "render"]), 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
