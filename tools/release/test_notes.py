import unittest
from unittest.mock import patch
from notes import fields, pull, render, section
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

    def test_a_note_is_the_pull_requests_sentence_credited_to_its_author(self):
        def pr(number, body, paths, title='chore: internal title', author='ada'):
            return dict(number=number, body=body, paths=paths, title=title, author=author)
        entries = [pr('1234', 'Release note: Find your sounds faster.\nArea: libraries', ['tools/a.py']),
                   pr('1235', 'Release note: See the knobs clearly.\n', ['plugin/src/editor.rs'], author=''),
                   pr('1236', 'Release note: none\nArea: sound', ['crates/engine/engine/src/lib.rs'],
                      title='refactor(engine): one queue (#1236)'),
                   pr('1238', 'Release note: Hidden work.', ['tools/inspect/src/main.rs'])]
        result = render(entries)
        self.assertIn('### Libraries & loading\n\n- Find your sounds faster. '
                      '([#1234](https://github.com/DerpcatMusic/KONTRA/pull/1234) by @ada)', result)
        self.assertIn('- See the knobs clearly. ([#1235](https://github.com/DerpcatMusic/KONTRA/pull/1235))', result)
        self.assertIn('<summary>Under the hood: 2 changes</summary>', result)
        under = result.split('<details>', 1)[1].split('</details>', 1)[0]
        self.assertIn('- refactor(engine): one queue ([#1236](https://github.com/DerpcatMusic/KONTRA/pull/1236) by @ada)', under)
        self.assertNotIn('#1234', under)
        self.assertNotIn('Hidden work', result)

    def test_a_release_opens_with_its_count_and_ends_with_the_compare_link(self):
        entry = dict(number='42', body='Release note: Louder.\nArea: sound', paths=[], title='feat: louder', author='ada')
        result = render([entry], '0.5.0', 'v0.4.4')
        self.assertTrue(result.startswith('1 change since [0.4.4](https://github.com/DerpcatMusic/KONTRA/releases/tag/v0.4.4), 1 of them'))
        self.assertIn('### Install', result)
        self.assertIn('**Full changelog:** [v0.4.4...v0.5.0](https://github.com/DerpcatMusic/KONTRA/compare/v0.4.4...v0.5.0)', result)
        self.assertNotIn('<details>', result)
        self.assertEqual(render([]), '')

    def test_only_published_release_at_head_skips_build(self):
        pages = [[{'tag_name': 'v3.old', 'draft': False}],
                 [{'tag_name': 'v3.new', 'draft': False}, {'tag_name': 'v3.draft', 'draft': True}]]
        self.assertTrue(already_released({'v3.new'}, pages))
        self.assertFalse(already_released({'v3.draft'}, pages))
        self.assertFalse(already_released({'unrelated'}, pages))

    @patch('notes.subprocess.check_output',
           return_value='{"title":"fix: smoother","body":"Release note: Smoother playback.","user":{"login":"ada"}}')
    def test_fetches_the_pull_request(self, run):
        self.assertEqual(pull('42'), dict(number='42', title='fix: smoother',
                                          body='Release note: Smoother playback.', author='ada'))
        run.assert_called_once_with(['gh', 'api', 'repos/DerpcatMusic/KONTRA/pulls/42'], text=True)


if __name__ == '__main__':
    unittest.main()
