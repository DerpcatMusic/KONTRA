"""User-facing nightly notes from accepted ledger deltas and reviewed changelog text."""
import base64
import json
from pathlib import Path
import re
import subprocess


def blocks(text, heading):
    unreleased = next((p for p in re.split(r'(?m)^## ', text)[1:]
                       if 'unreleased' in p.partition('\n')[0].lower()), '')
    for part in re.split(r'(?m)^### ', unreleased)[1:]:
        name, _, body = part.partition('\n')
        if name.strip() == heading:
            return [' '.join(b.split()) for b in re.findall(r'(?ms)^- (.*?)(?=^- |^### |\Z)',body)]
    return []


def plain(text):
    text = ' '.join(text.split()).strip().lstrip('- ')
    text = re.sub(r'`([^`]*)`',r'\1',text)
    text = re.sub(r'\b[0-9a-f]{7,64}\b', '', text)
    text = re.sub(r'\b\d+\s+(?:tests?|checks?|passed|ignored)\b', '', text, flags=re.I)
    text = ' '.join(text.split()).strip(' ;,')
    return text.rstrip('.') + '.' if text else ''


def category(fix):
    name = fix.get('category')
    if name in ('New','Fixed','Improved'): return name
    verb = fix['summary'].split()[0].lower()
    if verb in ('add','introduce','supply'):return 'New'
    if verb in ('reduce','reuse','share','avoid','compact','skip','optimize'):return 'Improved'
    return 'Fixed'


def render(repo, revision, version, changes, issues, previous, checksums=(), reviewed=None):
    groups = {name:[] for name in ('New','Fixed','Improved')}
    for fix in changes:
        note = plain(fix.get('release_note',fix['summary']))
        assert note, 'Empty release note'
        groups[category(fix)].append(note)
    lines = [f'KONTRA **{version}**', '',
             f'This nightly includes {len(changes)} changes since the previous published release.' if previous else 'This is the first published snapshot of the changes below.',
             'It is an experimental build; check the known issues before updating.', '']
    if reviewed is not None:
        lines = [f'KONTRA **{version}**', '', reviewed, '']
    else:
        for name, notes in groups.items():
            if notes: lines += [f'### {name}', '', *('- '+note for note in notes), '']
        if issues:lines += ['### Known issues','',*('- '+plain(issue) for issue in issues[:5]),'']
    lines += ['### Install','',f'[Installation and system requirements](https://github.com/{repo}/blob/v{version}/README.md).',
              'Linux and Windows: extract the ZIP, read README.txt, then run the included install script. Use the uninstall script to remove the installed files.',
              'macOS: run the universal .pkg installer.', '', '<details>', '<summary>Source range and SHA-256 checksums</summary>', '']
    if previous:
        base=previous['target_commitish']
        lines += [f'Commit range: `{base}...{revision}`.', f'[Compare source changes](https://github.com/{repo}/compare/{base}...{revision}).']
    else: lines += [f'Source commit: `{revision}`.']
    lines += [f'Source tag: [`v{version}`](https://github.com/{repo}/tree/v{version}).', f'<!-- kontra-source-tag: v{version} -->','',
              'release-manifest.json records source identity, per-format build information and download checksums.','']
    if checksums:lines += ['```text', *(f'{asset["sha256"]}  {asset["name"]}' for asset in checksums), '```','']
    lines += ['</details>','']
    return '\n'.join(lines)


def old_file(api, name, revision):
    try:
        content=api('contents/'+name+'?ref='+revision)
        return base64.b64decode(content['content']).decode()
    except subprocess.CalledProcessError as error:
        if b'HTTP 404' not in (error.stderr or b''):raise
        return ''


def generate(api, repo, revision, version, previous, changelog=None, checksums=()):
    current = Path('CHANGELOG.md').read_text() if changelog is None else changelog
    ledger = Path('release-fixes.json')
    old_text = old_file(api,'release-fixes.json',previous['target_commitish']) if previous else ''
    if ledger.exists():
        fixes=json.loads(ledger.read_text())['fixes']
        old={f['id'] for f in json.loads(old_text)['fixes'] if f['accepted']} if old_text else set()
        changes=[f for f in fixes if f['accepted'] and f['id'] not in old]
    else:
        before=old_file(api,'CHANGELOG.md',previous['target_commitish']) if previous else ''
        changes=[]
        for heading, name in [('Added','New'),('Fixed','Fixed'),('Changed','Improved')]:
            old=set(blocks(before,heading))
            changes += [dict(summary=b,category=name) for b in blocks(current,heading) if b not in old]
        assert changes,'Missing ledger and reviewed changelog changes'
    issues=blocks(current,'Known issues')
    if not issues:issues=blocks(current,'Known limits')
    # A reviewed version section takes precedence only for that release base.
    base = version.split('-', 1)[0]
    reviewed = next((part.partition('\n')[2].strip()
                     for part in re.split(r'(?m)^## ', current)[1:]
                     if part.partition('\n')[0].strip() == base), None)
    return render(repo,revision,version,changes,issues,previous,checksums,reviewed)
