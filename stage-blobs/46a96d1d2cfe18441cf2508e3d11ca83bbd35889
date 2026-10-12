import unittest

from check_glibc import required_version


class GlibcBaseline(unittest.TestCase):
    def test_numeric_versions_and_private_dependency(self):
        for output, expected in (("GLIBC_2.9 GLIBC_2.35 GLIBC_2.3.4", (2, 35)),
                                 ("GLIBC_2.39", (2, 39)),
                                 ("No version information found", (0, 0))):
            self.assertEqual(required_version(output), expected)
        with self.assertRaises(ValueError):
            required_version("GLIBC_PRIVATE")


if __name__ == "__main__":
    unittest.main()
