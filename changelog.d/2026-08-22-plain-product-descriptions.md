### The product descriptions get rewritten as plain description

`doc/organon_prd.md` §1.1's three lengths — the elevator pitch, the paragraph and the two
paragraphs — are rewritten. The old text described Organon through epigram (*"built on the
observation that an 'app' is an arrangement, and that every arrangement needs someone to talk
to"*), and an epigram is not a description: a reader who does not already know what the product
is cannot picture anything from it.

🚨 **Four specific things went, and each was a habit rather than a one-off slip.**

**History.** The old text explained the product partly by where it came from — *"the
generative-math visualizer it grew out of"*. Provenance is not part of an explanation of what
something is. §1.2 exists to make the correction that Organon is not the visualizer, and that is
the right place for it; a description should not need to say what the product is not.

**The validity framing.** *"No arrangement is valid without a live agent in it"* states a rule
about what is rejected, when the thing worth saying is what the agent does. It now reads *"Every
arrangement includes an agent, and the agent operates the application"*, and the enforcement
detail lives in §4's principles where a rule belongs.

⚠️ **The word "live", and personification generally.** Agents were described as *living* inside
the application and as being *taught* by skills. They are processes the application launches.

**Mechanism trivia wearing precision's clothes.** *"Twelve words address the regions that grid
allows"* and *"two regions may coexist only if they occupy different cells"* are true, and they
belong in `CONSOLE_ARCHITECTURE.md`. In a product description they cost a reader more than they
pay. The description says *"up to six regions on a three-by-two grid, and each region has a name
you type"*, and gives three examples.

📌 Also gone, on the same standard: contrast pairs used as a tic (*"rather than three programs"*,
*"but cannot grant it access"*), hedges (*"what a user would normally call an app"* → *"A saved
arrangement is an app"*), and sentences carrying three clauses where three sentences do the job.
The roadmap sentence about dispatching agent teams moved out of the description entirely; §1.4
owns the trajectory.

**`README.md` and `doc/how_organon_works.md` §1 and §16 move with it**, in the same change,
because they quote §1.1 and converting them piecemeal is how the six wordings this PRD exists to
end came about in the first place. `how_organon_works.md` §1's property headings lose the same
two habits: *"A layout is the unit of identity"* becomes *"A saved arrangement is an app"*, and
*"An agent is not optional"* becomes *"Every arrangement includes an agent"* — a negation stating
what is rejected, replaced by a sentence stating what is there.

📌 **The region arithmetic stays in `how_organon_works.md` and stays out of the PRD**, and that
split is the point rather than an inconsistency. Twelve region words of which up to six can be
held at once is exactly what an engineer reading a technical overview needs, and exactly what
costs a reader of a product description more than it pays. The same fact belongs in one document
and not the other.
