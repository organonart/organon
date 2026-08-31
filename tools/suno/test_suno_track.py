#!/usr/bin/env python3
"""Tests for suno_track.py — everything that can be checked without Suno.

    python3 tools/suno/test_suno_track.py

Stdlib `unittest`, no dependencies, matching the script it tests. The line
between what is covered here and what is not is the point of the file: every
network hop is out of reach from CI (and was out of reach from the sandbox this
was written in), so what is tested is the part that is pure — parsing the
cookie, assembling the headers and the Clerk query, absorbing `Set-Cookie`,
naming the output files. The remedy text on each failure is covered too, since
`doctor`'s whole value is that a failure names its own cause.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import suno_track as st  # noqa: E402


class CookieParsing(unittest.TestCase):
    def test_full_cookie_header(self):
        cookies = st.parse_cookie_header("__client=abc; ajs_anonymous_id=xyz; other=1")
        self.assertEqual(cookies["__client"], "abc")
        self.assertEqual(cookies["ajs_anonymous_id"], "xyz")

    def test_bare_jwt_is_accepted_as_the_client_cookie(self):
        # Pasting just the __client value is the common shortcut; take it.
        self.assertEqual(st.parse_cookie_header("  eyJraWQiOiJ4  ")["__client"], "eyJraWQiOiJ4")

    def test_empty_is_rejected_with_the_devtools_recipe(self):
        with self.assertRaises(st.SunoError) as caught:
            st.parse_cookie_header("   ")
        self.assertIn("empty", str(caught.exception))
        self.assertIn("Network", caught.exception.remedy)

    def test_header_without_client_names_what_it_did_find(self):
        with self.assertRaises(st.SunoError) as caught:
            st.parse_cookie_header("foo=bar; baz=qux")
        message = str(caught.exception)
        self.assertIn("no __client", message)
        self.assertIn("baz, foo", message)  # sorted, so the user can see what they copied


class HeaderAssembly(unittest.TestCase):
    def test_device_id_prefers_the_analytics_cookie(self):
        client = st.Suno({"__client": "a", "ajs_anonymous_id": "dev-1"}, clerk_js="5.0.0")
        self.assertEqual(client.device_id, "dev-1")

    def test_device_id_falls_back_to_a_uuid(self):
        client = st.Suno({"__client": "a"}, clerk_js="5.0.0")
        self.assertEqual(len(client.device_id), 36)

    def test_cookie_header_is_rebuilt_from_the_jar(self):
        client = st.Suno({"__client": "a", "ajs_anonymous_id": "d"}, clerk_js="5.0.0")
        self.assertEqual(client._headers()["Cookie"], "__client=a; ajs_anonymous_id=d")

    def test_authorization_appears_only_once_a_token_exists(self):
        client = st.Suno({"__client": "a"}, clerk_js="5.0.0")
        self.assertNotIn("Authorization", client._headers())
        client.token = "jwt"
        self.assertEqual(client._headers()["Authorization"], "Bearer jwt")

    def test_extra_headers_win(self):
        # authenticate() passes the raw __client as Authorization, overriding Bearer.
        client = st.Suno({"__client": "raw"}, clerk_js="5.0.0")
        client.token = "jwt"
        self.assertEqual(client._headers({"Authorization": "raw"})["Authorization"], "raw")

    def test_clerk_query_carries_both_pinned_versions(self):
        client = st.Suno({"__client": "a"}, clerk_js="9.9.9")
        query = client._clerk_query()
        self.assertIn("_clerk_js_version=9.9.9", query)
        self.assertIn(f"__clerk_api_version={st.CLERK_API_VERSION}", query)


class SetCookieAbsorption(unittest.TestCase):
    def test_rotated_cookie_replaces_the_old_one(self):
        client = st.Suno({"__client": "old"}, clerk_js="5.0.0")
        client._absorb_set_cookie(["__client=new; Path=/; HttpOnly; Secure"])
        self.assertEqual(client.cookies["__client"], "new")

    def test_a_malformed_header_is_skipped_not_fatal(self):
        client = st.Suno({"__client": "old"}, clerk_js="5.0.0")
        client._absorb_set_cookie(["=====", "__session=s"])
        self.assertEqual(client.cookies["__client"], "old")


class Slugify(unittest.TestCase):
    def test_punctuation_collapses_to_single_hyphens(self):
        self.assertEqual(st.slugify("Slow Doom Jazz!! (take 2)"), "slow-doom-jazz-take-2")

    def test_empty_gets_a_name_rather_than_an_empty_filename(self):
        self.assertEqual(st.slugify(""), "track")
        self.assertEqual(st.slugify("!!!"), "track")

    def test_length_is_bounded(self):
        self.assertLessEqual(len(st.slugify("word " * 40)), 48)


class _RecordingClient:
    """Stands in for Suno so _save's naming can be checked without a network."""

    def __init__(self):
        self.saved: list[str] = []

    def download(self, clip, path):
        self.saved.append(path)


def _clips(count: int) -> list[dict]:
    return [
        {
            "id": f"{index}bcdef1234567",
            "title": "My Take",
            "model_name": "chirp-v5",
            "metadata": {"duration": 120},
        }
        for index in range(count)
    ]


class OutputNaming(unittest.TestCase):
    """Suno returns two takes per request; neither may overwrite the other."""

    def _names(self, out, count):
        client = _RecordingClient()
        # _save reports each file on stdout; that is for a person, not a test run.
        with contextlib.redirect_stdout(io.StringIO()):
            st._save(client, _clips(count), argparse.Namespace(out=out))
        return client.saved

    def test_explicit_name_keeps_the_first_take_and_suffixes_the_rest(self):
        self.assertEqual(self._names("track.mp3", 2), ["track.mp3", "track-2.mp3"])

    def test_out_without_an_extension_still_gets_mp3_on_the_later_takes(self):
        # The bug this guards: rpartition on a dotless name once produced
        # "track-2.track".
        self.assertEqual(self._names("track", 2), ["track", "track-2.mp3"])

    def test_a_dot_in_a_directory_is_not_mistaken_for_an_extension(self):
        self.assertEqual(self._names("v1.2/track", 2), ["v1.2/track", "v1.2/track-2.mp3"])

    def test_three_takes_do_not_collide(self):
        self.assertEqual(
            self._names("t.mp3", 3), ["t.mp3", "t-2.mp3", "t-3.mp3"]
        )

    def test_without_out_names_come_from_title_and_clip_id(self):
        names = self._names(None, 2)
        self.assertEqual(names, ["my-take-0bcdef12.mp3", "my-take-1bcdef12.mp3"])
        self.assertEqual(len(set(names)), 2)


class Cli(unittest.TestCase):
    def test_generate_requires_a_style(self):
        with self.assertRaises(SystemExit) as caught:
            st.main(["generate"])
        self.assertEqual(caught.exception.code, 2)

    def test_a_subcommand_is_required(self):
        with self.assertRaises(SystemExit) as caught:
            st.main([])
        self.assertEqual(caught.exception.code, 2)

    def test_model_list_is_ordered_newest_first(self):
        # generate() walks this list, so a stale code must never be tried first.
        self.assertEqual(st.MODELS[0], "chirp-v5")
        self.assertEqual(st.MODELS[-1], "chirp-v3-5")


class Remedies(unittest.TestCase):
    """Every failure doctor can print must name a cause, not just a status."""

    def test_auth_failures_point_at_the_cookie(self):
        for status in (401, 403):
            self.assertIn("cookie", st.Suno._remedy_for_status(status))

    def test_payment_and_rate_limit_are_distinguished(self):
        self.assertIn("credits", st.Suno._remedy_for_status(402))
        self.assertIn("Rate limited", st.Suno._remedy_for_status(429))

    def test_an_unmapped_status_offers_no_false_remedy(self):
        self.assertIsNone(st.Suno._remedy_for_status(500))


if __name__ == "__main__":
    unittest.main(verbosity=2)
