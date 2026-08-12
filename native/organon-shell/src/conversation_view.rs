//! The conversation view: a [`Transcript`] drawn natively, with a composer under it
//! (Console Spike §5.9, the second front-end).
//!
//! Scrollback above, composer below — Telegram, WhatsApp, Claude Desktop and Claude Code
//! are all this shape. The claim the console is making is that **a TUI is not a design;
//! it is what you build when a character grid is the only canvas allowed.** So the
//! interesting part of this file is not the layout, it is the [`tool_card`]: a tool call
//! drawn as a *card* — name, arguments as fields, live-versus-complete state, output —
//! instead of as the text a terminal would have printed. The structure was in the event
//! stream all along; the grid was where it got flattened.
//!
//! # What this owns, and what it does not
//!
//! [`ConversationPane`] owns one agent process, the mapper, the transcript and the
//! composer's text. It does **not** own the window, the tab strip, or the backdrop —
//! `shell_main.rs` does, exactly as it does for a terminal tab.
//!
//! The composer **writes to stdin and renders nothing** (§5.9.3 rule 2). The human turn
//! comes back on the stream under `--replay-user-messages` and is rendered from there,
//! which is what makes ordering free instead of a splice-and-hope.
//!
//! # Bounding is the view's job, deliberately
//!
//! [`crate::conversation`] leaves per-element text unbounded on purpose — a tool result
//! can be a whole file, and truncating it in the model would misrepresent the tool's
//! output while looking like the tool's output. So the clipping happens here, where it is
//! a *presentation* choice and says so on screen ("+N more lines"), and the full text is
//! still in the model.

use std::collections::VecDeque;

use egui::{Color32, CornerRadius, Frame, Margin, RichText};

use crate::agent_map::EventMapper;
use crate::agent_session::{AgentSession, StreamItem};
use crate::conversation::{
    Arguments, Body, Change, Element, RunOutcome, ToolCard, ToolState, Transcript,
};
use crate::timeline::pinned_after_scroll;

const HUMAN: Color32 = Color32::from_rgb(0xc8, 0xe6, 0xc8);
const PROSE: Color32 = Color32::from_rgb(0xd2, 0xd8, 0xd2);
const DIM: Color32 = Color32::from_rgb(0x70, 0x7c, 0x70);
const RUNNING: Color32 = Color32::from_rgb(0xe6, 0xc0, 0x4c);
const OK: Color32 = Color32::from_rgb(0x6f, 0xc2, 0x76);
const BAD: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x5f);

/// How much of a tool's output a card draws before it says how much it is not drawing.
const OUTPUT_LINES: usize = 10;
/// The same, for the two halves of an `Edit` diff.
const DIFF_LINES: usize = 12;
/// Diagnostic lines kept (non-JSON stdout, stderr). Bounded and logged, never silent.
const LOG_LINES: usize = 200;

/// One conversation tab: a live agent, the transcript it is writing, and the composer.
pub struct ConversationPane {
    session: Option<AgentSession>,
    transcript: Transcript,
    mapper: EventMapper,
    /// Set when the process could not be started, or has ended. Shown in place of the
    /// composer, because a tab that silently accepts input nobody will read is worse
    /// than one that says it is dead.
    pub failure: Option<String>,
    pub composer: String,
    /// Non-event lines off the child — the `Warning: no stdin data received…` the CLI
    /// opens a real run with (§5.9.3 rule 6), and anything on stderr.
    log: VecDeque<String>,
    /// Whether the view follows new elements. Re-derived from where the reader actually
    /// left the scroll each frame, so auto-scroll never fights someone reading back.
    pinned: bool,
    /// Focus the composer on the first frame this pane is drawn.
    want_focus: bool,
}

impl ConversationPane {
    /// Start a conversation in `cwd`. A spawn failure is **kept, not returned** — the tab
    /// opens and says what went wrong, which is the only way a user finds out.
    pub fn new(cwd: Option<&str>) -> Self {
        let (session, failure) = match AgentSession::spawn(cwd) {
            Ok(session) => (Some(session), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            session,
            transcript: Transcript::new(),
            mapper: EventMapper::new(),
            failure,
            composer: String::new(),
            log: VecDeque::new(),
            pinned: true,
            want_focus: true,
        }
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn log(&self) -> impl Iterator<Item = &String> {
        self.log.iter()
    }

    /// Drain the agent and fold whatever arrived. Returns true when the view should
    /// repaint — appended *or* updated, since a streamed delta changes pixels without
    /// changing the element list.
    ///
    /// [`Change::Appended`] additionally re-arms the follow, which is the difference
    /// between "new content pulls the view down" and "a token lands and yanks the reader
    /// mid-sentence".
    pub fn pump(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else { return false };
        let items = session.pump();
        if items.is_empty() {
            return false;
        }
        let mut changed = false;
        for item in items {
            match item {
                StreamItem::Event(event) => {
                    for mapped in self.mapper.map(&event) {
                        match self.transcript.apply(mapped) {
                            Change::Appended(_) => {
                                self.pinned = true;
                                changed = true;
                            }
                            Change::Updated(_) | Change::Meta => changed = true,
                            Change::Ignored(_) => {}
                        }
                    }
                }
                StreamItem::Noise(line) => self.note(format!("stdout: {line}")),
                StreamItem::Stderr(line) => self.note(format!("stderr: {line}")),
                StreamItem::Eof => {
                    self.note("the agent process ended".to_string());
                    self.failure = Some(
                        "the agent process ended — close this tab and open a new one to \
                         start another conversation"
                            .to_string(),
                    );
                    changed = true;
                }
            }
        }
        changed
    }

    /// Send the composer's contents and clear it. Renders nothing locally (rule 2).
    fn submit(&mut self) {
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = self.session.as_mut() else { return };
        match session.send_user(&text) {
            Ok(()) => self.composer.clear(),
            Err(e) => {
                self.note(format!("could not send: {e}"));
                self.failure = Some(format!("the agent stopped listening: {e}"));
            }
        }
    }

    fn note(&mut self, line: String) {
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }
}

/// Draw the pane: scrollback, then composer.
///
/// Bottom-up, because the composer's height is known and the scrollback's is whatever is
/// left — the layout every chat client resolves in that order.
pub fn draw(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        composer(ui, pane);
        ui.add_space(4.0);
        status_line(ui, pane);
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            scrollback(ui, pane);
        });
    });
}

fn scrollback(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    let out = egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(pane.pinned)
        .show(ui, |ui| {
            ui.add_space(6.0);
            if pane.transcript.is_empty() {
                ui.label(
                    RichText::new("no messages yet — type below and press Enter")
                        .color(DIM)
                        .italics(),
                );
            }
            for element in pane.transcript.elements() {
                draw_element(ui, element);
                ui.add_space(8.0);
            }
        });
    pane.pinned =
        pinned_after_scroll(out.state.offset.y, out.content_size.y, out.inner_rect.height());
}

fn draw_element(ui: &mut egui::Ui, element: &Element) {
    match &element.body {
        Body::Human(h) => {
            Frame::new()
                .fill(Color32::from_rgb(0x11, 0x18, 0x11))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("you").color(DIM).small());
                    ui.label(RichText::new(&h.text).color(HUMAN));
                });
        }
        Body::Assistant(a) => {
            // No frame: the agent's prose is the page, not a card on it.
            let text = if a.complete { a.text.clone() } else { format!("{}▍", a.text) };
            ui.label(RichText::new(text).color(PROSE));
        }
        Body::Tool(card) => tool_card(ui, card),
        Body::RunEnd(end) => {
            let (label, color) = match end.outcome {
                RunOutcome::Ok => ("turn complete", DIM),
                RunOutcome::Error => ("turn failed", BAD),
                RunOutcome::Cancelled => ("turn cancelled", RUNNING),
            };
            let detail = end.detail.as_deref().unwrap_or("");
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("── {label}")).color(color).small());
                if !detail.is_empty() {
                    ui.label(RichText::new(detail).color(DIM).small());
                }
            });
        }
    }
}

/// **The artifact a terminal could not have shown.**
///
/// A terminal receives a tool call as whatever text the harness chose to print, already
/// flattened. The event stream carries it structured — name, the complete input object,
/// a correlation id, and later a result — so it is drawn as a card whose state is
/// visible: amber and "running" while the id is unresolved, green or red once it is.
/// `Edit` goes one step further and renders its `old_string`/`new_string` as a real diff,
/// because those arrive as *fields*, not as a patch someone has to parse back out of
/// prose.
fn tool_card(ui: &mut egui::Ui, card: &ToolCard) {
    let (state_text, accent) = match &card.state {
        ToolState::Running => ("running", RUNNING),
        ToolState::Complete { is_error: false, .. } => ("ok", OK),
        ToolState::Complete { is_error: true, .. } => ("error", BAD),
    };
    Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(8))
        .stroke(egui::Stroke::new(1.0f32, accent))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let name = card.name.as_deref().unwrap_or("(call not seen)");
                ui.label(RichText::new(name).color(accent).strong().monospace());
                ui.label(RichText::new(state_text).color(accent).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(card.call_id.as_str()).color(DIM).small().monospace());
                });
            });

            match edit_diff(card.name.as_deref(), &card.arguments) {
                Some(diff) => diff_body(ui, &diff),
                None => arguments_body(ui, &card.arguments),
            }

            if let Some(output) = card.state.output() {
                ui.add_space(4.0);
                let (shown, hidden) = clip_lines(output, OUTPUT_LINES);
                for line in shown {
                    ui.label(RichText::new(line).monospace().small().color(PROSE));
                }
                if hidden > 0 {
                    ui.label(RichText::new(format!("+{hidden} more lines")).color(DIM).small());
                }
            }
        });
}

fn arguments_body(ui: &mut egui::Ui, args: &Arguments) {
    for (key, value) in argument_fields(args) {
        ui.horizontal_wrapped(|ui| {
            if !key.is_empty() {
                ui.label(RichText::new(format!("{key}:")).color(DIM).small().monospace());
            }
            ui.label(RichText::new(value).color(PROSE).small().monospace());
        });
    }
}

fn diff_body(ui: &mut egui::Ui, diff: &EditDiff) {
    if !diff.path.is_empty() {
        ui.label(RichText::new(&diff.path).color(DIM).small().monospace());
    }
    let (removed, removed_more) = clip_slice(&diff.removed, DIFF_LINES);
    let (added, added_more) = clip_slice(&diff.added, DIFF_LINES);
    for line in removed {
        ui.label(RichText::new(format!("- {line}")).color(BAD).small().monospace());
    }
    if removed_more > 0 {
        ui.label(RichText::new(format!("  +{removed_more} more removed")).color(DIM).small());
    }
    for line in added {
        ui.label(RichText::new(format!("+ {line}")).color(OK).small().monospace());
    }
    if added_more > 0 {
        ui.label(RichText::new(format!("  +{added_more} more added")).color(DIM).small());
    }
}

fn status_line(ui: &mut egui::Ui, pane: &ConversationPane) {
    ui.horizontal(|ui| {
        if let Some(failure) = &pane.failure {
            ui.label(RichText::new(failure).color(BAD).small());
            return;
        }
        if pane.transcript.is_working() {
            let n = pane.transcript.running_tools().len();
            let plural = if n == 1 { "tool" } else { "tools" };
            ui.label(RichText::new(format!("● {n} {plural} running")).color(RUNNING).small());
        } else {
            let session = pane.transcript.session_id().unwrap_or("connecting…");
            ui.label(RichText::new(session).color(DIM).small().monospace());
        }
        if let Some(last) = pane.log.back() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(last).color(DIM).small());
            });
        }
    });
}

fn composer(ui: &mut egui::Ui, pane: &mut ConversationPane) {
    let live = pane.failure.is_none();
    ui.horizontal(|ui| {
        ui.label(RichText::new("›").color(if live { HUMAN } else { DIM }).monospace());
        let edit = egui::TextEdit::singleline(&mut pane.composer)
            .desired_width(ui.available_width())
            .font(egui::TextStyle::Monospace)
            .hint_text(if live { "message the agent" } else { "the agent is not running" });
        let response = ui.add_enabled(live, edit);
        if pane.want_focus && live {
            response.request_focus();
            pane.want_focus = false;
        }
        // Enter submits and keeps focus — the composer is where the next message is
        // going, so handing focus back to nothing would cost a click per turn.
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            pane.submit();
            pane.want_focus = true;
        }
    });
}

// ---------------------------------------------------------------------------
// The pure part — clipping and field extraction, tested headless
// ---------------------------------------------------------------------------

/// One `Edit` call, as a card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDiff {
    pub path: String,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// A tool's arguments as `(key, value)` rows.
///
/// ⚠️ **Only parsed once [`Arguments::complete`] is true.** While a call is streaming,
/// its text is genuinely half a JSON document, and the contract
/// [`crate::conversation::Arguments`] states is that a view may *show* it but must not
/// present it as structured data. So the streaming case comes back as a single unnamed
/// row carrying the raw fragment — which is the honest rendering of "this is arriving".
pub fn argument_fields(args: &Arguments) -> Vec<(String, String)> {
    if !args.complete {
        return if args.text.is_empty() {
            Vec::new()
        } else {
            vec![(String::new(), args.text.clone())]
        };
    }
    match serde_json::from_str::<serde_json::Value>(&args.text) {
        Ok(serde_json::Value::Object(map)) => map
            .into_iter()
            .map(|(k, v)| {
                let value = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                (k, one_line(&value))
            })
            .collect(),
        // Complete but not an object (or not parseable): show it rather than hide it.
        _ if args.text.is_empty() || args.text == "null" => Vec::new(),
        _ => vec![(String::new(), one_line(&args.text))],
    }
}

/// `Edit`'s structured before/after, when the call is one and its input has settled.
pub fn edit_diff(name: Option<&str>, args: &Arguments) -> Option<EditDiff> {
    if name != Some("Edit") || !args.complete {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&args.text).ok()?;
    let old = value.get("old_string")?.as_str()?;
    let new = value.get("new_string")?.as_str()?;
    Some(EditDiff {
        path: value.get("file_path").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        removed: old.lines().map(str::to_string).collect(),
        added: new.lines().map(str::to_string).collect(),
    })
}

/// The first `max` lines of `text`, and how many were left behind.
pub fn clip_lines(text: &str, max: usize) -> (Vec<&str>, usize) {
    let total = text.lines().count();
    (text.lines().take(max).collect(), total.saturating_sub(max))
}

fn clip_slice(lines: &[String], max: usize) -> (&[String], usize) {
    let shown = lines.len().min(max);
    (&lines[..shown], lines.len() - shown)
}

/// Collapse a value to one display line. A tool argument can be a whole file's contents;
/// the card shows that it is there and how big, not all of it.
fn one_line(value: &str) -> String {
    const LIMIT: usize = 160;
    let flat: String = value.replace(['\n', '\r'], "⏎");
    if flat.chars().count() <= LIMIT {
        return flat;
    }
    let head: String = flat.chars().take(LIMIT).collect();
    format!("{head}… ({} chars)", value.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(text: &str) -> Arguments {
        Arguments { text: text.to_string(), complete: true }
    }

    /// The contract that matters: a streaming call's half-JSON is shown, never parsed.
    #[test]
    fn partial_arguments_are_shown_raw_and_never_parsed() {
        let partial = Arguments { text: r#"{"file_path": "C:\\work\"#.into(), complete: false };
        let fields = argument_fields(&partial);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "", "a fragment has no key to claim");
        assert!(fields[0].1.contains("file_path"));
        assert!(argument_fields(&Arguments::pending()).is_empty());
    }

    #[test]
    fn complete_arguments_become_fields() {
        let fields = argument_fields(&complete(r#"{"file_path":"a.txt","limit":20}"#));
        assert_eq!(
            fields,
            vec![("file_path".to_string(), "a.txt".to_string()), ("limit".into(), "20".into())]
        );
    }

    /// A tool argument can be a whole file. The card must say so rather than draw it.
    #[test]
    fn a_huge_argument_is_summarised_on_one_line() {
        let body = "x".repeat(5000);
        let args = complete(&serde_json::json!({ "content": body }).to_string());
        let fields = argument_fields(&args);
        assert_eq!(fields.len(), 1);
        assert!(fields[0].1.ends_with("(5000 chars)"), "{:?}", fields[0].1);
        assert!(!fields[0].1.contains('\n'));
    }

    /// Newlines are flattened, because a "field" that is forty rows tall stops being one.
    #[test]
    fn multi_line_values_stay_one_line() {
        let args = complete(&serde_json::json!({ "cmd": "a\nb\nc" }).to_string());
        assert_eq!(argument_fields(&args)[0].1, "a⏎b⏎c");
    }

    /// The second artifact: `Edit` arrives with its before and after as *fields*, so the
    /// diff needs no parsing out of prose.
    #[test]
    fn edit_becomes_a_diff() {
        let args = complete(
            &serde_json::json!({
                "file_path": "src/lib.rs",
                "old_string": "let a = 1;\nlet b = 2;",
                "new_string": "let a = 1;\nlet b = 3;\nlet c = 4;",
            })
            .to_string(),
        );
        let diff = edit_diff(Some("Edit"), &args).expect("a diff");
        assert_eq!(diff.path, "src/lib.rs");
        assert_eq!(diff.removed, vec!["let a = 1;", "let b = 2;"]);
        assert_eq!(diff.added, vec!["let a = 1;", "let b = 3;", "let c = 4;"]);
    }

    /// …and only for `Edit`, only once settled, and never on a shape that lacks them.
    #[test]
    fn nothing_else_is_mistaken_for_a_diff() {
        let args = complete(r#"{"file_path":"a.txt"}"#);
        assert!(edit_diff(Some("Read"), &args).is_none());
        assert!(edit_diff(Some("Edit"), &args).is_none(), "no old/new means no diff");
        let streaming = Arguments { text: r#"{"old_string":"a","new_"#.into(), complete: false };
        assert!(edit_diff(Some("Edit"), &streaming).is_none(), "never parse a fragment");
    }

    /// Clipping is the VIEW's, and it reports what it hid — the model keeps the whole
    /// output, so "10 lines" must never be mistaken for "that was all of it".
    #[test]
    fn output_clipping_reports_what_it_hid() {
        let text = (1..=25).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let (shown, hidden) = clip_lines(&text, 10);
        assert_eq!(shown.len(), 10);
        assert_eq!(shown[0], "1");
        assert_eq!(hidden, 15);
        let (all, none) = clip_lines("one\ntwo", 10);
        assert_eq!(all, vec!["one", "two"]);
        assert_eq!(none, 0);
    }
}
