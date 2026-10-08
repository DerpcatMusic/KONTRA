"""Metadata-only audit aggregation; no library payloads or samples are persisted."""
import collections, hashlib, json, math, pathlib
out = pathlib.Path(__file__).parent / 'data'
out.mkdir(exist_ok=True)
manifest = pathlib.Path('/home/derpcat/.cache/kontakto-corpus/items.tsv').read_text().splitlines()
for family, cache in [('kontakt', '/home/derpcat/.cache/kontakto-gpt-decipher-dsp/saved-items'), ('uvi', '/home/derpcat/.cache/kontakto-audit-dsp/uvi')]:
    totals = collections.Counter(); files = collections.Counter(); kinds = collections.Counter()
    cached=list(pathlib.Path(cache).glob('*.tsv'))
    assert len(cached)==(834 if family=='kontakt' else 660), (family,len(cached))
    for p in cached:
        kind = manifest[int(p.stem)].split('\t')[0]
        seen = set()
        body=p.read_text()
        if family=='uvi':assert body.startswith('census2\n') and 'files\tparsed\t1' in body and 'files\tfailed\t1' not in body
        for line in body.splitlines():
            parts = line.split('\t')
            if len(parts) != 3: continue
            category, name, count = parts
            key = (category, name)
            totals[key] += int(count); seen.add(key)
        for key in seen:
            files[key] += 1; kinds[key + (kind,)] += 1
    with (out / f'dsp-{family}-usage.tsv').open('w') as f:
        f.write('category\tmodule\tinstances\tcontainers\tnki\tnkm\tuvi_program\n')
        for k in sorted(totals):
            if family == 'uvi' and k[0] not in {'effect_saved','effect_enabled','modulator_saved','modulator_enabled','effect_scope_enabled','external_enabled','route_enabled','files'}: continue
            if family == 'kontakt' and k[0] not in {'files','programs','slot_enabled','slot_bypassed','internal_enabled','internal_disabled','external_saved'}: continue
            f.write('\t'.join(map(str, [*k, totals[k], files[k], kinds[k+('kontakt',)], kinds[k+('kontakt-multi',)], kinds[k+('uvi-program',)]]))+'\n')
vec = json.loads(pathlib.Path('/home/derpcat/.t3/worktrees/KONTAKTO/gpt-decipher-dsp/docs/architecture-v2/KONTAKT_DSP_LAWS.vectors.json').read_text())['ahdsr_lifecycle']
refs = {(v['curve'], tuple(v['lengths']), v['release_tick']): v['samples'] for v in vec}
rows = []
for line in pathlib.Path('/home/derpcat/.cache/kontakto-audit-dsp/envelopes.tsv').read_text().splitlines():
    c, ns, r, samples = line.split('\t'); x = list(map(float,samples.split(','))); ns = tuple(map(int,ns.split(',')))
    y = refs[(float(c),ns,int(r))]; assert len(x)==len(y)==70
    e = [a-b for a,b in zip(x,y)]
    gated = [abs(20*math.log10(a/b)) for a,b in zip(x,y) if a>0.001 and b>0.001]
    rows.append({'curve':float(c),'lengths':ns,'release_tick':int(r),'max_abs':max(map(abs,e)), 'rms_error':math.sqrt(sum(v*v for v in e)/len(e)), 'max_level_error_db_above_minus60':max(gated,default=0)})
assert len(rows)==45
assert max(r['max_abs'] for r in rows)<1e-6
(out/'dsp-envelope-comparison.json').write_text(json.dumps(rows,indent=2)+'\n')
print('envelope max_abs',max(r['max_abs'] for r in rows),'rms',max(r['rms_error'] for r in rows),'max_db',max(r['max_level_error_db_above_minus60'] for r in rows))

provenance_path=out/'dsp-provenance.json'
provenance=json.loads(provenance_path.read_text()) if provenance_path.exists() else {}
provenance.update({'v2_baseline':'7e82b152','v1_reference':'0cb7a8a0','kontakt_census_branch':'v2/gpt-decipher-dsp@aa3430d6','manifest_sha256':hashlib.sha256(pathlib.Path('/home/derpcat/.cache/kontakto-corpus/items.tsv').read_bytes()).hexdigest(),'manifest_items':len(manifest),'kontakt_containers':834,'uvi_programs':660,'native_ahdsr_cases':45})
provenance_path.write_text(json.dumps(provenance,indent=2)+'\n')
