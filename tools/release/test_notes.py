import unittest
from unittest.mock import patch
from notes import fields, pull_body, render, section
from already_released import already_released


class NotesTest(unittest.TestCase):
    def test_fields_and_explicit_areas(self):
        self.assertEqual(fields('Title\nRelease note: Play more naturally.\nArea: sound\n'),
                         ('Play more naturally.', 'Sound & playing'))
        self.assertEqual(fields('release note: none\narea: interface'), ('', 'Interface'))
        self.assertEqual(fields('no release note here'), ('', None))
        self.assertEqual(fields('Release note:  \nArea: fix'), ('', 'Fixes'))
        for area, expected in [('sound', 'Sound & playing'), ('interface', 'Interface'),
                               ('libraries', 'Libraries & loading'), ('performance', 'Performance'),
                               ('fix', 'Fixes')]:
            self.assertEqual(fields(f'Release note: Music.\nArea: {area}')[1], expected)

    def test_paths(self):
        cases = [('tools/inspect/src/main.rs', None), ('.github/workflows/x.yml', None),
                 ('docs/plan.md', None), ('tools/xtask/src/main.rs', None),
                 ('plugin/src/playtest.rs', None), ('crates/engine/engine/tests/a.rs', None),
                 ('crates/app/ui/src/lib.rs', 'Interface'), ('plugin/src/editor.rs', 'Interface'),
                 ('crates/format/kontakt/src/lib.rs', 'Libraries & loading'),
                 ('crates/engine/stream/src/lib.rs', 'Libraries & loading'),
                 ('crates/app/library/src/lib.rs', 'Libraries & loading'),
                 ('crates/engine/engine/src/lib.rs', 'Sound & playing')]
        for path, expected in cases:
            with self.subTest(path=path):
                self.assertEqual(section([path]), expected)
        self.assertEqual(section(['tools/inspect/src/main.rs', 'crates/app/ui/src/lib.rs']), 'Interface')

    def test_render_uses_body_not_title(self):
        entries = [('Release note: Find your sounds faster.\nArea: libraries', '1234', ['tools/a.py']),
                   ('Release note: See the knobs clearly.\n', '1235', ['plugin/src/editor.rs']),
                   ('Release note: none\nArea: sound', '1236', ['crates/engine/engine/src/lib.rs']),
                   ('P8.4: internal technical title', '1237', ['crates/app/ui/src/lib.rs']),
                   ('Release note: Hidden work.', '1238', ['tools/inspect/src/main.rs'])]
        result = render(entries)
        self.assertIn('### Libraries & loading\n\n- Find your sounds faster. ([#1234](https://github.com/DerpcatMusic/KONTRA/pull/1234))', result)
        self.assertIn('### Interface\n\n- See the knobs clearly. ([#1235](https://github.com/DerpcatMusic/KONTRA/pull/1235))', result)
        self.assertIn('<summary>Internal: 3 changes</summary>', result)
        self.assertNotIn('P8.4', result)
        self.assertNotIn('Hidden work', result)

    def test_only_published_release_at_head_skips_build(self):
        pages = [[{'tag_name': 'v3.old', 'draft': False}],
                 [{'tag_name': 'v3.new', 'draft': False}, {'tag_name': 'v3.draft', 'draft': True}]]
        self.assertTrue(already_released({'v3.new'}, pages))
        self.assertFalse(already_released({'v3.draft'}, pages))
        self.assertFalse(already_released({'unrelated'}, pages))

    @patch('notes.subprocess.check_output', return_value='{"body":"Release note: Smoother playback."}')
    def test_fetches_pr_body(self, run):
        self.assertEqual(pull_body('42'), 'Release note: Smoother playback.')
        run.assert_called_once_with(['gh', 'api', 'repos/DerpcatMusic/KONTRA/pulls/42'], text=True)


if __name__ == '__main__':
    unittest.main()
