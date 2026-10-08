"""Keep plugin payloads in tmpfs; persist numeric evidence and text hashes only."""
import hashlib
import json
import shutil
from pathlib import Path
import tempfile

AUDIT_STAGES = set('preset_cache_lookup file_read_ni_container decrypt_expand ni_chunk_parse ni_objects_parse translate_resolve_ir translate_ksp_init translate_resource_ir_dsp translate_zones_sample_resolve translate_keys_validate sample_source_resolve sample_headers_latency_probe sample_preload runtime_alloc_init ksp_frontend ksp_on_init ksp_callback_lower uvi_read_translate uvi_lua_init'.split())
FIELDS = set('level subsystem event phase category kind sequence instance_id count counters data elapsed_ms timestamp status error position stage subsystem_id nodes id from to node_id edges blocks peak rms dc enabled params level_db rms_db peak_db gain_db gain latency latency_frames bypass bypassed signal_graph_trace sample_rate'.split())

def digest(value):
    return hashlib.sha256(value if isinstance(value, bytes) else value.encode()).hexdigest()

def redact(value):
    if isinstance(value, dict):
        return {key if key in FIELDS else 'field-' + digest(key): redact(item) for key, item in value.items()}
    if isinstance(value, list):
        return [redact(item) for item in value]
    if isinstance(value, str):
        return {'text_sha256': digest(value)}
    return value

class Capture:
    def __init__(self, work, env):
        self.work = work
        self.tmp = tempfile.TemporaryDirectory(prefix='kontra-gate-', dir='/dev/shm')
        self.root = Path(self.tmp.name)
        self.stderr = (self.root / 'stderr').open('w+b')
        for variable, leaf in [('KONTRA_REPORT_DIR', 'reports'), ('KONTRA_LOG_DIR', 'logs')]:
            folder = self.root / leaf; folder.mkdir()
            env[variable] = str(folder)
        env['KONTRA_DISABLE_NETWORK'] = '1'
        cache = env.get('KONTRA_GATE_ITEM_CACHE')
        env['XDG_CACHE_HOME'] = cache or '/proc/self/kontra-gate-no-cache'
        self.cache = Path(cache) if cache else None
        self.before = self.cache_stats()
        env['XDG_DATA_HOME'] = str(self.root / 'data')

    def cache_stats(self):
        files = [p for p in self.cache.rglob('*') if p.is_file()] if self.cache else []
        return {'files': len(files), 'bytes': sum(p.stat().st_size for p in files)}

    def finish(self):
        self.stderr.flush(); self.stderr.close()
        records = []
        stages = []
        for line in (self.root / 'stderr').read_bytes().splitlines():
            if not line.startswith(b'AUDIT '): continue
            try:
                stage = json.loads(line[6:])
            except (ValueError, UnicodeError): continue
            if stage.get('stage') in AUDIT_STAGES and isinstance(stage.get('ms'), (int, float)):
                stages.append({k:v for k,v in stage.items() if k=='stage' or k in ['ms','rss_kb','hwm_kb'] and isinstance(v,(int,float))})
        (self.work / 'load-stages.json').write_text(json.dumps({'stages':stages,'basis':'numeric-only explicit audit spans; enclosing spans overlap'}) + '\n')
        traces=[]
        for trace in sorted((self.root/'reports').rglob('signal-trace.json')):
            relative=trace.relative_to(self.root/'reports')
            if any(not p.isdigit() for p in relative.parts[:-1]): continue
            try:
                value=json.loads(trace.read_text())
                valid=(value.get('schema')==1 and isinstance(value.get('graph',{}).get('nodes'),list)
                       and isinstance(value.get('records'),list) and value.get('complete') is True and value.get('dropped')==0)
            except (ValueError,OSError,AttributeError): valid=False; value={}
            destination=self.work/relative; destination.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(trace,destination)
            chart=trace.with_suffix('.svg')
            if chart.is_file(): shutil.copyfile(chart,destination.with_suffix('.svg'))
            traces.append({'path':str(relative),'status':'VALID' if valid else 'UNKNOWN', 'schema':value.get('schema'), 'complete':value.get('complete'), 'dropped':value.get('dropped')})
        (self.work/'signal-traces.json').write_text(json.dumps({'traces':traces})+'\n')
        for path in sorted(self.root.rglob('*')):
            if not path.is_file():
                continue
            raw = path.read_bytes()
            record = {'file_sha256': digest(raw), 'bytes': len(raw), 'lines': []}
            if path.name in ['signal-trace.json','signal-trace.svg']:
                record['line_count']=len(raw.splitlines()); records.append(record); continue
            for line in raw.splitlines():
                try:
                    record['lines'].append(redact(json.loads(line)))
                except (ValueError, UnicodeError):
                    record['lines'].append({'line_sha256': digest(line)})
            record['line_count'] = len(record['lines'])
            records.append(record)
        (self.work / 'plugin-diagnostics.json').write_text(json.dumps({'files': records, 'capture': 'tmpfs-to-hashed-metadata', 'plugin_host_run': False, 'product_cache_before': self.before, 'product_cache_after': self.cache_stats()}) + '\n')
        self.tmp.cleanup()
