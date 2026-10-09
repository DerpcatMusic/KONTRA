"""Fixed RSS host viewport must agree with its mapped parent, child and CLAP API."""
import json
import sys
from pathlib import Path
from editor_rss import summarize
receipt=json.loads(Path(sys.argv[1]).read_text())
summarize(receipt['samples'])
for row in receipt['samples']:
    if row['phase'] in ('open','reopened'):
        assert (row['width'],row['height']) == (1180,760), 'mapped editor escaped fixed RSS viewport'
        assert (row['parent_width'],row['parent_height']) == (1180,760), 'mapped parent escaped fixed RSS viewport'
        assert (row['clap_width'],row['clap_height']) == (1180,760), 'CLAP size disagrees with fixed RSS viewport'
print('PASS: fixed requested/accepted/CLAP/parent/child viewport')
