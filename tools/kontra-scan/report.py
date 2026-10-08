#!/usr/bin/env python3
"""Regenerate the scope-5 report from the ONE shared scanner's current cache."""
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib, importlib.util, json, shutil, subprocess, sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('scanner',Path(__file__).with_name('kontra_scan.py'));s=importlib.util.module_from_spec(spec);spec.loader.exec_module(s)
root=Path(sys.argv[1] if len(sys.argv)>1 else str(Path.home()/'.cache/kontra-scan'))
out=Path('docs/audit-2026-10-08/UI_CENSUS.md');out.parent.mkdir(parents=True,exist_ok=True)
manifest=s.items(str(root/'v2-items.tsv')); expected=set(manifest)
records={}; revisions={}; rows={}
for v in ['v1','v2']:
 revisions[v]=hashlib.sha256((root/'bin'/('kontra-scan-'+v)).read_bytes()).hexdigest()
 records[v]={}
 for p in (root/'results'/v/'cache').glob('*.json'):
  r=json.loads(p.read_text())
  if r.get('revision')==revisions[v] and r['path'] in expected and p.stem==s.signature(r['path'],r['revision']):records[v][r['path']]=r
 rows[v]=list(records[v].values())
complete=all(set(records[v])==expected for v in records)
def table(headers,values):
 return ['| '+' | '.join(headers)+' |','| '+' | '.join(['---']*len(headers))+' |']+['| '+' | '.join(str(x).replace('|','/') for x in row)+' |' for row in values]
def programs(r):return r.get('programs',[])
def views(r):return [x for p in programs(r) for x in p.get('views',[])] or r.get('views',[])
def hits(v,predicate):return {r['path'] for r in rows[v] if predicate(r)}
def symbols(r,bypass=None):
 slots=r.get('metadata',{}).get('slots',[])
 return [x.get('symbols',{}) for x in slots if bypass is None or bool(x.get('bypassed'))==bypass] if slots else [x.get('symbols',{}) for x in programs(r)]
def mentioned(r,names):return any(any(x.get(n,0) for n in names) for x in symbols(r,False))
mechanisms={v:defaultdict(set) for v in records};incidence={v:defaultdict(set) for v in records};bypassed={v:defaultdict(set) for v in records};occ={v:Counter() for v in records};kinds={v:Counter() for v in records}
for v in records:
 for r in rows[v]:
  p=r['path'];m=mechanisms[v]
  if r.get('timed_out'):m['bounded worker timeout'].add(p)
  if r.get('ui') in ['blank','missing-images','error','budget-hit']:m['UI '+r['ui']].add(p)
  for token_counts in symbols(r,False):
   for token,count in token_counts.items():incidence[v][token].add(p);occ[v][token]+=count
  for slot in r.get('metadata',{}).get('slots',[]):
   if slot.get('bypassed'):
    for token in slot.get('symbols',{}):bypassed[v][token].add(p)
  for pr in programs(r):
   if pr.get('lua',{}):
    for phase in ['init','runtime']:
     if pr['lua'].get(phase+'_faults',0):m['Lua '+phase+' fault'].add(p)
   if pr.get('native_frontend_consumed') is False:m['authored native frontend requested but not consumed'].add(p)
   if pr.get('sample_zone_count')==0:m['zero retained sample zones: '+pr.get('zero_zone_reason','unknown')].add(p)
   if pr.get('fallback_note'):m['audition uses fallback note; parity excluded'].add(p)
   if pr.get('ksp_runtime_faults'):m['KSP note-time runtime fault/budget'].add(p)
   for slot in pr.get('ksp',{}).get('slots',[]):
    if slot.get('compile_fault'):m['KSP compiler rejection'].add(p)
    if slot.get('disabled_block_errors'):m['v1 compiler admitted disabled callback'].add(p)
    for phase in ['init','persistence_changed']:
     status=slot.get(phase,{}).get('status','unknown')
     if status not in ['absent','completed','unknown']:m['KSP '+phase+' '+status].add(p)
  for view in views(r):
   kinds[v].update(view.get('kinds',{}))
   if view.get('bound',0)<view.get('interactive',0):m['visible widget lacks scalar readback binding'].add(p)
   if view.get('missing_images',0):m['image lookup/decode failure'].add(p)
   if view.get('custom_font_uses',0):m['custom font requested'].add(p)
   if view.get('font_declared',0)>view.get('font_success',0):m['font declaration not resolved by service'].add(p)
   if view.get('passive_value_changes',0):m['passive paint changes semantic value'].add(p)
   for field in ['placeholder_widgets','unsupported_params','geometry']:
    for name,n in view.get(field,{}).items():
     if n:m[field+': '+name].add(p)
   for paint in view.get('renders',[]):
    if paint.get('budget_hit'):m['Original paint tree budget'].add(p)
    if paint.get('background',{}).get('plain_background_fraction',0)>.9:m['page >90% plain background candidate'].add(p)
lines=['# Whole-corpus Original UI census','',f'Scope 5, 2026-10-08. Coverage: **{"COMPLETE" if complete else "PARTIAL — sweep still running"}**. Frozen installed corpus: **834 Kontakt paths (781 NKI + 53 NKM), 660 UVI programs; 1,494 item IDs**. One row per path/program ID; each Kontakt multi includes every embedded program observed by its production loader. No product fixes. Generated from the current cache at {datetime.now(timezone.utc).isoformat(timespec='seconds')}.','',
'**Load admission, authored UI painting and audible audition are separate results.** The plain per-instrument answer is in [v1.tsv](/home/derpcat/.cache/kontra-scan/results/v1.tsv) and [v2.tsv](/home/derpcat/.cache/kontra-scan/results/v2.tsv). `loads=yes` means the production importer and initial playable bank/plan returned successfully. A missing image, script callback fault or silent note can coexist with admitted loading. `loads=no` includes a bounded 90-second worker timeout; it is an observed failure under this probe, not proof of permanent incompatibility.','',
'## Frozen builds and reproducibility','',
'Shared scanner instrumentation and CLI: `tools/kontra-scan@2355155b56a5ae26add8bdbc287d46d42e7825f2`, based on v2 `7e82b152`. Pinned Kontakt v1: `0cb7a8a0` plus scanner adapter `audit/ui-census-v1-scanner-20261008@788f41fafa7e21ddf7b1917bc4cf43e0a83876b8`. UVI v1 is explicitly a later, separate baseline: sidecar `audit/uvi-v1-scanner-20261008@1c198e60`, product base `4bffbb18`; pinned Kontakt is unchanged. All are optimized release. The adjacent [installed README](/home/derpcat/.cache/kontra-scan/bin/README.md) records exact binary hashes, build date, rebuilding and limits.','']
lines+=table(['Adapter','SHA-256'],[(v,revisions[v]) for v in records]+[('v1 UVI sidecar',s.SIDECAR_SHA)])
lines+=['','```sh','~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v2 \\','  --list ~/.cache/kontra-scan/v2-items.tsv --start 0 --count 25 \\','  --out ~/.cache/kontra-scan/results/v2','```','',
'Rerun the same arguments after exit 75, then advance the slice. Use the same command with `kontra-scan-v1` for the paired adapter. Every shard owns exactly one heavy call, defaults to 235 seconds, and starts a new item only if its full timeout fits. Per-item cache identity includes the binary, item/container size and mtime, sidecar and shared note-plan digest. Canonical results exclude stale revisions/signatures. No decrypted scripts, resources, PCM, keys, authored error messages, names or property payloads are persisted; counters/hashes and a small gallery of OUR renders are retained.','',
'Validation: stdlib driver/cache/privacy checks pass; three optimized Rust scanner checks pass on each baseline; both required wrapper `cargo test --release --features shots --no-run` checks passed before push. The phase tests exercise actual evaluator/VM boundaries, including persistence failure suppressed by a successful public compiler result, v1 waiting callbacks and compiler-disabled blocks.','',
'## Exhaustive category counts','',
'Counts below include only the current installed revision and manifest IDs. Categories are mutually exclusive with precedence budget-hit → error → blank → missing-images → no-ui → original-ok. Mechanisms later overlap; their counts cannot be added as instruments unlocked. `original-ok` certifies requested-image resolution and successful authored-view construction/paint, not vendor pixel, gesture, typography, automation or callback parity.']
lines+=['']+table(['Build','Rows / 1494','Kontakt / 834','UVI / 660','Loads yes','Loads no','Original OK','Missing images','Blank','No UI','Error','Budget hit','Audible','Silent'],[(v,len(rows[v]),sum('::' not in r['path'] for r in rows[v]),sum('::' in r['path'] for r in rows[v]),*(Counter(r.get('loads') for r in rows[v])[x] for x in ['yes','no']),*(Counter(r.get('ui') for r in rows[v])[x] for x in ['original-ok','missing-images','blank','no-ui','error','budget-hit']),*(Counter(r.get('plays_note') for r in rows[v])[x] for x in ['yes','silent'])) for v in records])
paired=set(records['v1'])&set(records['v2']);regressions=[p for p in paired if records['v1'][p]['loads']=='yes' and records['v2'][p]['loads']=='no'];improvements=[p for p in paired if records['v1'][p]['loads']=='no' and records['v2'][p]['loads']=='yes'];fallbacks=[p for p in paired if records['v1'][p].get('fallback_note') or records['v2'][p].get('fallback_note')]
mismatch=[p for p in paired if records['v1'][p].get('note_picked')!=records['v2'][p].get('note_picked') and all(records[v][p]['loads']=='yes' for v in records)]
lines+=['',f'Paired coverage: **{len(paired)}/1494**. V1 loads / v2 does not: **{len(regressions)}** ([complete observed list](/home/derpcat/.cache/kontra-scan/results/v1-loads-v2-doesnt.tsv)). V2 loads / v1 does not: **{len(improvements)}**. Fallback-note comparisons: **{len(fallbacks)}**, excluded from parity. Both-loaded note mismatches: **{len(mismatch)}**, excluded from sound-regression claims. Counts are exhaustive only when the coverage marker is COMPLETE.','',
'The UVI auditor’s prior stopped sample admitted AO **0/80 on v1 vs 80/80 on v2**, with v1 graph-preflight rejection and no timeouts. That earlier observation is separate from the current paired census. V1’s stronger typed UI/state does not imply stronger format/graph admission. The paired table above is the reproducible comparison at the declared baselines.','',
'### Per-library breakdown','']
libs=[]
for v in records:
 for library in sorted({r['library'] for r in rows[v]}):
  rs=[r for r in rows[v] if r['library']==library];ls=Counter(r['loads'] for r in rs);us=Counter(r['ui'] for r in rs)
  libs.append((v,library,len(rs),ls['yes'],ls['no'],us['original-ok'],us['missing-images'],us['blank'],us['error'],us['budget-hit']))
lines+=table(['Build','Library','Rows','Loads','Does not load','Original OK','Missing images','Blank','Error','Budget'],libs)
lines+=['','## Exhaustive measured failure mechanisms','',
'An unsupported parameter can be nonvisual metadata. An outside-page or zero-size widget can be authored intentionally. The frozen scalar criterion excludes typed text/array/service bindings. Separate bound_typed counts validate text/array targets in installed KSP models; they do not prove live typed edits. UVI targets and phantom-free controls stay unknown where the frozen baseline has no accessor or origin marker. A page mostly one colour is an unreadability candidate, not proof of native mismatch. Those distinctions are retained in the report rather than labeling every occurrence broken.','']
lines+=table(['Mechanism','v2 item incidence','v1 item incidence'],[(k,len(mechanisms['v2'][k]),len(mechanisms['v1'][k])) for k in sorted(set(mechanisms['v2'])|set(mechanisms['v1']),key=lambda k:(-len(mechanisms['v2'][k]),k))])
lines+=['','### Script-slot, callback and saved-state partitions','',
'Raw slots are partitioned before compilation into decode_failed / bypassed / inline_nonempty / linked_only / empty. Wire slot, owner and program index are distinct from compact runtime admission. Active slots skip bypassed/empty slots. V1 compile-admitted allows disabled non-init callback blocks; compile-clean requires zero `Program.errors`. Init and persistence_changed completion/faults are independently observed, never inferred from public `Ok`. `absent`, `compile_disabled`, `entered`, `completed`, `faulted`, `budget_stopped`, `waiting`, `deferred` and `dropped` remain distinct. Diagnostics contain a fixed safe category, static builtin and numeric location only.','']
fields=['bound_typed','sample_zone_count','slots_seen','slots_decode_failed','slots_bypassed','slots_inline_nonempty','slots_linked_only','slots_empty','active_script_slots','compiled_script_slots','clean_compiled_slots','disabled_block_errors','init_callbacks_completed','persistence_changed_completed','load_fault_records','ksp_runtime_fault_records']
lines+=table(['Field','v2 sum / observed rows','v1 sum / observed rows'],[(f,*[f"{sum(r[f] for r in rows[v] if isinstance(r.get(f),int))} / {sum(isinstance(r.get(f),int) for r in rows[v])}" for v in ['v2','v1']]) for f in fields])
phase_rows=[]
for v in ['v2','v1']:
 for phase in ['init','persistence_changed']:
  counts=Counter(slot.get(phase,{}).get('status','unknown') for r in rows[v] for pr in programs(r) for slot in pr.get('ksp',{}).get('slots',[]))
  phase_rows.extend((v,phase,status,counts[status]) for status in ['absent','compile_disabled','entered','completed','faulted','budget_stopped','waiting','deferred','dropped','unknown'])
lines+=['']+table(['Build','Observed callback phase','Status','Slot observations'],phase_rows)
saved_rows=[]
for v in ['v2','v1']:
 raw=Counter();admitted=Counter();integrity=Counter()
 for r in rows[v]:
  for slot in r.get('metadata',{}).get('slots',[]):
   integrity[slot.get('saved_table_integrity','unknown')]+=1
   if slot.get('saved_histogram_complete'):raw.update(slot.get('raw_saved_entries_by_sigil',{}))
  for pr in programs(r):admitted.update(pr.get('admitted_saved_entries_by_sigil',{}))
 saved_rows.extend((v,sigil,raw[sigil],admitted[sigil]) for sigil in ['$','~','%','?','@','!','empty','other'])
 lines+=['',f'{v} saved-table integrity (raw slot observations): '+', '.join(f'{k}={n}' for k,n in sorted(integrity.items()))+'. Only complete raw histograms enter the counts below.']
lines+=['']+table(['Build','Fixed sigil','Raw complete-table entries','Admitted entries'],saved_rows)
lines+=['','Saved sigils use only `$ ~ % ? @ ! empty other`; malformed table framing is distinguished from params() returning an empty Vec. Raw and admitted counts do not prove declaration-aware restoration. The manifest contains no NKSN snapshots; the separate prior 1,103-snapshot audit is not this denominator.','',
'## Conflux mandatory first witness','']
for v in ['v2','v1']:
 r=next((r for r in rows[v] if '/Conflux 1.1.0 ' in r['path'] and r['path'].endswith('/Conflux.nki')),None)
 if not r:lines+=['Current '+v+' witness pending.',''];continue
 lines+=[f"**{v}: loads {r['loads']}; Original {r['ui']}; bindings {r['controls_bound']}; audition {r['plays_note']}; note {r.get('note_picked')}; load {r['load_ms']} ms; peak RSS {r['peak_rss_mb']} MB.**",'']
 if v=='v2':
  for view in views(r)[:1]:
   paints=view.get('renders',[]);bg=paints[0].get('background',{}) if paints else {}
   lines+=[f"Main authored view: {view.get('widgets')} widgets, {view.get('visible')} visible, {view.get('bound')}/{view.get('interactive')} scalar bindings; {view.get('assets')} image requests / {view.get('missing_images')} missing. Declared background {bg.get('background_rgba')}; plain fraction {bg.get('plain_background_fraction',0):.4%}. This is a cream, nonuniform page, not a literal all-white pixel buffer. The renderer auditor's 94.94% figure uses its separate capture/layout and must not replace this matched witness.",'']
lines+=['W2 traced the six scalar-excluded main bindings to footer TextEdits; they are typed variable targets, not absent declarations. W5’s later implementation can type/read back all six; that later result is separate from this frozen baseline. W2 also removed 33 inferred phantom knobs, changing its later scalar denominator to about74/80 without losing real bindings. This baseline has no reliable origin marker; the census never subtracts a library-specific33.\n\nThe witness refutes “all scripts fail” and “all scalar controls are unbound”: three active slots admit and init completes on v2. The shared scanner tests readback, not actual dragging. Cross-scope diagnosis explains the user’s degraded result: authored native/package view requests have no frontend consumer; classic wallpaper/resource routing and contrast differ; light_under ignores solid page colours; picture/fonts and typed widgets lose authored semantics. The baseline also selects Vector when unsupported metadata is empty and resets view choice on interface publication; Conflux’s NKS diagnostics select Bitmap, so default Vector alone is not its complete explanation.','',
'For immovable knobs, the widgets witness found all 81 visible continuous controls bound/readable and 78 main controls are knobs. The slider-axis defect cannot explain those knobs. Quantized feedback loses fine fractional drag accumulation, authored drag travel is ignored, wheel/focus routing and stale UI/publication/queue admission can produce unchanged apparent values. Native scalar admission works independently. Confirm each gesture through the widgets/loop probes; this passive census does not claim a new pointer trace.','',
'## Ranked systemic fixes','',
'Reach is an overlap-aware union of measured mechanism candidates or active source-token users, never a sum of occurrences. “Fully unlocked” is unmeasured for every fix until matched native render, gesture, callback and state tests pass. Ranking prioritizes breadth and the substrate needed by other fixes. Effort S = localized existing-path repair, M = several adapters plus tests, L = shared typed service/lifecycle work. V1 is the first semantic reference; keep v2 ownership/real-time boundaries.','']
fixes=[
('Original authored view and stable choice','M','all admitted UI-bearing items',lambda r:bool(views(r)),'Honor Original by default and explicit choice across interface revisions; use v1 view policy, not diagnostics to select a renderer.','Fresh load, clear diagnostics, publish a changed property, switch views: Original and user choice remain stable.'),
('One correct resource resolver / native authored frontends','M/L','image lookup/decode failures',lambda r:any(x.get('missing_images',0) for x in views(r)),'Resolve source-family resources, loose/NKR/NICNT/UFS packages and wallpaper/native requests through existing bounded resolver; no library exceptions.','Authored fixture covers each container route, strip metadata and native view; every requested resource resolves and Original pixel comparison improves.'),
('Typed UI mutation/getter path','M','active UI setters/getters/aliases',lambda r:mentioned(r,['set_control_par','set_control_par_str','set_control_par_arr','get_control_par','get_control_par_str','set_text','move_control','hide_part']),'Normalize aliases once; typed indexed key/value/readback shares live storage and init/runtime semantics.','Set/get strings/reals/table/XY indexes independently; callback publication preserves exact values; programmatic setters do not recurse.'),
('All typed widget edits and source callbacks','L','unbound / placeholder widgets',lambda r:any(x.get('bound',0)<x.get('interactive',0) or x.get('placeholder_widgets') for x in views(r)),'Extend existing control ownership to table/XY/text/file/mouse/service widgets and event context; reuse v1 contracts.','Gesture → admission → value → handler → paint for every widget; include indexes, modifiers, file payloads, waits and rejected queue capacity.'),
('Declared page colours, fonts and readable native styling','M','fonts / >90% background candidates',lambda r:any(x.get('font_declared',0)>x.get('font_success',0) or any(y.get('background',{}).get('plain_background_fraction',0)>.9 for y in x.get('renders',[])) for x in views(r)),'Use declared background in contrast decisions and load custom/state fonts; preserve text alignment/style instead of stock replacement.','Matched declared-color fixture with real fonts, hover/pressed states and alpha; measure text/background contrast and native pixel regions.'),
('Geometry, parenting, strip/frame value mapping','M','geometry / strip users',lambda r:any(x.get('geometry') or x.get('image_strips',0) for x in views(r)),'Canonical geometry once; retain explicit axes, grid/pixel transitions, parent/z/hide and source frame laws.','Width-only and height-only fixtures, nested/cyclic panels, HiDPI and horizontal/vertical frame endpoints match reference.'),
('Saved UI state and snapshot policy','M/L','active persistence symbols',lambda r:mentioned(r,['make_persistent','make_instr_persistent','read_persistent_var','set_snapshot_type']),'Integrate existing typed persistence reader, consume immediate reads once, preserve string arrays, scopes and running snapshot policies.','All fixed sigils, empty string-array cells, menu position vs semantic value, four snapshot policies and host save/reopen retain state.'),
('Compiler/callback failures with visible safe diagnostics','M','KSP/Lua faults or disabled blocks',lambda r:any(x.get('script_error_count',0) or any(z.get('compile_fault') or z.get('disabled_block_errors') or any(z.get(p,{}).get('fault') for p in ['init','persistence_changed']) for z in x.get('ksp',{}).get('slots',[])) or any((x.get('lua') or {}).get(p+'_faults',0) for p in ['init','runtime']) for x in programs(r)),'Close unsupported surface using existing runtime; distinguish admission, disabled callbacks, init/persistence faults and runaway budgets.','Authored failure-phase fixtures produce truthful sanitized diagnostics; repaired callback completes and corpus gain is measured at exact slot ownership.'),
('Paint/publication budgets and incremental runtime UI','M/L','UI-bearing items / actual budget hits',lambda r:bool(views(r)),'Stable slot/control IDs and bounded incremental publication; virtualize large trees without changing authored meaning.','Large scripted scene paints within budget; waiting edits stay ordered; measure p50/p95 frame and input latency with identical scene fidelity.'),
('Native drag, fine adjust, wheel and host automation','M','continuous/automatable widget users',lambda r:any(any(x.get('kinds',{}).get(k,0) for k in ['ui_knob','ui_slider','ui_value_edit']) for x in views(r)),'Preserve fractional pointer accumulation, source sensitivity/axis/defaults and focus-aware wheel; expose authored automation IDs.','Actual pointer/keyboard/wheel/default/host gesture fixtures modify every bound continuous control; low-resolution fine drags accumulate and callbacks retain ordering.')]
lines+=table(['Rank','Mechanism','Effort','Measured v2 candidate items','Fully unlocked','Root repair / v1 reference','Proof'],[(i+1,name,effort,len(hits('v2',pred)),'Unknown',repair,proof) for i,(name,effort,label,pred,repair,proof) in enumerate(fixes)])
lines+=['','## Exhaustive spec incidence and status matrix','',
'The generated scanner whitelist is the union of the repository’s UI widgets, CONTROL_PAR identifiers, UI helpers/callbacks and persistence helpers. Comments and strings are skipped; one leading underscore alias is canonicalized. The whitelist is generated from compiler UI/keyboard/persistence builtin tables plus CONTROL_PAR/spec inventory; its digest/count is attached to each metadata record. Active incidence includes inactive preprocessor branches/unreachable functions and is not an execution count. Bypassed incidence is separate. Linked unresolved/native-package declarations can be absent from lexical counters; actual renderer widget inventory supplements them. Zero means unobserved in the surfaced inline declarations, not proof of complete corpus absence; decode failures, linked/native packages and generated UI can hide lexical use even after the manifest is complete.','',
'Expected contract, inspected v2 status and source/root-fix references below reuse the params auditor’s exhaustive matrix at `origin/audit/ui-params-20261008`. Its F1–F10 definitions and target tests are in [UI_PARAMS.md](UI_PARAMS.md); rendering and gesture status is completed by [UI_RENDER.md](UI_RENDER.md), [UI_WIDGETS.md](UI_WIDGETS.md) and [UI_LOOP.md](UI_LOOP.md). Correct means inspected/tested mechanism, not complete vendor fidelity. Unknown historical/vendor extension semantics remain explicit.','']
peer=subprocess.run(['git','show','origin/audit/ui-params-20261008:docs/audit-2026-10-08/UI_PARAMS.md'],capture_output=True,text=True,check=True).stdout
matrix={}
for line in peer.splitlines():
 if line.startswith('| `'):
  cells=[x.strip() for x in line.strip().strip('|').split('|')]
  name=cells[0].strip('`')
  if name.startswith('on '):name=name[3:]
  if len(cells)>=4 and name in Path('tools/kontra-scan/ui-symbols.txt').read_text().splitlines():matrix[name]=cells[1:3]
def constant_contract(name):
 if name=='get_folder':return ['Return the requested host/resource folder path for a native folder-ID enum','Missing: init evaluator returns an empty string (sampler-ksp/eval.rs:1265); add bounded source-family folder service, compare v1 path resolution.']
 if name=='show_library_tab':return ['Request the host library/browser tab to become visible','Missing: evaluator no-op and lowerer empty result (sampler-ksp/eval.rs:1139, lower.rs:2095); route an explicit editor/host request.']
 if name=='ui_control':return ['Run the originating control callback after its user edit, with current value and originating context; programmatic writes must not recurse','Partial: scalar control callback admission exists; typed service/event context and wait retention need widget/loop proof. UI_PARAMS F4/F10; UI_LOOP callback matrix.']
 if name=='ui_controls':return ['Multi-control callback dispatch with the native changed-control context and ordering','Unknown: distinguish callback syntax/profile from single-control admission; authored multi-control fixture plus ordered runtime observations required. KSP_SURFACE callback inventory.']
 if name=='ui_update':return ['UI update callback executes at the native scheduled UI refresh boundary','Partial/unknown: source support is distinct from UI publication cadence; verify actual callback scheduling and edits across waits. UI_LOOP lifecycle matrix.']
 if name in ['$CONTROL_PAR_X','$CONTROL_PAR_Y']:return ['Vendor XY indexed axis/property extension; verify native profile and get/set units before claiming support','Unknown vendor semantics: fixed public token is counted, no authored value persisted; measure typed axis/index set/get against v1 and native host.']
 if name=='$CONTROL_PAR_WAVE_END_':return ['Historical waveform-end identifier/alias; confirm profile spelling and indexed units','Unknown alias semantics: lexical incidence retained; compare standard WAVE_END native getter/setter on a minimal waveform fixture.']
 if name.startswith('$HIDE_'):return ['Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility','Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix.']
 if name.startswith('$KNOB_UNIT_'):return ['Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value','Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference.']
 if name.startswith('$NI_CONTROL_TYPE_'):return ['Read-only vendor widget type enum used by TYPE lookup','Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture.']
 if name.startswith('$NI_DND_') or name.startswith('$NI_FILE_TYPE_'):return ['Drop acceptance cardinality or file selector filter enum','Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10.']
 if name.startswith('$NI_MOUSE_') or name=='$NI_CONTROL_PAR_IDX':return ['Originating mouse event/inside state or indexed control position in its UI callback','Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194.']
 if name.startswith('$NI_WF_') or name.startswith('$NI_WT_') or name.startswith('$UI_W'):return ['Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified','Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows.']
 if name in ['$INST_ICON_ID','$INST_WALLPAPER_ID']:return ['Special instrument icon/wallpaper UI identity; resolve authored source resource','Partial/missing: classic wallpaper and native frontend resolution are incomplete. UI_RENDER native/wallpaper matrix; route existing source-family resolver.']
 if name=='persistence_changed':return ['Callback after native saved values are restored; derived UI/state is rebuilt before publication','Broad order retained, but faults can be suppressed as warnings; scanner independently observes completion/fault. UI_PARAMS lifecycle matrix; actual evaluator/VM scanner phase tests.']
 return ['UI/native runtime declaration; exact profile semantics require corresponding architecture/manual entry','Unknown in this census; runtime/gesture contract is not certified by lexical use. See scope matrix and KSP_SURFACE.json.']
names=Path('tools/kontra-scan/ui-symbols.txt').read_text().splitlines()
lines+=table(['Spec token','Expected contract','v2 inspected status / evidence / systemic fix','v2 active items','v1 active items','v2 bypassed items','v2 token occurrences'],[(n,*(matrix.get(n,constant_contract(n))),len(incidence['v2'][n]),len(incidence['v1'][n]),len(bypassed['v2'][n]),occ['v2'][n]) for n in names])
lines+=['','### Actual authored widget inventory','',
'Counts sum all observed authored views/programs; they are declarations retained in UI models, including hidden widgets, not unique visible controls or source-token occurrences. Different v1/v2 projection and shell layouts prevent treating count differences as native parity.','']
lines+=table(['Widget kind','v2 widgets','v1 widgets'],[(k,kinds['v2'][k],kinds['v1'][k]) for k in sorted(set(kinds['v2'])|set(kinds['v1']))])
lines+=['','## Small gallery of OUR Original renders','',
'Images below are rendered output, never extracted library assets. The gallery is under 50 MB. Screenshots show one observed page; successful paint does not certify interaction or a native-host match.','']
gallery=out.parent/'census-gallery';gallery.mkdir(exist_ok=True)
selected=[]
for old in gallery.glob('*.png'):old.unlink()
for v in ['v2','v1']:
 for r in rows[v]:
  if any(x[0]==v and x[2]['library']==r['library'] for x in selected):continue
  if len([x for x in selected if x[0]==v])>=3:break
  for view in views(r):
   paints=view.get('renders',[]) or [view.get('render',view)]
   shot=next((x.get('shot') for x in paints if x.get('shot') and Path(x['shot']).is_file()),None)
   if shot:
    name=f'{v}-{len(selected)+1}.png';shutil.copyfile(shot,gallery/name);selected.append((v,name,r));break
for v,name,r in selected:lines += [f"**{v}: {r['library']} — {r['ui']}; loads {r['loads']}.**",'',f'![{v} Original renderer](census-gallery/{name})','']
lines+=['## Other scopes and unknowns','',
'- Standalone authored-page paint is distinct from an unconsumed native/frontend request. Native requests are counted from actual evaluator models without args; Original OK remains a paint/resource check, not proof the native package has a consumer.\n- Readability/native pixel parity: background fractions are candidates; compare authored fixtures and matched native screenshots at identical page, value, state and DPI. Uniform hidden views are not a blank visible main page.',
'- Bound controls: this probe checks scalar readback. Gesture, typed cell/text/file edit and source callback/automation parity need actual pointer/host traces from widgets/loop scopes.',
'- UVI baseline native-key metadata: v2 7e82 lacks the newer keyboard snapshot accessor. Feature-gated observation at its inert setKeyColour/resetKeyColour stub preserves production behavior but lacks processor-local identity. Conflicting writes stay unknown. Sidecar uses conservative native snapshots; record declarations separately from sample audibility.',
'- Shared auditions: v2 establishes per-program key/velocity. Declared white keys intersect retained sample-zone coverage at velocity64; then zone coverage near middle C; then fallback. Avoid invalid/control/keyswitch declarations. Source is native_declared / zone_coverage / fallback, never density mislabeled native-valid. One half-second audible note is not full sample/DSP correctness; silent is inconclusive for articulation/CC/noise/control regions. No audio regression claim without identical per-ID notes. The corrected scanner sends a safe explicit fallback when surviving load-time zone coverage is empty, so note callbacks still run; a fully invalid keyboard yields no safe audition and plays_note=no, not a false silent-note claim.',
'- Instrumentation coverage: unknown is distinct from zero. V1 UVI sidecar does not expose every requested Lua/asset/budget field; those remain unknown. Its shared-note-plan override label is kept as adapter_pick_source, while pick_source uses the matching current v2 witness’s common-plan origin; unavailable origin stays unknown. Asset success counts are observed requests, not every archive member. Custom fonts have no v2 baseline service. V1 whole-editor pixels cannot establish authored-page background coverage. V2 paints standalone authored pages, not the whole plugin shell; a whole-editor tree budget failure on another UI branch must remain a separate measurement.',
'- Cache reproducibility: input resources must remain immutable; container mtime/size does not detect a loose-resource replacement. Use a fresh output after resource changes. Cold/warm load wall time and peak RSS are single runs, not matched performance benchmarks.',
'- Scope/format limits: multi/bank execution uses admitted production programs; unresolved linked scripts/native packages need resource traversal. Snapshot files/recovery saves are excluded from the frozen1494-ID list. Fully unlocked and corrected native-host parity remain unmeasured.',
'- Reuse before rebuilding: v1 live UI semantics, prior Kontakt/native-resource and UVI typed-UI branches, and the existing strict typed persistence decoder are references. Audit tools do not merge those product fixes. Source line spans belong to the frozen baseline; future integration must regenerate counts with a newly built shared scanner.',
'','## Requested extension fields and privacy boundary','',
'[Symbol aggregates](/home/derpcat/.cache/kontra-scan/results/v2/symbol-aggregates.tsv) include attempted/parsed/lexical coverage, scanner digest, NKI/NKM and program-owner incidence, initialized widget kinds and fixed saved sigils. [V1 Original OK / v2 missing or error](/home/derpcat/.cache/kontra-scan/v1ok-v2missing.tsv) is separate from the load-admission regression list.\n\nFirst nine stable fields: `path library loads ui controls_bound plays_note load_ms peak_rss_mb reason`. All columns below are exported by the ONE shared CLI; raw detailed metrics preserve independent slot/phase ownership. CONTROL_PAR refs are the `$CONTROL_PAR_*` subset of `ui_api_refs`, not a separate duplicated column.','',
'`'+'`, `'.join(s.COLUMNS[9:])+'`','',
'Detailed JSON retains Lua init/runtime safe first diagnostic categories/digests, actual paint and budget status, widget kinds, lookup/decode/font success/failure categories, frames/strips/margins, declared/observed background RGBA and pixel fraction, load path, sample residency/underruns, runtime behavior outcomes, wire/runtime slots, compile admission/cleanliness, and independent init/persistence phase outcomes. Never serialize authored fault messages, identifiers, saved values, source text or resource bytes.','']
assert all(sum(r.get('slots_'+k,0) for k in ['decode_failed','bypassed','inline_nonempty','linked_only','empty'])==r['slots_seen'] for v in records for r in rows[v] if isinstance(r.get('slots_seen'),int))
assert sum(p.stat().st_size for p in gallery.glob('*.png'))<50*1024*1024
out.write_text('\n'.join(lines)+'\n')
print(f'{out}: v1={len(rows["v1"])} v2={len(rows["v2"])} complete={complete}; spec rows={len(names)}')
