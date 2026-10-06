"""Extract interface identifiers and section coverage, not manual prose/examples.

Offline input is a pinned manual cache: PAGE.html plus pages.json SHA-256 records.
The known chapter set and representative symbols are checked before writing.
"""
import argparse
import hashlib
import json
import re
from html.parser import HTMLParser
from pathlib import Path

BASE = 'https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/'

arguments = argparse.ArgumentParser(description=__doc__)
arguments.add_argument('cache', type=Path, help='cached manual HTML and pages.json with SHA-256 values')
arguments.add_argument('output', type=Path, help='destination for the interface inventory')
args = arguments.parse_args()
CACHE = args.cache

SERVICES = {
    "callbacks": "event_processing",
    "variables": "script_state",
    "arithmetic-commands---operators": "language",
    "control-statements": "language",
    "user-defined-functions": "language",
    "array-commands": "script_state",
    "general-commands": "event_processing",
    "event-commands": "musical_ownership",
    "time-related-commands": "scheduler_transport",
    "built-in-variables-and-constants": "script_context",
    "group-commands": "mapping_selection",
    "keyboard-commands": "keyboard_state_ui",
    "zone-commands": "mapping_assets",
    "zone-parameters": "mapping_assets",
    "engine-parameter-commands": "engine_parameters",
    "engine-parameters": "dsp_graph",
    "user-interface-widgets": "script_ui",
    "user-interface-commands": "script_ui",
    "control-parameters": "script_ui",
    "load-save-commands": "async_assets_persistence",
    "midi-object-commands": "midi_objects",
    "multi-script": "event_processing",
    "music-information-retrieval": "async_analysis",
    "resource-container": "assets_packaging",
    "advanced-concepts": "cross_service_semantics"
}

class Inventory(HTMLParser):

    def __init__(self, page):
        super().__init__()
        self.page = page
        self.main = False
        self.sections = []
        self.stack = []
        self.heading = None
        self.heading_text = []
        self.code = None
        self.cell = None
        self.col = 0
        self.current = None
        self.title = ''
        self.pre = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'article' and attrs.get('id') == 'content-wrapper':
            self.main = True
        if not self.main:
            return
        if tag == 'section':
            self.stack.append(attrs.get('id', ''))
        if re.fullmatch('h[1-6]', tag) and attrs.get('class') == 'title':
            self.heading = tag
            self.heading_text = []
        if tag == 'tr':
            self.col = 0
        if tag in ('td', 'th'):
            self.col += 1
            self.cell = tag
        if tag == 'pre':
            self.pre += 1
        if tag == 'code' and (not self.pre) and self.cell and (self.cell == 'th' or self.col == 1):
            self.code = []

    def handle_data(self, text):
        if not self.main:
            return
        if self.heading:
            self.heading_text.append(text)
        if self.code is not None:
            self.code.append(text)

    def handle_endtag(self, tag):
        if not self.main:
            return
        if tag == self.heading:
            name = ' '.join(''.join(self.heading_text).replace('\xad', '').split())
            if tag == 'h1':
                self.title = name
            else:
                anchor = self.stack[-1] if self.stack else ''
                self.current = {'anchor': anchor, 'title': name, 'symbols': []}
                self.sections.append(self.current)
                if name.startswith('on '):
                    self.add('callback', name)
                elif re.match('ui_[a-z_]+$', name):
                    self.add('widget', name)
                else:
                    for command in re.findall(r'\b([a-z][a-z0-9_]*)\s*\(', name):
                        self.add('command', command)
                    if name in (
                        'exit', 'ignore_controller', 'ignore_midi', 'continue',
                        'reset_ksp_timer', 'expose_controls', 'make_perfview',
                    ):
                        self.add('command', name)
            self.heading = None
        if tag == 'code' and self.code is not None:
            text = ''.join(self.code).replace('\xad', '')
            for symbol in re.findall('[$%~?@!][A-Z][A-Z0-9_]*', text):
                self.add('symbol', symbol)
            if self.page in ('arithmetic-commands---operators', 'control-statements'):
                for command in re.findall(r'\b([a-z][a-z0-9_]*)\s*\(', text):
                    self.add('command', command)
            self.code = None
        if tag in ('th', 'td'):
            self.cell = None
        if tag == 'pre':
            self.pre -= 1
        if tag == 'section':
            self.stack.pop()
        if tag == 'article':
            self.main = False

    def add(self, kind, name):
        if self.current is None:
            return
        entry = {'kind': kind, 'name': name}
        if entry not in self.current['symbols']:
            self.current['symbols'].append(entry)

chapters = []
sources = json.loads((CACHE / 'pages.json').read_text())
expected_pages = set(SERVICES) | {
    'welcome-to-ksp', 'version-history', 'additional-resources'
}
assert {s['page'] for s in sources} == expected_pages, (
    'Manual chapter surface changed; review the coverage map.'
)

navigation = (CACHE / 'welcome-to-ksp.html').read_text().split(
    '<article class="topic content-container"', 1
)[0]
assert set(re.findall(r'New in Kontakt ([0-9.]+)', navigation)) == {'8.12'}, (
    'Manual version changed; review the target profile before extracting.'
)

for source in sources:
    page = source['page']
    if page not in SERVICES:
        continue
    raw = (CACHE / (page + '.html')).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == source['sha256']
    parser = Inventory(page)
    parser.feed(raw.decode())
    assert parser.title and (not parser.main), 'Missing or incomplete main article: ' + page
    if not parser.sections:
        parser.sections.append({'anchor': '', 'title': parser.title, 'symbols': []})
    for section in parser.sections:
        section['source'] = BASE + page + ('#' + section['anchor'] if section['anchor'] else '')
        if (page, section['title']) in [
            ('callbacks', 'on note'), ('callbacks', 'on release'), ('callbacks', 'on init'),
            ('arithmetic-commands---operators', 'Basic Operators'),
            ('arithmetic-commands---operators', 'Integer Number Commands'),
            ('arithmetic-commands---operators', 'Bitwise Operators'),
            ('variables', '$ (integer variable)'),
            ('variables', 'polyphonic $ (polyphonic integer)'),
            ('general-commands', 'play_note()'), ('general-commands', 'exit'),
            ('control-statements', 'Boolean Operators'),
            ('control-statements', 'if ... else ... end if'),
            ('control-statements', 'continue'), ('control-statements', 'while ()'),
            ('time-related-commands', 'wait()'), ('event-commands', 'ignore_event()'),
            ('event-commands', 'change_note()'), ('event-commands', 'change_velo()'),
        ]:
            section['implementation'] = 'partial_native_subset'
            section['evidence'] = 'KSP_FRONTEND.md'
        grouped = {}
        for symbol in section['symbols']:
            grouped.setdefault(symbol['kind'], []).append(symbol['name'])
        section['symbols'] = grouped
    chapters.append({
        'id': page, 'title': parser.title, 'service': SERVICES[page],
        'source': BASE + page, 'sha256': source['sha256'], 'sections': parser.sections,
    })

assert len(chapters) == len(SERVICES)

for chapter in chapters:
    anchors = {s['anchor'] for s in chapter['sections']}
    assert len(anchors) == len(chapter['sections']), 'Duplicate section: ' + chapter['id']

symbols = {
    name for chapter in chapters for section in chapter['sections']
    for names in section['symbols'].values() for name in names
}

for name in [
    'on controller', 'on note', 'play_note', 'wait', '$ENGINE_PAR_TUNE',
    '$CONTROL_PAR_POS_X', '$ZONE_PAR_HIGH_KEY', '$EVENT_ID',
]:
    assert name in symbols, name

result = {
    'target': 'Kontakt 8.12 KSP',
    'retrieved_on': '2026-10-06',
    'scope': ('All 25 functional chapters in the observed manual navigation; '
              'historical/resources/intro pages excluded from implementation counts.'),
    'limitations': [
        'Section and declaration index, not an exhaustive enumeration of every operand, '
        'parameter value, legal callback context or historical alias.',
        'Symbols are extracted from headings and definition-table cells; source review '
        'is required before implementing each section.',
        'No licensed Kontakt execution comparison has been performed. Native substrate '
        'is not KSP compatibility.',
    ],
    'default_status': {'implementation': 'missing', 'kontakt_fidelity': 'unverified'},
    'symbol_overrides': {
        name: {'implementation': 'partial_native_subset', 'evidence': 'KSP_FRONTEND.md'}
        for name in ['on note', 'on release', 'on init', 'play_note', 'wait', 'ignore_event', 'change_note', 'change_velo', '$EVENT_ID', '$EVENT_NOTE', '$EVENT_VELOCITY', '$NOTE_HELD', 'exit', 'if', 'while', 'continue', 'ui_knob', 'ui_slider', 'ui_button', 'ui_switch', 'make_perfview', 'on ui_control', 'abs', 'sgn', 'signbit', 'inc', 'dec']
    },
    'chapters': chapters,
}

args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')

print('chapters', len(chapters), 'sections', sum((len(c['sections']) for c in chapters)), 'unique identifiers', len(symbols))

for c in chapters:
    print(c['id'], len(c['sections']), sum((sum(map(len, s['symbols'].values())) for s in c['sections'])))
