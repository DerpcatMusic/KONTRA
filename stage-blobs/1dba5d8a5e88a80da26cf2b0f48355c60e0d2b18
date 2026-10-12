"""Receipt checks without loading a plugin or library."""
import json
from pathlib import Path
import tempfile
from load_host import SOURCE, receipts

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    binary = tmp / 'artifact'; binary.write_bytes(b'fixture')
    build = dict(version='0.3.326', source_revision=SOURCE, profile='release', dirty=False)
    rows = [dict(format=f, artifact=str(binary), sha256='a'*64, build=build) for f in ['clap','vst3']]
    cli = dict(artifact=str(binary), sha256='b'*64, build=build)
    artifacts = tmp/'artifacts.json'; native = tmp/'cli.json'
    native.write_text(json.dumps(cli))
    artifacts.write_text(json.dumps(rows))
    assert receipts(artifacts,native)[1] == cli
    for bad in [dict(build, dirty=True), dict(build, source_revision='0'*40), dict(build, profile='ci'), dict(build, version='0.3.325')]:
        native.write_text(json.dumps(dict(cli,build=bad)))
        try:receipts(artifacts,native)
        except AssertionError:pass
        else:raise AssertionError('wrong artifact provenance accepted')
    native.write_text(json.dumps(dict(cli,sha256='x'*64)))
    try:receipts(artifacts,native)
    except AssertionError:pass
    else:raise AssertionError('invalid hash accepted')
print('PASS: exact release provenance, clean source, both plugin formats and hash schema')
