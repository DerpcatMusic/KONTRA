# Local Lua failure source context

Failed UVI callbacks retain up to five numbered lines around the real Lua line,
using structured frames from the failed coroutine before it is dropped. Names
resolve to the currently loaded processor or approved module bytes; error messages
and formatted tracebacks are never parsed to select source. The error-only walk
is limited to 64 frames, and each displayed line is limited to 512 bytes.

Logs shows this local context only while bank, member, epoch, generation,
processor, frame, line and chunk still match the retained activation. After
retirement, a short hint explains that the local context is no longer retained.
Initialization `.exec` failures retain the known entry chunk/processor and an
explicit unavailable line; nested module failures are not assigned guessed lines.

The local Lua excerpt/display type does not serialize. Copied summaries, raw
records, journals and support exports retain failure/source metadata and existing
messages, without adding the local Lua excerpt. Build-verified owned Rust excerpts
and the existing KSP context policy remain separate.

The current Logs inventory grouping preserves ordinary Info stage transitions.
The original pasted report's 179 inventory chunks produce two summaries, with
40 separate stage transitions. An owned Clarinet journal gives 171 Info events
as 26 rows: 146 chunks become one summary covering 9,457/9,457 rows and
146/146 chunks, with all 22 stage transitions still separate. Complete available
raw inventory remains in the journal/support export. No severity change is needed.

The private initialization-handler experiment was rejected: replacing `.exec`
with xpcall made successful authored constructors observe getfenv levels 0–5
instead of 0–3. An authored mlua callback panic propagated under the existing
call, but the naive Rust handler turned it into a C-stack-overflow error. Nested
module `.eval` had already unwound the module's frame, leaving only the real
caller's require line for the outer observer. Initialization keeps its known
entry and explicitly unavailable failing line; no line is inferred from messages.
