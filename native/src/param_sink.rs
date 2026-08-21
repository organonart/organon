//! **Where an editor panel's control writes** — one panel body, two destinations.
//!
//! Organon's editor draws a parameter as `srow(ui, w2, "node bevel", &params.bevel, setter)`:
//! metadata from the param, the write through a `nih_plug::ParamSetter`. Organon Console has
//! the first half and cannot have the second.
//!
//! # 🚨 The wall
//!
//! **A parameter cannot be written from outside `nih_plug`.** `ParamMut` and every setter on
//! it are `pub(crate)`, `FloatParam`'s value fields are private, and nih-plug's standalone
//! `Wrapper` — the one type outside a host that implements `GuiContext` — lives in a private
//! module. `nih_plug` is an upstream git dependency, not a fork. Implementing `GuiContext`
//! yourself compiles and buys nothing: what it hands you is a `ParamPtr` you cannot write
//! through.
//!
//! # The way round it, and why it is not a workaround
//!
//! [`crate::preset::PresetValues`] is a **freely writable mirror** of the param set with its
//! own `to_shared()`, and by this project's convention its fields share their identifiers with
//! `OrganicMathParams` — a convention `param_table.rs` already leans its whole two-packer
//! design on. So the Console holds a `PresetValues`, the widgets write *it*, and the world is
//! driven from it. Nothing is faked and nothing is duplicated: the panel that the Console
//! draws is the panel the editor draws, compiled once.
//!
//! ⚠️ **What was actually missing was not a writer, it was an identity.** `&params.bevel` is a
//! `&FloatParam`; it does not tell a writer that its mirror is `pv.bevel`. That join is what
//! the macros at the bottom of this file supply, and they supply it by naming the field
//! **once**: `srow!(ui, w2, "node bevel", sink, p, bevel)` expands to both `&p.bevel` and
//! `|pv| &mut pv.bevel`. A rename on either side is a compile error, in the same way and for
//! the same reason `param_table!`'s slot lists are.
//!
//! # What the two modes share, and the one thing they do not
//!
//! Identical: the row grid ([`crate::theme::row_grid`]), the label, the ranges, the value
//! formatting, the enum variant names, the ⟲/● affordance — all of it read from the real
//! `OrganicMathParams`, which the Console holds a default instance of purely as **metadata**.
//!
//! ⚠️ **Not identical: the bar itself.** [`Sink::Host`] draws nih-plug's `ParamSlider`;
//! [`Sink::Mirror`] draws an `egui::Slider` over the same normalized 0..1 domain with the
//! param's own formatter. `ParamSlider` takes a `ParamSetter` in its constructor — writing is
//! not a thing it does, it is a thing it *is* — so there is no version of it that a mirror can
//! drive. The two read the same and land on the same grid lines; the fill differs.
//!
//! ⚠️ **Also not identical: ↑/↓ inside an open dropdown.** The editor's own combo live-applies
//! as you move, so the look scrubs; [`choice_row`] commits on the following frame, because the
//! popup borrows the child `Ui` while a write needs the sink. Both differences are recorded in
//! `CONSOLE_ARCHITECTURE.md` §3's ledger rather than living only here.

use crate::preset::PresetValues;
use crate::theme;
use nih_plug::prelude::*;
use nih_plug_egui::egui;
use nih_plug_egui::widgets::ParamSlider;

/// Where a panel's controls write, and where they read their live value from.
///
/// The two arms are not symmetric in cost: [`Host`](Sink::Host) is what Organon has always
/// done, and [`Mirror`](Sink::Mirror) exists because the Console cannot do it. See the module
/// doc for why that is a property of `nih_plug` rather than a gap in this crate.
pub(crate) enum Sink<'a, 'setter> {
    /// Organon's editor: the host owns the value. Writes are gesture-wrapped so the DAW
    /// records automation and undo exactly as it always has.
    Host(&'a ParamSetter<'setter>),
    /// Organon Console: there is no host, so a freely writable [`PresetValues`] owns the
    /// value and the world is driven from it.
    Mirror(&'a mut PresetValues),
}

/// A param whose live value a [`PresetValues`] field can stand in for.
///
/// Two directions, both expressed in the **normalized 0..1 domain**, because that is the one
/// currency every kind of control shares: a slider drags in it, a combo indexes in it, and
/// `Param::normalized_value_to_string` formats from it. Converting through the param's own
/// `preview_normalized`/`preview_plain` rather than reimplementing each range means a skewed
/// float, an integer's rounding and an enum's index all stay the engine's arithmetic.
///
/// ⚠️ **`Param` is sealed, so this cannot be a blanket impl over it** — each of the four kinds
/// Organon actually uses is written out. A fifth kind used in a panel body is a compile error
/// naming the missing impl, which is the right failure.
pub(crate) trait Mirrored: Param {
    /// The type the matching `PresetValues` field holds.
    type Field: Copy;
    fn to_norm(&self, v: Self::Field) -> f32;
    fn from_norm(&self, n: f32) -> Self::Field;
}

impl Mirrored for FloatParam {
    type Field = f32;
    fn to_norm(&self, v: f32) -> f32 {
        self.preview_normalized(v)
    }
    fn from_norm(&self, n: f32) -> f32 {
        self.preview_plain(n)
    }
}

impl Mirrored for IntParam {
    type Field = i32;
    fn to_norm(&self, v: i32) -> f32 {
        self.preview_normalized(v)
    }
    fn from_norm(&self, n: f32) -> i32 {
        self.preview_plain(n)
    }
}

impl Mirrored for BoolParam {
    type Field = bool;
    fn to_norm(&self, v: bool) -> f32 {
        self.preview_normalized(v)
    }
    fn from_norm(&self, n: f32) -> bool {
        self.preview_plain(n)
    }
}

/// ⚠️ **The `u32` a preset stores is the variant's declaration index** — that is what
/// `to_u32`/`from_u32` mean throughout `params.rs`, and what rides `Shared`. Rather than
/// assume index and normalized value are related by `i / (n - 1)`, this routes through
/// nih-plug's own `Enum::from_index` and `preview_normalized`, so an enum whose normalization
/// ever stops being uniform keeps working here for free.
impl<T: Enum + PartialEq> Mirrored for EnumParam<T> {
    type Field = u32;
    fn to_norm(&self, v: u32) -> f32 {
        self.preview_normalized(T::from_index(v as usize))
    }
    fn from_norm(&self, n: f32) -> u32 {
        self.preview_plain(n).to_index() as u32
    }
}

// ---------------------------------------------------------------------------
// The rows
// ---------------------------------------------------------------------------

/// **How a row reaches its mirror field**: a function from the whole [`PresetValues`] to the
/// one field, supplied by the macros below as `|pv| &mut pv.bevel`.
///
/// 🚨 **It is an accessor rather than a `&mut` because the field lives *inside* the sink.**
/// Passing both `&mut Sink` and `&mut pv.bevel` is two mutable borrows of the same value and
/// does not compile; handing the row a way to *take* the borrow, at the moment it needs it and
/// no longer, is the shape that does. A plain `fn` pointer, not a closure — there is nothing
/// to capture, and it keeps every row helper free of a generic parameter nobody reads.
pub(crate) type FieldOf<T> = fn(&mut PresetValues) -> &mut T;

/// The read-only half of [`FieldOf`], for the conditions a panel body asks *between* rows
/// (`if rd!(sink, p, mat_enable) { … }`). Separate so that reading which controls to show does
/// not demand a mutable sink and thereby lock the rows inside the `if` out of writing.
pub(crate) type FieldRef<T> = fn(&PresetValues) -> &T;

/// The row's live value in the normalized 0..1 domain, from whichever side owns it.
fn norm_of<P: Mirrored>(param: &P, sink: &mut Sink, get: FieldOf<P::Field>) -> f32 {
    match sink {
        Sink::Host(_) => param.unmodulated_normalized_value(),
        Sink::Mirror(pv) => param.to_norm(*get(pv)),
    }
}

/// The row's write, in the same domain. The single place a normalized value becomes either a
/// gesture-wrapped host edit or a mirror assignment.
fn set_norm<P: Mirrored>(param: &P, sink: &mut Sink, get: FieldOf<P::Field>, n: f32) {
    match sink {
        Sink::Host(setter) => {
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, n);
            setter.end_set_parameter(param);
        }
        Sink::Mirror(pv) => *get(pv) = param.from_norm(n),
    }
}

/// Scalar control row — `label | bar | value | gap | [⟲/●]`, on [`theme::row_grid`]'s grid.
///
/// The [`Sink::Host`] arm is what `lib.rs`'s `srow` does, verbatim, and must stay that way: the
/// editor is the reference rendering of an Organon panel, not a second opinion about one. Call
/// it through the `srow!` macro so the field is named once — see the module doc.
pub(crate) fn scalar_row<P: Mirrored>(
    ui: &mut egui::Ui,
    avail: f32,
    label: &str,
    param: &P,
    sink: &mut Sink,
    get: FieldOf<P::Field>,
) {
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let g = theme::row_grid(avail, spacing);
        ui.add_sized(
            [g.label_w, 18.0],
            egui::Label::new(egui::RichText::new(label).color(theme::TITANIUM())).truncate(),
        );
        let (rect, _) = ui.allocate_exact_size(egui::vec2(g.bar_w, 18.0), egui::Sense::hover());
        let mut sl = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        sl.set_clip_rect(rect.expand(1.0).intersect(sl.clip_rect()));
        match sink {
            Sink::Host(setter) => {
                sl.add(ParamSlider::for_param(param, setter).without_value().with_width(g.bar_w));
            }
            Sink::Mirror(_) => {
                // The same drag over the same domain — see the module doc on why the fill
                // cannot be the same widget.
                let mut n = norm_of(param, sink, get);
                let resp = sl.add(egui::Slider::new(&mut n, 0.0..=1.0).show_value(false));
                if resp.changed() {
                    set_norm(param, sink, get, n);
                }
            }
        }
        value_box(ui, param, sink, get);
        ui.add_space(g.gap);
        match crate::default_btn(ui) {
            Some(crate::DefaultAction::Record) => crate::record_default(param),
            // ⚠️ The param's *factory* default, not the mirror's starting value. `⟲` means the
            // same thing in both windows or it means nothing.
            Some(crate::DefaultAction::Reset) => {
                set_norm(param, sink, get, param.default_normalized_value())
            }
            None => {}
        }
    });
}

/// The typed readout beside the bar — `lib.rs`'s `value_box`, with the write forked.
fn value_box<P: Mirrored>(ui: &mut egui::Ui, param: &P, sink: &mut Sink, get: FieldOf<P::Field>) {
    // ⚠️ **Keyed by the enclosing `Ui` as well as the param, which `lib.rs`'s `value_box` is
    // not.** Its key is `Id::new("om_value_edit").with(param_ptr)` — absolute, so `push_id`
    // does not scope it — and that is fine in the editor, where a param appears in exactly one
    // card. The Console can hold two `/organon look surface` elements in one transcript reading
    // one params instance, so an absolute key would put both rows' typed-value buffers in the
    // same slot: click one and a text field opens in both. `ui.id()` already carries the
    // element namespace `organon_element` pushed.
    let id = ui.id().with("om_value_edit").with(crate::hash_of(&param.as_ptr()));
    let buf: Option<String> = ui.data_mut(|d| d.get_temp::<Option<String>>(id)).flatten();
    let current = norm_of(param, sink, get);
    match buf {
        Some(mut buf) => {
            let resp = ui.add_sized([theme::VALUE_W, 18.0], egui::TextEdit::singleline(&mut buf));
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(n) = param.string_to_normalized_value(&buf) {
                        set_norm(param, sink, get, n);
                    }
                }
                ui.data_mut(|d| d.insert_temp(id, None::<String>));
            } else {
                if !resp.has_focus() {
                    resp.request_focus();
                }
                ui.data_mut(|d| d.insert_temp(id, Some(buf)));
            }
        }
        None => {
            let text = param.normalized_value_to_string(current, false);
            let resp = ui
                .add_sized(
                    [theme::VALUE_W, 18.0],
                    egui::Button::new(egui::RichText::new(text).small()),
                )
                .on_hover_text("Click to type a value");
            if resp.clicked() {
                let seed = param.normalized_value_to_string(current, false);
                ui.data_mut(|d| d.insert_temp(id, Some(seed)));
            }
        }
    }
}

/// Checkbox row for a `BoolParam` — the one kind whose mirror needs no conversion at all.
pub(crate) fn check_row(
    ui: &mut egui::Ui,
    label: &str,
    param: &BoolParam,
    sink: &mut Sink,
    get: FieldOf<bool>,
) {
    let mut on = match sink {
        Sink::Host(_) => param.value(),
        Sink::Mirror(pv) => *get(pv),
    };
    if ui.checkbox(&mut on, label).changed() {
        match sink {
            Sink::Host(setter) => {
                setter.begin_set_parameter(param);
                setter.set_parameter(param, on);
                setter.end_set_parameter(param);
            }
            Sink::Mirror(pv) => *get(pv) = on,
        }
    }
}

/// Discrete-choice row — `label | dropdown | ⟲`, walking the param's own steps and labelling
/// each through the param's own value-to-string. `lib.rs`'s `param_combo_sized`, forked only
/// at the write.
pub(crate) fn choice_row<P: Mirrored>(
    ui: &mut egui::Ui,
    avail: f32,
    label: &str,
    param: &P,
    sink: &mut Sink,
    get: FieldOf<P::Field>,
    combo_w: f32,
) {
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let g = theme::combo_grid(avail, spacing, combo_w);
        ui.add_sized(
            [g.label_w, 18.0],
            egui::Label::new(egui::RichText::new(label).color(theme::TITANIUM())).truncate(),
        );
        let w = combo_w;
        let current = norm_of(param, sink, get);
        match param.step_count() {
            Some(n) if n >= 1 => {
                let cur_idx = (current * n as f32).round() as usize;
                let (crect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
                let mut cui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(crect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                // ⚠️ **Clip HORIZONTALLY ONLY. A vertical clip makes the dropdown unopenable,
                // and it does it silently** — egui builds a widget's hit area from the clip
                // rect, and `ComboBox` deliberately makes its button taller than its contents,
                // so an 18 pt clip trims exactly the padding that receives the click.
                // `lib.rs`'s `param_combo_sized` carries the full account of how that was
                // found; this is the same arithmetic.
                let pad_x = cui.spacing().button_padding.x + 1.0;
                let parent_clip = cui.clip_rect();
                cui.set_clip_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(crect.min.x - pad_x, parent_clip.min.y),
                        egui::pos2(crect.max.x + pad_x, parent_clip.max.y),
                    )
                    .intersect(parent_clip),
                );
                // ⚠️ **Which step the popup settled on is taken *out* of the closure rather
                // than applied inside it**, because the popup borrows `cui` — itself borrowed
                // from `ui` — while a write needs `sink`. The visible consequence is small and
                // worth naming: the editor's own combo live-applies an ↑/↓ move *while the
                // popup is open*, so the look scrubs as you move; this one commits on the
                // frame after. A borrow, not a design choice.
                let mut chosen: Option<usize> = None;
                let combo = egui::ComboBox::from_id_salt(label)
                    .width(w)
                    // Ellipsize a long selected label instead of widening the button past the
                    // fixed width (the popup shows full names).
                    .truncate()
                    .selected_text(param.normalized_value_to_string(current, false))
                    .show_ui(&mut cui, |ui| {
                        let down = ui
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                        let up = ui
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                        let enter =
                            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        let mut sel = cur_idx;
                        if down && sel < n {
                            sel += 1;
                        }
                        if up && sel > 0 {
                            sel -= 1;
                        }
                        let moved = sel != cur_idx;
                        if moved {
                            chosen = Some(sel);
                        }
                        for i in 0..=n {
                            let norm = i as f32 / n as f32;
                            let text = param.normalized_value_to_string(norm, false);
                            let resp = ui.selectable_label(i == sel, text);
                            if resp.clicked() {
                                chosen = Some(i);
                            }
                            // Keep the keyboard-selected row in view.
                            if i == sel && moved {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        enter
                    });
                if let Some(idx) = chosen {
                    set_norm(param, sink, get, idx as f32 / n as f32);
                }
                if combo.inner == Some(true) {
                    // The popup id hangs off the *response's* button id — the combo was shown
                    // on `cui`, the clipped child, so rebuilding it from `ui` would address a
                    // popup that never existed. `lib.rs` has the full note.
                    egui::Popup::close_id(ui.ctx(), combo.response.id.with("popup"));
                }
            }
            // Single-variant / non-discrete — nothing to choose; just show it.
            _ => {
                ui.add_sized(
                    [w, 18.0],
                    egui::Label::new(param.normalized_value_to_string(current, false)),
                );
            }
        }
        // The ⟲ on the scalar row's right-hand grid line, even though the combo is narrower.
        // Enums keep a plain ⟲ — no recorded default (#131).
        ui.add_space(g.gap);
        if crate::reset_btn(ui) {
            set_norm(param, sink, get, param.default_normalized_value());
        }
    });
}

// ---------------------------------------------------------------------------
// Values a panel computes rather than drags
// ---------------------------------------------------------------------------

/// The live value of a field, from whichever side owns it, **as the param's own plain type** —
/// so `rd!(sink, p, surface_mode)` is a `HostSurfaceMode` and `rd!(sink, p, mat_enable)` is a
/// `bool`, exactly as `params.surface_mode.value()` and `params.mat_enable.value()` are today.
///
/// ⚠️ **This is the half of the join that is easy to forget**, because a panel body reads
/// params to decide which controls to *show* (`if params.mat_enable.value() { … }`), and a
/// forgotten read compiles perfectly while pinning the Console's panel to Organon's defaults:
/// the checkbox ticks and the rows underneath it never appear.
pub(crate) fn read<P: Mirrored>(param: &P, sink: &Sink, get: FieldRef<P::Field>) -> P::Plain {
    match sink {
        Sink::Host(_) => param.modulated_plain_value(),
        Sink::Mirror(pv) => param.preview_plain(param.to_norm(*get(pv))),
    }
}

/// Write a value the panel computed rather than dragged — a loaded material graph, a recall.
/// Takes the param's own plain type, so a call site reads the same under both arms.
///
/// The mirror route goes **through the param's own normalization** (`preview_normalized`, then
/// [`Mirrored::from_norm`]) rather than assigning the field directly, which is what lets one
/// helper serve floats, ints, bools and enums without the caller naming a kind: the plain type
/// coming in and the mirror's storage type going out are both the param's business, not the
/// call site's.
pub(crate) fn write<P: Mirrored>(
    param: &P,
    sink: &mut Sink,
    get: FieldOf<P::Field>,
    v: P::Plain,
) {
    set_norm(param, sink, get, param.preview_normalized(v));
}

// ---------------------------------------------------------------------------
// Choosing the control from the param
// ---------------------------------------------------------------------------

/// **Which control a param gets, decided by the param's own type.**
///
/// 🚨 **This is a measurement, not a convention.** Across every one of the Look tab's **519**
/// rows — the twenty-five cards of Organon's editor as they stood when [`crate::panel_table`]
/// was written — there is **not one** case where the editor's choice of control disagrees with
/// the Rust type of the param behind it:
///
/// | param | rows | control |
/// |---|--:|---|
/// | [`FloatParam`] / [`IntParam`] | 399 | [`scalar_row`] |
/// | [`BoolParam`] | 83 | [`check_row`] |
/// | [`EnumParam`] | 37 | [`choice_row`] |
///
/// So a declarative panel table does **not** have to carry the widget kind, and carrying it
/// would be a second statement of something the type system already says. What the table
/// carries instead is the label and the grouping, which the param genuinely cannot supply.
///
/// ⚠️ **`Param` is sealed, so this cannot be a blanket impl** — the same constraint
/// [`Mirrored`] has, and the same right failure: a fifth param kind used in a panel is a
/// compile error naming the missing impl rather than a control silently drawn as a slider.
pub(crate) trait AutoRow: Mirrored {
    /// Draw this param's row. `combo_w` is consulted only by the kinds that open a dropdown;
    /// the others take their width from `avail` and the shared row grid, exactly as they do
    /// when a panel body calls them by name.
    fn auto(
        &self,
        ui: &mut egui::Ui,
        avail: f32,
        label: &str,
        sink: &mut Sink,
        get: FieldOf<Self::Field>,
        combo_w: f32,
    );
}

impl AutoRow for FloatParam {
    fn auto(
        &self,
        ui: &mut egui::Ui,
        avail: f32,
        label: &str,
        sink: &mut Sink,
        get: FieldOf<f32>,
        _combo_w: f32,
    ) {
        scalar_row(ui, avail, label, self, sink, get);
    }
}

impl AutoRow for IntParam {
    fn auto(
        &self,
        ui: &mut egui::Ui,
        avail: f32,
        label: &str,
        sink: &mut Sink,
        get: FieldOf<i32>,
        _combo_w: f32,
    ) {
        scalar_row(ui, avail, label, self, sink, get);
    }
}

/// ⚠️ **A checkbox ignores `avail`**, because [`check_row`] is a plain `ui.checkbox` rather than
/// a row on [`theme::row_grid`]'s grid — `lib.rs`'s `crow` has never taken a width and this must
/// keep drawing what it draws.
impl AutoRow for BoolParam {
    fn auto(
        &self,
        ui: &mut egui::Ui,
        _avail: f32,
        label: &str,
        sink: &mut Sink,
        get: FieldOf<bool>,
        _combo_w: f32,
    ) {
        check_row(ui, label, self, sink, get);
    }
}

impl<T: Enum + PartialEq> AutoRow for EnumParam<T> {
    fn auto(
        &self,
        ui: &mut egui::Ui,
        avail: f32,
        label: &str,
        sink: &mut Sink,
        get: FieldOf<u32>,
        combo_w: f32,
    ) {
        choice_row(ui, avail, label, self, sink, get, combo_w);
    }
}

/// A control at the default dropdown width — `lib.rs`'s `srow` / `crow` / `param_combo`, chosen
/// by the param. See [`AutoRow`].
pub(crate) fn auto_row<P: AutoRow>(
    ui: &mut egui::Ui,
    avail: f32,
    label: &str,
    param: &P,
    sink: &mut Sink,
    get: FieldOf<P::Field>,
) {
    param.auto(ui, avail, label, sink, get, theme::COMBO_W);
}

/// A control at `2 × COMBO_W` — `param_combo_sized`'s hero width, for the few dropdowns whose
/// variant names need the room (`generator`, `palette`, `surface_mode`, …). Identical to
/// [`auto_row`] for every control that does not open a dropdown.
// ⚠️ No caller until a declared panel has a `wide` item in it — the three panels the table
// starts with are sliders and a checkbox. `panel_table`'s `@draw`/`@one` arms reach it the
// moment one does, and the `allow` comes off then.
#[allow(dead_code)]
pub(crate) fn auto_row_wide<P: AutoRow>(
    ui: &mut egui::Ui,
    avail: f32,
    label: &str,
    param: &P,
    sink: &mut Sink,
    get: FieldOf<P::Field>,
) {
    param.auto(ui, avail, label, sink, get, 2.0 * theme::COMBO_W);
}

// ---------------------------------------------------------------------------
// The identity join
// ---------------------------------------------------------------------------
//
// Each macro names the field **once** and derives both handles from it — `&$p.$f` for the
// metadata and `|pv| &mut pv.$f` for the mirror. That is the whole of what was missing; see
// the module doc. Converting a panel is therefore a mechanical edit the compiler checks: a
// field present on `OrganicMathParams` but not on `PresetValues` (or the other way round)
// fails to expand, and one whose two sides disagree about type fails to unify.
//
// ⚠️ **`pub(crate) use`, not `#[macro_export]`, deliberately.** Exporting would hoist names as
// generic as `srow!` / `combo!` / `rd!` to the crate root *and* into this crate's public API,
// where they would sit beside `lib.rs`'s own `srow`/`crow` functions and read as the same
// thing. Import them where a panel body needs them:
// `use crate::param_sink::{combo, crow, rd, srow};`

/// `srow!(ui, avail, "label", sink, params, field)` — a scalar row. See the module doc.
macro_rules! srow {
    ($ui:expr, $avail:expr, $label:expr, $sink:expr, $p:expr, $f:ident) => {
        $crate::param_sink::scalar_row($ui, $avail, $label, &$p.$f, $sink, |pv| &mut pv.$f)
    };
}

/// `crow!(ui, "label", sink, params, field)` — a checkbox row. See the module doc.
macro_rules! crow {
    ($ui:expr, $label:expr, $sink:expr, $p:expr, $f:ident) => {
        $crate::param_sink::check_row($ui, $label, &$p.$f, $sink, |pv| &mut pv.$f)
    };
}

/// `combo!(ui, avail, "label", sink, params, field, width)` — a dropdown row. See the module doc.
macro_rules! combo {
    ($ui:expr, $avail:expr, $label:expr, $sink:expr, $p:expr, $f:ident, $w:expr) => {
        $crate::param_sink::choice_row($ui, $avail, $label, &$p.$f, $sink, |pv| &mut pv.$f, $w)
    };
}

/// `rd!(sink, params, field)` — the live value, from whichever side owns it. See [`read`].
macro_rules! rd {
    ($sink:expr, $p:expr, $f:ident) => {
        $crate::param_sink::read(&$p.$f, $sink, |pv| &pv.$f)
    };
}

/// `wr!(sink, params, field, value)` — a computed write. See [`write`].
macro_rules! wr {
    ($sink:expr, $p:expr, $f:ident, $v:expr) => {
        $crate::param_sink::write(&$p.$f, $sink, |pv| &mut pv.$f, $v)
    };
}

pub(crate) use {combo, crow, rd, srow, wr};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{HostSurfaceMode, MatChannel, OrganicMathParams};

    /// The round trip the whole mirror rests on: a `PresetValues` field converted into the
    /// normalized domain and back is the value it started as. Checked on all four kinds,
    /// because each takes a different route through `preview_plain`.
    #[test]
    fn every_mirrored_kind_round_trips_through_the_normalized_domain() {
        let p = OrganicMathParams::default();
        // f32, over a skewed range
        for v in [0.0f32, 0.25, 0.5, 1.0] {
            assert_eq!(p.bevel.from_norm(p.bevel.to_norm(v)), v, "bevel {v}");
        }
        // i32
        for v in [1i32, 4, 8] {
            assert_eq!(p.mp_octaves.from_norm(p.mp_octaves.to_norm(v)), v, "octaves {v}");
        }
        // bool
        for v in [false, true] {
            assert_eq!(p.mat_enable.from_norm(p.mat_enable.to_norm(v)), v, "enable {v}");
        }
        // enum, by the index a preset actually stores
        for v in 0..=p.surface_mode.step_count().unwrap() as u32 {
            assert_eq!(p.surface_mode.from_norm(p.surface_mode.to_norm(v)), v, "surface {v}");
        }
    }

    /// 🚨 The convention the identity join depends on, stated as an assertion rather than as
    /// prose: the `u32` a `PresetValues` field holds is the same index the param's own
    /// `to_u32` produces. If nih-plug ever normalized enums non-uniformly this would catch it
    /// before a dropdown started selecting the wrong variant.
    #[test]
    fn an_enum_mirrors_at_the_index_presets_store() {
        let p = OrganicMathParams::default();
        for m in [HostSurfaceMode::Original, HostSurfaceMode::Metaball, HostSurfaceMode::Voxel] {
            let idx = m.to_u32();
            assert_eq!(HostSurfaceMode::from_u32(p.surface_mode.from_norm(p.surface_mode.to_norm(idx))), m);
        }
        for c in [MatChannel::Albedo, MatChannel::Roughness] {
            let idx = c.to_u32();
            assert_eq!(p.mp_channel.from_norm(p.mp_channel.to_norm(idx)), idx);
        }
    }

    /// A mirror write lands in the `PresetValues` and nowhere else — the property that makes
    /// the Console's overlay safe to compute as a diff.
    #[test]
    fn a_mirror_write_touches_only_its_own_field() {
        let p = OrganicMathParams::default();
        let mut pv = PresetValues::capture_params_only(&p);
        let before = pv.clone();
        pv.bevel = p.bevel.from_norm(0.5);
        assert_ne!(pv.bevel, before.bevel);
        let mut probe = pv.clone();
        probe.bevel = before.bevel;
        assert_eq!(probe, before, "a bevel write moved something else");
    }
}
