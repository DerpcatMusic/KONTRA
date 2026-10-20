# Taking the proxy shell off soothe3

Status: **the image now loads and answers as itself**, but it is **not yet
host-loadable**. This note records the exact cut, the evidence, and the one
remaining blocker, so none of it has to be rediscovered.

## What PACE actually hides

The decode is not the interesting part. At load, the packed image is mapped
byte-for-byte as it sits on disk — dumping the live `.text` of a loaded copy is
identical to the file (`probe/init/mapped-.text.bin`), while the same RVA in the
restored image is real code:

```
packed   @ RVA 0x3309a0: b5 e5 8e a2 a5 d6 a7 ca   (not code)
restored @ RVA 0x3309a0: 48 89 5c 24 08 ...         (mov [rsp+8], rbx)
```

So Execution does not come from the loader's view of `.text`; the proxy runs the
plug-in out of its own copy. What the proxy *also* does, and what actually stops
a naive depace, is two things:

1. **The import directory is a decoy.** Data directory 1 points at
   `0x7acd76c`, four DLLs (KERNEL32, USER32, ADVAPI32, SHELL32) — the proxy's own
   imports. The plug-in's real IAT at RVA `0x1daf000` is orphaned: its slots still
   hold hint/name RVAs (1055 of them, 528 unique, e.g. `InitializeCriticalSection`,
   `D3DCompile`, `CryptGenRandom`). The loader never fills them, so the first
   `call qword [0x181daf5f0]` executes an RVA into `.rdata` and dies.

   The **complete original descriptor array survives** at RVA `0x219a298`
   (38 DLLs, real `OriginalFirstThunk`/`FirstThunk`, `KERNEL32.dll`,
   `D3DCOMPILE_47.dll`, `MSVCP140.dll`, `d2d1.dll`, …). Only the directory entry
   was moved.

2. **The TLS directory owns the CRT.** Its only callback is PACE's bootstrap
   (`0x7acc190`), but the *directory* is also what makes the loader allocate the
   TLS index the C++ runtime writes to `0x1826d4b60`. Deleting the directory to
   remove the bootstrap silently breaks every magic static: lazy-init guards are
   read through that index, so the class table is never built and
   `getClassInfo` returns zeros.

## The cut

From the restored image (`soothe3-restored.bin`):

| what | where | value |
| --- | --- | --- |
| entry point | `opt+16` (`0x7acbea0`) | `0x3309a0` — the plug-in's own `DllMain` |
| import directory | data dir 1 (`0x7acd76c/0x64`) | `0x219a298/0x30c` |
| TLS callbacks | file `0x249cb58` | `0` (keep the directory, drop the bootstrap) |

`0x3309a0` really is a `DllMain`: it tests `edx == 1`, calls the CRT init
(`0x18032f720`), runs static init (`0x180330860`), then a registration hook
(`base + 0x17b9ed8`). The proxy called the same address with **its context** in
`rcx`, not the module handle — which is why entering it directly used to fault.

Result:

```
loaded=1
exports init=1 factory=1 exit=1
InitDll=1
factory vendor=oeksound flags=16
classes=3
class[0] soothe3 / Audio Module Class / Fx|Dynamics EQ / oeksound / 1.0.3 / VST 3.7.12
class[1] soothe3 / Component Controller Class
class[2] soothe3 / Plugin Compatibility Class
```

No proxy, no licence, no TLS bootstrap.

## The remaining blocker

### First wall: orphaned static constructors

`createInstance` did not merely fail — it hung. The stuck thread was inside
`EnterCriticalSection` on the static lock at RVA `0x2779db0`, and the lock read
`lockCount = 0, recursion = 0, owner = 0`. A live critical section reads
`lockCount = -1`; this one was **zero-initialised and never initialised**, so
`RtlEnterCriticalSection` treats it as owned and waits forever.

The two constructors that should have fixed that are in `.text`:

```
0x1802ffbc0  sub rsp,0x28
             lea rcx,[0x182779d78]      ; lock A
             call InitializeCriticalSection
             ...
             jmp 0x1817b9af8            ; tail-call into the proxy VM
0x1802ffbf0  lea rcx,[0x182779db0]      ; lock B
             call InitializeCriticalSection
             ...
             jmp 0x1817b9af8
```

Neither address appears anywhere in the image — `0` references in `.rdata`, no
entry in the ~1161-pointer initialiser table at `0x1db0bc8`. They are **orphaned
on purpose**: the proxy's runtime calls them directly from the VM region, outside
the CRT's `_initterm` walk. Without the proxy they simply never run.

Initialising both by hand turns the deadlock into progress: `createInstance`
enters the plug-in and faults instead of waiting.

### Second wall: runtime-initialised pointers

With the locks repaired the fault is a **write to null**:

```
0x18088cce5  mov dword [rcx], 0x3f800000     rcx = 0
```

inside the `createInstance` path. The code around it is VM-shaped (`mov edx, esp`
as a loop counter, unaligned frame slots), so the missing value is another piece
of state the proxy would have planted.

### What this means

Everything the unpacker was asked to recover *has* been recovered: `.text`
decodes to real code, the entry point is the plug-in's own `DllMain`, the import
directory is the real 38-DLL table, and the loader runs it far enough to answer
as soothe3. What is left is not unpacking — it is **reproducing the protector's
runtime bootstrap**: the orphaned init calls, the VM dispatch tables, and the
class CIDs. The CIDs are still `00000000…` on every class, and `createInstance`
compares against zero and treats zero as "not found" (`0x180326b90`), so no host
could instantiate it even if the fault were fixed.

## Things that do not work (recorded so they are not retried)

- **Entry = real `DllMain`, TLS directory zeroed, imports untouched** →
  executes a hint/name RVA (`0x219f8a6`), `LoadLibrary` = 998.
- **Entry = real `DllMain`, imports restored, TLS zeroed** → loads and
  `InitDll` succeeds, but magic statics never initialise: class info is empty.
- **Keep the proxy entry, force its "licensed" branch** (`0x187acc0d1` `jne` →
  `jmp`, or `0x187acbee9` `test eax,eax` → `xor eax,eax`) → the proxy calls the
  plug-in with a half-built context; same crash.
- **Stub `DllMain` returning TRUE on the packed image** → `LoadLibrary` = 1 and
  `InitDll` runs, but the proxy's context is missing (`InitDll` faults at
  `0x180325960` reading `[0x20]`), and `.text` stays packed in memory.

## Reproducing

`tools/smooth-core/research/depace-probe/` holds the two Windows VST3 clients used
for every result above:

- `inventory.cpp` — `LoadLibraryW`, `InitDll`, `GetPluginFactory`,
  `IPluginFactory3::getClassInfoUnicode`, controller parameters, section dumps.
- `create.cpp` — calls `createInstance` on its own thread, and when it does not
  return, suspends it, reads `RIP`, the module it belongs to, the lock state at
  `0x2779db0`, the owning thread, and any windows the thread owns. It also
  initialises the two orphaned locks by hand so the deadlock becomes a fault.

Build both with mingw against the VST3 SDK (see the `build.sh` beside them) and
run under Wine against a renamed copy of the image.

The patched image used here lives outside the repo at
`/tmp/opencode/rea/soothe3-native.bin`. The bundle was removed from the DAW's
scan path: it loads but cannot be instantiated, so leaving it installed would
hang the host.
