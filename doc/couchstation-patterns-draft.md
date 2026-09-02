# CouchStation — three entries in the Organon Mind template

> **What this is.** A test cut. Three candidate patterns derived from
> [`doc/couchstation.md`](couchstation.md), written in the entry template the Organon
> Mind catalogue uses, to find out whether a form built for software patterns holds for
> a hardware one. It is a draft for evaluation, not a section of a specification.
>
> **The three are one from each of the three latent languages** the capture document
> contains and §5 does not: a control pattern (§7), a form pattern (§8), and a method
> pattern (§9). §5's fifteen are a furniture language and are not reproduced here.
>
> **Status.** Every ledger rule from `couchstation.md` §1 applies unchanged. Nothing
> below has been promoted a tier, and the Measured column is still empty.

---

## On the template, and the one field it needed

The template is Organon Mind's, which is Gang of Four's with three fields added over
five papers: *Failure signature*, *How you would know this is unnecessary*, and
*Evidence*. Both additions earn their place harder here than they do in software.

**Every entry carries an `Evidence` field, and it opens with a ledger marker.** In
OM-004 the evidence position is stated once for the whole paper, because the known use
of every entry there is one workstation in one room. That will not transfer. These
entries sit at three different tiers of support inside one document whose headline fact
is that none of it is measured, so an entry that did not carry its own marker would
inherit the confidence of its neighbours. The marker goes in the entry.

*Also known as*, *Sample interaction* and *Known uses* are omitted. A sample interaction
for a piece of furniture is a photograph, which Structure covers, and the known use of
all three is two builds by one person.

**One field behaved differently than expected and it is worth recording.** *How you
would know this is unnecessary* is the hardest field in the software catalogue. Here it
was the easiest, because a hardware pattern's retirement condition is usually a device
that does not exist yet, and naming that device is a concrete act. Two of the three
below name one. The third is self-retiring, which is a shape the software entries do
not have.

---

## Control · The Held Mode

**Intent.** Carry a mode in a control the body is holding, so that the same words can
mean different things and the person always knows which, without looking at anything.

**Motivation.** Speech into a workstation has at least three destinations: another
person in the room, a dictation buffer, and an agent that will act on it. A wake word
guesses the boundary from the audio and gets it wrong in both directions; it fires on a
sentence addressed to somebody else, and it leaves a microphone live in a room where you
also live. A mode menu resolves the ambiguity once and then persists, so the mistake
arrives hours later as a command pasted into a document as text.

A *held mode* is one that exists only while a control is under continuous pressure, and
ends when the pressure does. The CouchStation's proposed form uses the foot. No pedal
means the speech is not for the machine. The right pedal means dictation, text into the
buffer. The left pedal means command, interpreted and executed. Same words, three
meanings, resolved by a posture instead of a setting.

**Applicability.** Use where one input channel serves several destinations; where the
wrong destination is expensive or embarrassing; where the channel is always physically
available, so that off must be the resting state; and where the person's hands are
already committed, so the mode selector needs a limb of its own.

**Structure.** Three parts and one rule.

```
        ┌─ no pressure ──────→  not for the machine   (resting state)
        │
utterance ─ held control ──┬─→  dictation             (right)
                           └─→  command               (left)

        the mode is read from the control's live position.
        nothing is stored, so nothing can disagree.
```

The rule is the pattern: the mode is the control's present position, never a value
written down when the control was last touched. There is no state to get out of sync,
because there is no state.

**Participants.** *The held control*, which has a resting position and no memory. *The
utterance*, ambiguous on its own. *The destinations*, which must be mutually exclusive
and few. *The person*, who feels which mode is active instead of reading it.

**Collaborations.** Sits under **Voice Carries Content, Hands Carry Intent**
(`couchstation.md` §5, 12), which requires the seam between speech and precision to be
somewhere physical; this is where. It consumes a limb, so it depends on **The Free
Limb**. Where a destination is irreversible, it must be a separate control rather than a
region of one control's travel: see **A Detent for the Irreversible**.

**Consequences.**

- *Gain:* the mode cannot stick, because releasing ends it.
- *Gain:* no false trigger, and no open microphone in a room you live in.
- *Cost:* speech now requires a free limb, which a workstation can supply and a laptop
  cannot.
- *Cost:* the number of destinations is bounded by how many positions a foot can find
  without looking, and that number is small.
- *Trap:* a held control that latches on a double tap has become a menu again, and
  inherits every failure this entry exists to avoid.

**Implementation.** Prefer positions a foot can tell apart by feel: separate pedals beat
regions of one pedal's travel. Feet are good at hold and at coarse pressure and useless
at precision, so map the destination to *which* pedal and reserve travel *within* a pedal
for magnitude. Do not buy sim racing pedals; a load-cell brake wants 20–100 kg because it
is simulating a car. Transcription pedals are light, short-throw, built for continuous
use, enumerate as plain HID, and cost $25–50.

**Evidence.** *Untested.* Argued from the current build, where push-to-talk is bound to a
thumb control on the left hand and works; the three-destination version has been proposed
and not tried. Feet are unclaimed in both builds as of this writing.

**Failure signature.** Text that should have been a command, and commands that should
have been text, both discovered later. A person who says a sentence twice, louder the
second time, because the first went nowhere. In the wake-word version, an assistant
answering a question addressed to somebody else in the room.

**How you would know this is unnecessary.** If the destinations stopped being ambiguous.
Two conditions would each do it alone, and naming them is what makes this checkable.
Speech with exactly one destination has no mode to hold. And a system that could pick the
destination from the content, with a false-positive rate low enough that a wrong pick
costs nothing, would not need the body. The second is what wake words and transcript
heuristics attempt. This entry is a claim that they cannot get there, and it is refuted by
a system that does.

**Related patterns.** The physical form of **Mode Visibility** (OM-001, 2), which requires
the indicator to sit where the input is given; a held control meets that with
proprioception instead of pixels, and is the one indicator a person cannot fail to notice
while acting. **Progressive Autonomy** (OM-001, 11) defines its rungs by reversibility,
which is the constraint deciding how many destinations a foot may safely carry.

---

## Form · Rigid Tools, Compliant Body

**Intent.** Hold the devices in a frame that does not move and the body in a support that
does, then couple the two, so a change of posture carries the tool geometry with it
instead of destroying it.

**Motivation.** A sim rig is rigid everywhere. The wheel is where it was yesterday, and so
is your spine, for the whole stint. A recliner is compliant everywhere. The body can move
all day, and every time it does, every device relationship is destroyed and rebuilt by
hand. Both are right answers to their own problem, and neither answers eight hours of
thinking, where the hand must find a control without looking and the body must be free to
change position several times.

The two requirements look opposed and are not. They are requirements on different frames.
The **tool frame**, meaning where the controls sit relative to each other and to the hand,
wants rigidity. The **body frame** wants compliance. What every existing product does is
apply one answer to both.

**Applicability.** Long sessions at low arousal, where the body will change position
because nothing is stopping it. Where a control is acquired from memory. Where posture
change is a comfort requirement and not an occasional adjustment. It does not apply where
the body is externally loaded, as in a vehicle or at a machine tool, because there the
clamp is the whole point.

**Structure.** A **base** carrying every device, so the tool geometry is one object
depending on neither floor nor wall. A **support** that is compliant and re-formed by
hand. A **coupling** between them, so a change in the support's geometry moves the base's
devices to match. The coupling is the pattern. Without it you own a wheel stand and a
beanbag that happen to be near each other.

**Participants.** *The base*, rigid, carrying compute, screen and hand supports. *The
support*, compliant, with no correct configuration. *The coupling*, which preserves
relative geometry across a posture change. *The devices*, positioned once, and then
holding still relative to each other while the body moves.

**Collaborations.** Supplies the mechanism for **The Two Postures** (§5, 8): two detents
are a coupling with two settings. Takes from **Independent Hand Planes** (§5, 11) what the
tool frame has to hold, and from **The Pillow Is The Chair** (§5, 9) what the body frame
must be made of. **One Carload** (§5, 5) bounds how much frame may be built, and is the
constraint most likely to be violated by a coupling.

**Consequences.**

- *Gain:* the hand finds a control from memory after a posture change, which is the thing
  neither existing product delivers.
- *Cost:* a coupling is a mechanism, and mechanisms are the heavy, expensive part of Tier
  3, worth getting wrong slowly.
- *Cost:* it argues against enclosure. A coupled frame that wraps the body traps heat and
  reads as claustrophobic by hour three.
- *Trap:* rigidity in the tool frame is easy to over-read as rigidity in quantity. The
  structural load here is a forearm and a mouse; extrusion sized for 15–20 Nm of wheel
  torque is the over-engineering that violates One Carload.

**Implementation.** Build the base first and the coupling last. Tiers 1 and 2 supply the
two frames uncoupled, which is enough to discover the geometry; the coupling is what Tier
3 is for, and building it before the numbers exist is buying a guess in aluminium. Arm
booms swing away rather than up, because a sim rig is climbed into and this should open.
A screen boom geared to the recline is the mechanised form, and is the piece to build
last of all.

**Evidence.** *Argued.* Build B has the two frames and no coupling: the support is
compliant, the devices are on stools, and a posture change abandons the geometry rather
than carrying it. That is the pattern observed by its absence, which is weaker than
watching it work.

**Failure signature.** The person stops changing posture, because each change costs a
device re-setup. Or the reverse: they change posture freely and spend the first minute
after each change groping for a mouse that is no longer under the hand. Either way the
station has silently become a single-posture station, and the two-posture claim in §5 has
quietly stopped being true of it.

**How you would know this is unnecessary.** If the devices stopped needing a fixed
position relative to the hand. Two things would do it. Hand tracking accurate enough that
a controller has no physical home, which is the far version. Or a device that rests on the
body rather than beside it, which is the near one and closer than it sounds: a controller
strapped to the hand needs no frame at all, and moves with the posture for free. This
entry rests on the claim that a resting forearm needs a surface and a surface needs a
position. Support the forearm from the body and the coupling has nothing left to do.

**Related patterns.** Requires **The Forearm, Not the Wrist**, which says what the tool
frame must actually carry. Read against **Steal the Skeleton, Reject the Flesh**, which
supplies the parts and warns which of the sim rig's design drivers do not transfer.

---

## Method · The Cheap Tier Is The Instrument

**Intent.** Treat the cheapest tier as the measuring device that produces the numbers the
expensive tier locks, so that a purpose-built frame holds a measured position instead of
offering a range that might contain one.

**Motivation.** Conventional ergonomics runs in one direction. Buy the chair, buy the
desk, set the heights from a chart, then train the body to meet the furniture. Every
adjustment knob on an office chair is an apology for that ordering, and the range is wide
because the manufacturer does not know your numbers and neither do you.

Ordered the other way, the found-object build stops being a compromise. Two stools and a
beanbag hold nothing in place, and that is exactly what makes them an instrument: over
weeks the hands relocate the supports without being asked, and the positions they settle
at are the measurement. The expensive tier's job is to hold that measurement. **Tier 1 is
not a compromised Tier 3; it is the prerequisite.**

**Applicability.** Any body-relative specification whose correct dimensions vary per
person and are not known in advance. Where the cheap version is functionally complete
even though it is unrepeatable. Where weeks of use are available before the expensive
commitment falls due.

**Structure.** Three tiers, with the relation between them running backwards from the
usual one.

| Tier | What it is | What it produces |
|---|---|---|
| **1** | Found objects. Stools, a plinth, a beanbag. | the numbers |
| **2** | Found objects plus a few bought adjustables. | the tolerances |
| **3** | Purpose-built. Extrusion, booms, locked. | a frame that holds them |

Tier 3 consumes Tier 1's output. Built without it, a Tier 3 frame is a Tier 3 frame
carrying Tier 2's adjustment range, which is the object the market already sells.

**Participants.** *The found object*, chosen for a dimension rather than for its function.
*The hand*, which relocates a support without being asked to. *The measurement*, taken
after the position stops moving. *The frame*, which holds one position and offers little
else.

**Collaborations.** Consumes the output of **The Hand Lands First** (§5, 10), which is how
a Tier 1 position gets found at all. Supplies **Narrow Range, Locked Hard**, which is the
only shape of frame this ordering permits. Depends on **Commodity Substrate** (§5, 15),
because a Tier 1 that cannot be assembled from what is already in the room is not an
instrument anyone will pick up.

**Consequences.**

- *Gain:* the frame needs few axes, a narrow range and hard locks, which is cheaper and
  stiffer than the adjustable version.
- *Gain:* the entry level is usable on its own, so the standard does not open with a
  purchase.
- *Cost:* weeks. There is no way to shorten Tier 1 and have it still be a measurement.
- *Cost:* a frame that fits one measured body does not fit a second, and a household with
  two people needs two frames.
- *Trap:* a compliant support has a conform-in period, so the numbers taken in the first
  days are the wrong numbers. The beanbag was uncomfortable on first sitting and became
  the best support tried; a measurement taken on day one records a rejection, not a fit.

**Implementation.** Take the seven positions per build and express each as a ratio of the
body measurement it descends from, so the record survives a change of house and a change
of body. Measure after a position has stopped moving for a week, and record the date you
took it. Add one element at a time and live with it for a week; several new inputs in a
weekend means none of them acquires muscle memory, and the method gets blamed on the idea.

**Evidence.** *Argued*, from two builds. Neither has been measured. The numbers this entry
tells you to take are precisely the ones `couchstation.md` §10 lists as missing, so this is
an argument for a procedure nobody has yet carried out. That is the weakest position an
entry can hold, and it is stated here so a reader can discount it rather than discovering
it later.

**Failure signature.** A purpose-built frame with a wide adjustment range, and a person
who still adjusts it daily. Or its opposite: a frame locked to a position measured on day
two, which the body has been fighting ever since. Both read as frame problems and are
ordering problems.

**How you would know this is unnecessary.** If the numbers could be derived instead of
discovered. A fitting flow that took body measurements and produced correct positions on
the first attempt, agreeing with the positions people reached the slow way, would remove
Tier 1's reason to exist. §4.2 argues that such a flow is multiplication once the ratios
are known, so this entry is self-retiring by construction: the check is whether a derived
position and a discovered one match for the same body. Nobody has run that comparison, and
until somebody does, discovery is the only source of the ratios a derivation would need.

**Related patterns.** Downstream of **The Body Specifies, The Furniture Complies** (§4.2),
which is the law this is the procedure for. Feeds **Certify Configurations, Not Products**,
since a submitted build is a Tier 1 measurement someone else took.

---

## What the test showed

**The form holds, with one adaptation and one strain.**

The adaptation is `Evidence`, described above, and it is not optional for this material.

The strain is `Structure`. In the software catalogue that field carries a sequence diagram
or a state branch, and the SVG does real work. Two of the three entries above have a
Structure that is a physical arrangement, which wants an orthographic sketch or a
photograph with dimensions on it. **The Cheap Tier Is The Instrument** has no physical
structure at all; its Structure is a table of a process, which is the field being used for
something the template did not anticipate. That is worth watching before a fourth or fifth
entry is written, because it is the field most likely to want splitting.

**Two naming questions are open.** *Rigid Tools, Compliant Body* puts two of the three
elements in the name and leaves out the coupling, which is the actual pattern; *The
Coupled Frames* is plainer and says the load-bearing part, at the cost of the vividness the
house rule asks for in a leaf. And *The Held Mode* is a container name on a leaf entry, so
it may want to become the section and hand its content to a vivid one.
