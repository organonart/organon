//! Pins the egui contract that the editor's combo-popup close depends on (#545).
//!
//! `param_combo_sized` closes a dropdown by hand — Enter on the keyboard-cycle path
//! commits the selection and the caller closes the popup. Doing that needs the
//! popup's `Id`, and `ComboBox::widget_to_popup_id` is private, so `lib.rs` derives
//! it as `combo.response.id.with("popup")`.
//!
//! That derivation is load-bearing and was **wrong** in the first cut of #545: it
//! rebuilt the id from the *outer* `Ui` instead, so `Popup::close_id` addressed a
//! popup that never existed and Enter silently stopped closing the dropdown — no
//! panic, no warning, just a dead key. Nothing caught it. The suite was green, the
//! editor compiled, and the only symptom was a keypress that did nothing.
//!
//! Why the mistake was easy: the combo is shown on a **clipped child** `Ui`, not the
//! `ui.horizontal` one it looks like it belongs to, and `Ui::new_child` gives that
//! child its own id (`self.id.with("child").with(next_auto_id_salt)`). Both
//! derivations compile and both look plausible at the call site.
//!
//! `ComboBox::is_open` is public and routes through the private `widget_to_popup_id`,
//! so it works as an oracle without reaching into egui internals.
//!
//! **What this guards, and what it does not.** These are integration tests against the
//! *egui API*, not unit tests of `param_combo_sized`. They fail if a future egui
//! version changes how `ComboBox` derives its popup id — which is exactly the breakage
//! that hit us going 0.31 → 0.33, and the one that will come again at 0.34+. They do
//! **not** fail if someone edits the call site in `lib.rs` back to the outer-`Ui`
//! derivation: testing that would need a `ParamSetter` and a live param, which is not
//! constructible here. So this closes the version-bump half of the gap and leaves the
//! call-site half open; the call site carries a comment explaining why it reads from
//! the response.
//!
//! The remaining un-testable piece is the keypress itself — whether Enter reaches the
//! combo at all. That is a Mac check, noted in STATUS.
//!
//! Runs headless — egui layout is pure CPU, no GPU and no window.

use nih_plug_egui::egui;

/// Build a `ComboBox` the way `param_combo_sized` does — on a clipped child `Ui` —
/// and return `(the combo's widget id, our derived popup id, the id the outer `Ui`
/// would have produced)`.
fn ids_from_a_combo_on_a_child_ui(ctx: &egui::Context, salt: &str) -> (egui::Id, egui::Id, egui::Id) {
    let mut out = None;
    // The frame's paint output is irrelevant — we only want the ids egui assigned.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                // The id the buggy version computed, from the `ui.horizontal` closure's ui.
                let from_outer = ui.make_persistent_id(salt).with("popup");

                // What `param_combo_sized` actually does: an exact-size, clipped child
                // so `.truncate()` ellipsizes at the combo's width rather than the row's.
                // The clip is why this reads as "the same ui" at a glance.
                let (crect, _) =
                    ui.allocate_exact_size(egui::vec2(120.0, 18.0), egui::Sense::hover());
                let mut cui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(crect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                cui.set_clip_rect(crect.expand(1.0).intersect(cui.clip_rect()));

                let combo = egui::ComboBox::from_id_salt(salt)
                    .width(120.0)
                    .selected_text("Original")
                    .show_ui(&mut cui, |ui| {
                        ui.label("Original");
                    });

                out = Some((combo.response.id, combo.response.id.with("popup"), from_outer));
            });
        });
    });
    out.expect("the combo closure must run")
}

/// The popup id must come from the combo's own response, and the outer-`Ui`
/// derivation must be a genuinely different id — otherwise this test proves nothing
/// and the #545 bug would have been invisible here too.
#[test]
fn a_combos_popup_id_comes_from_its_response_not_the_outer_ui() {
    let ctx = egui::Context::default();
    let (widget_id, from_response, from_outer) = ids_from_a_combo_on_a_child_ui(&ctx, "algorithm");

    assert_ne!(
        from_response, from_outer,
        "the outer ui and the clipped child must derive DIFFERENT popup ids — if egui \
         ever makes these equal, the #545 bug becomes unobservable and this guard is \
         no longer testing anything"
    );

    // Opening our derived id must be what `ComboBox` itself considers open.
    egui::Popup::open_id(&ctx, from_response);
    assert!(
        egui::ComboBox::is_open(&ctx, widget_id),
        "`combo.response.id.with(\"popup\")` is not the id ComboBox uses — the editor's \
         Popup::close_id would address a popup that does not exist"
    );

    // And closing it must actually close.
    egui::Popup::close_id(&ctx, from_response);
    assert!(
        !egui::ComboBox::is_open(&ctx, widget_id),
        "closing the response-derived popup id left the popup open"
    );
}

/// The bug itself, pinned as a negative: closing the *outer*-derived id must not
/// close the real popup. If this ever starts passing silently, the derivation in
/// `param_combo_sized` has stopped mattering and should be re-read rather than
/// trusted.
#[test]
fn closing_the_outer_derived_id_does_not_close_the_real_popup() {
    let ctx = egui::Context::default();
    let (widget_id, from_response, from_outer) = ids_from_a_combo_on_a_child_ui(&ctx, "palette");

    egui::Popup::open_id(&ctx, from_response);
    assert!(egui::ComboBox::is_open(&ctx, widget_id), "precondition: the popup is open");

    egui::Popup::close_id(&ctx, from_outer);
    assert!(
        egui::ComboBox::is_open(&ctx, widget_id),
        "this is the #545 bug: closing the outer-ui-derived id must leave the real \
         popup open. If this assertion fails, the two ids have converged and the \
         first test in this file is the one to trust."
    );
}
