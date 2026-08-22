### The unfurl gets a `twitter:image` and a canonical, and the apex/www split is named

The card shipped and a paste still showed nothing, so the served page was checked rather than the
repository: `og:image` resolves, `og.png` returns 2400×1260 `image/png`, `twitter:card` is
`summary_large_image`, and the declared dimensions match the file. Everything is correct. **The
cause is the unfurl cache** — the link was pasted before the card existed, and every platform
caches the result per URL for hours to days, with no invalidation when the tags change. Appending
any query string is a new URL to every cache and is the way to test it.

⚠️ **The check did surface one genuine weakness: `og:url` says `https://organon.art/` and the page
is served from `https://www.organon.art/`.** A scraper hitting the apex takes a redirect to `www`
and then reads an `og:url` pointing back at the apex. Most follow it; some treat `og:url` as
canonical and re-fetch, and a few give up. That is a coin-flip nobody should be relying on, so both
pages now carry an explicit `<link rel="canonical">` naming the apex, which states the intent
rather than leaving it to be inferred from a tag that means something slightly different.

🚨 **The real fix is a domain setting and it is not in this repository.** Vercel currently makes
`www` primary and redirects the apex to it; making the **apex** primary would remove the redirect
entirely and make the tags describe the URL that is actually served. That is one toggle in the
project's Domains panel, and it is a decision about the brand rather than about the markup, so it
is named here instead of worked around.

✏️ **`twitter:image` is added even though `og:image` is supposed to be enough.** X falls back to
the Open Graph tag, and so do most consumers — but not all of them, and a handful of unfurlers read
only the `twitter:` namespace. One line each, and it removes a category of "works everywhere except
there".
