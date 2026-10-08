#!/usr/bin/env python3
"""Regenerate the scope-5 report from the ONE shared scanner's current cache."""
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib, importlib.util, json, math, shutil, statistics, subprocess, sys
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
  if r.get('revision')==revisions[v] and r['path'] in expected and p.stem==s.signature(r['path'],r['revision']):records[v][r['path']]=s.extra_columns(r)
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
  if r.get('loads')=='yes' and r.get('plays_note')=='silent':m['selected-note audition silent in 0.5-second probe'].add(p)
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
   if pr.get('fallback_note'):
    m['no safe audition key; sound unmeasured' if pr.get('pick') is None else 'audition uses fallback note; parity excluded'].add(p)
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
   if isinstance(view.get('font_declared'),int) and isinstance(view.get('font_success'),int) and view['font_declared']>view['font_success']:m['font declaration not resolved by service'].add(p)
   if view.get('passive_value_changes',0):m['passive paint changes semantic value'].add(p)
   for field in ['placeholder_widgets','unsupported_params','geometry']:
    for name,n in view.get(field,{}).items():
     if n:m[('Native bridge candidate ' if view.get('source_presentation')=='native-package' else '')+field+': '+name].add(p)
   for paint in view.get('renders',[]):
    if paint.get('budget_hit'):m['Original paint tree budget'].add(p)
    if view.get('visible',0)>0 and paint.get('background',{}).get('plain_background_fraction',0)>.9:m['page >90% plain background candidate'].add(p)
lines=['# Whole-corpus Original UI census','',f'Scope 5, 2026-10-08. Coverage: **{"COMPLETE" if complete else "PARTIAL — sweep still running"}**. Frozen installed corpus: **834 Kontakt paths (781 NKI + 53 NKM), 660 UVI programs; 1,494 item IDs**. One row per path/program ID; each Kontakt multi includes every embedded program observed by its production loader. Scope 5 adds scanner instrumentation and audit documentation; product fixes belong to the frozen integration checkpoint. Generated from the current cache at {datetime.now(timezone.utc).isoformat(timespec='seconds')}.','',
'**Load admission, authored UI painting and audible audition are separate results.** The plain per-instrument answer is in [v1.tsv](/home/derpcat/.cache/kontra-scan/results/v1.tsv) and [v2.tsv](/home/derpcat/.cache/kontra-scan/results/v2.tsv). `loads=yes` means the production importer and initial playable bank/plan returned successfully. A missing image, script callback fault or silent note can coexist with admitted loading. `loads=no` includes a bounded 90-second worker timeout; it is an observed failure under this probe, not proof of permanent incompatibility.','',
'## Frozen builds and reproducibility','',
'Current v2 product checkpoint: `integrate/core-v2@9993db691a5f69d31980357694a678e358785e5e`, with the shared scanner extension. Native observer source: `tools/kontra-scan-native-9993-20261008@ef88dcb6bd5e50ff12ad8317bdb73da92fcd415f`. The installed README distinguishes the actual frozen binary source from later instrumentation. Historical partial v2 `7e82b152` results are preserved at audit checkpoint `236d3882` and do not enter these current-revision counts. Pinned Kontakt v1: `0cb7a8a0` plus scanner adapter `audit/ui-census-v1-scanner-20261008@59c6cbbbcc72cc38efda00fbde8cf9c2be8b4076`. UVI v1 is explicitly a later, separate baseline: sidecar `audit/uvi-v1-scanner-20261008@026bdbb49f29a5ad752b3470a5f6f64a20a8957d`, product base `4bffbb18`; pinned Kontakt is unchanged. All are optimized release. The adjacent [installed README](/home/derpcat/.cache/kontra-scan/bin/README.md) records exact binary hashes, build date, rebuilding and limits.','']
lines+=table(['Adapter','SHA-256'],[(v,revisions[v]) for v in records]+[('v1 UVI sidecar',s.SIDECAR_SHA)])
lines+=['','```sh','~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v2 \\','  --list ~/.cache/kontra-scan/v2-items.tsv --start 0 --count 25 \\','  --out ~/.cache/kontra-scan/results/v2','```','',
'Rerun the same arguments after exit 75, then advance the slice. Use the same command with `kontra-scan-v1` for the paired adapter. Every shard owns exactly one heavy call, defaults to 235 seconds, and starts a new item only if its full timeout fits. Per-item cache identity includes the binary, item/container size and mtime, sidecar and shared note-plan digest. Canonical results exclude stale revisions/signatures. No decrypted scripts, resources, PCM, keys, authored error messages, names or property payloads are persisted; counters/hashes and a small gallery of OUR renders are retained.','',
'Validation: stdlib driver/cache/privacy checks pass; three optimized Rust scanner checks pass on each baseline; both required wrapper `cargo test --release --features shots --no-run` checks passed before push. The phase tests exercise actual evaluator/VM boundaries, including persistence failure suppressed by a successful public compiler result, v1 waiting callbacks and compiler-disabled blocks.','',
'## Exhaustive category counts','',
'Counts below include only the current installed revision and manifest IDs. Categories are mutually exclusive with precedence budget-hit → error → blank → missing-images → no-ui → original-ok. Mechanisms later overlap; their counts cannot be added as instruments unlocked. `original-ok` certifies requested-image resolution and successful authored-view construction/paint, not vendor pixel, gesture, typography, automation or callback parity.']
lines+=['']+table(['Build','Rows / 1494','Kontakt / 834','UVI / 660','Loads yes','Loads no','Original OK','Missing images','Blank','No UI','Error','Budget hit','Audible','Silent'],[(v,len(rows[v]),sum('::' not in r['path'] for r in rows[v]),sum('::' in r['path'] for r in rows[v]),*(Counter(r.get('loads') for r in rows[v])[x] for x in ['yes','no']),*(Counter(r.get('ui') for r in rows[v])[x] for x in ['original-ok','missing-images','blank','no-ui','error','budget-hit']),*(Counter(r.get('plays_note') for r in rows[v])[x] for x in ['yes','silent'])) for v in records])
paired=set(records['v1'])&set(records['v2']);regressions=[p for p in paired if records['v1'][p]['loads']=='yes' and records['v2'][p]['loads']=='no'];improvements=[p for p in paired if records['v1'][p]['loads']=='no' and records['v2'][p]['loads']=='yes'];fallbacks=[p for p in paired if records['v1'][p].get('fallback_note') or records['v2'][p].get('fallback_note')]
mismatch=[p for p in paired if records['v1'][p].get('note_picked')!=records['v2'][p].get('note_picked') and all(records[v][p]['loads']=='yes' for v in records)]
sound_comparable=paired-set(fallbacks)-set(mismatch)
sound_differences={v:sorted(p for p in sound_comparable if all(records[w][p]['loads']=='yes' for w in records) and records[v][p].get('plays_note')=='silent' and records['v2' if v=='v1' else 'v1'][p].get('plays_note')=='yes') for v in records}
sound_counts=Counter(records['v2'][p]['library'] for p in sound_differences['v2'])
lines+=['',f'Paired coverage: **{len(paired)}/1494**. V1 loads / v2 does not: **{len(regressions)}** ([complete observed list](/home/derpcat/.cache/kontra-scan/results/v1-loads-v2-doesnt.tsv)). V2 loads / v1 does not: **{len(improvements)}**. Fallback-note comparisons: **{len(fallbacks)}**, excluded from parity. Both-loaded note mismatches: **{len(mismatch)}**, excluded from sound-regression claims. Counts are exhaustive only when the coverage marker is COMPLETE.','',
f'Same-note auditions among fully admitted, non-fallback pairs: **{len(sound_differences["v2"])} v1 audible / v2 silent**, and **{len(sound_differences["v1"])} v2 audible / v1 silent**. These are observed half-second probe differences, not native-host sound certification or permanent silence. Both versions used the same recorded note; resource residency, callback state and signal-graph stages require diagnosis before assigning a root cause. The per-item TSVs retain every note and result. No-safe-key programs remain unmeasured and are excluded.','',
*table(['Library','Same-note v1 audible / v2 silent'],sorted(sound_counts.items())),'',
'The UVI auditor’s prior stopped sample admitted AO **0/80 on v1 vs 80/80 on v2**, with v1 graph-preflight rejection and no timeouts. That earlier observation is separate from the current paired census. V1’s stronger typed UI/state does not imply stronger format/graph admission. The paired table above is the reproducible comparison at the declared baselines.','',
'### Load onset and first authored frame (section J)','',
'Numeric timing fields observe actual output/paint from the first production program import, with Original painting and audition concurrent. The lexical metadata prepass, process spawn and PNG/hash work are outside this clock. A multi shares the item clock. first_audio_ms observes the first finite, exactly nonzero output block; the audible result separately requires amplitude above1e-5. Missing/silent/no-safe-key output remains unknown, never a zero onset. These one-shot CPU-scanner wall times include machine contention and are not matched native-host or warm-cache performance acceptance.','']
timing_rows=[]
for v in ['v2','v1']:
 for corpus in ['Kontakt','UVI']:
  rs=[r for r in rows[v] if ('::' in r['path'])==(corpus=='UVI')]
  for field in ['first_audio_ms','ui_first_frame_ms']:
   ns=sorted(float(r[field]) for r in rs if isinstance(r.get(field),(float,int)))
   timing_rows.append((v,corpus,field,len(ns),len(rs)-len(ns),round(statistics.median(ns),2) if ns else 'Unknown',round(ns[max(0,math.ceil(.95*len(ns))-1)],2) if ns else 'Unknown'))
lines+=table(['Build','Corpus','Timing field','Observed numeric','Unknown','Median ms','p95 ms'],timing_rows)
lines+=['']+table(['Build','Product cache state','Rows'],[(v,k,n) for v in ['v2','v1'] for k,n in sorted(Counter(r.get('cache_state','unknown') for r in rows[v]).items())])
lines+=['', 'load_ms is unchanged and includes pinned-v1 deferred initial sample-bank preload; it is not first sound. cache_state describes the product metadata/header cache, not metrics reuse or OS page cache. Pinned Kontakt v1 scanner disables those cache reads/writes; frozen v2 has no product metadata cache, so those adapters report cold. UVI sidecar uses Worker::start after its metadata/assets prepass, includes required pre-audition native snapshots, and observes concurrent paint/audio. Its load_ms retains its separate earlier legacy origin. The common driver forces persistent decoded PCM caching off; that observed product condition is cold. Unknown remains explicit. OS cache is uncontrolled. Future integration cache paths require actual cache-hit telemetry before warm/cold acceptance.', '',
'### Per-library breakdown','']
libs=[]
for v in records:
 for library in sorted({r['library'] for r in rows[v]}):
  rs=[r for r in rows[v] if r['library']==library];ls=Counter(r['loads'] for r in rs);us=Counter(r['ui'] for r in rs)
  libs.append((v,library,len(rs),ls['yes'],ls['no'],us['original-ok'],us['missing-images'],us['blank'],us['error'],us['budget-hit']))
lines+=table(['Build','Library','Rows','Loads','Does not load','Original OK','Missing images','Blank','Error','Budget'],libs)
lines+=['','## Exhaustive measured failure mechanisms','',
'At frozen checkpoint 9993, Kontakt Source.read consumes Resources.read as Option; typed read_result failures are masked by that compatibility adapter. A reported lookup-not-found therefore does not prove an absent file: invalid, ambiguous, inaccessible or corrupt resources can yield the same observation. Request namespace and own-index attribution are not exposed by this frozen collector and remain unknown. Future typed resolver changes require a separate checkpoint measurement.','',
'An unsupported parameter can be nonvisual metadata. An outside-page or zero-size widget can be authored intentionally. The frozen scalar criterion excludes typed text/array/service bindings. Separate bound_typed counts validate text/array targets in installed KSP models; they do not prove live typed edits. UVI targets and phantom-free controls stay unknown where the frozen baseline has no accessor or origin marker. A page mostly one colour is an unreadability candidate, not proof of native mismatch. Those distinctions are retained in the report rather than labeling every occurrence broken.','']
lines+=table(['Mechanism','v2 item incidence','v1 item incidence'],[(k,len(mechanisms['v2'][k]),len(mechanisms['v1'][k])) for k in sorted(set(mechanisms['v2'])|set(mechanisms['v1']),key=lambda k:(-len(mechanisms['v2'][k]),k))])
lines+=['','### Script-slot, callback and saved-state partitions','',
'Raw slots are partitioned before compilation into decode_failed / bypassed / inline_nonempty / linked_only / empty. Only actual record/parameter errors count as decode_failed; saved-table uncertainty retains decoded source disposition independently. Wire slot, owner and program index are distinct from compact runtime admission. Active slots skip bypassed/empty slots. V1 compile-admitted allows disabled non-init callback blocks; compile-clean requires zero `Program.errors`. Init and persistence_changed completion/faults are independently observed, never inferred from public `Ok`. `absent`, `compile_disabled`, `entered`, `completed`, `faulted`, `budget_stopped`, `waiting`, `deferred` and `dropped` remain distinct. Diagnostics contain a fixed safe category, static builtin and numeric location only.','']
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
 lines+=['',f'{v} saved-table integrity (raw slot observations): '+', '.join(f'{k}={n}' for k,n in sorted(integrity.items()))+'. Only complete raw histograms enter the counts below; these are known-subset counts, and all-slot raw totals remain unknown when any histogram is incomplete.']
lines+=['']+table(['Build','Fixed sigil','Raw complete-table entries','Admitted entries'],saved_rows)
lines+=['','Saved sigils use only `$ ~ % ? @ ! empty other`; malformed table framing is distinguished from params() returning an empty Vec. Raw and admitted counts do not prove declaration-aware restoration. The manifest contains no NKSN snapshots; the separate prior 1,103-snapshot audit is not this denominator.','',
'## Conflux mandatory first witness','']
for v in ['v2','v1']:
 r=next((r for r in rows[v] if '/Conflux 1.1.0 ' in r['path'] and r['path'].endswith('/Conflux.nki')),None)
 if not r:lines+=['Current '+v+' witness pending.',''];continue
 lines+=[f"**{v}: loads {r['loads']}; Original {r['ui']}; bindings {r['controls_bound']}; audition {r['plays_note']}; note {r.get('note_picked')}; load {r['load_ms']} ms; first sound {r.get('first_audio_ms','unknown')} ms; first authored frame {r.get('ui_first_frame_ms','unknown')} ms; product cache {r.get('cache_state','unknown')}; peak RSS {r['peak_rss_mb']} MB.**",'']
 if v=='v2':
  for view in views(r)[:1]:
   paints=view.get('renders',[]);bg=paints[0].get('background',{}) if paints else {}
   lines+=[f"Main authored view: {view.get('widgets')} widgets, {view.get('visible')} visible, {view.get('bound')}/{view.get('interactive')} scalar bindings; {view.get('assets')} legacy asset declarations / {view.get('missing_images')} observed missing resources; actual lookup {view.get('asset_lookup_ok')}/{view.get('asset_lookup_requested')}, decode {view.get('asset_decode_ok')}/{view.get('asset_decode_requested')}. Native consumer attempted {view.get('native_frontend_consumed')}; Native paint OK {view.get('native_paint_ok','unknown')}; decoded package fonts {view.get('font_success','unknown')}. Declared background {bg.get('background_rgba')}; plain fraction {bg.get('plain_background_fraction',0):.4%}. This fraction measures pixels matching the retained legacy declared background; a Native package can cover that colour completely. It does not establish the effective Native page background. The renderer auditor’s historical 94.94% figure belongs to its earlier capture/layout.",'']
lines+=['This checkpoint includes the integration owners’ script, widget, Native frontend and load fixes. V2 Conflux was measured again against the frozen current binary; the unchanged v1 baseline reuses its earlier signature-matched per-item witness. Their timings are not a simultaneous benchmark. Historical claims that Conflux had no Native consumer or that its page was predominantly cream do not describe this new v2 checkpoint. Scalar and typed readback remain separate, and passive paint cannot certify pointer gestures, typography or automation. W0’s separate matched Conflux gesture test reports 124 witnesses passing; full corpus gesture coverage remains unknown.','',
'## Ranked systemic fixes','',
'Reach is an overlap-aware union of measured mechanism candidates or active source-token users, never a sum of occurrences. “Fully unlocked” is unmeasured for every fix until matched native render, gesture, callback and state tests pass. Ranking covers remaining observed failures and explicitly named qualification gaps. Already integrated Original-default and Conflux frontend repairs are historical context, not fresh failures. Candidate counts do not establish defects or the number fully unlocked. Effort S = localized existing-path repair, M = several adapters plus tests, L = shared typed service/lifecycle work. V1 is the first semantic reference; keep v2 ownership/real-time boundaries.','']
def scalar_or_typed_gap(r):
 return any(isinstance(v.get('bound'),int) and isinstance(v.get('interactive'),int) and isinstance(v.get('bound_typed'),int) and v['bound']+v['bound_typed']<v['interactive'] for v in views(r))
def script_failure(r):
 return any(pr.get('ksp_runtime_faults') or any(slot.get('compile_fault') or slot.get('disabled_block_errors') or any(slot.get(phase,{}).get('status') in ['faulted','budget_stopped','compile_disabled'] for phase in ['init','persistence_changed']) for slot in pr.get('ksp',{}).get('slots',[])) for pr in programs(r))
def lua_failure(r):
 return any(any((pr.get('lua') or {}).get(phase+'_faults',0) for phase in ['init','runtime']) for pr in programs(r))
def legacy_geometry(r):
 return any(v.get('source_presentation')!='native-package' and (v.get('geometry') or v.get('image_strips',0)) for v in views(r))
def font_or_background(r):
 return any((isinstance(v.get('font_declared'),int) and isinstance(v.get('font_success'),int) and v['font_declared']>v['font_success']) or any(x.get('background',{}).get('background_rgba') is not None and x.get('background',{}).get('plain_background_fraction',0)>.9 for x in v.get('renders',[])) for v in views(r))
fixes=[
('Remaining authored resource resolution','M/L','observed lookup/decode failures',lambda r:any(v.get('missing_images',0) for v in views(r)),
 'Repair existing source-family lookup/decoder paths for the remaining requested resources; retain successful Native package routing.',
 'Same item/page/value/DPI resolves every requested asset and produces an improved matched Original render.'),
('Playable-range and same-note sound qualification','M','admitted unselected or silent audition',lambda r:r.get('loads')=='yes' and r.get('plays_note') in ['no','silent'],
 'Resolve unselected programs using authored key/state contracts; diagnose equal-note audible/silent differences with per-node signal-graph traces before assigning a DSP or streaming repair. Unselected sound remains unmeasured.',
 'Declared-valid keys intersect non-purged/non-bypassed velocity64 coverage; both versions use the same recorded key and keyswitch. Trace the differing stages; no invented per-library notes.'),
('KSP callback/compiler gaps remaining after integration','M','observed phase/runtime failures',script_failure,
 'Repair the measured static builtin/category at exact slot ownership; preserve actual init and persistence phase boundaries.',
 'A failing-first fixture completes the repaired callback, then the same item loses its measured fault or compiler rejection.'),
('Lua initialization, runtime and budget faults','M','observed Lua faults',lua_failure,
 'Close the measured scripted-worker API/budget gap using the existing host; do not replace scripts with the offline loader.',
 'Same authored scene/callback completes, with init/runtime fault counts separate and budget stops measured.'),
('Original paint and frontend completion','M/L','actual UI failures',lambda r:r.get('ui') in ['error','blank','budget-hit'] or any(pr.get('native_frontend_consumed') is False for pr in programs(r)),
 'Repair actual frontend/paint failures or bound incremental scene work; the successful Conflux Native path is already integrated.',
 'Large and failure-prone scenes paint without budget/error placeholders; request, consumer entry and authored paint success are independently observed.'),
('Remaining scalar/typed/service binding qualification','M/L','known scalar plus typed target gaps',scalar_or_typed_gap,
 'Inspect only targets lacking both observed scalar and typed readback; separately qualify service-backed widgets and current callbacks.',
 'Each actual widget edit reaches its owning target, reads back exact type/value and runs the correct callback. Passive paint alone cannot prove this.'),
('Legacy geometry, parenting and strip fidelity','M','legacy candidates, not Native tree flags',legacy_geometry,
 'Confirm retained bridge candidates against authored layout before changing the shared geometry/frame path; hidden/outside widgets may be intentional.',
 'Nested panels, axes, HiDPI, visibility and frame endpoints match at identical state. Native scene layout requires its own exposed inventory.'),
('Remaining font and background fidelity','M','observed font failures / declared-colour candidates',font_or_background,
 'Repair actual unresolved fonts or proven contrast/style mismatches; Native package font inventory is separate from graph font usage.',
 'Real authored fonts/styles and text/background regions match. A colour coverage candidate alone does not prove unreadability.'),
('Saved-state and snapshot qualification','M/L','active persistence users',lambda r:mentioned(r,['make_persistent','make_instr_persistent','read_persistent_var','set_snapshot_type']),
 'Qualify complete fixed-sigil restoration, menu semantic values, string-array empties and snapshot policies on the integrated path; unknown v1 raw histograms remain unknown.',
 'State save/reopen and all four snapshot policies retain exact values before persistence_changed. The1494 manifest contains no standalone snapshots.'),
('Corpus interaction and automation qualification','M','continuous widget users',lambda r:any(any(v.get('kinds',{}).get(k,0) for k in ['ui_knob','ui_slider','ui_value_edit']) for v in views(r)),
 'Extend the existing real gesture/host gate beyond Conflux; current passive census does not establish drag, wheel, reset or automation failure.',
 'Actual pointer/key/wheel/reset/host gestures work on each widget family, with exact typed readback and callback ownership across waits.')]
lines+=table(['Rank','Mechanism','Effort','Measured v2 candidate items','Fully unlocked','Root repair / v1 reference','Proof'],[(i+1,name,effort,len(hits('v2',pred)),'Unknown',repair,proof) for i,(name,effort,label,pred,repair,proof) in enumerate(fixes)])
lines+=['','## Exhaustive spec incidence and status matrix','',
'The generated scanner whitelist is the union of the repository’s UI widgets, CONTROL_PAR identifiers, UI helpers/callbacks and persistence helpers. Comments and strings are skipped; one leading underscore alias is canonicalized. The whitelist is generated from compiler UI/keyboard/persistence builtin tables plus CONTROL_PAR/spec inventory; its digest/count is attached to each metadata record. Active incidence includes inactive preprocessor branches/unreachable functions and is not an execution count. Bypassed incidence is separate. Linked unresolved/native-package declarations can be absent from lexical counters; actual renderer widget inventory supplements them. Zero means unobserved in the surfaced inline declarations, not proof of complete corpus absence; decode failures, linked/native packages and generated UI can hide lexical use even after the manifest is complete.','',
'The expected contracts and historical 7e82 inspected status/source references below reuse the params auditor’s exhaustive matrix at `origin/audit/ui-params-20261008`. Its F1–F10 definitions and target tests are in [UI_PARAMS.md](https://github.com/DerpcatMusic/KONTRA/blob/f3242f451ce245a12ebe9fc99a64402572f2a8cd/docs/audit-2026-10-08/UI_PARAMS.md); rendering and gesture status is completed by [UI_RENDER.md](https://github.com/DerpcatMusic/KONTRA/blob/3c185d4dface744617d9407823032a1eb471bff8/docs/audit-2026-10-08/UI_RENDER.md), [UI_WIDGETS.md](https://github.com/DerpcatMusic/KONTRA/blob/c7781195f02910500577032741c061768eadf781/docs/audit-2026-10-08/UI_WIDGETS.md) and [UI_LOOP.md](https://github.com/DerpcatMusic/KONTRA/blob/982100c90d92ab31e127a542495d7ff45d86a3dd/docs/audit-2026-10-08/UI_LOOP.md). Historical statuses are not current 9993 failures: this checkpoint includes subsequent implementation fixes. Current execution/paint counts are shown separately; unmeasured current semantic support remains unknown. Correct in the historical matrix means inspected/tested mechanism, not complete vendor fidelity.','']
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
lines+=table(['Spec token','Expected contract','Historical 7e82 inspection / evidence / fix reference','v2 active items','v1 active items','v2 bypassed items','v2 token occurrences'],[(n,*(matrix.get(n,constant_contract(n))),len(incidence['v2'][n]),len(incidence['v1'][n]),len(bypassed['v2'][n]),occ['v2'][n]) for n in names])
lines+=['','### Actual authored widget inventory','',
'Counts sum all observed retained bridge views/programs; Native scene-tree kinds are not exposed by this inventory. They are declarations retained in UI models, including hidden widgets, not unique visible controls or source-token occurrences. Different v1/v2 projection and shell layouts prevent treating count differences as native parity.','']
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
'- Original uses the actual Native consumer when a Native package is present. Consumer admission and paint success are separate. Legacy widget/geometry/strip counts and placeholder/property flags describe the retained bridge model, not Native scene-tree layout. A bridge placeholder flag does not mean the consumed Native frontend lacks that widget. Native font file decode counts, when exposed, do not certify actual graph typography.\n- Readability/native pixel parity: background fractions are candidates; compare authored fixtures and matched native screenshots at identical page, value, state and DPI. Uniform hidden views are not a blank visible main page.',
'- Bound controls: this probe checks scalar readback. Gesture, typed cell/text/file edit and source callback/automation parity need actual pointer/host traces from widgets/loop scopes.',
'- UVI native-key metadata records authored declarations separately from sample audibility. Sidecar uses conservative native snapshots; absent or conflicting declarations remain unknown.',
'- Shared auditions: v2 establishes per-program key/velocity. Declared white keys intersect retained sample-zone coverage at velocity64; then zone coverage near middle C; then fallback. Avoid invalid/control/keyswitch declarations. Source is native_declared / zone_coverage / fallback, never density mislabeled native-valid. One half-second audible note is not full sample/DSP correctness; silent is inconclusive for articulation/CC/noise/control regions. No audio regression claim without identical per-ID notes. The corrected scanner sends a safe explicit fallback when surviving load-time zone coverage is empty, so note callbacks still run; a fully invalid keyboard yields no safe audition and plays_note=no, not a false silent-note claim.',
'- Instrumentation coverage: unknown is distinct from zero. V1 UVI sidecar does not expose every requested Lua/asset/budget field; those remain unknown. Its shared-note-plan override label is kept as adapter_pick_source, while pick_source uses the matching current v2 witness’s common-plan origin; unavailable origin stays unknown. Asset success counts are observed requests, not every archive member. Native font metrics remain unknown where the frozen binary does not expose its package font inventory; legacy font-service zeros cannot establish a Native font failure. V1 whole-editor pixels cannot establish authored-page background coverage. V2 paints standalone authored pages, not the whole plugin shell; a whole-editor tree budget failure on another UI branch must remain a separate measurement.',
'- Cache reproducibility: input resources must remain immutable; container mtime/size does not detect a loose-resource replacement. Use a fresh output after resource changes. Cold/warm load wall time and peak RSS are single runs, not matched performance benchmarks.',
'- Scope/format limits: multi/bank execution uses admitted production programs; unresolved linked scripts/native packages need resource traversal. Snapshot files/recovery saves are excluded from the frozen1494-ID list. Fully unlocked and corrected native-host parity remain unmeasured.',
'- Sound correctness: selected sample family, round-robin order, filter/effect retention and native internal DSP parity are UNKNOWN. Retained zone counts and audible mixed output do not identify the samples or certify the processing graph.\n- Reuse before rebuilding: v1 live UI semantics, prior Kontakt/native-resource and UVI typed-UI branches, and the existing strict typed persistence decoder are references. Audit tools do not merge those product fixes. Source line spans belong to the frozen baseline; future integration must regenerate counts with a newly built shared scanner.',
'','## Requested extension fields and privacy boundary','',
'[Symbol aggregates](/home/derpcat/.cache/kontra-scan/results/v2/symbol-aggregates.tsv) include attempted/parsed/lexical coverage, scanner digest, NKI/NKM and program-owner incidence, initialized widget kinds and fixed saved sigils. [V1 Original OK / v2 missing or error](/home/derpcat/.cache/kontra-scan/v1ok-v2missing.tsv) is separate from the load-admission regression list.\n\nFirst nine stable fields: `path library loads ui controls_bound plays_note load_ms peak_rss_mb reason`. All columns below are exported by the ONE shared CLI; raw detailed metrics preserve independent slot/phase ownership. CONTROL_PAR refs are the `$CONTROL_PAR_*` subset of `ui_api_refs`, not a separate duplicated column.','',
'`'+'`, `'.join(s.COLUMNS[9:])+'`','',
'Detailed JSON retains Lua init/runtime safe first diagnostic categories/digests, actual paint and budget status, widget kinds, lookup/decode/font success/failure categories, frames/strips/margins, declared/observed background RGBA and pixel fraction, load path, sample residency/underruns, runtime behavior outcomes, wire/runtime slots, compile admission/cleanliness, and independent init/persistence phase outcomes. Never serialize authored fault messages, identifiers, saved values, source text or resource bytes.','']
assert all(sum(r.get('slots_'+k,0) for k in ['decode_failed','bypassed','inline_nonempty','linked_only','empty'])==r['slots_seen'] for v in records for r in rows[v] if isinstance(r.get('slots_seen'),int))
assert sum(p.stat().st_size for p in gallery.glob('*.png'))<50*1024*1024
out.write_text('\n'.join(lines)+'\n')
print(f'{out}: v1={len(rows["v1"])} v2={len(rows["v2"])} complete={complete}; spec rows={len(names)}')
