"""Exercise the editor setup retry without root or package installation."""
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]


class EditorInstall(unittest.TestCase):
    def test_bounded_setup_retries_and_propagates_failure(self):
        workflow = (ROOT / '.github/workflows/plugin-ui.yml').read_text()
        step = workflow.split('      - name: Install virtual display and software graphics\n', 1)[1].split('      - uses:', 1)[0]
        self.assertIn('timeout-minutes: 7', step)
        command = textwrap.dedent(step.split('        run: |\n', 1)[1])
        self.assertIn('sudo timeout --kill-after=10s 3m', command)
        for failure, success, expected in [('none', True, 1), ('once', True, 2), ('always', False, 2), ('timeout', True, 2)]:
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                scripts = {
                    'sudo': '#!/bin/bash\nexec "$@"\n',
                    'sleep': '#!/bin/bash\nexit 0\n',
                    'dpkg': '#!/bin/bash\nexit 0\n',
                    'timeout': '''#!/bin/bash
                        echo attempt >> "$RETRY_LOG"
                        count=$(wc -l < "$RETRY_LOG")
                        [[ "$1" == --kill-after=10s && "$2" == 3m ]] || exit 99
                        shift 2
                        [[ "$FAILURE" != timeout || "$count" != 1 ]] || exit 124
                        exec "$@"
                    ''',
                    'apt-get': '''#!/bin/bash
                        [[ " $* " != *" install "* ]] && exit 0
                        count=$(wc -l < "$RETRY_LOG")
                        [[ "$FAILURE" != always && ( "$FAILURE" != once || "$count" != 1 ) ]]
                    ''',
                }
                for name, script in scripts.items():
                    path = root / name
                    path.write_text(textwrap.dedent(script))
                    path.chmod(0o755)
                log = root / 'attempts'
                env = dict(os.environ, PATH=str(root) + ':' + os.environ['PATH'], FAILURE=failure, RETRY_LOG=str(log))
                result = subprocess.run(['bash', '-e', '-c', command], env=env, capture_output=True, timeout=5)
                self.assertEqual(result.returncode == 0, success, result.stderr)
                self.assertEqual(len(log.read_text().splitlines()), expected)


if __name__ == '__main__':
    unittest.main()
