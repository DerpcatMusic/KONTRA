import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import stage_nightly


class StageTests(unittest.TestCase):
    def test_linux_install_and_uninstall(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            root = Path(directory).resolve()
            bundles = root / 'bundles'
            (bundles / 'KONTRA.vst3/Contents').mkdir(parents=True)
            (bundles / 'KONTRA.vst3/Contents/plugin').write_text('vst')
            (bundles / 'KONTRA.clap').write_text('clap')
            stage = root / 'stage'
            stage_nightly.stage('linux-x86_64', stage, bundles)
            home = root / 'home'
            home.mkdir()
            env = dict(os.environ, HOME=str(home))
            subprocess.run(['bash', str(stage / 'install.sh')], env=env, check=True)
            self.assertEqual((home / '.clap/KONTRA.clap').read_text(), 'clap')
            self.assertEqual((home / '.vst3/KONTRA.vst3/Contents/plugin').read_text(), 'vst')
            subprocess.run(['bash', str(stage / 'uninstall.sh')], env=env, check=True)
            self.assertFalse((home / '.clap/KONTRA.clap').exists())
            self.assertFalse((home / '.vst3/KONTRA.vst3').exists())

    def test_windows_stage_contains_instructions(self):
        with tempfile.TemporaryDirectory(dir='.') as directory:
            root = Path(directory)
            (root / 'bundles/KONTRA.vst3').mkdir(parents=True)
            (root / 'bundles/KONTRA.clap').write_text('clap')
            stage_nightly.stage('windows-x86_64', root / 'stage', root / 'bundles')
            self.assertTrue((root / 'stage/install.ps1').is_file())
            self.assertTrue((root / 'stage/uninstall.ps1').is_file())
            self.assertIn('install.ps1', (root / 'stage/README.txt').read_text())


if __name__ == '__main__':
    unittest.main()
