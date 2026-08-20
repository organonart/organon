### Organon Console: a `panel` region is a scrolling stack of panels, and a panel no longer lives in the transcript

A region holding `panel` drew one sentence apologising for itself — *"an Organon editor panel
belongs here. Tier 2 gives it a body."* It has a body now: **a scrolling column of Organon's own
editor panels**, added and removed by verb, in a rectangle whose size has nothing to do with how
many panels are in it. James's framing: *"we should be able to pop up panels in one of the
viewports we have assigned as a panel … so we could create our own stacks that would scroll. And
that means even if a viewport took up only the top left or top right, we could still scroll many
panels with the same scrolling mechanism."*

🚨 **The stack removes the blocker rather than working around it.** `CONSOLE_ARCHITECTURE.md` §2
recorded the obstacle as *"a third word naming which panel, since two rings cannot say it"* —
`/viewport <region> <content>` already spends both of its argument rings. A stack dissolves that
by splitting the sentence in two: `/viewport left panel` declares the region, and a **different**
command puts a panel in it. Nothing here needs three rings, and that is a property of the split
rather than of a longer grammar. The new verb is `organon console stack <action> <panel>` —
`add` or `remove`, then a slug — two required rings, exactly `viewport`'s shape.

🚨 **Emptying the column is `stack remove all`, and "clear" is deliberately not a third action.**
The slash grammar fills *required* arguments positionally and *optional* ones by keyword, so an
optional trailing panel would have made the typed line `/stack add panel surface` while the CLI
stayed `organon console stack add surface` — one verb with two spellings, which is the drift this
tree spends most of its refusals preventing. Both words are required, and the emptying word rides
the **panel** ring as `all`: `region::CLEAR_WORD`'s own arrangement one module over, on the
precedent `console.background`'s three backdrop *sources* set. ⚠️ `add all` is refused by name
rather than read as "every panel" — filling a column from a word somebody typed meaning the
opposite is not a convenience.

🚨 **A panel lives only in a stack. The transcript is no longer a home for one, and that route is
retired rather than left unreachable.** James, on being shown the fallback: *"Would we ever want a
panel inline? A panel should not scroll away. That doesn't make sense."* The sharp form is that
**a transcript is a log and a control is not a log entry** — a panel is used while watching what
it changes, and a control that scrolls off mid-drag was never usable. `/organon look surface` now
targets the stack, and with no region holding one it is **refused by name**, with the command that
makes one; it does not fall back and it does not silently do nothing. `Body::Organon`,
`OrganonBlock`, `Transcript::insert_organon` and `organon_element` are gone with it, and
`conversation_view::draw` no longer takes an `OrganonDraw` at all. 📌 One piece of evidence
that made this cheap: §3's ledger recorded the inline panel as *"reached, not seen"* — no human
had ever looked at one — so this retires something that was never validated rather than removing
something known to work. ⚠️ `/surface` is untouched: a rendered surface with its own controls
beneath it is an artifact travelling with its panel, which is a different thing.

🚨 **The egui-id collision, third instance, settled before the draw call was written.** §1.11
records this fixed twice — `organon_element` scoped widgets by the panel's *slug* (separating two
different panels, which could never collide, while merging two elements of the *same* panel), and
the typed-value box's key was absolute (`Id::new("om_value_edit")` plus the param pointer), so
clicking one value box opened a text field in both. Both fixes say one thing: **an Organon
panel's egui namespace is where it is drawn, never what is drawn.** So `panel_stack::draw` pushes
the region's own word and then, per panel, a **serial issued once and never reused** — removing
the third panel cannot hand its open dropdown or half-typed value to the fourth. Both pushes
happen *inside* `draw` rather than being inherited from the caller's `Ui`, which is what lets the
property be tested here instead of resting on an id salt in another crate. The test draws four
Surface bodies in one frame — one stack shown by two regions, two copies inside it — under
parents deliberately given the **same** id salt, and asserts that the exact key
`param_sink::value_box` builds (`ui.id().with("om_value_edit").with(<param ptr>)`, one param
behind all four) is distinct at every site. A companion test removes each half of the key in turn
and requires the namespaces to collapse, so it is a mutation test rather than an assertion that
happens to hold.

🚨 **One stack, console-wide — every `panel` region is a view of it.** Two reasons. The first is
the argument `panel_surface::OrganonPanels` already makes one level down: two panel regions are
two views of one instrument, and a column that read differently in each would make the claim
`/organon` exists to make false on sight. The second is this tree's own rule about unreachable
arms — the add verb has no room for a region word, so a per-region stack would give every region
after the first a column nothing could ever fill. What *is* per-region is the scroll position,
because the scroll area is keyed by the region. `/organon`'s answer names the first region holding
`panel` in `Region::ALL` order (largest first, the same determinism that decides which `agent`
region gets the live tab), so a person always learns which rectangle to look at.

⚠️ **Only Look ▸ Surface has a body, and no second panel was transplanted.** §1.11 requires a hand
to confirm the first one moves the picture and nobody has done that; the other twenty-four draw
their existing honest "not transplanted yet" line **inside the stack**, which is a stack of
twenty-four named things rather than twenty-four empty boxes.

⚠️ **The wheel over a stack no longer scrolls the transcript.** §1.14 predicted this exactly —
*"it becomes real the moment a region holds something scrollable"* — because `term_view` reads the
wheel from raw input and nothing tells it which region the pointer is in. The panel regions join
the portal and the `3d` region in the rectangle list `term_view::draw` already tests against;
`portal::pointer_inside_any` is the one mechanism, not a second gesture vocabulary.

📌 **The stack takes its target.** `panel_stack::draw` paints into the `egui::Ui` it is handed
and never reaches for the context, a named layer or the window. That is a spelling choice, not a
feature: James's standing note is that the console's own surface is meant to become a physically
lit 3D surface (#17), and `egui → texture` is the half the console does not have — so a stack
that assumed "the window" would be something to unpick later. **Nothing was built toward it** —
no offscreen path, no texture per region, no producer machinery, since a texture and a copy per
region per frame is a real cost for a capability nothing uses. It cost one parameter that was
going to be there anyway.

📌 **Default-inert.** A console nobody has typed `/viewport` at runs the identical code it did
before: the stack is empty, no region holds `panel`, and `redraw`'s single-region fast path is
untouched. An empty stack region keeps saying so, and now names the verb that fills it.

⚠️ **What a green build does not prove.** Nobody has looked at this. Whether a column of Organon's
controls beside a live transcript reads as the instrument's own editor or as a cramped imitation
of it, whether the gap between cards is right, and whether scrolling a stack next to a running
conversation feels right are all James's calls and no amount of green answers any of them.
