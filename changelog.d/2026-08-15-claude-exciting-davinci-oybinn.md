### The console shows a picture and a document, from a path a human typed

`/media <path>` puts a file in a conversation tab. Two kinds, `image` (PNG and JPEG) and
`markdown`, joining `scene` and `panel` in `organon_core::kind` — the same one vocabulary both
front-ends resolve from, extended rather than sidestepped. Several paths in one line make one
exhibit with several items, which is the shape a gallery of generated candidates needs and the
one that cannot be retrofitted later without touching every kind written before it.

🚨 **A media kind names no file, and that is what made it addable at all.** The patch wire is
`patch <up> <rows> [kind]` — three positional fields and no payload slot — because the patch
protocol's whole property is that *a program which can print cannot drive the machine*. A kind
carrying a path would end that outright: anything able to append a line to the console's sidecar
could make the console open any file the user can read. So `Kind::Image` does not mean "this
file"; it means **the exhibit the human loaded**, exactly as `Kind::Scene` means "the scene the
console is rendering" rather than naming a generator. Both placements build their payload from
console-side state, and `/media` is a **view-lane** verb — deliberately absent from
`console_specs()` and therefore from the MCP catalog, so an agent cannot call it. #56 leaves
"how an exhibit reaches the console" open between an agent verb and sniffing tool results; this
tier picks neither, and the absence is the decision.

⚠️ **The terminal placement is honest rather than complete, and the difference is stated in the
code.** `organon console patch 0 4 image` claims its rows and draws a line saying an exhibit is
shown in a conversation tab. That is not a media-shaped exception: the invariant
`every_shared_kind_has_exactly_one_patch_arm` defends is that the CLI cannot accept a kind this
front-end then *silently* ignores, and a notice in the claimed rows is not silence. The picture
is a conversation placement here because a scene patch works only by virtue of there being
exactly one scene texture that every scene quad samples — an exhibit has a texture per item, so
painting one in a character grid means a per-patch texture ledger keyed on something a terminal
pane does not have. A companion test now pins that every kind either draws itself or says why
not, so a future kind cannot claim rows in silence.

**Nothing touches the frame thread.** Opening a file and decoding a JPEG both depend on a disk
and on somebody else's bytes, so each item is read on its own thread and the frame only ever
collects from a channel. A picture arrives a frame or more after it is asked for and shows
`reading...` until it does — the same deferral a conversation surface already has. `Failed` is a
state of its own rather than a missing entry, because a blank rectangle and a broken file must
not look alike; collapse them and a bad path reads as "still loading" forever.

**Refusals name the file and say what would have worked.** An `.mp3` is refused *by name* with
its real reason — audio needs a playback device and a player, not just a decoder — rather than
getting the answer a typo gets, because a refusal that cannot tell "I do not know this
extension" from "I know exactly what this is and have not built it" is a dead end for the person
reading it. PDF, video and LaTeX carry their own reasons in the same table. There is deliberately
**no `media` kind**: "images/mp3/pdf" is three unrelated engineering problems wearing one word,
and a kind named after the union of them would promise all three from the arm that delivers one.

📌 **Two tables that can silently disagree are now pinned together.** `image` is built
`default-features = false`, so an extension offered by `exhibit::IMAGE_EXTENSIONS` with no
matching cargo feature is not a compile error anywhere — it is a file the console accepts,
dispatches, reads off the disk and *then* fails to decode. `native/tests/exhibit_formats.rs`
encodes and decodes every offered extension in memory and fails the build if the two drift. No
fixture is committed: every image in that test is synthesised in RAM, because a repository that
gains sample media never loses it.

The eviction policy is the surfaces' own — `surfaces_to_evict` is now generic over its key, so
the two texture ledgers share one policy instead of growing two that can disagree about which
picture a long session keeps. **Documents are budgeted too, by bytes rather than by count**:
`documents_to_evict` is the weighed twin, pure and tested, and a separate function because "how
many entries fit" is unanswerable in advance when the entries are different sizes — one oversized
document therefore goes alone rather than taking its small, freshly-read neighbours with it. A
document's text is an `Arc<str>` for the same reason the map exists at all: the whole
`ExhibitContents` map is handed to the view every frame, and a `String` there meant a README
deep-copied sixty times a second for as long as it was held.

Every eviction prints a line naming what went and why, documents included, and drops the entry
rather than only the texture — which is what makes the next frame re-read the file: an exhibit
item is **a reference, never bytes**, so an eviction costs a re-read and never costs the picture.
Pictures are scaled to a 2048 px long edge before upload and files past 64 MB are refused before
the decoder is handed them, since a decoder asked for a 500 MB PNG allocates its full buffer
before anything can object.
