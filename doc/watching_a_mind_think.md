# Watching a Mind Think

**Author**: James Andrew Walsh
**Subject**: The Organon inference visualizer, explained plainly
**In one line**: the object is a to-scale diagram of a language model, and the light is tied to the model's own effort as it writes

You saw a video: a glowing object turning in the dark, pulsing in waves while a language model answered a prompt. Here is exactly what it is, start to finish.

The short version, so the rest has somewhere to land: **the shape of the object is a real, to-scale diagram of the model's architecture**, built automatically from the model file, nothing invented. **The light moving through it is tied to the model's own effort** as it produces each word. The shape is true. The light is an honest stand-in. I'll be careful throughout about which is which, because that line is the whole point.

## The model, in sixty seconds

The thing generating the text is a **transformer**, the same kind of model as the ones behind most chat assistants. You only need four facts about it to read the object:

- It is a **stack of identical layers**, one sitting on top of the next. A small model has a couple dozen; the one in the video has around forty.
- Running through every layer, end to end, is the **residual stream**. Think of it as a shared notepad that each word carries with it. Every layer reads the notepad, does some work, and writes its contribution back. It is the spine of the whole computation.
- Inside each layer are two working parts: a set of **attention heads** (several of them, side by side, each gathering information from other words in the sentence in its own way), and one **MLP** (a "feed-forward" block) that does the heavy per-word transformation.
- Everything flows in **one direction**. A word enters at the bottom as an embedding, passes up through every layer via the residual stream, and the top layer's result is turned into the prediction of the next word.

That's the entire model for our purposes: a tall stack of layers, a spine running through them, heads and an MLP at each level, and a single upward direction of travel.

[[PAGEBREAK]]

## The object, part by part

Here is the object drawn as a clean schematic: first from the side, so you can see the layers stacked, and then end-on, looking straight down its length, which is the view where the parts are easiest to name.

[[FIGURE]]

Now each piece, in the order people usually ask about them.

### The axle down the middle: the residual stream

The big square beam running down the center of the object **is the residual stream**, the shared notepad from the sixty-second summary. It is drawn as one point per layer, sitting on the center line, with each point wired to the next, all the way down the stack. When you look straight down the length of the object (the end-on view), those points stack up into what reads as a solid square axle. This isn't a loose analogy on my part: it is literally what the drawing code assembles and labels as the residual-stream backbone.

### The spokes around it: the attention heads

The spokes radiating out from the axle **are the attention heads**. Each layer has a ring of them standing out sideways from the spine, one spoke per head, each wired back to that layer's point on the axle. Count the spokes and you have counted the model's attention heads. Because every layer has the same number of heads, the rings line up as you look down the length, which is why the whole thing reads as a wheel with a hub and spokes.

### The long arm along the top: the MLP

There is one arm that is longer than the rest and always points the same way, along the top. **That is the MLP, the feed-forward block.** Each layer emits exactly one of these, always in the same direction and reaching further out than the heads, so across all the layers they line up into a single rail running the length of the object. That's why it looks different from the spokes: the spokes shoot out *sideways*, but the MLP arm runs *lengthwise*, parallel to the spine, the same direction as the axle rather than across it. That's the "arm at the top that runs the other way" you pointed at.

### The length of the object: layer depth, and its direction

The long axis of the object (the direction the axle runs) **is depth through the model's layers.** One end is the first layer, nearest the incoming word; the far end is the last layer, nearest the output. So the object's length is not decorative: travelling along it, end to end, is travelling through the model in the exact order it actually computes, from embeddings in to next-word prediction out.

## What the flashing is

Every time the model produces **one word** (strictly, one token: a word or a piece of one), the running model emits a single snapshot of activity, and the visual repaints the brightness of every part of the object from it. So the flashing is not random and it is not decorative timing: it is **one flash per word the model generates.** Fast flashing means words are coming out fast.

Within each flash:

- The **axle pulses** with how *uncertain* the model is at that moment: how many plausible next words it is weighing.
- The **spokes shimmer** with that same uncertainty, spread out across the heads, which is the fast flickering you see around the rim.
- The **top arm pulses** with the model's *confidence* in the word it just committed to.

A faint wave also travels along the length of the object from one word to the next, so it reads as alive rather than blinking all at once. When the model is cruising through an easy, obvious stretch of text, it glows calm and even. When it hits something it has to work at, the whole stack and all the spokes light up. You are, quite literally, watching it find the hard parts.

## What's real, and what's a stand-in

This is the distinction I most want to be clear about, because the object is *more* impressive when you know exactly what it is and isn't.

**The structure is real. The brightness is an honest proxy, not a microscope.**

Real: the number of layers, the number of heads, the whole skeleton. Those are read straight out of the model's own file. If the model has forty layers and sixteen heads, the object has forty slices and sixteen spokes. Nothing there is stylized.

A proxy: the *brightness* of each part is driven by two things the running model readily reports for each word (how uncertain its next-word choice was, and how confident it was in the one it picked), shaped across the real layers and heads. It faithfully tracks **when the model hesitates versus when it commits**, which is a genuine signal about the model's effort. But it is **not** a readout of the individual internal values inside the network. A more literal version, lighting each layer from its true internal activity, is a planned upgrade, and the important thing is that it drops into this exact same object with no change to the shape. The socket for it is already built; today it's filled with the honest effort signal instead.

So when you show someone the video, the truthful caption is: *the shape is the real architecture, to scale; the light shows the model's effort as it writes, word by word.*

## "Is this the residual stream?"

You asked me directly, so: **yes.** The axle down the middle is the residual stream. Not a metaphor reaching for one, but the actual thing the object is built around. In a real transformer the residual stream is the shared vector each layer reads from and adds back into, and that is exactly how the object is wired: every spoke and the top arm at a given level connect to that level's point on the central axle, the same way attention and the MLP attach to the residual stream in the real model. They read the spine, do their work, and write back to it.

The one honest footnote, same as above: right now the axle's *brightness* comes from the effort proxy, not from the true magnitude of the residual vector at each layer. Reading that real value is the upgrade I mentioned, and it would light this same axle directly. The wiring is faithful today; the lighting gets more literal next.

## In a sentence

It's a to-scale portrait of the model (spine, spokes, and top rail, all counted from the real file) lit, one flash per word, by how hard the model is working to say the next thing. Beautiful because it's honest, not instead of it.
