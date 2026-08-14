//! The workspace timeline — a `&[SessionEvent]` rendered as typed cards
//! (Console #7 T1).
//!
//! One visual treatment per [`EventKind`]: message bubbles sided by issuer, a plan
//! card with its step list, tool/command cards with name + args + status +
//! duration, compact one-line rows for the artifact/surface/task/extension
//! lifecycle moments, and an approval card that stands apart — it is the one card
//! asking the human for something. Everything here is a function of the event
//! slice + [`TimelineState`]; no globals, no `EDITION` reads, so the whole module
//! renders headless in tests from a feature-off build.
//!
//! **T1 boundary:** the Approve/Deny buttons *return* a [`TimelineAction`] to the
//! caller — they mutate nothing. Wiring a decision back into an agent (and into
//! the log as its own event) is the bridge's job in later #7 tiers; a timeline
//! that edited its own history would be the two-vocabularies mistake in miniature.

use egui::{Color32, CornerRadius, Frame, Margin, RichText};

use crate::session::{
    ApprovalState, CommandRunRecord, EventKind, Issuer, RunStatus, SessionEvent, ToolCallRecord,
};
use crate::theme::Theme;

/// What the user asked the timeline to do this frame. `seq` names the event the
/// decision belongs to — the caller owns what (if anything) happens next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineAction {
    Approve { seq: u64 },
    Deny { seq: u64 },
}

/// Scroll persistence across frames. One bit, deliberately: pinned means "keep me
/// on the newest event"; scrolling up releases it, scrolling back to the bottom
/// re-arms it — [`pinned_after_scroll`] is that rule as a pure, tested function.
pub struct TimelineState {
    pub pinned: bool,
}

impl Default for TimelineState {
    fn default() -> Self {
        // A fresh timeline follows the story until the user says otherwise.
        Self { pinned: true }
    }
}

/// Within this many points of the bottom still counts as "at the bottom" — a
/// half-line of drift from egui's float layout must not silently unpin the view.
const PIN_SLACK: f32 = 24.0;

/// The pin rule: pinned iff the viewport's bottom edge sits within [`PIN_SLACK`]
/// of the content's end. Content shorter than the viewport is always pinned —
/// there is nowhere to scroll away to.
pub fn pinned_after_scroll(offset_y: f32, content_h: f32, viewport_h: f32) -> bool {
    offset_y + viewport_h + PIN_SLACK >= content_h
}

/// Render the timeline into `ui`. `scripted` keeps the product's honesty rule:
/// while a mock agent is (or was) the source, the timeline says so on its face —
/// a replay must never pass as a live agent.
pub fn show(
    ui: &mut egui::Ui,
    events: &[SessionEvent],
    state: &mut TimelineState,
    scripted: bool,
    theme: &Theme,
) -> Option<TimelineAction> {
    let mut action = None;

    if scripted {
        Frame::new()
            .fill(theme.timeline_scripted_fill)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("● scripted demo").color(theme.timeline_scripted_mark),
                    );
                    ui.weak("— every event below is replayed from a built-in script, not a live agent");
                });
            });
        ui.add_space(4.0);
    }

    let out = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(state.pinned)
        .show(ui, |ui| {
            ui.add_space(4.0);
            for event in events {
                card(ui, event, &mut action, theme);
                ui.add_space(6.0);
            }
        });
    // Re-derive the pin from where the user actually left the view: scrolling up
    // unpins, returning to the bottom re-pins — auto-scroll never fights a reader.
    state.pinned =
        pinned_after_scroll(out.state.offset.y, out.content_size.y, out.inner_rect.height());

    action
}

/// Who a record's issuer is, as the timeline names them.
fn issuer_label(issuer: &Issuer) -> String {
    match issuer {
        Issuer::User => "You".into(),
        Issuer::PrimaryAgent => "Pi".into(),
        Issuer::Worker(id) => format!("worker · {id}"),
        Issuer::Automation => "automation".into(),
    }
}

fn status_text(status: &RunStatus, theme: &Theme) -> (&'static str, Color32) {
    match status {
        RunStatus::Pending => ("pending", theme.timeline_status_pending),
        RunStatus::Running => ("running", theme.timeline_status_running),
        RunStatus::Ok => ("ok", theme.timeline_status_ok),
        RunStatus::Failed => ("failed", theme.timeline_status_failed),
        RunStatus::Denied => ("denied", theme.timeline_status_denied),
        RunStatus::Cancelled => ("cancelled", theme.timeline_status_cancelled),
    }
}

/// Compact single-line JSON for card display; `null` (a command with no args)
/// renders as nothing rather than the literal word.
fn args_text(args: &serde_json::Value) -> String {
    if args.is_null() { String::new() } else { args.to_string() }
}

/// A framed card body used by the plan/tool/command/approval treatments.
fn card_frame(ui: &mut egui::Ui, accent: Option<Color32>, add: impl FnOnce(&mut egui::Ui)) {
    let mut frame = Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8));
    if let Some(accent) = accent {
        frame = frame.stroke(egui::Stroke::new(1.0, accent));
    }
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn card(
    ui: &mut egui::Ui,
    event: &SessionEvent,
    action: &mut Option<TimelineAction>,
    theme: &Theme,
) {
    match &event.kind {
        EventKind::Message(m) => message_bubble(ui, &m.from, &m.text, theme),
        EventKind::Plan(p) => card_frame(ui, None, |ui| {
            ui.strong(format!("Plan — {}", p.summary));
            for (i, step) in p.steps.iter().enumerate() {
                ui.label(format!("{}. {step}", i + 1));
            }
        }),
        EventKind::ToolCall(t) => tool_card(ui, t),
        EventKind::CommandRun(r) => command_card(ui, r, theme),
        EventKind::Approval(a) => {
            // The card that asks the human for something stands apart: accent
            // stroke, explicit buttons. Pending = requested with no decision yet.
            let pending = a.state == ApprovalState::Requested && a.decided_unix_ms.is_none();
            let accent = theme.timeline_approval_accent;
            card_frame(ui, Some(accent), |ui| {
                ui.strong(RichText::new("Approval requested").color(accent));
                ui.label(&a.action);
                ui.weak(format!("target: {}", a.target));
                if pending {
                    ui.horizontal(|ui| {
                        if ui.button("Approve").clicked() {
                            *action = Some(TimelineAction::Approve { seq: event.seq });
                        }
                        if ui.button("Deny").clicked() {
                            *action = Some(TimelineAction::Deny { seq: event.seq });
                        }
                    });
                } else {
                    ui.weak(format!("{:?}", a.state));
                }
            });
        }
        // The four lifecycle moments are ambient context, not conversation — one
        // compact line each, so the story's dialogue stays the visual spine.
        EventKind::ArtifactEvent(a) => {
            ui.weak(format!(
                "▤ artifact {} · {} ({})",
                a.action, a.artifact.id, a.artifact.mime
            ));
        }
        EventKind::SurfaceEvent(s) => {
            ui.weak(format!(
                "▢ surface {} · {} [{}]",
                s.action, s.surface.title, s.surface.kind
            ));
        }
        EventKind::TaskEvent(t) => {
            ui.weak(format!("☰ task {} · {} — {}", t.action, t.task.title, t.task.state));
        }
        EventKind::ExtensionEvent(e) => {
            ui.weak(format!(
                "✦ extension {} · {} ({}, {})",
                e.action, e.extension.id, e.extension.kind, e.extension.state
            ));
        }
    }
}

/// Message bubbles: the user sits right, everyone else left — the chat convention
/// that makes issuer-side legible without reading a name.
fn message_bubble(ui: &mut egui::Ui, from: &Issuer, text: &str, theme: &Theme) {
    let from_user = *from == Issuer::User;
    let layout = if from_user {
        egui::Layout::top_down(egui::Align::Max)
    } else {
        egui::Layout::top_down(egui::Align::Min)
    };
    let fill = if from_user { theme.timeline_bubble_user } else { theme.timeline_bubble_other };
    ui.with_layout(layout, |ui| {
        Frame::new()
            .fill(fill)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() * 0.75);
                ui.weak(issuer_label(from));
                ui.label(text);
            });
    });
}

fn tool_card(ui: &mut egui::Ui, t: &ToolCallRecord) {
    card_frame(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.strong(format!("⚙ {}", t.name));
            match &t.result {
                Some(_) => ui.weak("→ done"),
                None => ui.weak("… running"),
            };
        });
        let args = args_text(&t.args);
        if !args.is_empty() {
            ui.weak(args);
        }
    });
}

fn command_card(ui: &mut egui::Ui, r: &CommandRunRecord, theme: &Theme) {
    let (status, color) = status_text(&r.status, theme);
    card_frame(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.strong(format!("$ {}", r.command.name));
            ui.label(RichText::new(status).color(color));
            if let Some(ended) = r.ended_unix_ms {
                ui.weak(format!("{} ms", ended.saturating_sub(r.started_unix_ms)));
            }
        });
        let args = args_text(&r.command.args);
        if !args.is_empty() {
            ui.weak(args);
        }
        ui.weak(format!(
            "{} → {} · approval: {:?}",
            issuer_label(&r.issuer),
            r.target,
            r.approval
        ));
        if !r.evidence.is_empty() {
            ui.weak(format!("evidence: {}", r.evidence.join(", ")));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_rule_is_a_pure_function_of_geometry() {
        // At the exact bottom: pinned.
        assert!(pinned_after_scroll(700.0, 1000.0, 300.0));
        // Within slack of the bottom: still pinned — float drift must not unpin.
        assert!(pinned_after_scroll(700.0 - PIN_SLACK, 1000.0, 300.0));
        // Scrolled meaningfully up: unpinned, auto-scroll stops fighting the reader.
        assert!(!pinned_after_scroll(400.0, 1000.0, 300.0));
        assert!(!pinned_after_scroll(0.0, 1000.0, 300.0));
        // Content shorter than the viewport: nowhere to scroll away to — pinned.
        assert!(pinned_after_scroll(0.0, 120.0, 300.0));
    }

    #[test]
    fn fresh_timeline_follows_the_newest_event() {
        assert!(TimelineState::default().pinned);
    }

    #[test]
    fn issuer_labels_side_with_the_product_voice() {
        assert_eq!(issuer_label(&Issuer::User), "You");
        assert_eq!(issuer_label(&Issuer::PrimaryAgent), "Pi");
        assert_eq!(issuer_label(&Issuer::Worker("cc".into())), "worker · cc");
        assert_eq!(issuer_label(&Issuer::Automation), "automation");
    }

    #[test]
    fn null_args_render_as_nothing() {
        assert_eq!(args_text(&serde_json::Value::Null), "");
        assert_eq!(args_text(&serde_json::json!({ "a": 1 })), "{\"a\":1}");
    }
}
