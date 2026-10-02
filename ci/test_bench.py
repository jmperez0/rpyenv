import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402


class Summary(unittest.TestCase):
    def test_reports_means_and_the_overhead(self):
        text = bench.summary([
            {"command": "through the shim", "mean": 0.0300, "stddev": 0.0010},
            {"command": "directly", "mean": 0.0250, "stddev": 0.0008},
        ])
        self.assertIn("| through the shim | 30.0 ms | ± 1.0 ms |", text)
        self.assertIn("| directly | 25.0 ms | ± 0.8 ms |", text)
        self.assertIn("Shim overhead: 5.0 ms (20.0%)", text)


if __name__ == "__main__":
    unittest.main()
