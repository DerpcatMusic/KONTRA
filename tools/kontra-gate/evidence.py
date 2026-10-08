"""Keep plugin payloads in tmpfs; persist numeric evidence and text hashes only."""
import hashlib
import json
from pathlib import Path
import tempfile

FIELDS = set('level subsystem event phase category kind sequence instance_id count counters data elapsed_ms timestamp status error position stage subsystem_id nodes node_id level_db rms_db peak_db gain_db gain latency latency_frames bypass bypassed signal_graph_trace sample_rate'.split())

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
        # No decoded preset/sample cache can be written by a gate worker.
        env['XDG_CACHE_HOME'] = '/proc/self/kontra-gate-no-cache'
        env['XDG_DATA_HOME'] = str(self.root / 'data')

    def finish(self):
        self.stderr.flush(); self.stderr.close()
        records = []
        for path in sorted(self.root.rglob('*')):
            if not path.is_file():
                continue
            raw = path.read_bytes()
            record = {'file_sha256': digest(raw), 'bytes': len(raw), 'lines': []}
            if any(word in path.name.lower() for word in ['signal-graph', 'signal_graph', 'engine-trace']):
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
        (self.work / 'plugin-diagnostics.json').write_text(json.dumps({'files': records, 'capture': 'tmpfs-to-hashed-metadata', 'plugin_host_run': False}) + '\n')
        self.tmp.cleanup()
