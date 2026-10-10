#!/usr/bin/env python3
"""Source custody and scalar models only. Never starts Cargo, a host or a reader."""
import hashlib
import json
import re
import subprocess
from pathlib import Path
from bs4 import BeautifulSoup

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
BASE = '66eaf072d12387012c3a17c13b4140310f59e985'
CODE = '275494599eb9b3f9758e7c63a99827ea1ef382b0'
EVIDENCE = Path('/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff')

def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])

def sha(data):
    return hashlib.sha256(data).hexdigest()

def source(path, revision=CODE):
    return git('show', f'{revision}:{path}')

def extent(data, pattern):
    text = data.decode()
    match = re.search(pattern, text, re.M)
    assert match, pattern
    start = text.rfind('\n', 0, match.start()) + 1
    opening = text.index('{', match.end())
    depth, end = 1, opening + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    # All selected bodies have balanced literal braces; rustfmt also parses them.
    first = text.count('\n', 0, start) + 1
    last = text.count('\n', 0, end) + 1
    lines = data.splitlines(keepends=True)
    body = b''.join(lines[first - 1:last])
    return dict(first=first, last=last, full_lines_sha256=sha(body),
                source_body_sha256=sha(body.lstrip(b' ').rstrip(b'\n')))

def function(data, name):
    if name == 'evaluate':
        # Pin the voice route evaluator, not Shape::evaluate or the free helper.
        return extent(data, r'^    fn evaluate\(\n        &mut self,')
    return extent(data, rf'^\s*(?:pub(?:\([^)]*\))?\s+)?fn {re.escape(name)}\b')

changed = [p.decode() for p in git('diff', '--name-only', BASE, CODE).splitlines()]
assert changed == ['crates/sampler-core/src/control.rs', 'crates/sampler-core/src/voice_mod.rs',
                   'crates/sampler-core/tests/controls.rs', 'crates/sampler-kontakt/tests/production_modulation.rs']
files = []
for path in changed:
    blob = source(path)
    assert (ROOT / path).read_bytes() == blob
    result = subprocess.run(['rustfmt', '--edition', '2024', '--config', 'skip_children=true',
                             '--emit', 'stdout', str(ROOT / path)], capture_output=True)
    assert result.returncode == 0, result.stderr.decode()
    files.append(dict(path=path, git_blob=git('rev-parse', f'{CODE}:{path}').decode().strip(),
                      bytes=len(blob), sha256=sha(blob), rustfmt_parse_exit=0))

symbols = {
 'crates/sampler-core/src/control.rs': ['with_controls', 'schema_replacement_rejects_invalid_compiled_input_index'],
 'crates/sampler-core/src/voice_mod.rs': ['remap_controls', 'new_resolved', 'evaluate', 'project_parameters', 'bytes_per_voice'],
 'crates/sampler-core/src/prepare.rs': ['with_voice_modulation', 'with_voice_chains'],
 'crates/sampler-core/src/engine_parameters.rs': ['with_group_envelope_parameters_batch', 'set_engine_parameter', 'engine_parameter', 'set_engine_parameter_in', 'engine_parameter_in'],
 'crates/sampler-ksp/src/lib.rs': ['bind_modules', 'derived_control_id'],
 'crates/sampler-kontakt/src/load.rs': ['prepare_inner'],
 'crates/sampler-core/src/dsp/control.rs': ['validate_dsp_controls', 'initial_parameters', 'edit_parameters'],
 'crates/sampler-core/src/script.rs': ['with_script_resources'],
 'crates/sampler-core/src/widget.rs': ['with_widgets'],
 'crates/sampler-core/src/parameter_registry.rs': ['with_parameter_registry'],
 'crates/sampler-kontakt/tests/production_modulation.rs': ['fixture_target_script', 'scripted_plan'],
}
spans = []
for path, names in symbols.items():
    blob = source(path)
    for name in names:
        spans.append(dict(path=path, symbol=name, source_sha=CODE,
                          file_sha256=sha(blob), git_blob=git('rev-parse', f'{CODE}:{path}').decode().strip(),
                          **function(blob, name)))
# Exact unchanged bind-after-modulation call order at the reviewed pin.
path = 'crates/sampler-core/src/lower.rs'
blob = source(path)
lines = blob.splitlines(keepends=True)
body = b''.join(lines[714:725])
spans.append(dict(path=path, symbol='lower_with modulation then bind_behaviors', source_sha=CODE,
                  first=715, last=725, file_sha256=sha(blob),
                  git_blob=git('rev-parse', f'{CODE}:{path}').decode().strip(),
                  full_lines_sha256=sha(body), source_body_sha256=sha(body.lstrip(b' ').rstrip(b'\n'))))

new_tests = {
 'crates/sampler-core/src/control.rs': [('schema_replacement_rejects_invalid_compiled_input_index', 'sampler-core', 'lib', 'Invalid old index rejects before publication')],
 'crates/sampler-core/tests/controls.rs': [
  ('modulation_inputs_follow_identity_after_reorder_append_and_real_default_replacement', 'sampler-core', 'controls', 'Repeated sorted replacement, inserted integer/toggle, actual typed depth/bypass PCM, unchanged sentinel/voice'),
  ('modulation_schema_rejects_removed_and_non_real_inputs_without_touching_active_plan', 'sampler-core', 'controls', 'Both sparse IDs removed/integer/toggle reject; existing audible active generation/revision survives'),
  ('schema_replacement_preserves_native_envelope_and_dsp_identity_consumers', 'sampler-core', 'controls', 'Real amplitude envelope lane reject/preserve, onset engine write and ControlGain plus pan consume stable IDs')],
 'crates/sampler-kontakt/tests/production_modulation.rs': [
  ('serialized_scripts_enabled_schema_shift_preserves_depth_bypass_pcm_and_voice_owners', 'sampler-kontakt', 'production_modulation', 'Public own NKS/WAV, KSP init/UI bind and sorted shift; amplitude/pan depth+bypass PCM, sibling, sentinel, overlap release'),
  ('serialized_scripts_enabled_live_writes_match_unbound_pcm_across_partitions', 'sampler-kontakt', 'production_modulation', 'Exact reviewer $Real/slot12 index5->6 trigger; positive saved PCM and typed writes vs same NKS scripts disabled, irregular/empty partitions'),
  ('serialized_scripts_enabled_schema_shift_keeps_pitch_step_and_sibling_pan', 'sampler-kontakt', 'production_modulation', 'Same public script-enabled fixture with own ramp WAV; six-semitone vs neutral cursor step, bypass+zero depth, sibling pan and voice continuity')],
}
tests = []
for path, entries in new_tests.items():
    blob = source(path)
    for name, package, target, witness in entries:
        tests.append(dict(name=name, package=package, target=target, witness=witness,
                          path=path, source_sha=CODE, status='NOT_RUN', native='UNKNOWN', **function(blob, name)))
        assert '#[test]' in blob.decode()[:blob.decode().index('fn ' + name)][-60:]

original_path = 'crates/sampler-kontakt/tests/production_modulation.rs'
old, current = source(original_path, BASE), source(original_path)
original_names = re.findall(rb'^fn (serialized_\w+)\(', old, re.M)
assert len(original_names) == 10
original_tests = []
for encoded in original_names:
    name = encoded.decode()
    a, b = function(old, name), function(current, name)
    assert a['full_lines_sha256'] == b['full_lines_sha256'], name
    original_tests.append(dict(name=name, unchanged_full_lines_sha256=b['full_lines_sha256'], status='NOT_RUN_ON_SUCCESSOR'))

layouts = []
path = 'crates/sampler-core/src/voice_mod.rs'
old, current = source(path, BASE), source(path)
for name in ['Program', 'VoiceModulation', 'ModShape', 'VoiceModState']:
    pattern = rf'^(?:pub(?:\([^)]*\))?\s+)?struct {name}\b'
    a, b = extent(old, pattern), extent(current, pattern)
    assert a['full_lines_sha256'] == b['full_lines_sha256'], name
    layouts.append(dict(symbol=name, unchanged_full_lines_sha256=b['full_lines_sha256']))
for name in ['evaluate', 'bytes_per_voice', 'project_parameters']:
    a, b = function(old, name), function(current, name)
    assert a['full_lines_sha256'] == b['full_lines_sha256'], name
    layouts.append(dict(symbol=name, unchanged_full_lines_sha256=b['full_lines_sha256']))
# Preserve Switch/UVI laws and every other Rust blob outside the exact fix.
assert source('crates/sampler-core/src/engine_parameters.rs', BASE) == source('crates/sampler-core/src/engine_parameters.rs')

callers = []
for line in git('grep', '-n', '-F', '.with_controls(', CODE, '--', '*.rs').decode().splitlines():
    _, path, number, text = line.split(':', 3)
    number = int(number)
    blob = source(path)
    lines = blob.splitlines(keepends=True)
    first, last = max(1, number - 8), min(len(lines), number + 10)
    body = b''.join(lines[first - 1:last])
    production = path in ['crates/sampler-core/src/lower.rs', 'crates/sampler-core/src/engine_parameters.rs', 'crates/sampler-ksp/src/lib.rs']
    callers.append(dict(path=path, line=number, call=text.strip(), source_sha=CODE,
                        category='production schema composition' if production else 'test/example fixture or this API regression',
                        first=first, last=last, full_lines_sha256=sha(body), file_sha256=sha(blob)))

manifest = json.loads((EVIDENCE / 'official-docs/manifest.json').read_text())
sections = json.loads((EVIDENCE / 'official-docs/sections.json').read_text())
ids = ['kontakt-modulation', 'ksp-engine', 'ksp-engine-commands', 'ksp-ui', 'ksp-controls']
references = []
for item in manifest:
    if item['id'] not in ids:
        continue
    path = EVIDENCE / 'official-docs' / (item['id'] + '.html')
    assert sha(path.read_bytes()) == item['sha256'], path
    references.append(dict(item, verified_archive=str(path), authority='requirements only, not native execution'))
assert len(references) == 5
required_sections = [s for s in sections if s.get('document_sha256') in {r['sha256'] for r in references}
                     and (s['section'] in ['AHDSR', 'LFO controls', 'Modulation', 'set_engine_par()', 'get_ui_id()', 'ui_slider', 'load_performance_view()']
                          or s['section'].startswith(('get_mod_idx', 'get_target_idx')))]
# The section index has no standalone get_mod/get_target rows. Extract the
# exact section from its rehashed immutable full HTML; do not infer the contract.
commands = next(r for r in references if r['id'] == 'ksp-engine-commands')
soup = BeautifulSoup(Path(commands['verified_archive']).read_bytes(), 'html.parser')
for name in ['get_mod_idx()', 'get_target_idx()']:
    heading = next(h for h in soup.find_all('h2') if h.get_text(strip=True) == name)
    section = heading.find_parent('section')
    required_sections.append(dict(section=name, url=commands['final_url'] + '#' + section['id'],
                                  document_sha256=commands['sha256'],
                                  excerpt=section.get_text(' ', strip=True),
                                  authority='Exact rehashed cached official HTML; requirements only'))
assert any(s['section'] == 'get_ui_id()' for s in required_sections)
assert any(s['section'] == 'set_engine_par()' for s in required_sections)
assert any(s['section'].startswith('get_mod_idx') for s in required_sections)
assert any(s['section'].startswith('get_target_idx') for s in required_sections)

# Independent scalar model of the OWN remap contract. Not Rust/native execution.
old = [(1000, 'real'), (3000, 'real')]
new = [(500, 'integer'), (1000, 'real'), (2000, 'toggle'), (3000, 'real')]
def remap(index, old, new):
    if index >= len(old):
        raise ValueError('invalid old index')
    identity = old[index][0]
    for index, (candidate, kind) in enumerate(new):
        if candidate == identity:
            if kind != 'real':
                raise ValueError('non-real')
            return index
    raise ValueError('missing')
assert [remap(i, old, new) for i in [0, 1]] == [1, 3]
for index, schema in [(9, new), (0, [(3000, 'real')]), (0, [(1000, 'integer'), (3000, 'real')])]:
    try:
        remap(index, old, schema)
    except ValueError:
        pass
    else:
        raise AssertionError('invalid schema accepted')
h = 0x6c62272e07bb014262b821756295c58d
for byte in b'ksp/0/$Real':
    h = ((h ^ byte) * 0x0000000001000000000000000000013b) % (1 << 128)
assert h == 0x6eee5e4ce8303263a90ab0ecbf6aea3d
for gain, pan, expected in [(1., .75, [.125, .5]), (.75, .5, [.1875, .375]), (.75, .25, [.28125, .375])]:
    assert [.5 * gain * (1 - pan), .5 * gain] == expected
subprocess.run(['git', '-C', str(ROOT), 'diff', '--check'], check=True)

result = dict(status='PASS_SOURCE_CUSTODY_AND_SCALAR_MODEL_ONLY', base=BASE, code_sha=CODE,
              code_tree=git('rev-parse', f'{CODE}^{{tree}}').decode().strip(), files=files, spans=spans,
              tests=tests, original_ten_tests=original_tests, unchanged_layout_and_audio_bodies=layouts,
              callers=callers, references=references, requirement_sections=required_sections,
              rea_receipt_sha256=sha((OUT / 'rea-current-document.json').read_bytes()),
              limits=['No Cargo/rustc/clippy/build/typecheck, Rust test, host, native process or comparative benchmark.',
                      'Rustfmt parses syntax only. Scalar expectations are predictions, not observed PCM.',
                      'Native parity UNKNOWN. Original blocked66eaf review is not successor approval.',
                      'Performance acceptance UNACHIEVED until comparable CPU AND RAM measurements against BOTH v1 and Kontakt.'])
(OUT / 'source-manifest.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(dict(status=result['status'], code_sha=CODE, files=len(files), spans=len(spans),
                      new_tests=len(tests), original_tests_unchanged=len(original_tests), callers=len(callers),
                      verified_archives=len(references), native='UNKNOWN', rust_tests='NOT_RUN'), indent=2))
