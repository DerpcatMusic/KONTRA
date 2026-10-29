#!/usr/bin/env python3
"""Reads the playability rows (plugin/src/playtest.rs, KONTRA_PLAYTEST_OUT) and prints one
line per program, then the gaps ranked by how many programs they hit.

    tools/v3-survey/playability.py ROWS.jsonl

Run the scenario first (release, nice 19, one job):
    KONTRA_PLAYTEST_OUT=rows.jsonl nice -n 19 build-slot cargo test --release -p kontra-plugin \
        --lib playtest -- --ignored --nocapture
"""
import json
import re
import sys
from collections import defaultdict

rows = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
print(f"{'program':58} {'works':5} {'load':>5} {'hold':>6} {'fails':>5} {'ui':>9} {'clicks':>6} {'diag B/A':>8}")
for r in rows:
    ui, d = r['ui'], r['diagnostics']
    print(f"{r['name'][:58]:58} {'yes' if r['works'] else 'NO':5} {r['load_s']:5.1f} {r['hold_ms']:6.1f} "
          f"{len(r['failed']):5} {ui['reached']:>4}/{ui['touched']:<4} {len(ui['clicks']):6} {d['broken']:>3}/{d['audible']:<4}")

# A failure's kind: its text with the segment, numbers and names taken out.
def kind(failure):
    text = failure.split(': ', 1)[1] if ': ' in failure else failure
    if ': ' in text and not text.startswith(('control', 'controls')):
        text = text.split(': ', 1)[1]
    text = re.sub(r'controls click when moved: .*', 'controls click when moved', text)
    text = re.sub(r'control .*?: clips', 'a control clips', text)
    return re.sub(r'[-+]?\d+(\.\d+)?', 'N', text)

gaps = defaultdict(lambda: [set(), []])
for r in rows:
    for f in r['failed']:
        g = gaps[('play', kind(f))]
        g[0].add(r['name'])
        g[1].append(f)
    for t in r['diagnostics']['top']:
        impact, feature = t.split(' ', 1)
        g = gaps[(impact.lower(), re.sub(r' ×\d+$', '', feature))]
        g[0].add(r['name'])
        g[1].append(f"{r['name']}: {t}")
print('\n## gaps, by programs hit (play = the scenario failed; broken/audible = load diagnostics)')
for (what, text), (names, examples) in sorted(gaps.items(), key=lambda kv: (-len(kv[1][0]), kv[0])):
    print(f"{len(names):3}  {what:7} {text}")
    for e in examples[:3]:
        print(f"       {e[:160]}")
quiet = sum(len(r['ui']['quiet']) for r in rows)
print(f"\n{quiet} controls moved with no audible change (menus for the next note, or dead): see each row's ui.quiet")
