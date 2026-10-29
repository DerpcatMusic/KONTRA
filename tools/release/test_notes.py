import unittest
from notes import clean, render, section


class NotesTest(unittest.TestCase):
    def test_real_titles(self):
        cases = [
            ('P9.4: fit both library views inside the rack (#1167)', 'Fit both library views inside the rack'),
            ('P9.5: shade the Original stock knob ring (#1157)', 'Shade the Original stock knob ring'),
            ('P9.3: draw picture controls in Vector view (#1151)', 'Draw picture controls in Vector view'),
            ('P8.4: a part\'s interface draws Original, Vector or KONTRA; the pick and the default kept with the project (#1143)', "A part's interface draws Original, Vector or KONTRA; the pick and the default kept with the project"),
            ('Correct UVI MultiLFO saw waveform (#1180)', 'Correct UVI MultiLFO saw waveform'),
            ('Draw live UVI table values (#1176)', 'Draw live UVI table values'),
            ('Fix UVI key-follow mapper polarity (#1154)', 'Fix UVI key-follow mapper polarity'),
            ('fix: wait for offline sample windows (#1142)', 'Wait for offline sample windows'),
        ]
        for title, expected in cases:
            with self.subTest(title=title):
                self.assertEqual(clean(title), expected)
        self.assertEqual(clean('P5.53: fix(tools): repair loading (#1234)'), 'Repair loading')

    def test_sections_and_links(self):
        entries = [('P9.3: draw picture controls (#1151)', '1151', ['crates/app/ui/src/lib.rs']),
                   ('Fix filter (#1)', '1', ['crates/dsp/filter/src/lib.rs']),
                   ('Improve cache speed (#2)', '2', ['crates/engine/src/lib.rs']),
                   ('Inspect state (#3)', '3', ['tools/inspect/src/main.rs'])]
        result = render(entries)
        self.assertIn('- Draw picture controls. ([#1151](https://github.com/DerpcatMusic/KONTRA/pull/1151))', result)
        self.assertIn('### Sound & playing', result)
        self.assertIn('### Performance', result)
        self.assertIn('<summary>Internal: 1 change</summary>', result)
        self.assertEqual(section('Load a library', ['crates/format/kontakt/src/lib.rs']), 'Libraries & loading')


if __name__ == '__main__':
    unittest.main()
