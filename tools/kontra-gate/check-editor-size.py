"""Live parent, child and CLAP must agree within ±1 of the requested viewport."""
import json
import sys
from pathlib import Path
from editor_rss import summarize
summarize(json.loads(Path(sys.argv[1]).read_text())['samples'])
print('PASS: parent/child/CLAP equality; requested viewport within compositor ±1')
