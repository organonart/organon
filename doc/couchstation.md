# The CouchStation — principles, findings, and open questions

> **What this document is.** The durable capture of a design conversation held
> 2026-08-30/31, covering the form factor, its anthropometric basis, its pattern
> language, its input model, and the shape of the project that would publish it. It
> exists so a new session can pick this up without re-deriving any of it.
>
> **What it is not.** A specification. The numbers that would make it one have not been
> taken yet — see §10. This is the reasoning that a spec would be built on, plus an
> honest account of which parts are observed and which are argued.
>
> **Status.** n=2. Two builds, two houses, six days. Everything here is provisional and
> most of it is one person's experience. Read §1's ledger before quoting any of it.

---

## 1. The honesty ledger

The house convention from `MIND_ARCHITECTURE.md` §3 transfers directly, and a hardware
claim written from a single body needs it more than a software one does. Every claim in
this document carries one of these:

| Marker | Means |
|---|---|
| **Observed** | Happened, repeatedly, to one person in two builds. Not measured with an instrument. |
| **Measured** | A number taken with a tool. ⚠️ **There are currently none.** |
| **Derived** | An exact consequence of something observed or measured. |
| **Argued** | Reasoned from first principles or from an adjacent field. Not yet tested here. |
| **Untested** | Proposed and not tried at all. |

🚨 **The single most important fact about this document is that the Measured column is
empty.** Everything below is Observed, Derived, or Argued. That is fine for a design
brief and disqualifying for a standard. §10 lists what has to be measured to fix it.

---

## 2. The claim

**A desk is not furniture. It is ergonomic infrastructure for a hand problem.**

Every dimension of every desk and every task chair descends from one requirement: that
both hands be held at the same height, on the same plane, a fixed distance apart, for
hours. That is a *typing* requirement. It is the most constraining single demand in
conventional computing and nothing else about the desk survives its removal.

When voice carries the content and the hands carry only intent, the requirement is gone.
The two hands stop having to mirror each other, and each is free to rest wherever its own
arm naturally falls. **The furniture did not get replaced. It got obsoleted**, because
the devices stopped needing a shared surface. *(Argued, and Observed to hold across two
builds.)*

What replaced it was already in the room. A large screen on a low mass, a couch at
viewing distance, controllers in the lap — that is the console living room, the most
comfortable human-computer posture ever mass-produced, sitting unused for work since the
1990s because work meant typing. *(Argued.)*

---

## 3. The empirical basis — two builds

The move between houses was an accident that functioned as a controlled trial. Different
room, different furniture, no shared objects. What reassembled itself is the invariant.

| | **Build A** — Zephyr Cove (rental, ~5 days) | **Build B** — current house |
|---|---|---|
| Support | Leather couch, bed-rest pillow with arms | Beanbag (Target), legs extended |
| Deck | Entertainment center / TV stand | Low stand; subwoofer as center plinth |
| Near screen | 49" curved ultrawide on the deck | Same panel, lower |
| Far field | Ceiling projector | Projector |
| Left hand | Razer Tartarus on couch cushion | Tartarus on a stool, height-fitted |
| Right hand | Mouse on pad on couch arm | Mouse on a stool, height-fitted |
| Compute | Mini PC on the deck | Mini PC on the plinth |
| Console | — | Subwoofer top: mic, Stream Deck, indicator lamp |

**Did not survive:** the entertainment center, the couch as primary support, the room,
the wall, every specific object.

**Survived:** reclined and fully supported, legs extended. Two hands at two *independent*
heights, each on its own support. Big panel near, projection far. Push-to-talk under the
thumb. Compute and controls resting on found objects chosen for their *height*. Nothing
mounted to a wall. Whole station rebuilt in a strange house in days.

📌 **The geometry survived and the furniture did not.** That is the finding that makes a
body-relative specification the right form and a product list the wrong one. *(Observed.)*

⚠️ Build A is gone. Its photographs remain and contain a panel of known width (49"
diagonal, 16:9), so its dimensions are recoverable by scaling to within an inch or two.
That reconstruction has not been done and should be, before the photos are the only
record and nobody remembers which lens.

---

## 4. The anthropometric basis

### 4.1 Neutral body posture is the target

The correct reference for a long-duration reclined workstation is not motorsport. It is
**neutral body posture** — the position the human body assumes in microgravity under no
external load, measured by NASA for spacecraft design (NASA-STD-3000). Approximately:
hip and knee each open to ~128–133°, head slightly forward, neck gently flexed, and —
the part that matters most here — **the arms float forward at roughly mid-torso height,
forearms near horizontal, elbows bent past 90°.**

A reclined station is the closest one-G approximation available, because reclining
transfers spinal load from the discs to the backrest. **The design target is a one-G
approximation of neutral body posture.** *(Argued.)*

⚠️ **This yields an immediate correction to Build B.** Stools put the hand supports down
near hip level. Neutral posture wants them up near chest height with the forearm level.
Untested; cheap to try.

### 4.2 Dimensions should be ratios, not inches

Express every dimension as a **ratio of a body measurement** and tolerance the ratio.
Screen distance as a coefficient of seated eye height; deck height as a function of
seated elbow height; hand-support reach as a function of the cubit — which is literally
elbow-to-fingertip, the measurement that governs where a hand controller can rest.

Three things fall out for free: the spec scales to any human without a second document;
a fitting flow becomes multiplication rather than modelling; and tolerance bands stop
being invented. This is the Vitruvian move — *the body specifies, the furniture
complies* — and it is the through-line of the whole project. *(Argued.)*

### 4.3 Population data

**ANSUR II** — the US Army anthropometric survey, public domain, several thousand
subjects, ~90 measurements each — is the honest substrate for turning one body into a
population. It gives real percentile distributions instead of invented ranges.

⚠️ **Caveat that belongs in the published spec, not in a footnote:** it is a military
sample and skews younger and fitter than the general population. Naming that limitation
is more rigorous than most published ergonomics manages.

---

## 5. The pattern language

Written in the form of Christopher Alexander's *A Pattern Language* — name, context,
conflict, and a `Therefore:` resolution. The form is chosen deliberately: Alexander is
the origin of "design patterns" in software, so a project bridging a workstation and an
operating system using his structure closes a loop that has been open since 1977.

Ordered large to small, as he does.

**1. Separation of Repose.** A couch you work on stops being a couch you rest on.
*Therefore: dedicate a support to the station and leave another for rest — never
colonize the room's primary seating.*

**2. The Room Is The Monitor.** At reclined distance the display stops being a window you
lean toward and becomes a wall you sit back from. *Therefore: bias every choice toward
larger-and-farther over smaller-and-closer, and light the surface behind the screen — a
bright panel in a dark room at distance is the station's main source of visual fatigue.*

**3. Near Field And Far Field.** A desk and a sim rig both trap the eyes at one focal
distance for hours, which is the actual mechanism behind most "screen fatigue."
*Therefore: give the station two focal planes — a near screen for text and work, a far
projection for state and monitoring — and move between them. Refocusing is what prevents
accommodative strain. The far field is the room, not part of the frame.*

**4. Nothing Touches The Walls.** *Therefore: every element rests, clamps, or leans — no
drilling, no wall mounts, no landlord permission. The station assembles by placement,
which is what makes it available to renters and survivable in a borrowed house.*

**5. One Carload.** *Therefore: no element may exceed what one person can carry to a car
alone.* This quietly rules out the standing desk, the monitor arm on a bolted base, and
the office chair. It is why the station reassembles in a strange house in an hour, and it
was proved under evacuation rather than in theory.

**6. The Deck.** A single low horizontal mass carries the screen, the compute, the
cables, and the light. *Therefore: choose one broad low surface at roughly 24–32" rather
than a desk at 29–30" — because the screen sits* on top *of it, and that delta is the
whole discovery.*

**7. Recline Sets Height.** Screen height and recline angle are not independent;
reclining rotates the eye line upward. *Therefore: choose recline first, because it is
the comfort variable, and derive screen centre height from it — never the reverse.*

**8. The Two Postures.** Agentic work alternates between *directing* and *waiting*, and
those are different bodies: leaning in with hands engaged and close focus, then leaning
back with hands off entirely. **The wait state is a first-class posture and no furniture
on earth acknowledges it** — a car has one posture because you never hand the wheel to
something else and watch. *Therefore: build two detents, Direct and Observe, and make the
transition one motion.* Build B already has this, accidentally, as two pieces of
furniture — beanbag plus near screen to direct, couch plus projector to observe.

**9. The Pillow Is The Chair.** Rigid seating enforces one posture; a body at rest wants
several across a day. *Therefore: use compliant support you re-form by hand rather than a
mechanism you adjust — the support should have no correct configuration.*
⚠️ **Compliant support has a conform-in period.** The beanbag was uncomfortable on first
sitting and became the best support tried. The showroom test is actively misleading and
the spec must say so, or every reader will reject the right answer after eight seconds.
*(Observed.)*

**10. The Hand Lands First.** All conventional ergonomics runs backwards: buy the chair,
buy the desk, set the heights, then train the body to meet the furniture — every
adjustment knob on an office chair is an apology for that. *Therefore: let the hand fall
where it wants with nothing under it, watch where it lands, and move a support to that
point.* Requires no tape measure, no calculation and no purchase. It is the Vitruvian
principle made operational and it is the most original idea in this project.

**11. Independent Hand Planes.** Typing demands both hands coplanar, coequal and a fixed
distance apart. *Therefore: support each hand separately at whatever height its own arm
falls to, and never require the two to agree.* This is the pattern that removes the desk.

**12. Voice Carries Content, Hands Carry Intent.** Speech is high-bandwidth and
imprecise; hands are low-bandwidth and exact. *Therefore: never make voice do selection
or navigation, never make hands compose prose, and put the seam between them somewhere
physical.*

**13. The Trigger Under The Thumb.** The keystone — remove it and the station collapses
back into a desk within a day. *Therefore: bind the talk boundary to a control already
under a resting finger. Never a wake word, which is both a false-trigger problem and a
live microphone in a room where you also live.*

**14. Silent Mass.** The computer left the floor under a desk and arrived at ear level,
four feet from your head. *Therefore: treat acoustic output as a hard spec item, and
prefer a machine small enough to hide inside the deck.*

**15. Commodity Substrate.** *Therefore: never specify a dimension that no existing
mass-market product already satisfies.* A design constraint, not an economic accident: it
bounds the spec to reachable geometry and is the reason the form can propagate at the
speed of an idea rather than a supply chain.

📌 **The load-bearing pair is 11 and 13.** Eleven removes the desk; thirteen makes eleven
survivable. Everything else is consequence.

---

## 6. The pattern inside the patterns

Every time so far, **the correct answer has come in cheaper than the expected one.** A
$50 Marketplace TV stand beat a desk. A Target beanbag beat a leather couch. A subwoofer
beat a monitor stand. Two stools beat an ergonomic tray system.

That is not luck four times running. It is what happens in a design space nobody has
worked in: the good solutions are lying around unclaimed because no one thought to look.
It is the strongest argument for publishing a **standard** rather than a **product**, and
it is also the anti-elitism that keeps the entry tier honest. *(Observed.)*

---

## 7. The input model

### 7.1 Channels

Voice for content. Two hands for intent — left on a chorded controller (currently a Razer
Tartarus), right on a mouse. Feet, unclaimed as of this writing, for state.

**The spec should define an input *class*, never a product**: voice-primary, plus two
independently-supported hand controllers, plus a push-to-talk trigger reachable without
repositioning either hand. Specify the class and the spec outlives the hardware.

### 7.2 Feet — the modal speech idea

Feet are good at *hold*, good at *coarse analog pressure*, and useless at precision. That
is an exact match for push-to-talk, which is the most frequent action in the workflow and
currently consumes the most valuable digit on the left hand.

The design that matters is not convenience, it is **disambiguation**:

| Foot | Speech means |
|---|---|
| no pedal | nothing — you are talking to a person, or thinking aloud |
| right pedal | dictation — text into the buffer |
| left pedal | command — interpreted and executed |

Same words, three meanings, resolved by the body. The mode cannot get stuck because
releasing ends it, and you always know which mode you are in because you can feel it.
Every shipping assistant guesses at this with wake words, transcript heuristics and modal
toggles. *(Untested. Argued strongly.)*

### 7.3 The throttle — effort as an analog control

**Pedal depth at the moment of asking sets how much compute the ask is worth.**

The reason this beats the state of the art: an entire category of software — model
routers — tries to *predict* which model a query deserves by classifying the prompt. The
information they need is not in the text, it is in the person. You always know how hard
your question is at the instant you ask it, and that knowledge currently has nowhere to
go. **Every router is trying to infer something your foot could simply tell it.**

Design decisions that came out of the discussion:

- ⚠️ **Map the pedal to an effort budget, not to a model.** Position → a normalized
  scalar; a config maps the scalar onto model choice, thinking budget, tool latitude, and
  agent fan-out. Model lineups turn over every few months; "how much is this worth" is
  permanent. Same move as specifying the input class rather than the Tartarus.
- **Sample at release, not at press.** You speak on the pedal and wherever your foot is
  when you stop talking is your bid — which matches how difficulty is actually
  discovered, three seconds into a question you thought was simple.
- **Quantize into three or four zones**, ideally with physical detents. Feet are not
  precise.
- **The indicator lamp becomes a tachometer.** Green → amber → red as you press: a live
  readout of what you are about to spend, without moving your eyes. A fuel gauge for
  session spend follows for free.
- **Escalation in flight** is the version nobody else will have: press deeper while a
  cheap model is running and the request upgrades mid-generation.

### 7.4 The brake, and the third mapping

If the throttle is analog, so is stopping. **Light brake** — pause, keep everything.
**Full brake** — abort and revert. The pedal box then covers the two most frequent
operations in agentic work, both of which are genuinely continuous and both of which are
currently expressed as menu items.

The third candidate is **autonomy** — light press asks before each action, floored means
go. It is arguably the deepest of the three, because permission is the worst UX in agent
tooling: a modal setting configured hours before the moment it matters. 🚨 **But a foot
slip must never mean "do irreversible things without asking."** Autonomy wants a hard
detent or a second pedal, never a continuous slide.

📌 **Effort, interruption and autonomy are all things you feel continuously and currently
express discretely, out of band, through a menu.** That is not a coincidence — it is a
whole class of controls that got flattened into settings because there was no free limb
to put them on. The limb was always there.

---

## 8. The cockpit — what the sim racing world gets right and wrong

Sim rigs look like dentist chairs for structural reasons, not styling ones. Every design
driver behind them is a solution to a problem this form does not have:

| Sim rig driver | The CouchStation |
|---|---|
| A racing seat is a **clamp** — bolsters resist 2–4 lateral G | Zero lateral G. Every bolster is pure loss. |
| Rigidity is the feature; the body must not move relative to the wheel | Posture must change through the day |
| 20–90 minute stints at high adrenaline | 4–12 hours at low arousal, where everything is noticed |
| Exactly one posture, both hands on the wheel throughout | Two postures; hands leave, rest, and return |
| One focal plane, one gaze direction | Near field and far field (Pattern 3) |
| 8040 profile resists 15–20 Nm of direct-drive wheel torque | Structural load is a forearm and a mouse |

⚠️ **The market is over-engineered for a force load this form does not have, and that
over-engineering is exactly what violates One Carload.** A 60–100 lb rig exists to resist
a wheel. 4040 or 2040 extrusion is ample here.

**Steal the skeleton, reject the flesh.** The extrusion, clamps and quick-releases are
right. The seat and the entire visual language are wrong.

### 8.1 The architectural idea

**A sim rig is rigid everywhere. A couch is compliant everywhere. A coding cockpit is
rigid in the tool frame, compliant in the body frame, and the two are coupled.**

Rigid where the devices live, so the hand finds a control without looking and it is where
it was yesterday. Compliant where the body lives, so eight hours does not hurt. Coupled,
so changing posture *preserves relative geometry* rather than abandoning it. Nothing on
the market does the third part: a sim rig holds geometry but forbids posture change, a
recliner permits posture change but destroys geometry. *(Argued.)*

### 8.2 Form, in prose

A low wide **base/sled** everything mounts to, so the geometry travels as one object and
nothing depends on floor or walls; ideally the compute lives inside it. A **recline
module** that is a chaise, not a bucket — no bolsters, no shell, compliant surface on a
mechanism with two hard detents; the nearest existing product to the right idea is a
**zero-gravity lounger**, which is designed around the neutral-posture principle, costs
almost nothing, and looks nothing like a Recaro. **Two independent arm booms** carrying a
pad *and* the device, supporting the whole forearm — floating forearms fatigue in
minutes, supported ones last all day — that swing **away**, because a sim rig is climbed
into and this should open. A **screen boom on the same base**, ideally geared to the
recline so gaze angle holds across both postures (Pattern 7, mechanized). A **foot
module** angled for legs-out geometry. A **centre console** between the legs — already
invented, out of a subwoofer. **Open sides, nothing overhead**: enclosure feels great for
twenty minutes and claustrophobic by hour three, and it traps heat.

Register: not motorsport cosplay, which signals speed and aggression — the wrong register
for eight hours of thinking. Studio, or the cockpit of a sailboat. Warm materials. The
nearest object in furniture history is the **Eames lounge**, designed around a specific
relaxed recline and famously good for hours.

**The test: it should be comfortable enough to fall asleep in. Because sometimes you
will.**

### 8.3 Sourcing findings

- **The seat is already solved.** A beanbag beat every engineered chair tried. Every full
  sim cockpit therefore asks you to pay for, and then discard, the one component already
  beaten. **You do not need a cockpit, you need a wheel stand** — whose entire job is
  holding controls rigidly in front of a person sitting in their own seat.
- **The one part a sim cockpit gets right for this posture is the angled pedal plate**,
  designed for a reclined driver with legs extended. A flat floor pedal is at the wrong
  angle for a legs-out body and the ankle notices within an hour. A wedge under the back
  edge fixes it for nothing.
- **Do not buy sim pedals for push-to-talk.** A load-cell brake wants 20–100 kg because
  it simulates a car. **Transcription foot pedals** — court reporters and medical
  transcriptionists have foot-controlled audio all day since the tape era — are light,
  short-throw, built for continuous use, enumerate as plain HID, and cost $25–50.
- **The category nobody looks at: film grip.** Super clamps and magic arms solve "position
  a device precisely in three dimensions, lock it hard, strike it in sixty seconds into a
  bag" — better than sim racing does, at a tenth of the weight. That is Pattern 10 made
  permanent and it is One Carload compliant in a way no rig is. **Split the sourcing:**
  base and pedal structure from extrusion, device positioning from film grip. Neither
  industry sells the whole object because nobody has built it yet.
- **Other objects that earned a mention:** the hospital-style **overbed table** (height
  adjustable, lockable, cantilevered so the base slides under a low seat, $40–80) is the
  Tier 2 upgrade to a stool. Tattoo chairs and massage tables solve "hold a body
  comfortably for six hours while a worker positions tools around it," which is a closer
  problem statement than a car.

---

## 9. Tiers, and what certification should mean

### 9.1 The tiers

| Tier | What it is | Rough cost |
|---|---|---|
| **1** | Found objects only. Stools, a plinth, a beanbag. | $0–200 |
| **2** | Found objects plus a few bought adjustables — arms, stands, a table, pedals. | +$200–500 |
| **3** | Purpose-built. Extrusion, booms, locked, dimensioned, repeatable. | +$500–1500 |

🚨 **Tier 1 is not a compromised Tier 3. It is the prerequisite.** *Tier 1 discovers your
numbers; Tier 3 locks them.* You cannot build a good frame until weeks in Tier 1 have let
your hands tell you where they want to be — which is also why a Tier 3 frame does not
need a wide adjustment range. It needs a narrow range around a measured position, few
axes, adjusted rarely, locked hard. **The frame does not have to fit everyone. It has to
fit one measured body, and hold.**

That is the PADI structure, and it is also what keeps the project from becoming rig-flexing:
the entry level is a beanbag and two stools, and the spec says so on purpose.

### 9.2 Certify configurations, not products

⚠️ **A product cannot be compliant by itself.** An entertainment center is not "Level 3" —
it is Level 3 *in combination with* a given panel, a given support height, at a given
recline, for a body of a given size. Compliance is relational. Certifying SKUs would
create an unmaintainable matrix, a liability surface, and an answer that is simply wrong.

📌 **PADI certifies divers, not regulators.** So: the catalog holds *facts* about products
— measured dimensions, price, availability, no judgment. The solver certifies *assembled
builds* against a body. A person submits their build — the numbers plus photos — and gets
a certified listing at a level. That is the community engine, and every submission is free
data that tightens the tolerance bands.

---

## 10. What has to be measured

🚨 **This is the gap between this document and a specification.** All of it is
perishable — the current build will mutate again, and Build A already has.

Seven numbers, per build:

1. Eye-to-screen distance
2. Screen centre height above floor
3. Recline angle
4. Deck height
5. Left hand support height
6. Right hand support height
7. Head-to-compute distance (for the acoustic budget)

Plus the body measurements they should be expressed as ratios *of*: seated eye height,
seated elbow height, cubit (elbow to fingertip), span, standing height.

And the reconstruction of Build A from photographs, scaled against the 49" panel.

---

## 11. The project shape

- **Repo:** `organonart/couchstation`, public. Same spin-off pattern as
  `organonart/organon-mind`.
- **Site:** `couchstation.org` (domain acquired). Same construction as `site/` in this
  repo — hand-authored HTML, no build step, no external requests, **quoting** the spec
  rather than re-authoring it.
- **First four files:** `README.md` (the claim, the layer cake, a photo), `SPEC.md` (v0.1,
  provisional, with the ledger), `GOVERNANCE.md`, `catalog/` (seeded from the two builds).
- **Then:** `PATTERNS.md` — §5 of this document is ready to lift as-is.

### 11.1 The layer cake

The reason this belongs next to Organon rather than beside it:

| Layer | |
|---|---|
| **CouchStation** | the body |
| **Organon Linux** (the Omarchy 4 fork) | the environment |
| **Organon Console** | the workspace |
| **Organon / Mind** | the instruments |

Four layers, and the claim that makes it credible is that all four are in daily use by the
person specifying them.

### 11.2 Governance — decide it now, while it is free

**The .org defines and measures. Anything that sells is a separate name.** Free to decide
today, expensive to decide in a year when there is money and someone's feelings involved.

### 11.3 Deliberately deferred

- **Scraping retail sites for a catalog.** Breaks, ToS-hostile, links rot. The durable
  form is a hand-curated `catalog/` that anyone can PR into — better on legality and
  longevity, and it gives strangers a reason to contribute. Automate later as a
  link-checker over the curated list, never a live scrape.
- **3D model generation.** Phase three. The reference-image composition trick is the
  valuable half and needs no meshes; meshes only earn their cost if a configurator
  validates dimensions.
- **The conversational configurator.** Genuinely good, and entirely downstream of a spec
  that does not exist. Built first, it is an elaborate way of having not written the spec;
  built after, it is just constraint-solving the spec against the catalog.
- **Webcam biometric fitting.** ⚠️ If it is ever built: **on-device only, never upload a
  frame, output only the derived numbers, let the person edit every one**, and say so in
  plain language above the button. A body-measurement feature that phones home would
  poison the one thing this project actually sells, which is that its numbers can be
  trusted. Technically: monocular pose recovers *proportions* well and *absolute scale*
  badly, so a known-scale object in frame is required; and the flow must degrade to a
  tape measure that stays first-class, so nothing waits on CV work.
- ⚠️ **Generated imagery of real purchasable products must be visibly labeled as
  generated.** Someone buying a $200 stand because a composite flattered it would be the
  first real breach of trust.

---

## 12. Naming

The couch is not the invariant — it did not survive the move. But the name is out, the
domain is bought, and it does the important work of saying *not a desk, and you are
reclined*. Bluetooth does not describe anything either.

📌 **Keep the flag; generalize the law.** The spec must never say "couch." It says **the
support**, with the couch as one compliant member of a class that also holds a beanbag, a
chaise, a recliner, a floor cushion, a car seat.

---

## 13. Immediate next actions

1. **Measure Build B tonight** (§10). It is the only perishable item on this list.
2. **Reconstruct Build A** from the thread photographs, scaled to the 49" panel.
3. **Buy one transcription foot pedal** (~$40) and move push-to-talk to it. Highest-value
   single change; frees the left thumb for a layer shift.
4. **Buy one super clamp and one magic arm** (~$100) and put the Tartarus on it. Tests the
   Tier 3 boom concept for the price of a dinner.
5. ⚠️ **Add one input at a time and live with it a week.** Six new channels installed in a
   weekend means none of them get muscle memory and the idea gets blamed.
6. **Do not buy an aluminium rig yet.** Still Tier 1; the numbers aren't in. Buying now is
   buying a guess in aluminium.
7. Scaffold `organonart/couchstation` with the four files in §11, lifting §5 as
   `PATTERNS.md`.

---

## 14. Provenance

Everything here came out of one conversation, 2026-08-30/31, during and after an
evacuation from the Hawk Fire in Reno and a move to a house in Zephyr Cove and then
onward. Build A was assembled in a rental at 2am from an entertainment center found in a
spare bedroom. That is not colour — **it is why One Carload is a pattern and not a
preference**, and it is the only reason the portability claim has any evidence behind it
at all.
