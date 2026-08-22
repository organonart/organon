### organon.art now deploys from this repository, and `site/README.md` says how

The Vercel project **`organon`** — the one that has always held `organon.art` — was
repointed from `organonart/organon-private` to this repository on 2026-08-22, with Root
Directory `site`, production branch `main`, framework preset Other and no build, output or
install command. 📌 **Repointing rather than creating a project is why there was no DNS
change**: the domains never left the project they were attached to, so `organon.art`,
`www.organon.art` and the `.vercel.app` aliases carried over untouched.

⚠️ **The settings live in the Vercel dashboard, not in this tree, so `site/README.md`'s
table is a mirror and says so.** Writing it down is worth the risk of drift because the
settings are invisible from here and a wrong one fails in a way that looks like a broken
page rather than a misconfiguration — a build command left over from the previous
repository fails on the first push against a directory with no `package.json`, and the
error names the site.

⚠️ **`vercel.json` has to stay inside `site/`.** Vercel reads it from the *root directory*,
which is `site`, not from the repository root. Moving it up one level silently stops
`cleanUrls` applying, and nothing today would reveal that — there is one page and it is
`index.html`. The first `/docs` link would be what broke.

⚠️ **And the Ignored Build Step cuts both ways, which is the part to remember.**
`git diff --quiet HEAD^ HEAD ./` stops a Rust-only commit redeploying a page it did not
touch, and this repository takes many commits a day of which almost none are the site. The
inverse is that **a change made anywhere else cannot deploy this page**: if a future
`/docs` is ever generated from something outside `site/`, its output must land in here or
the site will not rebuild — and the symptom is a deployment that never fires, not one that
fails.
