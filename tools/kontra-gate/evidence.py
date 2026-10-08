"""Keep plugin payloads in tmpfs; persist numeric evidence and text hashes only."""
import hashlib
import json
import shutil
from pathlib import Path
import tempfile
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from contention import Activity, QuietBusy

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
        self.activity = None
        if env.get('KONTRA_GATE_ACTIVITY') == '1':
            self.activity = Activity(work)
            try: self.activity.start()
            except QuietBusy:
                self.stderr.close(); self.tmp.cleanup()
                raise

    def cache_stats(self):
        files = [p for p in self.cache.rglob('*') if p.is_file()] if self.cache else []
        return {'files': len(files), 'bytes': sum(p.stat().st_size for p in files)}

    def finish(self):
        if self.activity: self.activity.finish()
        self.stderr.flush(); self.stderr.close()
        records = []
        trace = next(self.root.rglob('signal-trace.json'), None)
        trace_ok = False
        if trace:
            try:
                value = json.loads(trace.read_text())
                trace_ok = isinstance(value, dict) and isinstance(value.get('nodes'), list)
            except (ValueError, OSError):
                pass
        # W6's fixed prepared graph exporter guarantees no PCM/authored names/text.
        # Keep its exact JSON and companion SVG for per-item inspection.
        if trace_ok:
            shutil.copyfile(trace, self.work / 'signal-trace.json')
            chart = trace.with_suffix('.svg')
            if chart.is_file(): shutil.copyfile(chart, self.work / 'signal-trace.svg')
        for path in sorted(self.root.rglob('*')):
            if not path.is_file():
                continue
            raw = path.read_bytes()
            record = {'file_sha256': digest(raw), 'bytes': len(raw), 'lines': []}
            if any(word in path.name.lower() for word in ['signal-graph', 'signal_graph', 'engine-trace', 'signal-trace']):
                try:
                    record['signal_graph_trace'] = redact(json.loads(raw))
                except (ValueError, UnicodeError):
                    pass
            for line in raw.splitlines():
                try:
                    record['lines'].append(redact(json.loads(line)))
                except (ValueError, UnicodeError):
                    record['lines'].append({'line_sha256': digest(line)})
            record['line_count'] = len(record['lines'])
            records.append(record)
        (self.work / 'plugin-diagnostics.json').write_text(json.dumps({'files': records, 'capture': 'tmpfs-to-hashed-metadata', 'plugin_host_run': False, 'product_cache_before': self.before, 'product_cache_after': self.cache_stats()}) + '\n')
        self.tmp.cleanup()
