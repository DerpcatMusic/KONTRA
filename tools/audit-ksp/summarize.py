#!/usr/bin/env python3
"""Aggregate per-path exposure; script repetitions never inflate instrument counts."""
import collections, json, pathlib, statistics, sys
cache=pathlib.Path.home()/'.cache/kontakto-audit-ksp'
master={s.split('\t')[0] for s in (pathlib.Path.home()/'.cache/kontakto-gpt-format-gaps/census/files.tsv').read_text().splitlines()[1:]}
records={x['path']:x for p in sorted(cache.glob('shard-*.jsonl')) for s in p.read_text().splitlines() for x in [json.loads(s)]}
report={}
for label,items in [('all',list(records.values())),('master',[x for p,x in records.items() if p in master]),('nki',[x for p,x in records.items() if p.endswith('.nki')])]:
    counts=collections.Counter(); exposure=collections.Counter(); hashes=set(); kinds=collections.Counter(); widget_files=collections.Counter(); tags=collections.Counter(); warnings=collections.Counter(); unsupported=collections.Counter(); coverage=collections.Counter(); times=collections.defaultdict(list); gaps={}
    for x in items:
        counts['paths']+=1; counts['read_ok']+=x['read']; ss=x['scripts']; counts['script_paths']+=bool(ss)
        counts['all_compile_paths']+=bool(ss) and all(s.get('compile') for s in ss)
        counts['all_init_paths']+=bool(ss) and all(s.get('init_frontend') for s in ss)
        counts['all_v1_compile_paths']+=bool(ss) and all(s['v1']['compile'] and s['v1']['block_errors']==0 for s in ss)
        counts['all_v1_init_paths']+=bool(ss) and all(s['v1']['init'] for s in ss)
        seen=set();seen_widgets=set();seen_warn=set();seen_unsupported=set();seen_coverage=set(); seen_tags=set()
        for s in ss:
            counts['scripts']+=1;hashes.add(s['hash']); counts['v2_compile']+=s.get('compile',False);counts['v2_init']+=s.get('init_frontend',False);counts['v2_bind']+=s.get('bind',False);counts['ui']+=s.get('ui',False)
            counts['v1_compile']+=s['v1']['compile'];counts['v1_clean_compile']+=s['v1']['compile'] and s['v1']['block_errors']==0; counts['v1_init']+=s['v1']['init'];counts['v1_init_no_diagnostics']+=s['v1']['init'] and s['v1']['runtime_diagnostics']==0
            counts['nonidentity_menus']+=s.get('nonidentity_menus',0)
            seen.update(s['tokens']); seen_widgets.update(s.get('widgets',{})); kinds.update(s.get('widgets',{}));seen_warn.update(s.get('warnings',{}));seen_unsupported.update(s.get('ui_unsupported',[]));seen_coverage.update(f'{n}:{c}' for n,c,_ in s.get('coverage',[]));seen_tags.update(s['saved_tags']);tags.update(s['saved_tags'])
            for k,v in [('v2_init_us',s['init_frontend_us']),('v2_compile_us',s['compile_us']),('v1_load_us',s['v1']['us'])]:times[k].append(v)
            if not s.get('compile'):gaps[x['path']]=s['failure']
        exposure.update(seen);widget_files.update(seen_widgets);warnings.update(seen_warn);unsupported.update(seen_unsupported);coverage.update(seen_coverage)
        counts['saved_string_array_paths']+=('!' in seen_tags)
    report[label]={'counts':dict(counts),'unique_scripts':len(hashes),'exposure':dict(exposure.most_common()),'widget_instances':dict(kinds),'widget_paths':dict(widget_files),'saved_entries':dict(tags),'warnings_paths':dict(warnings.most_common()),'ui_unsupported_paths':dict(unsupported.most_common()),'coverage_paths':dict(coverage.most_common()),'failures':gaps,'timing_us':{k:{'median':statistics.median(v),'p95':sorted(v)[int(.95*(len(v)-1))],'max':max(v),'sum':sum(v)} for k,v in times.items() if v}}
print(json.dumps(report,indent=2,sort_keys=True))
