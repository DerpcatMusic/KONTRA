"""Metadata-only check: private bank members and bounded FIFO shards."""
import contextlib, io, json, runpy, sys, tempfile
from pathlib import Path
from unittest.mock import patch

with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp);engine=root/'engine';engine.write_bytes(b'fixture')
    manifest=root/'items.tsv';manifest.write_text('/Libraries/UVI/A.ufs::PRIVATE_ONE\n/Libraries/UVI/A.ufs::PRIVATE_TWO\n')
    output=root/'receipts'
    argv=['slot-census.py','--engine',str(engine),'--list',str(manifest),'--out',str(output),'--count','50']
    result=type('Result',(),dict(returncode=0,stdout=json.dumps(dict(programs=[]))))()
    with patch.object(sys,'argv',argv), patch('subprocess.run',return_value=result), patch('time.monotonic',side_effect=[0,0,201]), contextlib.redirect_stdout(io.StringIO()):
        runpy.run_path(str(Path(__file__).with_name('slot-census.py')),run_name='__main__')
    receipts=list(output.glob('*.json'))
    assert len(receipts)==1, 'a shard must yield its FIFO slot after 200 seconds'
    text=receipts[0].read_text();receipt=json.loads(text)
    assert 'PRIVATE_' not in text and receipt['path']=='/Libraries/UVI/A.ufs'
    assert type(receipt['diagnostic_item_index']) is int
print('native census privacy and shard checks PASS')
