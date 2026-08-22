### The link preview was broken by the apex/www redirect, not by the cache

The card shipped and a paste still showed nothing, so the served page was checked rather than the
repository: `og:image` resolves, `og.png` returns 2400×1260 `image/png`, `twitter:card` is
`summary_large_image`, and the declared dimensions match the file. Everything is correct. **The
cause is the unfurl cache** — the link was pasted before the card existed, and every platform
caches the result per URL for hours to days, with no invalidation when the tags change. Appending
any query string is a new URL to every cache and is the way to test it.

🚨 **That first diagnosis was wrong, and forcing a refresh through Telegram's @WebpageBot is what
disproved it.** A manual purge returned *no preview at all* — which is not what a stale cache looks
like. Measured with a crawler user-agent:

```
https://organon.art/og.png       308  ->  https://www.organon.art/og.png
https://www.organon.art/og.png   200      image/png, 114110 B
```

**Telegram follows the redirect for the PAGE and refuses to follow one for the IMAGE.** It fetched
the HTML through the apex→www 308 without complaint, read all six og tags, then found an
`og:image` that answered 308 rather than an image — and rendered nothing. The apex/www split was
recorded in the first draft of this entry as a theoretical weakness worth hardening against. It was
the bug.

✏️ **So every absolute URL on both pages now names `www`, the host that answers 200** — `og:url`,
`og:image`, `twitter:image` and the canonical. ⚠️ **These are coupled to a setting that is not in
this repository**, and the coupling is stated at the tags themselves: Vercel makes `www` primary,
and if that is ever flipped to the apex the redirect reverses and every one of these lines has to
flip with it *in the same change* — or the identical bug returns pointing the other way, with the
same silent symptom.

📌 **The lesson is about the diagnosis rather than the tags.** "It is the cache" explained the first
observation, was consistent with the second, and was still wrong; what settled it was fetching the
resource as a crawler instead of reasoning about who caches what. A redirect that every browser
follows invisibly is exactly the kind of thing that only fails for the one client that does not.

✏️ **`twitter:image` is added even though `og:image` is supposed to be enough.** X falls back to
the Open Graph tag, and so do most consumers — but not all of them, and a handful of unfurlers read
only the `twitter:` namespace. One line each, and it removes a category of "works everywhere except
there".
