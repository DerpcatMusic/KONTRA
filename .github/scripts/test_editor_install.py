"""Exercise offline editor setup without root, network or package installation."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]


class EditorInstall(unittest.TestCase):
    def test_offline_setup_retries_and_propagates_failure(self):
        workflow = (ROOT / '.github/workflows/plugin-ui.yml').read_text()
        step = workflow.split('      - name: Install virtual display and software graphics\n', 1)[1].split('      - name:', 1)[0]
        self.assertIn('timeout-minutes: 7', step)
        command = textwrap.dedent(step.split('        run: |\n', 1)[1])
        self.assertNotIn('apt-get', command)
        self.assertIn('sha256sum --check', command)
        self.assertIn('sleep "$((5 * 2 ** (attempt - 1)))"', command)
        for failure, success, expected in [('preinstalled', True, 0), ('none', True, 1), ('once', True, 2), ('always', False, 5), ('timeout', True, 2), ('corrupt', False, 0)]:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                cache = root / 'plugin/editor-debs'
                cache.mkdir(parents=True)
                package = cache / 'fixture.deb'
                package.write_bytes(b'fixture')
                digest = hashlib.sha256(package.read_bytes()).hexdigest()
                (cache / 'SHA256SUMS').write_text(f'{digest}  fixture.deb\n')
                if failure == 'corrupt':
                    package.write_bytes(b'changed')
                scripts = {
                    'sudo': '#!/bin/bash\nexec "$@"\n',
                    'sleep': '#!/bin/bash\nexit 0\n',
                    'dpkg': '''#!/bin/bash
                        if [[ "$1" == -L ]]; then
                          [[ "$FAILURE" == preinstalled || -f "$INSTALLED" ]] || exit 1
                          echo /usr/share/vulkan/icd.d/lvp_icd.json
                        elif [[ "$1" == --unpack ]]; then
                          count=$(wc -l < "$RETRY_LOG")
                          [[ "$FAILURE" != always && ( "$FAILURE" != once || "$count" != 1 ) ]]
                        elif [[ "$1" == --configure ]]; then
                          touch "$INSTALLED"
                        else
                          exit 99
                        fi
                    ''',
                    'timeout': '''#!/bin/bash
                        echo attempt >> "$RETRY_LOG"
                        count=$(wc -l < "$RETRY_LOG")
                        [[ "$1" == --kill-after=10s && "$2" == 60s ]] || exit 99
                        shift 2
                        [[ "$FAILURE" != timeout || "$count" != 1 ]] || exit 124
                        exec "$@"
                    ''',
                    'apt-get': '#!/bin/bash\nexit 99\n',
                }
                for tool in ['xvfb-run', 'xauth', 'dbus-run-session', 'glxinfo', 'vulkaninfo', 'ffmpeg', 'unzip']:
                    scripts[tool] = '#!/bin/bash\nexit 0\n'
                for name, script in scripts.items():
                    path = root / name
                    path.write_text(textwrap.dedent(script))
                    path.chmod(0o755)
                log = root / 'attempts'
                env = dict(os.environ, PATH=str(root) + ':' + os.environ['PATH'], FAILURE=failure, RETRY_LOG=str(log), INSTALLED=str(root / 'installed'))
                result = subprocess.run(['bash', '-e', '-c', command], cwd=root, env=env, capture_output=True, timeout=5)
                self.assertEqual(result.returncode == 0, success, result.stderr)
                self.assertEqual(len(log.read_text().splitlines()) if log.exists() else 0, expected)


if __name__ == '__main__':
    unittest.main()
