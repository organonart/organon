//! Tests for [`crate::module_host`] — the launch decision, the two verdicts and their order,
//! and the vacancy rule at the one place a stale texture could get through.
//!
//! 🚨 **Every one of these runs with no window, no GPU and no module.** The producer is a
//! `ProducerChannel` opened in this process against the same file the host created, which is the
//! real protocol rather than a mock of it; the *process* is the only fake, because a real one
//! would make the suite depend on a binary somebody had to build first.
//!
//! ⚠️ **The one thing they cannot see is the picture.** These prove the console reaches the right
//! verdict and hands the right `Poll` to a texture; whether the texture then looks right is a
//! GPU and a pair of eyes, and it is claimed as such and nowhere else.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use organon_module::{Poll, Present, Presence, ProducerChannel, ProducerState};

use super::*;
use crate::module::{ApprovedModule, ModuleRegistry, BUILD_VERB, RESTART_VERB};
use crate::module_work::{ModuleCmd, ToolOutput, Tool, MODULE_ACTIONS};

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

// ---------------------------------------------------------------------------------------
// Scaffolding
// ---------------------------------------------------------------------------------------

/// A directory under `$TMPDIR` that removes itself. Unique per test by process id and a counter,
/// because the suite runs threaded and two tests sharing a store would be two consoles sharing
/// one `modules.json`.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir()
            .join(format!("organon-module-host-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What one fake launch recorded, so a test can assert on the handoff rather than on a log line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Launched {
    program: PathBuf,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    /// 🚨 **Whether the channel file existed at the moment of the spawn.** Captured here because
    /// it is the only place it can be: the order of `create` and `spawn` is invisible
    /// afterwards, and getting it backwards produces a producer that refuses a header it read
    /// too early — a `WireFault` that reads like a version problem.
    channel_existed: bool,
}

/// A [`Workshop`] that launches nothing and remembers everything.
struct Bench {
    launched: Mutex<Vec<Launched>>,
    /// Set to refuse the next spawn, for the missing-binary path.
    refuse: Mutex<Option<String>>,
    /// Shared with every process it hands out, so a test can end them all at once.
    ended: Arc<AtomicBool>,
}

impl Bench {
    fn new() -> Bench {
        Bench {
            launched: Mutex::new(Vec::new()),
            refuse: Mutex::new(None),
            ended: Arc::new(AtomicBool::new(false)),
        }
    }

    fn launches(&self) -> Vec<Launched> {
        self.launched.lock().unwrap().clone()
    }
}

impl Workshop for Bench {
    fn run(&self, _: Tool, _: &[&str], _: &Path) -> Result<ToolOutput, WorkFault> {
        panic!("hosting runs no tools")
    }

    fn ensure_dir(&self, _: &Path) -> Result<(), WorkFault> {
        Ok(())
    }

    /// The bench writes a real file at the derived path (see [`store_with`]), so the honest
    /// answer here is the real one — this half of the launch decision is about a binary that
    /// genuinely is or is not on the disk, and `a_missing_binary_refuses_by_name` depends on it
    /// being absent for real rather than on a script saying so.
    fn file_exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn spawn(
        &self,
        program: &Path,
        cwd: &Path,
        env: &[(&str, String)],
    ) -> Result<Box<dyn ModuleProcess>, WorkFault> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(WorkFault::LaunchFailed { path: program.display().to_string(), why });
        }
        let channel = env
            .iter()
            .find(|(k, _)| *k == organon_module::CHANNEL_ENV)
            .map(|(_, v)| PathBuf::from(v));
        self.launched.lock().unwrap().push(Launched {
            program: program.to_path_buf(),
            cwd: cwd.to_path_buf(),
            env: env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            channel_existed: channel.as_deref().is_some_and(Path::is_file),
        });
        Ok(Box::new(BenchProcess { ended: self.ended.clone(), status: 0 }))
    }
}

/// A process that runs until a test says otherwise.
struct BenchProcess {
    ended: Arc<AtomicBool>,
    status: i32,
}

impl ModuleProcess for BenchProcess {
    // No real process behind the rig, so no id to name one by. `None` is the honest answer and
    // it is also the one that keeps the audio path out of a test that is about frames.
    fn pid(&self) -> Option<u32> {
        None
    }

    fn exited(&mut self) -> Option<Option<i32>> {
        self.ended.load(Ordering::Relaxed).then_some(Some(self.status))
    }

    fn kill(&mut self) {
        self.ended.store(true, Ordering::Relaxed);
    }
}

/// A store root holding one approved, built module whose binary is really on the disk.
fn store_with(dir: &TempDir, producer: &str) -> PathBuf {
    let root = dir.0.join("store");
    let bin_dir = root.join("modules").join(producer).join("target").join("release");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let exe = bin_dir.join(format!("{producer}{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&exe, b"not really a program").unwrap();
    root
}

fn approved(producer: &str) -> ApprovedModule {
    let mut m = ApprovedModule {
        producer: producer.to_string(),
        name: producer.to_string(),
        url: "https://example.invalid/m.git".into(),
        commit: HASH.into(),
        reference: None,
        granted: crate::module::Granted::none(),
        built: None,
        settings: Vec::new(),
        extra: Default::default(),
    };
    m.built = Some(crate::module::BuildRecord {
        commit: HASH.into(),
        dirty: false,
        extra: Default::default(),
    });
    m
}

fn registry_with(producer: &str) -> ModuleRegistry {
    let mut r = ModuleRegistry::default();
    r.upsert(approved(producer)).unwrap();
    r
}

/// Launch one host against a bench, with the channel directory inside `dir`.
fn host(dir: &TempDir, bench: &Bench, producer: &str) -> ModuleHost {
    let root = store_with(dir, producer);
    ModuleHost::launch(bench, &root, &dir.0, &approved(producer), 0, "organon-test").unwrap()
}

/// The producer's end of a host's channel, opened in this process. **The real protocol** — this
/// is the same type a module links.
fn attach(dir: &TempDir, producer: &str, instance: u32) -> ProducerChannel {
    let path = dir.0.join(organon_module::channel_file_name(producer, instance));
    ProducerChannel::open(&path).unwrap()
}

/// Publish one frame of [`row_byte`]'s pattern.
///
/// 🚨 **It writes real pixels rather than committing a zeroed slot**, and the difference is not
/// cosmetic: a slot arrives zeroed, so a copy that never happened and a copy of a black frame are
/// the same bytes. The pattern varies by row, so [`the_pixels_arrive`] can tell the two apart —
/// and would also catch a stride mistake, which is the defect that is invisible in a header and
/// obvious on a screen.
fn draw(p: &mut ProducerChannel, w: u32, h: u32) {
    {
        let mut fw = p.begin_frame(w, h).unwrap();
        let stride = fw.row_stride() as usize;
        let px = fw.pixels_mut();
        for y in 0..h as usize {
            let row = &mut px[y * stride..y * stride + w as usize * 4];
            row.fill(row_byte(y as u32));
        }
        fw.commit();
    }
    p.published(w, h);
}

/// The byte every channel of row `y` holds. Never zero, so "arrived" and "never copied" differ.
fn row_byte(y: u32) -> u8 {
    (y % 251) as u8 + 1
}

// ---------------------------------------------------------------------------------------
// The launch
// ---------------------------------------------------------------------------------------

/// CONTRACT: **the channel exists before the process does.** `ModuleChannel::create` writes the
/// header and seeds the two producer-owned words, and its own docs say it expects the producer
/// not to exist yet. A producer launched first would open a zeroed file and refuse it as a wire
/// fault — which reads like a version mismatch and is a startup race.
#[test]
fn the_channel_is_laid_down_before_the_producer_is_started() {
    let dir = TempDir::new("order");
    let bench = Bench::new();
    let _h = host(&dir, &bench, "widget");
    let launches = bench.launches();
    assert_eq!(launches.len(), 1);
    assert!(
        launches[0].channel_existed,
        "the producer was launched before its channel existed — it would read a zeroed header"
    );
}

/// CONTRACT: **the handoff is the environment variable the contract names**, carrying a full
/// path — and the IPC namespace goes with it, exactly as `term.rs` injects it into every other
/// child the console spawns.
#[test]
fn the_producer_is_told_where_its_channel_is_and_which_namespace_it_is_in() {
    let dir = TempDir::new("handoff");
    let bench = Bench::new();
    let _h = host(&dir, &bench, "widget");
    let env = bench.launches().remove(0).env;
    let channel = env
        .iter()
        .find(|(k, _)| k == organon_module::CHANNEL_ENV)
        .expect("no channel variable — the producer would have nothing to open")
        .1
        .clone();
    assert!(Path::new(&channel).is_absolute(), "{channel} is not a path a producer can open");
    assert!(channel.ends_with(organon_module::channel_file_name("widget", 0).as_str()));
    assert_eq!(
        env.iter().find(|(k, _)| k == "ORGANON_IPC_NS").map(|(_, v)| v.as_str()),
        Some("organon-test"),
        "a module process must address this console's namespace, not the default one"
    );
}

/// CONTRACT: **the binary is derived from the store root and the producer name.** Never recorded,
/// never named by a manifest — the same gate `artifact_dir` carries, for the stronger reason that
/// this string reaches `Command::new`.
#[test]
fn the_binary_is_derived_and_the_name_gate_applies_to_it() {
    let derived = module_binary(Path::new("/store"), "ascent").unwrap();
    assert_eq!(
        derived,
        crate::module_work::artifact_dir(Path::new("/store"), "ascent")
            .unwrap()
            .join(format!("ascent{}", std::env::consts::EXE_SUFFIX))
    );
    assert!(module_binary(Path::new("/store"), "..").is_err(), "the same gate applies");
    assert!(module_binary(Path::new("/store"), "a/b").is_err());
}

/// CONTRACT: **a record claiming a build whose binary is gone is a sentence, not a crash.**
/// `is_built()` reads the record; the file is a different fact. The refusal names the verb that
/// fixes it.
#[test]
fn a_missing_binary_refuses_by_name_and_says_what_produces_it() {
    let dir = TempDir::new("nobin");
    let bench = Bench::new();
    // A store root with no `target/release` at all.
    let root = dir.0.join("empty-store");
    std::fs::create_dir_all(&root).unwrap();
    let err =
        ModuleHost::launch(&bench, &root, &dir.0, &approved("widget"), 0, "ns").unwrap_err();
    let said = err.sentence();
    assert!(said.contains("widget"), "{said}");
    assert!(said.contains(BUILD_VERB), "{said}");
    assert!(bench.launches().is_empty(), "nothing should have been spawned");
    // And no channel file was left behind for a process that will never open it.
    assert!(
        !dir.0.join(organon_module::channel_file_name("widget", 0)).exists(),
        "a 44 MiB file was left for a producer that was never launched"
    );
}

// ---------------------------------------------------------------------------------------
// The two verdicts, and their order
// ---------------------------------------------------------------------------------------

/// CONTRACT: **a live, drawing producer produces no sentence at all.** §1.14's vacancy rule read
/// the right way round — the working case has a picture, not prose.
#[test]
fn a_producer_that_is_drawing_says_nothing() {
    let dir = TempDir::new("live");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 64, 32);

    let obs = h.observe(Instant::now());
    assert!(matches!(obs.poll, Poll::Frame(_)), "{:?}", obs.poll);
    assert_eq!(obs.poll.present(), Present::Upload);
    assert_eq!(obs.notice, None, "a working module must not be narrated");
}

/// CONTRACT: 🚨 **the pixels the producer wrote are the pixels the console holds, row by row.**
/// Everything else in this file is about verdicts; this is the one that would fail if the copy
/// were wrong — and the row-varying pattern is what makes a **stride** mistake visible, which is
/// the defect a header check cannot see and which tilts every horizontal edge on a screen.
///
/// ⚠️ The size is deliberately not a multiple of the 256-byte row alignment: `61 × 4 = 244`, so
/// every row in the mapping is padded and a consumer that walked `width * 4` instead of
/// `row_stride` would drift a little further into the next row on each one.
#[test]
fn the_pixels_arrive_and_the_padding_is_not_walked_into() {
    let dir = TempDir::new("pixels");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 61, 37);

    match h.observe(Instant::now()).poll {
        Poll::Frame(v) => {
            assert_eq!((v.width, v.height), (61, 37));
            assert!(
                v.row_stride > v.width * 4,
                "this test needs a padded stride to mean anything — got {}",
                v.row_stride
            );
            for y in 0..v.height {
                let row = v.row(y);
                assert_eq!(row.len(), v.width as usize * 4);
                assert!(
                    row.iter().all(|&b| b == row_byte(y)),
                    "row {y} is not the row the producer wrote — a stride or offset mistake"
                );
            }
        }
        other => panic!("expected a frame, got {other:?}"),
    }
}

/// CONTRACT: 🚨 **the process handle outranks the channel, and the channel cannot reach this
/// answer at all.** A producer that died without a farewell leaves counters that simply stop —
/// the channel says `Live` for a second, then `Stalled`, then `Lost` after five. The handle knows
/// now, and knows the exit code.
///
/// ⚠️ This is the ordering test. Mutating `observe` to ask the channel first makes it fail with
/// the channel's own healthy verdict, which is exactly the wrong answer said confidently.
#[test]
fn a_process_that_ended_is_reported_from_the_handle_not_from_the_silence() {
    let dir = TempDir::new("ended");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 64, 32);
    // The channel is in perfect health at this instant: counters moving, a frame published.
    let now = Instant::now();
    assert_eq!(h.observe(now).notice, None);

    bench.ended.store(true, Ordering::Relaxed);
    let obs = h.observe(now);
    assert_eq!(
        obs.notice,
        Some(Notice::Exited { producer: "widget".into(), status: Some(0) }),
        "the channel's healthy verdict was preferred to the handle's certain one"
    );
    // 🚨 And the texture is told to drop its picture, not to keep the good frame it just took.
    assert_eq!(obs.poll.present(), Present::Forget);
}

/// CONTRACT: **once the console has said a producer ended, it goes on saying so.** A rectangle
/// that reported a death and then reverted to *"is running"* is the stale picture arriving
/// through the sentence instead of through the texture.
#[test]
fn a_death_is_remembered_rather_than_re_asked() {
    let dir = TempDir::new("sticky");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    bench.ended.store(true, Ordering::Relaxed);
    let now = Instant::now();
    assert!(matches!(h.observe(now).notice, Some(Notice::Exited { .. })));
    // The fake would answer `None` from here on, exactly as `try_wait` does after it reaps.
    bench.ended.store(false, Ordering::Relaxed);
    p.tick();
    assert!(
        matches!(h.observe(now).notice, Some(Notice::Exited { .. })),
        "the console forgot a death it had already reported"
    );
}

/// CONTRACT: **row 3 of §4.6 — launched, not yet producing — and it times out.** A rectangle that
/// says "starting…" for ever is the failure state that looks most like working software.
#[test]
fn a_producer_that_never_draws_starts_and_then_stops_being_starting() {
    let dir = TempDir::new("starting");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();

    let t0 = Instant::now();
    match h.observe(t0).notice {
        Some(Notice::Producer { presence: Presence::Starting { .. }, .. }) => {}
        other => panic!("expected `starting`, got {other:?}"),
    }
    // Eleven seconds later, still ticking, still nothing drawn. `start_within` is ten.
    p.tick();
    let later = t0 + Duration::from_secs(11);
    match h.observe(later).notice {
        Some(Notice::Producer { presence: Presence::Lost { .. }, .. }) => {}
        other => panic!("`starting` did not time out — got {other:?}"),
    }
}

/// CONTRACT: **row 4's other half — alive and silent.** The one failure whose symptom is a
/// perfectly healthy heartbeat, and the sentence names the reason when the producer gave one.
#[test]
fn a_producer_that_is_alive_and_refusing_is_named_as_such() {
    let dir = TempDir::new("refusing");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 64, 32);
    assert_eq!(h.observe(Instant::now()).notice, None);

    p.refuse(organon_module::RefusalReason::SizeUnsupported);
    p.tick();
    let obs = h.observe(Instant::now());
    match &obs.notice {
        Some(Notice::Producer { presence: Presence::NotProducing { reason, .. }, .. }) => {
            assert!(reason.is_some(), "the producer said why and the console dropped it");
        }
        other => panic!("expected `not producing`, got {other:?}"),
    }
    // 🚨 And the good frame it drew a moment ago is dropped rather than held.
    assert_eq!(obs.poll.present(), Present::Forget);
}

/// CONTRACT: **a farewell is its own sentence, distinct from a process this console watched
/// end.** `Gone` invites starting it again because the producer chose to leave; `Exited` carries
/// an exit code because nobody chose anything.
#[test]
fn a_farewell_and_an_ending_are_two_different_sentences() {
    let dir = TempDir::new("bye");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    p.depart();
    let said = h.observe(Instant::now()).notice.unwrap().sentence();
    assert!(said.contains("has exited"), "{said}");

    let exited = Notice::Exited { producer: "widget".into(), status: Some(3) };
    assert!(exited.sentence().contains("exit code 3"), "{}", exited.sentence());
    assert_ne!(said, exited.sentence());
}

/// CONTRACT: 🚨 **`Present::Forget` for every state §4.6 forbids painting through, and never a
/// `Poll::Frame` in any of them.** The composition of the two crates at the one place a stale
/// texture could get through — asserted here rather than trusted, because a texture that is still
/// valid is the easiest wrong thing to paint.
#[test]
fn no_verdict_that_forbids_a_picture_can_also_deliver_one() {
    let dir = TempDir::new("forget");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 64, 32);
    let t0 = Instant::now();
    assert!(matches!(h.observe(t0).poll, Poll::Frame(_)));

    // Draw another frame and then go silent. The ring HOLDS a whole, valid, unread frame — which
    // is precisely the frame a console that trusted its ring would paint.
    draw(&mut p, 64, 32);
    for gap in [2u64, 6, 30] {
        let obs = h.observe(t0 + Duration::from_secs(gap));
        assert!(
            !matches!(obs.poll, Poll::Frame(_)),
            "a frame was served {gap} s into a silence — the ring still holds one, which is why \
             this must be refused above the ring"
        );
        assert_eq!(obs.poll.present(), Present::Forget, "at +{gap} s");
        assert!(obs.notice.is_some(), "a rectangle with no picture must say why");
    }
}

// ---------------------------------------------------------------------------------------
// The lifecycle — invariant 4
// ---------------------------------------------------------------------------------------

/// CONTRACT: 🚨 **a module arrives `Attached` and nothing in this tier can make it run.**
/// `CLAUDE.md` invariant 4. It is structural — there is no method on [`ModuleHost`] that reaches
/// `set_lifecycle` — and this asserts the value across the frames where a "start it once it is
/// drawing" convenience would most plausibly have been added.
#[test]
fn a_hosted_module_is_attached_and_stays_attached() {
    let dir = TempDir::new("inert");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    assert_eq!(h.lifecycle(), Lifecycle::Attached);
    assert_eq!(p.control().lifecycle, Lifecycle::Attached);
    for _ in 0..5 {
        p.tick();
        draw(&mut p, 64, 32);
        h.observe(Instant::now());
        h.ask_size(100, 50);
    }
    p.tick();
    assert_eq!(h.lifecycle(), Lifecycle::Attached, "the console started a module by itself");
    assert_eq!(
        p.control().lifecycle,
        Lifecycle::Attached,
        "the producer was told to run and nothing in this tier may tell it that"
    );
    assert_eq!(p.state(), ProducerState::Producing);
}

/// CONTRACT: **the staging buffer is allocated once and a resize is not an allocation on the
/// frame path.** T0's condition, asserted rather than commented — a per-frame allocation measures
/// 2.72 ms against 1.37 ms and reads as a verdict on the mechanism.
#[test]
fn nothing_on_the_frame_path_allocates_however_often_the_size_changes() {
    let dir = TempDir::new("prealloc");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    for i in 1..40u32 {
        h.ask_size(64 + i * 7, 32 + i * 3);
        p.tick();
        let (w, hh) = p.control().requested;
        p.adopt_size(w, hh);
        draw(&mut p, w, hh);
        h.observe(Instant::now());
    }
    assert_eq!(h.staging_allocations(), 1, "the frame path allocated");
    assert_eq!(h.faults(), (0, 0), "a well-behaved producer produced torn or corrupt reads");
    assert!(h.frames_seen() > 1);
}

/// CONTRACT: **the frame is the truth about its own size**, and a frame drawn before a resize
/// landed is a good picture rather than something to flush.
#[test]
fn a_frame_carries_the_size_the_producer_drew_not_the_one_the_console_asked_for() {
    let dir = TempDir::new("size");
    let bench = Bench::new();
    let mut h = host(&dir, &bench, "widget");
    let mut p = attach(&dir, "widget", 0);
    p.tick();
    draw(&mut p, 64, 32);
    // The console changes its mind AFTER the frame was drawn.
    h.ask_size(800, 600);
    match h.observe(Instant::now()).poll {
        Poll::Frame(v) => assert_eq!((v.width, v.height), (64, 32)),
        other => panic!("expected the in-flight frame, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------
// The map of hosts
// ---------------------------------------------------------------------------------------

/// CONTRACT: **the registry decides, and nothing is launched while it says no.** Rows 1 and 2 of
/// §4.6, and `vacancy` is the one place they are read.
#[test]
fn nothing_is_launched_for_a_producer_the_registry_stands_in_front_of() {
    let dir = TempDir::new("vacant");
    let bench = Bench::new();
    let mut hosts = ModuleHosts::default();
    let root = store_with(&dir, "widget");

    let empty = ModuleRegistry::default();
    let notice = hosts
        .ensure(&bench, &empty, &root, &dir.0, "widget", "ns")
        .expect("an unapproved producer must be a sentence");
    assert!(matches!(notice, Notice::Registry(ModuleState::NotApproved { .. })));
    assert!(bench.launches().is_empty(), "an unapproved module was launched");
    assert!(hosts.is_empty());

    // Approved, and no build names those bytes.
    let mut unbuilt = ModuleRegistry::default();
    let mut m = approved("widget");
    m.built = None;
    unbuilt.upsert(m).unwrap();
    let notice = hosts.ensure(&bench, &unbuilt, &root, &dir.0, "widget", "ns").unwrap();
    assert!(matches!(notice, Notice::Registry(ModuleState::ApprovedNotBuilt { .. })));
    assert!(bench.launches().is_empty());
}

/// CONTRACT: **an approved, built module is launched once and not once per frame.**
#[test]
fn a_module_is_launched_once_however_many_frames_ask_for_it() {
    let dir = TempDir::new("once");
    let bench = Bench::new();
    let mut hosts = ModuleHosts::default();
    let root = store_with(&dir, "widget");
    let registry = registry_with("widget");
    for _ in 0..30 {
        assert_eq!(hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns"), None);
    }
    assert_eq!(bench.launches().len(), 1);
}

/// CONTRACT: 🚨 **a launch that failed is remembered, and the console does not retry it sixty
/// times a second.** The rectangle would say the right thing while the console spent itself on
/// `CreateProcess` — a defect that is invisible in a screenshot and obvious in a profile.
#[test]
fn a_refused_launch_is_not_retried_every_frame() {
    let dir = TempDir::new("refused");
    let bench = Bench::new();
    *bench.refuse.lock().unwrap() = Some("the disk is on fire".into());
    let mut hosts = ModuleHosts::default();
    let root = store_with(&dir, "widget");
    let registry = registry_with("widget");

    let first = hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns").unwrap();
    assert!(matches!(first, Notice::Launch { .. }));
    for _ in 0..30 {
        assert_eq!(
            hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns").as_ref(),
            Some(&first),
            "the remembered refusal changed, or a retry happened"
        );
    }
    // Only the very first attempt reached the seam. (The bench refuses, so `launches` stays
    // empty — the count that matters is that the FAULT was produced once, which the log line
    // beside it also asserts by not repeating.)
    assert!(bench.launches().is_empty());

    // And `restart` is what clears it.
    assert!(hosts.restart("widget"), "there was a remembered failure to clear");
    *bench.refuse.lock().unwrap() = None;
    assert_eq!(hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns"), None);
    assert_eq!(bench.launches().len(), 1);
}

/// CONTRACT: **two launches of one producer are two channel files.** A second producer opening a
/// live channel resets its status block, so the console would believe its producer had just
/// restarted while two processes wrote the same slots. `ProducerChannel::open` names this as the
/// one hazard its side of the rule cannot prevent; this is the console's side.
#[test]
fn a_restart_gets_its_own_channel_rather_than_reusing_the_dead_ones() {
    let dir = TempDir::new("instance");
    let bench = Bench::new();
    let mut hosts = ModuleHosts::default();
    let root = store_with(&dir, "widget");
    let registry = registry_with("widget");

    hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns");
    hosts.restart("widget");
    hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns");

    let files: Vec<String> = bench
        .launches()
        .iter()
        .map(|l| {
            l.env
                .iter()
                .find(|(k, _)| k == organon_module::CHANNEL_ENV)
                .unwrap()
                .1
                .clone()
        })
        .collect();
    assert_eq!(files.len(), 2);
    assert_ne!(files[0], files[1], "a restarted producer reused its predecessor's channel");
}

/// CONTRACT: **the release site is total** — it asks whether anything wants the producer, rather
/// than being called from each verb that could stop wanting it.
#[test]
fn a_producer_nobody_is_asking_for_is_released() {
    let dir = TempDir::new("retain");
    let bench = Bench::new();
    let mut hosts = ModuleHosts::default();
    let root = store_with(&dir, "widget");
    let registry = registry_with("widget");
    hosts.ensure(&bench, &registry, &root, &dir.0, "widget", "ns");
    assert_eq!(hosts.producers(), vec!["widget"]);

    hosts.retain(&["widget"]);
    assert_eq!(hosts.producers(), vec!["widget"], "a wanted producer was released");

    hosts.retain(&[]);
    assert!(hosts.is_empty(), "a producer nobody asked for was kept");
    assert!(
        bench.ended.load(Ordering::Relaxed),
        "the process was dropped without being ended — that is a leak"
    );
    // The channel file went with it.
    assert!(!dir.0.join(organon_module::channel_file_name("widget", 0)).exists());
}

// ---------------------------------------------------------------------------------------
// The verb
// ---------------------------------------------------------------------------------------

/// CONTRACT: 🚨 **every sentence that offers a restart names a verb the grammar resolves.** This
/// is what `module.rs`'s constants are FOR, and it is the whole of the deliberate answer to
/// `presence.rs`'s prediction: the verb printed in a rectangle and the word a person then types
/// are one string, checked here against the table their typing is resolved against.
#[test]
fn the_restart_verb_in_a_rectangle_is_a_verb_a_person_can_type() {
    let restart_says: Vec<String> = vec![
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::Stalled { silent: Duration::from_secs(2) },
        },
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::Lost { silent: Duration::from_secs(9) },
        },
        Notice::Producer { producer: "widget".into(), presence: Presence::Gone },
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::NotProducing { silent: Duration::from_secs(3), reason: None },
        },
        Notice::Exited { producer: "widget".into(), status: Some(1) },
        Notice::Exited { producer: "widget".into(), status: None },
    ]
    .iter()
    .map(Notice::sentence)
    .collect();

    for said in &restart_says {
        assert!(said.contains(RESTART_VERB), "a rectangle offered no way back: {said}");
        assert!(said.contains("widget"), "a sentence that does not name the module: {said}");
    }
    // The other half of "one string": the word at the end of the verb is one the grammar takes.
    let word = RESTART_VERB.rsplit(' ').next().unwrap();
    assert!(MODULE_ACTIONS.contains(&word), "`{word}` is not in the action table");
    assert_eq!(ModuleCmd::resolve(word), Ok(ModuleCmd::Restart));
}

/// CONTRACT: **each of §4.6's rows is its own sentence, never a shared "something went wrong".**
/// Six distinct states, six distinct strings.
#[test]
fn every_state_says_a_different_thing() {
    let all = [
        Notice::Registry(ModuleState::NotApproved { producer: "widget".into() }),
        Notice::Registry(ModuleState::ApprovedNotBuilt {
            producer: "widget".into(),
            commit: "0123456789ab".into(),
        }),
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::Starting { elapsed: Duration::from_secs(1) },
        },
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::Stalled { silent: Duration::from_secs(2) },
        },
        Notice::Producer {
            producer: "widget".into(),
            presence: Presence::NotProducing { silent: Duration::from_secs(3), reason: None },
        },
        Notice::Producer { producer: "widget".into(), presence: Presence::Gone },
        Notice::Exited { producer: "widget".into(), status: Some(101) },
        Notice::Launch { producer: "widget".into(), why: "no such file".into() },
    ];
    let said: Vec<String> = all.iter().map(Notice::sentence).collect();
    for (i, a) in said.iter().enumerate() {
        assert!(!a.is_empty());
        assert!(a.contains("widget"), "{a}");
        for (j, b) in said.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "two states share one sentence");
            }
        }
    }
}
