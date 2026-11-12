"""Tests of the packaging script's version handling (TD-151).

Run: python3 -m unittest discover -s tools/packaging -p 'test_*.py'
"""
import shutil
import subprocess
import unittest

import package


class DebianVersion(unittest.TestCase):
    def test_release_versions_are_unchanged(self):
        self.assertEqual(package.deb_version("1.2.3"), "1.2.3")

    def test_pre_release_suffixes_use_a_tilde(self):
        self.assertEqual(package.deb_version("1.0.0-rc.1"), "1.0.0~rc.1")
        self.assertEqual(package.deb_version("2.0.0-beta.3"), "2.0.0~beta.3")
        self.assertEqual(package.deb_version("0.2.0-dev"), "0.2.0~dev")

    @unittest.skipUnless(shutil.which("dpkg"), "dpkg is not installed")
    def test_dpkg_orders_versions_as_semantic_versioning_does(self):
        # Each version is older than the next one, in Semantic Versioning's order.
        ordered = ["1.0.0-alpha.1", "1.0.0-beta.1", "1.0.0-beta.2", "1.0.0-rc.1", "1.0.0",
                   "1.0.1", "1.1.0-dev", "1.1.0"]
        for older, newer in zip(ordered, ordered[1:]):
            with self.subTest(older=older, newer=newer):
                result = subprocess.run(["dpkg", "--compare-versions", package.deb_version(older),
                                         "lt", package.deb_version(newer)])
                self.assertEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
