### A video end card, rendered from the site rather than drawn beside it

`site/endcard.html` + `site/endcard.png` — the still a video cuts to before it fades out. 16:9,
3840×2160, the shell ground and its dot grid, the mark and tracked wordmark, the logline in italic
serif closed by the one teal cursor, and a footed rule carrying **organon.art**,
**github.com/organonart/organon** and the colophon's own *come to the bench — contributions welcome*.

📌 **It is a sibling of `og.html`, not an extension of it**, and it is HTML for the same reason the
link card is: a frame nobody can re-render is a frame that goes stale silently. Headless Chrome is
already on any machine that can look at the site, there is still no image toolchain here, and none
is wanted.

⚠️ **Three things differ from the link card, each because the medium differs.** It is 16:9, because
a video frame is and an `og:image` is not. The address is set large and in bone rather than small
and in titanium — on a card `organon.art` is a footnote, on a screen somebody is watching it is the
thing they are meant to type. And it carries the repository and an invitation, which the card has
no room for.

⚠️ **Nothing in the foot is set below 19px, and that floor is the medium talking, not taste.** A
video frame is re-encoded before anybody sees it: at 720p every length is multiplied by 0.67, and
tracked uppercase at 16px came back out of that mushy. The card can afford 15px because it is
served as pixels and never re-encoded. The first cut used the card's sizes directly, which is the
obvious move and the wrong one.

🚨 **It carries its own copy of the palette**, on exactly `og.html`'s terms — a rendered frame is a
flat file and cannot read a custom property at playback time. `site/README.md` called `og.html`
**"the only real duplication on the site"**, and that is now false; it was already loose, since
`docs.html` copies the token block too. Replaced with the claim that survives adding another one:
*anything the site renders to a flat file has its own copy of these colours.* A count in prose is
wrong from the commit after it is written — this change is the commit after.

⚠️ **The committed PNG was rendered one step down both font stacks** — Bitstream Charter for the
serif, not Source Serif 4 — because type here is system-only and this repository ships neither
face. A machine with them installed produces a *better* frame and a *different* one. So: render
every cut of a given video on the same machine, or the end card changes between takes for a reason
nobody will look for.
