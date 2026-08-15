//! # The exhibit — one kind, one or more items, and a source the human named
//!
//! An *exhibit* is what the console shows when it shows **media**: a picture, a document, and
//! one day a sound. It is [`crate::kind::Kind`]'s payload for the two media kinds, and it is
//! deliberately the smallest thing that can carry them — a kind, a list of items, and nothing
//! else.
//!
//! ## Why this is in the spine
//!
//! For `kind.rs`'s reason, one layer on. The kind vocabulary is shared by two front-ends in
//! two crates that cannot see each other; an exhibit is the payload *both* of them place, so
//! it has to sit where both can reach it. It is also pure by construction — a path, an
//! extension and a word — with **no IO of its own**, which is what lets the whole resolution
//! path be tested without a single fixture on disk. (#56 T4's own bar: *do not commit media
//! fixtures of any size*.)
//!
//! ## 🚨 A reference, never bytes
//!
//! An item is a **path**, not a buffer. That is a day-one constraint rather than an
//! optimisation, and it is what makes eviction free: a texture the budget drops can always be
//! re-decoded from the file it came from, so dropping one costs a re-read and never costs the
//! picture. It is also why this module does no IO — the bytes are the *decoder's* business, on
//! a thread that is not the frame's, and by then the path has already been through here.
//!
//! ## 🚨 Where a path may come from
//!
//! **A human's typed command, and nowhere else.** Not a tool result, not an agent's argument,
//! not a line on the patch sidecar — `kind.rs`'s `Kind` doc owns that argument, and the short
//! version is that the patch protocol's whole security property is that a program which can
//! print cannot make the console read a file. #56 leaves "how an exhibit reaches the console"
//! deliberately undecided between an agent verb and the console recognising a path in a tool
//! result; this tier chooses **neither**, because both hand path selection to something that is
//! not the person at the keyboard. When that question is decided, it is decided here.
//!
//! ## What is refused, and why refusing loudly is the feature
//!
//! [`ALL_EXTENSIONS`] is the whole of what this build can show. Everything else is refused —
//! but an audio or PDF file is refused **by name**, saying that the console knows what the file
//! is and that this build does not show it yet, because *"a `media` kind that accepts every
//! path and silently fails on most is worse than one that supports images and says so"*
//! (James, 2026-08-15). A refusal that cannot tell "I do not know this extension" from "I know
//! exactly what this is and have not built it" is a dead end for the person reading it.
//!
//! ⚠️ **There is deliberately no `media` kind**, and `kind.rs`'s own refusal test asserts the
//! word resolves to nothing. "Images/mp3/pdf" is three unrelated engineering problems wearing
//! one word; naming a kind after the union of them would promise all three from the arm that
//! delivers one.

use crate::kind::Kind;
use std::fmt;
use std::path::{Path, PathBuf};

/// The file extensions [`Kind::Image`] answers to, lowercase and without the dot.
///
/// ⚠️ **This is the DECODER's list, not the image format zoo.** The root crate builds `image`
/// with `default-features = false` and an explicit feature list, so a format missing there
/// fails at run time no matter what this table says. Adding one means adding the cargo feature
/// in the same change — `native/Cargo.toml` names this constant so the two are found together.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg"];

/// The file extensions [`Kind::Markdown`] answers to.
///
/// Not `txt`: a text file is not a Markdown document, and quietly running one through a
/// Markdown renderer is the kind of approximation this vocabulary refuses everywhere else.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Every extension this build can show, in the order a refusal should list them.
pub const ALL_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "md", "markdown"];

/// Extensions this build **recognises and does not show**, each with what it would take.
///
/// 📌 **Known-but-unbuilt is a different answer from unknown, and it is worth a table.** Each
/// of these is a whole engineering problem rather than a missing match arm (#56 T7): audio
/// needs a playback device, a decoder and a thread that is not the UI thread — the player is
/// the work, not the decode; PDF means pdfium or mupdf, a large C++ dependency; video needs
/// decode *plus* audio sync *plus* seeking. All of them land behind an opt-in cargo feature on
/// the `--with-llm` precedent, so the default build stays C++-free and fast.
///
/// The second field is printed straight to the person, so it is a plain-English reason and not
/// an issue number.
pub const KNOWN_UNBUILT: &[(&str, &str)] = &[
    ("mp3", "audio needs a playback device and a player, not just a decoder"),
    ("wav", "audio needs a playback device and a player, not just a decoder"),
    ("flac", "audio needs a playback device and a player, not just a decoder"),
    ("ogg", "audio needs a playback device and a player, not just a decoder"),
    ("pdf", "PDF rendering means a large C++ dependency, and it is opt-in when it lands"),
    ("mp4", "video needs decode plus audio sync plus seeking"),
    ("mov", "video needs decode plus audio sync plus seeking"),
    ("webm", "video needs decode plus audio sync plus seeking"),
    ("tex", "LaTeX has no strong pure-Rust typesetter yet"),
];

/// One thing an exhibit shows: a file, and the name to put under it.
///
/// The label is derived rather than carried — see [`Item::new`] — so nothing upstream has to
/// invent one, and two items from the same directory never end up labelled identically by
/// accident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The file this item is a reference to, exactly as the human typed it.
    ///
    /// ⚠️ **Not canonicalised, and not resolved.** Canonicalising is IO, which this module does
    /// not do; it is also how a path stops being the thing the person typed, which is the
    /// string any refusal has to quote back to be useful.
    pub path: PathBuf,
    /// What to draw under the item: the file name alone, ASCII-folded.
    pub label: String,
}

impl Item {
    /// An item from a path, labelled with its file name.
    ///
    /// ⚠️ **The label is ASCII-folded**, because it is drawn. `conversation_view`'s glyph
    /// allowlist walks every string the console paints and egui's bundled fonts carry no
    /// box-drawing and no CJK — a file named in another script would ship as tofu, which reads
    /// as the console being broken rather than as a font gap. A non-ASCII byte becomes `?`, and
    /// a name that folds away entirely falls back to a fixed word so the item is never
    /// captionless.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let raw =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let folded: String =
            raw.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' }).collect();
        let label = if folded.trim().is_empty() { "(unnamed)".to_string() } else { folded };
        Self { path, label }
    }
}

/// A kind, one or more items, and nothing else.
///
/// ⚠️ **Collections from day one, and that is not speculative generality.** Three generated
/// candidates arriving as *one exhibit with three items* is the case this exists for; a
/// gallery scroller and a tap-to-maximise grid are then presentation choices of one kind
/// rather than two features. Retrofitting "several items" onto a single-item exhibit means
/// touching every kind written before it, which is exactly the cost #56 T4 refuses to defer.
///
/// 🚨 **One kind for the whole exhibit, never a mixed bag.** A picture and a document do not
/// share a renderer, a budget or a placement, so an exhibit holding both would be two exhibits
/// wearing one name — and every consumer would have to switch per item. [`Exhibit::resolve`]
/// refuses a mixed list rather than splitting it, because splitting silently turns one thing
/// the human asked for into two things they did not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exhibit {
    /// What every item in this exhibit is.
    pub kind: Kind,
    /// At least one, in the order the human named them.
    pub items: Vec<Item>,
}

impl Exhibit {
    /// The kind a file extension names, or `None` for anything this build cannot show.
    ///
    /// Lowercased before matching — an extension is the one place a case difference is a
    /// keyboard artifact rather than a distinct name, and `PHOTO.PNG` off a camera is the
    /// common case rather than an edge one. That is not the `Kind::from_word` rule being
    /// relaxed: a *kind word* travels on a wire between two programs, where a case difference
    /// means someone built the line wrong, and a *file extension* comes off a disk.
    pub fn kind_for_extension(ext: &str) -> Option<Kind> {
        let ext = ext.to_ascii_lowercase();
        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            Some(Kind::Image)
        } else if MARKDOWN_EXTENSIONS.contains(&ext.as_str()) {
            Some(Kind::Markdown)
        } else {
            None
        }
    }

    /// The kind a path names, by its extension alone.
    ///
    /// ⚠️ **By extension, never by sniffing the bytes.** Sniffing is IO, and this module has
    /// none; more to the point, the person typed a name and the name is what they will read in
    /// a refusal. A `.png` that is secretly a JPEG fails in the decoder with the file named,
    /// which is the honest place for it to fail.
    pub fn kind_for_path(path: &Path) -> Option<Kind> {
        path.extension().and_then(|e| e.to_str()).and_then(Self::kind_for_extension)
    }

    /// Build an exhibit from the paths a human typed, or refuse with a reason naming the file.
    ///
    /// The three refusals are three genuinely different mistakes and each gets its own arm:
    /// nothing named at all, a file this build will not show, and a set of files that are not
    /// all the same kind.
    pub fn resolve(paths: &[PathBuf]) -> Result<Self, ExhibitError> {
        let Some(first) = paths.first() else {
            return Err(ExhibitError::Empty);
        };
        let kind = Self::classify(first)?;
        for path in &paths[1..] {
            let other = Self::classify(path)?;
            if other != kind {
                return Err(ExhibitError::Mixed {
                    first: first.clone(),
                    first_kind: kind,
                    offender: path.clone(),
                    offender_kind: other,
                });
            }
        }
        Ok(Self { kind, items: paths.iter().map(Item::new).collect() })
    }

    /// One path's kind, or the refusal that says why not — split out so [`Self::resolve`]'s
    /// loop reads as the one thing it is about.
    fn classify(path: &Path) -> Result<Kind, ExhibitError> {
        if let Some(kind) = Self::kind_for_path(path) {
            return Ok(kind);
        }
        let ext =
            path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        match KNOWN_UNBUILT.iter().find(|(e, _)| *e == ext) {
            Some((_, why)) => {
                Err(ExhibitError::NotYet { path: path.to_path_buf(), why: (*why).to_string() })
            }
            None => Err(ExhibitError::Unknown { path: path.to_path_buf() }),
        }
    }

    /// How many items — always at least one, which [`Self::resolve`] is what guarantees.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Always false. Present because clippy asks for it beside [`Self::len`], and because a
    /// reader who sees `len` will reach for it.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Why a set of paths is not an exhibit, in the words the person who typed them will read.
///
/// 🚨 **Every arm names the file.** A decoder's own error text ("invalid PNG signature",
/// "unexpected EOF") describes bytes rather than a decision, and lands in a console where
/// several things could have produced it. The rule this type exists to keep is the one
/// `UnknownKind` keeps one module over: a refusal says what was asked for, and what would
/// have worked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExhibitError {
    /// `/media` with no path after it.
    Empty,
    /// A file this build recognises and does not show — the interesting refusal.
    NotYet { path: PathBuf, why: String },
    /// An extension nothing in this build claims.
    Unknown { path: PathBuf },
    /// Several files that are not all the same kind.
    Mixed { first: PathBuf, first_kind: Kind, offender: PathBuf, offender_kind: Kind },
}

impl fmt::Display for ExhibitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExhibitError::Empty => {
                write!(f, "name a file to show -- this build shows {}", ALL_EXTENSIONS.join(", "))
            }
            ExhibitError::NotYet { path, why } => write!(
                f,
                "`{}` is not shown in this build: {why}. Showing {}",
                path.display(),
                ALL_EXTENSIONS.join(", ")
            ),
            ExhibitError::Unknown { path } => write!(
                f,
                "`{}` is not a file this build shows -- showing {}",
                path.display(),
                ALL_EXTENSIONS.join(", ")
            ),
            ExhibitError::Mixed { first, first_kind, offender, offender_kind } => write!(
                f,
                "one exhibit shows one kind: `{}` is {}, but `{}` is {}",
                first.display(),
                first_kind.as_word(),
                offender.display(),
                offender_kind.as_word()
            ),
        }
    }
}

impl std::error::Error for ExhibitError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    /// The two extension tables and the union must agree, or a refusal offers a word that
    /// resolves to nothing — the `kind.rs` failure, one layer down.
    #[test]
    fn the_extension_tables_and_the_union_agree() {
        let mut union: Vec<&str> =
            IMAGE_EXTENSIONS.iter().chain(MARKDOWN_EXTENSIONS.iter()).copied().collect();
        union.sort_unstable();
        let mut all = ALL_EXTENSIONS.to_vec();
        all.sort_unstable();
        assert_eq!(union, all, "ALL_EXTENSIONS is the two tables and nothing else");
        for ext in ALL_EXTENSIONS {
            assert!(Exhibit::kind_for_extension(ext).is_some(), "`{ext}` resolves to a kind");
        }
    }

    /// Nothing may be in both the shown list and the known-unbuilt list: a file cannot both
    /// render and be refused, and a table that said so would make the outcome depend on which
    /// one was consulted first.
    #[test]
    fn nothing_is_both_shown_and_refused() {
        for (ext, _) in KNOWN_UNBUILT {
            assert!(
                !ALL_EXTENSIONS.contains(ext),
                "`{ext}` is listed as shown and as not-yet-built"
            );
            assert_eq!(Exhibit::kind_for_extension(ext), None);
        }
    }

    #[test]
    fn an_extension_is_matched_regardless_of_case() {
        assert_eq!(Exhibit::kind_for_path(&p("/x/PHOTO.PNG")), Some(Kind::Image));
        assert_eq!(Exhibit::kind_for_path(&p("/x/notes.MD")), Some(Kind::Markdown));
        assert_eq!(Exhibit::kind_for_path(&p("/x/a.JpEg")), Some(Kind::Image));
    }

    #[test]
    fn a_file_with_no_extension_is_refused_rather_than_guessed() {
        assert_eq!(Exhibit::kind_for_path(&p("/x/README")), None);
        let err = Exhibit::resolve(&[p("/x/README")]).expect_err("no extension, no kind");
        assert!(matches!(err, ExhibitError::Unknown { .. }));
    }

    /// 🚨 The refusal this tier is really about: the console knows what an mp3 is and says so,
    /// rather than answering the way it answers a typo.
    #[test]
    fn a_known_unbuilt_file_is_refused_by_name_with_a_reason() {
        let err = Exhibit::resolve(&[p("/music/take.mp3")]).expect_err("audio is not built");
        let ExhibitError::NotYet { ref path, ref why } = err else {
            panic!("an mp3 is known-but-unbuilt, not unknown: {err:?}")
        };
        assert_eq!(path, &p("/music/take.mp3"));
        assert!(why.contains("playback device"), "the reason is the real one: {why}");
        let sentence = err.to_string();
        assert!(sentence.contains("take.mp3"), "the refusal names the file: {sentence}");
        assert!(sentence.contains("png"), "and what would have worked: {sentence}");
    }

    /// The other half: a genuine typo does *not* get the known-unbuilt sentence, or the two
    /// answers collapse into one and the distinction stops being worth having.
    #[test]
    fn an_unknown_extension_is_a_different_refusal_from_a_known_one() {
        let err = Exhibit::resolve(&[p("/x/thing.qqq")]).expect_err("nothing claims .qqq");
        assert!(matches!(err, ExhibitError::Unknown { .. }));
        let sentence = err.to_string();
        assert!(sentence.contains("thing.qqq"));
        assert!(!sentence.contains("playback device"), "not the audio reason: {sentence}");
    }

    #[test]
    fn every_known_unbuilt_extension_refuses_with_its_own_reason() {
        for (ext, why) in KNOWN_UNBUILT {
            let path = p(&format!("/x/file.{ext}"));
            let err = Exhibit::resolve(&[path]).expect_err("`{ext}` is not built");
            let ExhibitError::NotYet { why: got, .. } = err else {
                panic!("`{ext}` should be known-but-unbuilt")
            };
            assert_eq!(&got, why, "`{ext}` carries its own reason");
        }
    }

    #[test]
    fn several_files_of_one_kind_are_one_exhibit() {
        let ex = Exhibit::resolve(&[p("/a/one.png"), p("/a/two.jpg"), p("/a/three.jpeg")])
            .expect("three pictures are one image exhibit");
        assert_eq!(ex.kind, Kind::Image);
        assert_eq!(ex.len(), 3, "collections from day one, not one item retrofitted later");
        assert_eq!(ex.items[1].label, "two.jpg");
    }

    /// A mixed list is refused rather than split, and the refusal names **both** files —
    /// naming only the offender leaves the person guessing which kind the exhibit had become.
    #[test]
    fn a_mixed_list_is_refused_naming_both_sides() {
        let err = Exhibit::resolve(&[p("/a/pic.png"), p("/a/notes.md")])
            .expect_err("a picture and a document are two exhibits");
        let sentence = err.to_string();
        assert!(sentence.contains("pic.png"), "{sentence}");
        assert!(sentence.contains("notes.md"), "{sentence}");
        assert!(sentence.contains("image") && sentence.contains("markdown"), "{sentence}");
    }

    #[test]
    fn no_paths_at_all_is_its_own_refusal() {
        let err = Exhibit::resolve(&[]).expect_err("nothing to show");
        assert_eq!(err, ExhibitError::Empty);
        assert!(err.to_string().contains("png"), "it still says what would work");
    }

    /// The label is drawn, so it is ASCII or it is tofu. Both halves matter: the folding, and
    /// the fallback that stops a fully-folded name leaving an item captionless.
    #[test]
    fn a_label_is_ascii_and_never_empty() {
        let item = Item::new(p("/x/\u{8272}\u{5f69}.png"));
        assert!(item.label.is_ascii(), "a drawn label is ASCII: {:?}", item.label);
        assert_eq!(item.label, "??.png");
        assert_eq!(Item::new(p("/x/photo.png")).label, "photo.png", "ASCII is left alone");
        let bare = Item::new(p("/"));
        assert_eq!(bare.label, "(unnamed)", "a path with no file name still captions");
    }

    /// The path is kept verbatim, because it is what a refusal quotes back and what the
    /// decoder opens. Canonicalising it here would be IO and would lose the typed string.
    #[test]
    fn the_typed_path_survives_verbatim() {
        let ex = Exhibit::resolve(&[p("./rel/../odd//name.png")]).expect("resolves by extension");
        assert_eq!(ex.items[0].path, p("./rel/../odd//name.png"));
    }

    /// Every error arm prints something, names its file where it has one, and stays ASCII —
    /// these strings reach the console's own drawn surface.
    #[test]
    fn every_refusal_is_ascii_and_says_what_would_have_worked() {
        let errs = [
            ExhibitError::Empty,
            ExhibitError::NotYet { path: p("/a.mp3"), why: "because".into() },
            ExhibitError::Unknown { path: p("/a.qqq") },
            ExhibitError::Mixed {
                first: p("/a.png"),
                first_kind: Kind::Image,
                offender: p("/b.md"),
                offender_kind: Kind::Markdown,
            },
        ];
        for err in &errs {
            let sentence = err.to_string();
            assert!(!sentence.is_empty());
            assert!(sentence.is_ascii(), "a drawn refusal is ASCII: {sentence}");
        }
    }
}
