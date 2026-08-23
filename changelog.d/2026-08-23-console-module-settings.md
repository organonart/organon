### Added

- **`console setting <producer> <key> <value>`** — `/setting moonlight host studio-pc` in the
  composer — writes one setting into an approved module's own settings file, so a typed word can
  reach a hosted module that is **already running**. `CONSOLE_ARCHITECTURE.md` §1.22 owns the
  mechanism.
- **`[[settings]]` in `organon-module.toml`.** A module declares the keys it answers to; the
  console records them at approval and refuses every other key by name. 🚨 It never learns what a
  key *means* — `doc/organon_module_viewport.md` §4.6's *"never: what the module is"*, read from
  the configuration side — so `SettingSpec` carries a word and a line of prose, and no type.
- **`ORGANON_MODULE_SETTINGS`** in a module's launch environment: the second derived handoff
  beside `ORGANON_MODULE_CHANNEL`, naming `<store>/module-settings/<producer>.json`.

### Notes

- 🚨 **A file rather than a message, on purpose.** `organon-module`'s input ring carries four
  verbs and refuses a generic message as *"the one addition that would make every future verb
  free, which is to say ungranted for ever."* That refusal is right and it left no way to type a
  host name at a viewport. A file both ends already agree on costs the protocol nothing.
- ⚠️ **The vocabulary travels with the approval, not with the repository.** A module that adds a
  setting in a later commit does not gain it until that commit is approved — §3.2's *"the unit is
  a commit"* applied to configuration.
- ⚠️ **The value is the rest of the line** (`ConsoleOp::Preset`'s rule), so a machine called
  `attic nas` survives — which is what makes the no-whitespace rule on a key load-bearing.
- 📌 `Reversal::Recoverable`, the one verb near `console module` that earns it, which is also why
  it is a verb of its own rather than a sixth `console module` action.
