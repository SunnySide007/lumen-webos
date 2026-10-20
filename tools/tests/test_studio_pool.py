"""Tests for studio/pool.py: python3 -I -m unittest discover -s tools/tests"""

import concurrent.futures
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'studio'))

import pool  # noqa: E402

GB = 1 << 30


def square(x):
    return x * x


def die(_):
    os._exit(1)  # as if the kernel killed the worker for memory


class MemoryTest(unittest.TestCase):
    def test_reads_mem_available(self):
        with tempfile.NamedTemporaryFile('w', delete=False) as f:
            f.write('MemTotal:       8131536 kB\nMemFree:  100 kB\nMemAvailable:   6000000 kB\n')
        self.addCleanup(os.remove, f.name)
        self.assertEqual(pool.memory_bytes(f.name), 6000000 * 1024)

    def test_falls_back_without_meminfo(self):
        total = pool.memory_bytes('/nonexistent/meminfo')
        self.assertTrue(total is None or total > 0)


class WorkersTest(unittest.TestCase):
    def workers(self, free, per_worker, reserve=0, cpus=16):
        with mock.patch.object(pool, 'memory_bytes', return_value=free), \
                mock.patch.object(pool.os, 'cpu_count', return_value=cpus):
            return pool.default_workers(per_worker, reserve=reserve)

    def test_one_per_cpu_up_to_eight(self):
        self.assertEqual(self.workers(64 * GB, GB // 10), 8)
        self.assertEqual(self.workers(64 * GB, GB // 10, cpus=3), 3)

    def test_memory_limits_the_count(self):
        # 60% of 8 GB is 4.8 GB: four workers of 1 GB.
        self.assertEqual(self.workers(8 * GB, GB), 4)

    def test_the_reserve_comes_off_first(self):
        self.assertEqual(self.workers(8 * GB, GB, reserve=6 * GB), 1)

    def test_always_at_least_one(self):
        self.assertEqual(self.workers(GB, 4 * GB, reserve=2 * GB), 1)

    def test_unknown_memory_goes_by_cpus(self):
        self.assertEqual(self.workers(None, GB), 8)


class OrderedTest(unittest.TestCase):
    def test_results_come_in_order_through_a_small_window(self):
        with pool.executor(2) as workers:
            self.assertEqual(list(pool.ordered(workers, square, range(10), window=3)), [x * x for x in range(10)])

    def test_a_dead_worker_raises_instead_of_hanging(self):
        with pool.executor(2) as workers:
            with self.assertRaises(concurrent.futures.process.BrokenProcessPool):
                list(pool.ordered(workers, die, range(4), window=2))


if __name__ == '__main__':
    unittest.main()
