//! The mock agent — a scripted event stream standing in for Pi (Shell #7 T1).
//!
//! Tier 1 of the agent bridge is the *workspace side*: the timeline has to render
//! every [`EventKind`] before any real adapter exists to emit them. `MockAgent`
//! replays a script through a **pull API** — the caller's frame loop calls
//! [`MockAgent::tick`] with its own clock and receives whatever events are due. No
//! threads, no timers, no channels: the same code path is driven by egui's frame
//! clock in the app and by literal integers in tests, which is what makes the demo
//! deterministic and headless-testable. The real Pi adapter (later #7 tiers)
//! replaces the *source* of events, not the timeline that renders them.
//!
//! **Honesty rule:** everything a `MockAgent` emits is scripted. The app keeps a
//! visible "scripted demo" marker on the timeline whenever one is attached
//! (`timeline::show`'s `scripted` flag) — the UI must never present the replay as
//! a live agent.

use serde_json::json;

use crate::session::{
    Approval, ApprovalState, Artifact, ArtifactEventRecord, Command, CommandRunRecord, EventKind,
    ExtensionEventRecord, ExtensionRecord, Issuer, MessageRecord, PlanRecord, RunStatus,
    SessionLog, Surface, SurfaceEventRecord, Task, TaskEventRecord, ToolCallRecord, now_unix_ms,
};

/// One beat of a script: emit `kind` once `after_ms` milliseconds have elapsed
/// since the agent's first tick.
pub struct ScriptedEvent {
    pub after_ms: u64,
    pub kind: EventKind,
}

/// Replays a [`ScriptedEvent`] script against a caller-supplied clock.
pub struct MockAgent {
    /// Sorted by `after_ms` at construction — `tick`'s early-out scan requires it,
    /// and sorting once here means a hand-edited script cannot silently skip beats.
    script: Vec<ScriptedEvent>,
    /// Index of the next unemitted event; events are emitted exactly once, in order.
    next: usize,
    /// The `now_ms` of the first tick — the script's t=0. The agent has no clock of
    /// its own; whatever monotonic source the caller uses *becomes* the story clock.
    started_at_ms: Option<u64>,
}

impl MockAgent {
    pub fn new(mut script: Vec<ScriptedEvent>) -> Self {
        script.sort_by_key(|e| e.after_ms);
        Self { script, next: 0, started_at_ms: None }
    }

    /// Pull every event now due. `now_ms` is any monotonic millisecond counter —
    /// the first call pins t=0, so absolute origin never matters. A `now_ms` that
    /// runs backwards clamps to t=0 rather than panicking or re-emitting.
    pub fn tick(&mut self, now_ms: u64) -> Vec<EventKind> {
        let start = *self.started_at_ms.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(start);
        let mut due = Vec::new();
        while let Some(event) = self.script.get(self.next) {
            if event.after_ms > elapsed {
                break;
            }
            due.push(event.kind.clone());
            self.next += 1;
        }
        due
    }

    /// True once every scripted event has been emitted. The app keeps a finished
    /// agent attached — the timeline stays labeled scripted for as long as its
    /// content is.
    pub fn is_done(&self) -> bool {
        self.next == self.script.len()
    }

    /// The built-in demo: one coherent story — the PRD's PBR-material-change
    /// scenario — that exercises **all nine** [`EventKind`] variants in ~8.5s.
    /// A test pins the coverage with an exhaustive match, so a tenth variant is a
    /// compile error there until this script grows.
    pub fn demo() -> Self {
        // Scripted base for the records' own timestamp fields (command start/end,
        // approval requested-at). These are *content*, not envelope timestamps —
        // the SessionLog stamps the envelope itself — and a **fixed** epoch, not
        // the wall clock: two constructions of the demo are identical, which the
        // determinism test relies on and which is what "scripted" means.
        let t0 = 1_750_000_000_000_u64;
        let task = |state: &str| Task {
            id: "task-copper".into(),
            title: "Retune hero surface to brushed copper".into(),
            owner: Some("pi".into()),
            state: state.into(),
            depends_on: vec![],
            evidence: if state == "review" { vec!["art-copper-ab".into()] } else { vec![] },
        };
        let msg = |from: Issuer, text: &str| {
            EventKind::Message(MessageRecord { from, text: text.into() })
        };
        let script = vec![
            ScriptedEvent {
                after_ms: 0,
                kind: msg(
                    Issuer::User,
                    "The hero surface reads too cold — try a brushed-copper PBR set, \
                     but keep the anisotropy response we tuned last week.",
                ),
            },
            ScriptedEvent {
                after_ms: 500,
                kind: msg(
                    Issuer::PrimaryAgent,
                    "On it. I'll derive a copper base color and metallic curve, regenerate \
                     the roughness map, and put an A/B viewport up so you can judge it live.",
                ),
            },
            ScriptedEvent {
                after_ms: 1000,
                kind: EventKind::Plan(PlanRecord {
                    summary: "Brushed-copper retune of the hero surface".into(),
                    steps: vec![
                        "Derive copper base color + metallic response from the current set".into(),
                        "Regenerate roughness and anisotropy maps, preserving the brush axis".into(),
                        "Stand up a live A/B viewport and hand the verdict back".into(),
                    ],
                }),
            },
            ScriptedEvent {
                after_ms: 1500,
                kind: EventKind::TaskEvent(TaskEventRecord {
                    action: "created".into(),
                    task: task("queued"),
                }),
            },
            ScriptedEvent {
                after_ms: 2000,
                kind: EventKind::TaskEvent(TaskEventRecord {
                    action: "state".into(),
                    task: task("running"),
                }),
            },
            ScriptedEvent {
                after_ms: 2500,
                kind: EventKind::ToolCall(ToolCallRecord {
                    name: "material.read".into(),
                    args: json!({ "surface": "hero", "fields": ["base_color", "metallic", "roughness", "anisotropy"] }),
                    result: Some(json!({
                        "base_color": "#8A8D8F", "metallic": 1.0,
                        "roughness": 0.42, "anisotropy": 0.65
                    })),
                }),
            },
            ScriptedEvent {
                after_ms: 3000,
                kind: EventKind::CommandRun(CommandRunRecord {
                    issuer: Issuer::PrimaryAgent,
                    command: Command {
                        name: "material.set".into(),
                        args: json!({
                            "surface": "hero", "base_color": "#B87333",
                            "metallic": 0.92, "roughness": 0.38
                        }),
                    },
                    target: "viewport".into(),
                    started_unix_ms: t0 + 2_600,
                    ended_unix_ms: Some(t0 + 2_910),
                    status: RunStatus::Ok,
                    evidence: vec![],
                    approval: ApprovalState::Automatic,
                }),
            },
            ScriptedEvent {
                after_ms: 3500,
                kind: msg(
                    Issuer::PrimaryAgent,
                    "Base color and metallic curve are in. Regenerating the roughness map \
                     along the existing brush axis now.",
                ),
            },
            ScriptedEvent {
                after_ms: 4000,
                kind: EventKind::ToolCall(ToolCallRecord {
                    name: "texture.generate".into(),
                    args: json!({ "map": "roughness", "brush_axis_deg": 12.5, "resolution": 2048 }),
                    result: Some(json!({ "ok": true, "px": 2048 })),
                }),
            },
            ScriptedEvent {
                after_ms: 4500,
                kind: EventKind::CommandRun(CommandRunRecord {
                    issuer: Issuer::PrimaryAgent,
                    command: Command {
                        name: "viewport.snapshot".into(),
                        args: json!({ "id": "vp-copper-ab", "label": "copper-vs-steel" }),
                    },
                    target: "viewport".into(),
                    started_unix_ms: t0 + 4_100,
                    ended_unix_ms: Some(t0 + 4_290),
                    status: RunStatus::Ok,
                    evidence: vec!["art-copper-ab".into()],
                    approval: ApprovalState::Automatic,
                }),
            },
            ScriptedEvent {
                after_ms: 5000,
                kind: EventKind::ArtifactEvent(ArtifactEventRecord {
                    action: "created".into(),
                    artifact: Artifact {
                        id: "art-copper-ab".into(),
                        creator: Issuer::PrimaryAgent,
                        created_unix_ms: t0 + 4_290,
                        source_task: Some("task-copper".into()),
                        source_command_run: Some("viewport.snapshot".into()),
                        mime: "image/png".into(),
                        inputs: vec![],
                    },
                }),
            },
            ScriptedEvent {
                after_ms: 5500,
                kind: EventKind::SurfaceEvent(SurfaceEventRecord {
                    action: "create".into(),
                    surface: Surface {
                        id: "vp-copper-ab".into(),
                        kind: "organon-viewport".into(),
                        title: "Copper A/B".into(),
                    },
                }),
            },
            ScriptedEvent {
                after_ms: 6000,
                kind: EventKind::SurfaceEvent(SurfaceEventRecord {
                    action: "focus".into(),
                    surface: Surface {
                        id: "vp-copper-ab".into(),
                        kind: "organon-viewport".into(),
                        title: "Copper A/B".into(),
                    },
                }),
            },
            ScriptedEvent {
                after_ms: 6500,
                kind: EventKind::TaskEvent(TaskEventRecord {
                    action: "state".into(),
                    task: task("review"),
                }),
            },
            ScriptedEvent {
                after_ms: 7000,
                kind: msg(
                    Issuer::PrimaryAgent,
                    "The A/B viewport is up — copper left, the old steel right, same \
                     lighting rig. If it reads right, I'd like to save the set as a \
                     project preset.",
                ),
            },
            ScriptedEvent {
                after_ms: 7500,
                kind: EventKind::Approval(Approval {
                    id: "ap-copper-preset".into(),
                    action: "write material preset 'brushed-copper' to the project gallery".into(),
                    target: "project".into(),
                    state: ApprovalState::Requested,
                    requested_unix_ms: t0 + 7_500,
                    decided_unix_ms: None,
                }),
            },
            ScriptedEvent {
                after_ms: 8000,
                kind: EventKind::ExtensionEvent(ExtensionEventRecord {
                    action: "proposed".into(),
                    extension: ExtensionRecord {
                        id: "ext-material-ab-lens".into(),
                        version: "0.1".into(),
                        kind: "viewport lens".into(),
                        source: "session-draft".into(),
                        state: "draft".into(),
                        evidence: vec!["art-copper-ab".into()],
                        scope: "session".into(),
                    },
                }),
            },
            ScriptedEvent {
                after_ms: 8500,
                kind: msg(
                    Issuer::PrimaryAgent,
                    "That's the whole pass. The preset write is waiting on your approval \
                     above; the A/B lens proposal can wait until you've lived with the \
                     copper for a day.",
                ),
            },
        ];
        Self::new(script)
    }
}

/// Dev flag: `ORGANON_SHELL_DEMO=1` starts the scripted demo at launch — the
/// capture/CI path, no click needed. Anything else (or unset) is off. Read by the
/// binary, never by the lib's UI code, so tests are immune to the environment.
pub fn demo_env_requested() -> bool {
    std::env::var("ORGANON_SHELL_DEMO").is_ok_and(|v| v == "1")
}

/// The on-disk session a demo run writes: a fresh `demo-<unix-ms>` id under the
/// real OrganonShell store, so even the scripted story leaves a genuine
/// `events.jsonl` behind. `None` (no data dir, store unwritable) degrades to an
/// in-memory-only timeline — the demo must never fail to start over disk state.
pub fn open_demo_log() -> Option<SessionLog> {
    SessionLog::open_default(&format!("demo-{}", now_unix_ms())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Index per variant — the exhaustive match is the point: a tenth `EventKind`
    /// fails to compile here until the demo script (and this map) covers it.
    fn variant_index(kind: &EventKind) -> usize {
        match kind {
            EventKind::Message(_) => 0,
            EventKind::Plan(_) => 1,
            EventKind::ToolCall(_) => 2,
            EventKind::ArtifactEvent(_) => 3,
            EventKind::SurfaceEvent(_) => 4,
            EventKind::TaskEvent(_) => 5,
            EventKind::Approval(_) => 6,
            EventKind::CommandRun(_) => 7,
            EventKind::ExtensionEvent(_) => 8,
        }
    }

    #[test]
    fn demo_covers_every_variant_and_reads_in_order() {
        let mut agent = MockAgent::demo();
        // The first tick pins t=0; the second, far past the total duration,
        // drains the rest of the story.
        let mut all = agent.tick(0);
        all.extend(agent.tick(1_000_000));
        assert!(agent.is_done());
        assert!(all.len() >= 18, "the demo is a story, not a smoke sample: {}", all.len());

        let mut seen = [false; 9];
        for kind in &all {
            seen[variant_index(kind)] = true;
        }
        assert!(seen.iter().all(|s| *s), "every EventKind variant must appear: {seen:?}");

        // The story shape: opens with the user asking, closes with Pi reporting.
        assert!(matches!(&all[0], EventKind::Message(m) if m.from == Issuer::User));
        assert!(matches!(
            all.last().unwrap(),
            EventKind::Message(m) if m.from == Issuer::PrimaryAgent
        ));
        // The approval lands pending — the timeline's Approve/Deny path is reachable.
        assert!(all.iter().any(|k| matches!(
            k,
            EventKind::Approval(a) if a.state == ApprovalState::Requested
        )));
    }

    #[test]
    fn tick_is_deterministic_and_emits_each_event_once() {
        let mut agent = MockAgent::demo();
        // The first tick pins t=0 at whatever the caller's clock says.
        let at_start = agent.tick(50_000);
        assert_eq!(at_start.len(), 1, "only the after_ms=0 event is due at t=0");

        // Nothing new before the next beat…
        assert!(agent.tick(50_400).is_empty());
        // …and a clock that runs backwards clamps instead of re-emitting.
        assert!(agent.tick(49_000).is_empty());

        // Draining incrementally yields exactly the same sequence as one big tick.
        let mut incremental = at_start;
        for ms in (50_500..60_000).step_by(250) {
            incremental.extend(agent.tick(ms));
        }
        assert!(agent.is_done());
        let mut fresh = MockAgent::demo();
        let mut one_shot = fresh.tick(0);
        one_shot.extend(fresh.tick(1_000_000));
        assert_eq!(incremental, one_shot, "pull cadence must not change the story");

        // A finished agent stays quiet.
        assert!(agent.tick(u64::MAX).is_empty());
    }

    #[test]
    fn scripts_are_sorted_at_construction() {
        let out_of_order = vec![
            ScriptedEvent {
                after_ms: 100,
                kind: EventKind::Message(MessageRecord { from: Issuer::User, text: "b".into() }),
            },
            ScriptedEvent {
                after_ms: 0,
                kind: EventKind::Message(MessageRecord { from: Issuer::User, text: "a".into() }),
            },
        ];
        let mut agent = MockAgent::new(out_of_order);
        let mut all = agent.tick(0);
        all.extend(agent.tick(1_000));
        let texts: Vec<_> = all
            .iter()
            .map(|k| match k {
                EventKind::Message(m) => m.text.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(texts, ["a", "b"], "a hand-edited script must not skip beats");
    }
}
