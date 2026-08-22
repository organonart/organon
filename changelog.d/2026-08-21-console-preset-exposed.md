### A preset records what it is *about*, and every preset that already exists keeps loading

Groundwork for building a panel out of a preset (organon#124). James: *"we can tell from the
preset what values we have adjusted from the default … we could construct custom panels or even
a single custom panel with sections and sliders and dropdowns that are **tailored to the exact
changes that we made on that preset**."*

**The diff itself is easy and now exists**: `PresetValues::changed_from_default()` answers which
fields a preset moved off the factory baseline, by serialized name. 📌 **There is no field list
in it** — it serializes both sides and compares values, walking `for_each_tab_field!`'s own
declaration order — so a param added to `PresetValues` appears in the diff with no edit anywhere.
Walking that table rather than the JSON's own keys does three things at once: a stable
declaration order instead of serde's map order; only fields a preset actually captures, so the
per-display quality settings (`taa_*`, `pathtrace_enable`, `rt_debug`, …) that are deliberately
absent from the tab partition are absent from a diff too; and no `hdr_path`, which is a file path
rather than a value and has no control to draw.

🚨 **But a pure diff has two flaws, and `Preset::exposed` is what closes them.** A preset that
deliberately sets a value **to** its default is invisible to a comparison — and returning
something to neutral is a real compositional act. And **volume**: a preset may differ in hundreds
of fields, and a "tailored" panel showing all of them is not tailored. So a preset now carries an
optional set of exposed field names, seeded from the diff when a person saves one and editable
afterwards. What a preset is about becomes **stated rather than inferred**.

⚠️ **`None` means "nobody has said", and it is the answer for every preset that exists today.**
`#[serde(default)]` is what lets a `presets.json` written before this parse unchanged, and
`Preset::exposed_fields()` falls back to the diff for such a preset — so the feature works on a
store nobody has re-saved and gets better when they do. `a_stored_preset_with_no_exposed_key_still_loads`
pins it on the wire, against JSON with no `exposed` key at all.

🚨 **`subset_entry` builds its JSON object by hand, so the new field had to be written there or
it would have been dropped in silence.** That function assembles `{"name", "values"}` rather than
serializing `Preset`, which means a new field on the struct round-trips in memory, survives a
load, and vanishes on the next save — the quietest failure available.
`the_exposed_set_survives_a_save` is the guard. The set is filtered to the same bucket as the
values for the same reason they are: a tab subset that named a field it did not write would build
a panel row over a value the file does not hold.

⚠️ **A name this build no longer has is reported, never dropped.** The set is serialized field
names, so a renamed param silently leaves it. `exposed_fields()` answers only what it can draw
and `unknown_exposed()` carries the rest out by name, for the surface with a human in front of it
to print — the house rule about refusing by name, one layer down.

⚠️ **Nothing draws any of this yet**, and no `Shared` field moved. The panel that consumes it and
the `/preset` verbs that load it are the next changes; this is the format they need, landed
separately so that "old presets still load" is a claim with its own tests rather than a line in a
larger PR.
