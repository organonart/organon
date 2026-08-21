### The Windows deploy scripts could not be parsed by the PowerShell that ships with Windows

`native\deploy.ps1` and `native\bundle.ps1` are the documented Windows install path in
`CLAUDE.md`. Both were UTF-8 **without a BOM** while holding non-ASCII prose (`→`, `⚠️`,
em dashes), and **Windows PowerShell 5.1 reads a BOM-less `.ps1` as ANSI/CP1252**. Measured
on a stock Windows 11 workstation: `deploy.ps1` produced **8 parse errors** and `bundle.ps1`
**2**, so neither would run at all. The identical bytes with a BOM prepended parse with
**zero** errors — the content was never wrong, only its label.

🚨 **The mechanism is why "it is only a comment character" is the wrong instinct.** `→`
(U+2192) is UTF-8 `E2 86 92`; read as CP1252 the three bytes become `â†’`, and the last of
them is **U+2019, a right single quotation mark** — which PowerShell honours as a string
delimiter. Ten arrows inside a comment block are ten stray delimiters. The parser ran off
the end of a 377-line file hunting a terminator and reported *"Missing closing `}`"* at
`function Add-UserPathEntry`, roughly 200 lines from anything actually wrong. The error
names the wrong cause, which is the expensive part.

⚠️ **CI had a gate for exactly this and it was green the whole time.** The
*Validate the PowerShell deploy scripts* step exists, in its own comment's words, because
these two files' "FIRST syntax check would otherwise be a person on a Windows box trying to
deploy". It runs `shell: pwsh` — PowerShell **7**, which defaults to UTF-8 and reads a
BOM-less file correctly. So the guard could not observe the failure it was built to
prevent: a clean AST parse in CI and an unrunnable file on the target machine are perfectly
consistent, because they are two different readers. 5.1 is the only PowerShell on a stock
Windows install; `pwsh` is a separate download.

The fix is in two halves, and the second is the one that matters. Both scripts gain a UTF-8
BOM, which preserves the house typography rather than flattening it to ASCII. And the CI
step gains a **byte-level** check ahead of the parse: a `.ps1` must be pure ASCII **or**
carry a BOM. That is a claim about encoding, which is what was actually broken — the AST
parse cannot substitute for it, because under `pwsh` it cannot fail. The check is
mutation-tested rather than merely asserted: stripping the BOM from each file makes it
report the true byte counts (1077 and 252 non-ASCII) and fail, while a pure-ASCII file with
no BOM still passes.

📌 `.gitattributes` already carried a deliberate note on why `*.ps1` is **not** pinned to
CRLF. That reasoning is untouched and still correct — it is about line endings, and this is
about encoding. The note now says so, so the next reader does not mistake one settled
question for both.
