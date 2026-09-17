"""Check evaluator assertions against missing, known-good and broken behaviors.

Only monkeypatches this interpreter. Does not modify either upstream checkout.
PYTHONPATH=<cachetools>/src:<blinker>/src python3 acceptance_selftest.py
"""
import contextlib
import io
import unittest
from unittest.mock import patch

import acceptance
import cachetools
from cachetools import func
from blinker import Signal


def run_case(cls):
    stream = io.StringIO()
    result = unittest.TextTestRunner(stream=stream).run(
        unittest.defaultTestLoader.loadTestsFromTestCase(cls))
    return result.wasSuccessful(), stream.getvalue()


def discard(self, key):
    try:
        del self[key]
    except KeyError:
        return False
    return True


original_cached = cachetools.cached

def with_reset(*args, **kwargs):
    decorate = original_cached(*args, **kwargs)
    def decorator(function):
        f = decorate(function)
        if not kwargs.get('info', False):
            f.cache_reset_stats = None
        else:
            # Test-only reference adapter: reset original wrapper counter cells,
            # retaining the upstream cache implementation, lock, and cache_info.
            cells = dict(zip(f.__code__.co_freevars, f.__closure__ or ()))
            def reset():
                with f.cache_lock if f.cache_lock is not None else contextlib.nullcontext():
                    for name in ('hits', 'misses'):
                        if name in cells:
                            cells[name].cell_contents = 0
            f.cache_reset_stats = reset
        return f
    return decorator


class AcceptanceSelfTest(unittest.TestCase):
    def test_unmodified_upstream_rejected(self):
        for cls in (acceptance.Discard, acceptance.ResetStats, acceptance.ReceiverCount):
            ok, _ = run_case(cls)
            self.assertFalse(ok, cls.__name__)

    def test_reference_behaviors_accepted(self):
        with patch.object(cachetools.Cache, 'discard', discard, create=True), \
             patch.object(cachetools, 'cached', with_reset), \
             patch.object(func, 'cached', with_reset), \
             patch.object(Signal, 'receiver_count', property(lambda s: len(s.receivers)), create=True):
            for cls in (acceptance.Discard, acceptance.ResetStats, acceptance.ReceiverCount):
                ok, output = run_case(cls)
                self.assertTrue(ok, output)

    def test_discard_must_not_read_value(self):
        def bad_discard(self, key):
            try:
                self.pop(key)
            except KeyError:
                return False
            return True
        with patch.object(cachetools.Cache, 'discard', bad_discard, create=True):
            self.assertFalse(run_case(acceptance.Discard)[0])

    def test_stats_reset_must_not_clear_cache(self):
        def wrong(*args, **kwargs):
            decorate = with_reset(*args, **kwargs)
            def decorator(function):
                f = decorate(function)
                if kwargs.get('info', False): f.cache_reset_stats = f.cache_clear
                return f
            return decorator
        with patch.object(cachetools, 'cached', wrong), patch.object(func, 'cached', wrong):
            self.assertFalse(run_case(acceptance.ResetStats)[0])


if __name__ == '__main__':
    unittest.main()
