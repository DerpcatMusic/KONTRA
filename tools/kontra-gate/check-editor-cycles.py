"""Numeric lifecycle checks; no plugin, library, or timing work."""
import copy
import unittest

from editor_rss import CYCLE_PHASES, summarize_cycles


class Cycles(unittest.TestCase):
    def rows(self):
        return [dict(phase=phase, sample=i, rss_kib=102400, hwm_kib=204800,
                     swap_kib=0, editor_children=int(index % 2 == 1),
                     width=1180 if index % 2 else 0, height=780 if index % 2 else 0,
                     parent_width=1180 if index % 2 else 0,
                     parent_height=780 if index % 2 else 0,
                     clap_width=1180 if index % 2 else 0,
                     clap_height=780 if index % 2 else 0,
                     wall_ns=(index*10+i)*500000000+1,
                     cpu_ns=(index*10+i)*5000000,
                     audio_thread_cpu_ns=(index*10+i)*1000000, threads=4,
                     audio_blocks=(index*10+i)*375,
                     audio_busy_ns=(index*10+i)*1000000,
                     audio_callback_overruns=0,
                     plugin_perf=dict(busy_ns=1, span_ns=10, voices=0, audible=0,
                                      dropouts=0, memory=0, freed=0, disk=0,
                                      underruns=0, loaded=1, blocks=(index*10+i)*375))
                for index, phase in enumerate(CYCLE_PHASES) for i in range(10)]

    def test_plateau(self):
        result = summarize_cycles(self.rows(), (1180, 780))
        self.assertEqual(result['closed_growth_mib'], 0)
        self.assertEqual(result['open_growth_mib'], 0)
        self.assertAlmostEqual(result['phases']['open_1']['cpu_percent'], 1)
        self.assertAlmostEqual(result['phases']['open_1']['audio_callback_busy_percent'], .2)
        self.assertAlmostEqual(result['phases']['open_1']['audio_thread_cpu_percent'], .2)

    def test_counter_or_viewport_failure(self):
        rows = self.rows()
        for key, value in [('wall_ns', 0), ('cpu_ns', -1), ('audio_blocks', True),
                           ('parent_width', 1182), ('clap_height', 779), ('threads', 0),
                           ('audio_thread_cpu_ns', None), ('plugin_perf', None)]:
            bad = copy.deepcopy(rows)
            bad[31][key] = value
            with self.assertRaises(AssertionError, msg=key):
                summarize_cycles(bad, (1180, 780))
        for key, value in [('loaded', 0), ('blocks', True), ('underruns', -1)]:
            bad = copy.deepcopy(rows)
            bad[31]['plugin_perf'][key] = value
            with self.assertRaises(AssertionError, msg=key):
                summarize_cycles(bad, (1180, 780))
        bad = copy.deepcopy(rows)
        del bad[31]['plugin_perf']['busy_ns']
        with self.assertRaises(AssertionError):
            summarize_cycles(bad, (1180, 780))
        bad = copy.deepcopy(rows)
        bad[31]['cpu_ns'] = 0
        with self.assertRaises(AssertionError):
            summarize_cycles(bad, (1180, 780))
        with self.assertRaises(AssertionError):
            summarize_cycles(rows[:-1], (1180, 780))

    def test_growth_remains_observed_not_a_leak_claim(self):
        rows = self.rows()
        for row in rows:
            if row['phase'] == 'closed_4': row['rss_kib'] += 10240
        result = summarize_cycles(rows, (1180, 780))
        self.assertEqual(result['closed_growth_mib'], 10)
        self.assertNotIn('leak', result)
        self.assertNotIn('status', result)


if __name__ == '__main__': unittest.main()
