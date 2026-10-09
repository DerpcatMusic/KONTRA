"""Synthetic presented-pixel regressions; never writes instrument images."""
import numpy as np
import json
import tempfile
from pathlib import Path
from presented import black_regions, chrome_copies, visibility_flicker, frame_times, presented_cell

image = np.full((240, 320, 3), 30, dtype=np.uint8)
image[50:210, 200:280] = 0
assert [200, 50, 80, 160] in black_regions(image)
assert not black_regions(np.full_like(image, 8))
# A varied chrome patch must be found both at its origin and displaced.
rng = np.random.default_rng(1)
patch = rng.integers(10, 220, (20, 80, 3), dtype=np.uint8)
image[:20, :80] = patch
image[80:100, 100:180] = patch
assert chrome_copies(image, (0, 0, 80, 20)) == [[0, 0, 80, 20], [100, 80, 80, 20]]
assert chrome_copies(np.full_like(image, 30), (0, 0, 80, 20)) == []
shown = np.full((64, 64, 3), 30, dtype=np.uint8)
shown[::2, ::2] = 220
hidden = np.full_like(shown, 30)
assert visibility_flicker([shown, hidden, shown, hidden, shown])
assert not visibility_flicker([shown] * 5)
assert frame_times([0, .016, .032, .132])['p99_ms'] == 100
assert presented_cell([], 'fixture', 'cold', 'a'*40)['status'] == 'UNKNOWN'
cell = {'item_sha256': 'fixture', 'condition': 'cold', 'source_sha': 'a'*40,
        'status': 'FAIL', 'duplicate_chrome': [{'bounds': [[0, 0, 80, 20], [100, 80, 80, 20]]}]}
assert presented_cell([cell], 'fixture', 'cold', 'a'*40)['status'] == 'FAIL'
assert presented_cell([cell], 'fixture', 'cold', 'b'*40)['status'] == 'UNKNOWN'
assert presented_cell([dict(cell, status='UNKNOWN', duplicate_chrome=[]), cell], 'fixture', 'cold', 'a'*40)['status'] == 'FAIL'
import adapters
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    root.joinpath('manifest.json').write_text(json.dumps({'sha': 'a'*40, 'conditions': ['cold', 'os-warm']}))
    root.joinpath('items.tsv').write_text('fixture\tfixture\n')
    folder = root / 'presented/fixture'; folder.mkdir(parents=True)
    bound = dict(cell, schema=1, item_sha256=adapters.hashlib.sha256(b'fixture').hexdigest())
    folder.joinpath('metrics.json').write_text(json.dumps(bound))
    result = adapters.presented(root, root)
    assert [r['status'] for r in result['cells']] == ['FAIL', 'UNKNOWN']
    folder.joinpath('metrics.json').write_text(json.dumps(dict(bound, source_sha='b'*40)))
    assert adapters.presented(root, root)['status'] == 'UNKNOWN'
print('PASS: black region, dark negative, duplicate chrome, flat negative, flicker, static negative, frame interval')
