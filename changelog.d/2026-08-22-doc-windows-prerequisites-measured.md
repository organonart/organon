### What Organon takes from the machine that built it, measured for the first time

A build machine is configured *by* building the thing, and from the inside a supplied
dependency and an absent one look identical — the program works either way. Nobody had
ever asked an organon binary what it actually imports, so the answer to "what will a
stranger's machine be missing" had never been anything but a guess.

`doc/shipping-windows.md` is that answer for `organon-console.exe`, measured on
organon-one at `dc7196c` with `dumpbin` and recorded with a ledger that separates what
was run from what was reasoned.

**The Visual C++ floor is 14.0 — the 2015 redistributable — and the reason is written
next to the number** so it does not decay into a presence check. The binary imports nine
symbols from `VCRUNTIME140.dll`, every one of which has been there since 14.0, and it
does **not** import `__CxxFrameHandler4`, which is what would have pushed the floor to
14.20. Two absences carry as much as the imports: no `MSVCP140.dll` and no
`VCRUNTIME140_1.dll`, so nothing C++ is statically linked into this build.

🚨 **Both absences end the moment `embedded-llm` is added** — llama.cpp is C++ — so
bundling the LLM runtime moves the floor, and the size of that move is measurable
before the decision is taken rather than after.

⚠️ **`dumpbin /dependents` cannot see Organon's graphics dependency at all.**
`d3d12.dll`, `vulkan-1.dll`, `dxcompiler.dll` and `d3dcompiler_47.dll` are present as
strings in the binary and absent from the import table, because wgpu resolves its
backends with `LoadLibrary` at runtime — while `dxgi.dll` and `opengl32.dll` *are*
statically imported, which is what makes the list look complete when it is not. The
prerequisites therefore split into two classes that fail differently: a missing C
runtime kills the process before `main()` with no window and no log line, and only an
installer's prerequisite check can cover it; a missing GPU adapter fails afterwards,
where the product can still say so.

The document also records what nothing in this repository currently does: no CI job
builds this binary on the platform it ships to. That is a trade the workflow header
already states and defends on cost, not an omission — but it was made when no artifact
left the machine, and "compile coverage" is a different claim from "this binary has been
produced on this platform".
