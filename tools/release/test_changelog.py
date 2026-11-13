import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('changelog', Path(__file__).resolve().parent / 'changelog.py')
changelog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(changelog)


class ChangelogTest(unittest.TestCase):
    def test_pulls_and_commits_by_month_newest_first_with_notes(self):
        pulls = [dict(number=1, title='fix: old (#1)', mergedAt='2026-10-02T00:00:00Z', body='Release note: none'),
                 dict(number=2, title='feat: new (#2)', mergedAt='2026-11-02T00:00:00Z', body='Release note: Knobs turn.')]
        text = changelog.render(pulls, [('a' * 40, '2026-11-01T00:00:00+00:00', 'direct commit')])
        self.assertLess(text.index('### 2026-11'), text.index('### 2026-10'))
        self.assertIn('- feat: new ([#2](https://github.com/DerpcatMusic/KONTRA/pull/2)): Knobs turn.', text)
        self.assertIn('- fix: old ([#1](https://github.com/DerpcatMusic/KONTRA/pull/1))\n', text)
        self.assertIn('- direct commit ([aaaaaaaaa]', text)


if __name__ == '__main__':
    unittest.main()
