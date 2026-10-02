import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402


class Summary(unittest.TestCase):
    def test_reports_medians_and_the_overhead_from_them(self):
        text = bench.summary([
            {"command": "through the shim", "mean": 0.0300, "median": 0.0290, "stddev": 0.0010},
            {"command": "directly", "mean": 0.0250, "median": 0.0200, "stddev": 0.0008},
        ])
        self.assertIn("| through the shim | 30.0 ms | 29.0 ms | ± 1.0 ms |", text)
        self.assertIn("| directly | 25.0 ms | 20.0 ms | ± 0.8 ms |", text)
        self.assertIn("Shim overhead (from the medians): 9.0 ms (45.0%)", text)

    def test_a_hyperfine_without_a_median_falls_back_to_the_mean(self):
        text = bench.summary([
            {"command": "through the shim", "mean": 0.0300, "stddev": 0.0010},
            {"command": "directly", "mean": 0.0250, "stddev": 0.0008},
        ])
        self.assertIn("| through the shim | 30.0 ms | 30.0 ms | ± 1.0 ms |", text)
        self.assertIn("Shim overhead (from the medians): 5.0 ms (20.0%)", text)


if __name__ == "__main__":
    unittest.main()
