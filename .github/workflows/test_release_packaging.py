"""Focused nightly retention, notes and staging regressions; no network/builds."""
import ast
import base64
import json
import hashlib
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import os
import unittest
from unittest.mock import patch
import release_notes
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
import stage_nightly

ROOT = Path(__file__).resolve().parents[2]

class PackagingTests(unittest.TestCase):
    def test_all_published_releases_survive(self):
        tree = ast.parse(Path(__file__).with_name('publish_nightly.py').read_text())
        function = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'finish')
        calls = []
        records = [dict(id=i, tag_name='nightly' if i==1 else f'v0.1.{i}', draft=False,
                        target_commitish='b'*40, published_at=f'2026-09-0{i}', body='',
                        assets=[dict(name=f'KONTRA-nightly-{p}.zip',state='uploaded',size=1,digest='sha256:'+'0'*64)
                                for p in ('linux-x86_64','windows-x86_64')]) for i in (1,2,3)]
        new = dict(id=4,tag_name='v0.3.306-nightly.20261008.gaaaaaaaaaaaa',draft=False,prerelease=False,target_commitish='a'*40)
        records.append(new)
        context = dict(re=re,verify=lambda *a:None,source_tag=lambda *a:None,
                       releases=lambda:records,gh=lambda *a:calls.append(a),delete_ref=lambda *a:calls.append(a),
                       api=lambda path,*a:dict(id=4,target_commitish='a'*40) if path=='releases/latest' else dict(status='identical'),
                       PUBLISHED=('linux-x86_64','windows-x86_64'),SHA='a'*40,managed_ref=lambda tag:True)
        exec(compile(ast.Module(body=[function],type_ignores=[]),'finish','exec'),context)
        context['finish'](new,dict(version='0.3.306-nightly.20261008.gaaaaaaaaaaaa',revision='a'*40),b'')
        self.assertFalse(calls,'Published releases and source tags must never be deleted')

    def test_previous_source_is_closest_ancestor_even_after_late_publication(self):
        tree=ast.parse(Path(__file__).with_name('publish_nightly.py').read_text())
        function=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='previous_snapshot')
        records=[dict(target_commitish=name,draft=False,published_at=date) for name,date in
                 [('old','2026-10-09'),('new','2026-10-08'),('future','2026-10-10')]]
        def api(path):
            base=path.split('/')[1].split('...')[0]
            return dict(status='behind' if base=='future' else 'ahead',total_commits=10 if base=='old' else 1)
        context=dict(api=api,SHA='current')
        exec(compile(ast.Module(body=[function],type_ignores=[]),'previous_snapshot','exec'),context)
        self.assertIs(context['previous_snapshot'](records),records[1])

    def test_patched_source_uses_identical_line_endings_on_windows(self):
        attrs=subprocess.check_output(['git','check-attr','text','eol','--','vendor/symphonia-format-riff/src/lib.rs'],cwd=ROOT,text=True)
        self.assertIn(': text: auto',attrs)
        self.assertIn(': eol: lf',attrs)

    def test_ledger_delta_drives_readable_notes(self):
        previous = dict(target_commitish='b'*40)
        before = dict(fixes=[dict(id='old',summary='Old change',accepted=True)])
        current = dict(fixes=before['fixes']+[
            dict(id='new',summary='Add folder browsing',release_note='Browse presets in folders.',category='New',accepted=True),
            dict(id='fix',summary='Fix stuck notes',release_note='Release held notes when playback stops.',category='Fixed',accepted=True),
            dict(id='speed',summary='Reduce startup work',release_note='Open libraries faster.',category='Improved',accepted=True),
            dict(id='held',summary='Future behavior',accepted=False)])
        changelog='## Unreleased\n### Fixed\n- Fallback changelog fix.\n### Known issues\n'+''.join(f'- Issue {i}.\n' for i in range(7))
        def api(path,*args):
            if path.startswith('contents/'):
                data=json.dumps(before) if 'release-fixes.json' in path else changelog
                return dict(content=base64.b64encode(data.encode()).decode())
            if path.startswith('compare/'):return [dict(status='ahead',total_commits=1,commits=[dict(sha='a'*40,commit=dict(message='audit wall'))])]
            if '/pulls?' in path:return [[]]
            return dict(sha='a'*40,commit=dict(message='audit wall'))
        with tempfile.TemporaryDirectory() as d, patch('pathlib.Path.cwd',return_value=Path(d)):
            folder=Path(d);folder.joinpath('release-fixes.json').write_text(json.dumps(current))
            with patch.object(release_notes,'Path',side_effect=lambda p:folder/p):
                body=release_notes.generate(api,'example/KONTRA','a'*40,'0.3.306-nightly.test',previous,changelog)
        for title in ('New','Fixed','Improved','Known issues','Install'):
            self.assertIn('### '+title,body)
        for text in ('Browse presets in folders.','Release held notes when playback stops.','Open libraries faster.'):
            self.assertIn('- '+text,body)
        self.assertNotIn('Old change',body);self.assertNotIn('Future behavior',body)
        self.assertNotIn('No reviewed changes',body);self.assertNotIn('audit wall',body)
        self.assertNotIn('Issue 5.',body);self.assertIn('<details>',body)
        self.assertNotIn('a'*40,body.split('<details>')[0])

    def test_publication_queue_keeps_pending_pushes(self):
        workflow=Path(__file__).with_name('nightly.yml').read_text()
        publish=workflow.split('  release:\n',1)[1]
        self.assertIn('queue: max',publish)
        self.assertIn('nightly-build-${{ matrix.name }}-${{ github.sha }}',workflow)
        ci=ROOT.joinpath('.github/workflows/ci.yml').read_text()
        self.assertEqual(ci.count('inputs.release_validation && github.sha || github.ref'),3)

    def test_fake_stages_preserve_licenses_identity_and_install_paths(self):
        with tempfile.TemporaryDirectory(prefix='kontra-lean-stage-') as d:
            root=Path(d);licenses=root/'license-bundle';licenses.mkdir()
            (root/'.github').symlink_to(ROOT/'.github',target_is_directory=True)
            for name in stage_nightly.BUNDLE_LEGAL:
                path=licenses/name;path.parent.mkdir(parents=True,exist_ok=True)
                path.write_text('Complete fixture license: '+name+'\n',encoding='utf-8')
            inventory={'symphonia-0.5.5.crate':b'original-source',
                       'symphonia-format-riff-0.5.5.crate':b'patched-source',
                       'option-ext-0.2.0.crate':b'original-option-source'}
            (licenses/'sources').mkdir()
            for name,data in inventory.items():(licenses/'sources'/name).write_bytes(data)
            (licenses/'THIRD_PARTY_NOTICES.txt').write_text('symphonia 0.5.5: MPL-2.0\nsymphonia-format-riff 0.5.5: MPL-2.0\noption-ext 0.2.0: MPL-2.0\nFull MPL license text.\n')
            for platform,target in stage_nightly.TARGETS.items():
                windows=platform.startswith('windows-');bundles=root/'target/bundles'/target;bundles.mkdir(parents=True)
                (bundles/'KONTRA.clap').write_bytes(b'CLAP fixture')
                path=bundles/('KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3' if windows else 'KONTRA.vst3/Contents/x86_64-linux/KONTRA.so')
                path.parent.mkdir(parents=True);path.write_bytes(b'VST3 fixture')
                binary=root/'target'/target/'release'/('kontakto-standalone.exe' if windows else 'kontakto-standalone')
                binary.parent.mkdir(parents=True)
                builds={fmt:dict(version='0.3.306-nightly.test',revision='a'*40,target=target,profile='release',
                                features=['plugin','library-access']+([fmt] if fmt!='standalone' else list(stage_nightly.FORMATS))) for fmt in stage_nightly.FORMATS}
                output=root/('KONTRA-nightly-'+platform)
                for fmt in ('clap','vst3'):(root/(fmt+'-build-info.json')).write_text(json.dumps(builds[fmt]))
                binary.write_text('#!/usr/bin/env bash\ncat <<\'BUILD\'\n'+json.dumps(builds['standalone'])+'\nBUILD\n');binary.chmod(0o755)
                workflow=ROOT.joinpath('.github/workflows/nightly.yml').read_text()
                command=textwrap.dedent(workflow.split('      - name: Stage\n',1)[1].split('        run: |\n',1)[1].split('      - name: Verify Linux glibc baseline',1)[0])
                command=command.replace('${{ matrix.name }}',platform).replace('${{ matrix.target }}',target)
                env=dict(os.environ,RUNNER_OS='Windows' if windows else 'Linux',GITHUB_SHA='a'*40,GITHUB_ENV=str(root/'env'))
                subprocess.run(['bash','-e','-o','pipefail','-c',command],cwd=root,env=env,check=True,capture_output=True)
                metadata=json.loads(output.with_suffix('.build.json').read_text())
                ext='ps1' if windows else 'sh';exe='kontakto-standalone.exe' if windows else 'kontakto-standalone'
                self.assertEqual({p.name for p in output.iterdir()}, {'KONTRA.clap','KONTRA.vst3',exe,'README.txt','LICENSES.txt','install.'+ext,'uninstall.'+ext})
                for name,text in [(name,ROOT.joinpath(name).read_text(encoding='utf-8')) for name in stage_nightly.LEGAL]:
                    self.assertIn('===== '+name+' =====\n'+text,(output/'LICENSES.txt').read_text(encoding='utf-8'))
                for name,digest in metadata['files'].items():self.assertEqual(hashlib.sha256((output/name).read_bytes()).hexdigest(),digest)
                self.assertEqual(metadata['builds'],builds)
                self.assertFalse((output/'SOURCE_COMMIT.txt').exists())
                if not windows:
                    import zipfile
                    with zipfile.ZipFile(root/stage_nightly.SOURCES_ASSET) as archive:
                        self.assertEqual({n:archive.read('sources/'+n) for n in inventory},inventory)
                    # Exercise the shipped shell script with only its home references
                    # redirected in a temporary copy; never reassign the real HOME.
                    destination=root/'fake-user'
                    env=dict(os.environ,KONTRA_TEST_INSTALL_ROOT=str(destination))
                    destination.joinpath('.vst3/KONTRA.vst3').mkdir(parents=True)
                    destination.joinpath('.vst3/KONTRA.vst3/obsolete').write_text('old plugin file')
                    keep=destination/'.vst3/other.vst3';keep.write_text('other plugin')
                    for script in ('install','uninstall'):
                        text=(output/(script+'.sh')).read_text().replace('$HOME','$KONTRA_TEST_INSTALL_ROOT')
                        check=output/(script+'-sandbox.sh');check.write_text(text)
                        subprocess.run(['bash','-n',str(check)],check=True)
                        if script=='install':
                            for _ in range(2):subprocess.run(['bash',str(check)],env=env,check=True,stdout=subprocess.DEVNULL)
                            self.assertFalse(destination.joinpath('.vst3/KONTRA.vst3/obsolete').exists())
                            self.assertEqual(destination.joinpath('.clap/KONTRA.clap').read_bytes(),b'CLAP fixture')
                            self.assertEqual(destination.joinpath('.local/bin/kontakto-standalone').read_bytes(),binary.read_bytes())
                            self.assertTrue(destination.joinpath('.local/bin/kontakto-standalone').stat().st_mode&0o111)
                            settings=destination/'.config/kontra/settings.json';settings.parent.mkdir(parents=True);settings.write_text('keep')
                        else:
                            for _ in range(2):subprocess.run(['bash',str(check)],env=env,check=True,stdout=subprocess.DEVNULL)
                            self.assertFalse(destination.joinpath('.clap/KONTRA.clap').exists())
                            self.assertFalse(destination.joinpath('.vst3/KONTRA.vst3').exists())
                            self.assertEqual(settings.read_text(),'keep')
                        check.unlink()
                    self.assertEqual(keep.read_text(),'other plugin')
                bad=dict(builds,clap=dict(builds['clap'],revision='b'*40))
                with self.assertRaises(AssertionError):stage_nightly.stage(platform,root/('bad-'+platform),bundles,binary,licenses,bad,'a'*40)
                self.assertFalse(root.joinpath('bad-'+platform).exists())
            (licenses/'sources/symphonia-format-riff-0.5.5.crate').unlink()
            with self.assertRaises(FileNotFoundError):stage_nightly.stage(platform,root/'missing-source',bundles,binary,licenses,builds,'a'*40)
            self.assertFalse(root.joinpath('missing-source').exists())

if __name__=='__main__':unittest.main()
