"""Independent held-out behavior checks. No benchmark tool knowledge in prompts."""
import gc
import sys
import unittest
import warnings


class Discard(unittest.TestCase):
    def test_delete_path(self):
        from cachetools import Cache, FIFOCache, LFUCache, LRUCache, RRCache
        for base in (Cache, FIFOCache, LFUCache, LRUCache, RRCache):
            class Checked(base):
                deleted = 0
                def __getitem__(self, key):
                    raise AssertionError('discard must not read the value')
                def __missing__(self, key):
                    raise AssertionError('discard must not call missing')
                def __delitem__(self, key):
                    self.deleted += 1
                    return super().__delitem__(key)
            c = Checked(4, getsizeof=len)
            c['a'], c['b'] = 'aa', 'b'
            self.assertIs(c.discard('a'), True)
            self.assertEqual(c.deleted, 1)
            self.assertEqual(c.currsize, 1)
            self.assertIs(c.discard('a'), False)
            self.assertNotIn('a', c)
            self.assertIn('b', c)
            # Rebuild with ordinary reads to exercise eviction metadata after deletion.
            d = base(2)
            d['a'], d['b'] = 1, 2
            d.discard('a')
            d['c'], d['d'] = 3, 4
            self.assertEqual(len(d), 2)
            self.assertNotIn('a', d)
            while d:
                d.popitem()
            self.assertEqual(d.currsize, 0)


class ResetStats(unittest.TestCase):
    def test_wrapper_variants(self):
        from cachetools import cached
        class Lock:
            entered = 0
            def __enter__(self): self.entered += 1
            def __exit__(self, *args): pass
        for enabled in (True, False):
            for locking in (True, False):
                lock = Lock()
                store = {} if enabled else None
                calls = []
                @cached(store, lock=lock if locking else None, info=True)
                def f(x):
                    calls.append(x)
                    return x * 2
                f(1)
                f(1)
                before = dict(store) if enabled else None
                count = lock.entered
                self.assertIsNone(f.cache_reset_stats())
                if locking: self.assertGreater(lock.entered, count)
                self.assertEqual((f.cache_info().hits, f.cache_info().misses), (0, 0))
                if enabled:
                    self.assertEqual(store, before)
                self.assertEqual(len(calls), 1 if enabled else 2)
                f(1)
                self.assertEqual((f.cache_info().hits, f.cache_info().misses),
                                 (1, 0) if enabled else (0, 1))
                f.cache_clear()
                self.assertEqual((f.cache_info().hits, f.cache_info().misses), (0, 0))
                self.assertEqual(f.cache_info().currsize, 0)
        for store in (None, {}):
            for lock in (None, Lock()):
                f = cached(store, lock=lock, info=False)(lambda: 1)
                self.assertIsNone(f.cache_reset_stats)

    def test_convenience(self):
        from cachetools import func
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', DeprecationWarning)
            for name in func.__all__:
                f = getattr(func, name)(maxsize=8)(lambda x: x)
                f(2)
                f(2)
                self.assertIsNone(f.cache_reset_stats())
                self.assertEqual(tuple(f.cache_info()), (0, 0, 8, 1))


class ReceiverCount(unittest.TestCase):
    def test_lifecycle(self):
        from blinker import Signal, NamedSignal, ANY
        for signal in (Signal(), NamedSignal('check')):
            self.assertEqual(signal.receiver_count, 0)
            notices = []
            def on_notice(*a, **kw): notices.append(len(kw))  # do not retain receiver references
            signal.receiver_connected.connect(on_notice, weak=False)
            signal.receiver_disconnected.connect(on_notice, weak=False)
            def receiver(*a, **kw): raise AssertionError('must not call receiver')
            sender_a, sender_b = object(), object()
            signal.connect(receiver, sender_a)
            signal.connect(receiver, sender_b)
            signal.connect(receiver, ANY)
            count = len(notices)
            self.assertEqual(signal.receiver_count, 1)
            self.assertEqual(len(notices), count)
            signal.disconnect(receiver, sender_a)
            self.assertEqual(signal.receiver_count, 1)
            signal.disconnect(receiver)
            self.assertEqual(signal.receiver_count, 0)
            signal.connect(receiver)
            self.assertEqual(signal.receiver_count, 1)
            del receiver
            gc.collect()
            self.assertEqual(signal.receiver_count, 0)
            with self.assertRaises(AttributeError): signal.receiver_count = 9


if __name__ == '__main__':
    case = sys.argv.pop(1)
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(
        {'discard': Discard, 'reset-stats': ResetStats, 'receiver-count': ReceiverCount}[case])
    sys.exit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())
