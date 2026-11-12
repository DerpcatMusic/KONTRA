import unittest
from pr import problems

BODY = 'What and why.\n\nRelease note: Long preset menus scroll.\nArea: interface\n'


class PullRequestTest(unittest.TestCase):
    def test_a_plain_title_and_note_pass(self):
        self.assertEqual(problems('fix(ui): long preset menus scroll', BODY), [])
        self.assertEqual(problems('feat(kontakt)!: snapshots keep their effects', 'x\nRelease note: none\n'), [])

    def test_titles_that_only_their_author_understands_fail(self):
        for title in ('Lane B: zones by name', 'P8.17: playability', 'Script parity: Choral',
                      'fix(dsp): match FUN_14193ae70', 'fix(dsp): read the table at 0x14a5ee2b0',
                      'fix(ui): menus scroll.', 'fix: ' + 'a' * 80):
            with self.subTest(title=title):
                self.assertTrue(problems(title, BODY))

    def test_the_release_note_is_required_and_plain(self):
        self.assertTrue(problems('fix: a', 'no note'))
        self.assertTrue(problems('fix: a', 'Release note:\nArea: fix'))
        self.assertTrue(problems('fix: a', 'Release note: Louder.\n'))
        self.assertTrue(problems('fix: a', 'Release note: Fixed `Choir::set`.\nArea: sound'))
        self.assertTrue(problems('fix: a', 'Release note: Round 4 of lane B.\nArea: sound'))


if __name__ == '__main__':
    unittest.main()
