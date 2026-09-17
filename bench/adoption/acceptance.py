"""Host-owned behavioral checks; run with PYTHONPATH=<trial>/src."""
import sys
import threading
import unittest
import warnings

from cachetools import Cache, FIFOCache, LFUCache, LRUCache, RRCache, TTLCache, TLRUCache, cached
from cachetools import func


class NegativeSize(unittest.TestCase):
    def test_rejection_is_atomic(self):
        factories = [Cache, FIFOCache, LFUCache, LRUCache, RRCache,
                     lambda **kw: TTLCache(ttl=60, **kw),
                     lambda **kw: TLRUCache(ttu=lambda k, v, now: now + 60, **kw)]
        for factory in factories:
            with self.subTest(factory=factory):
                c = factory(maxsize=3, getsizeof=lambda v: v)
                c['a'], c['b'] = 1, 2
                for key in ['new', 'a', 'b']:
                    with self.assertRaises(ValueError):
                        c[key] = -1
                    self.assertEqual(dict(c), {'a': 1, 'b': 2})
                    self.assertEqual(c.currsize, 3)
                c['zero'] = 0
                self.assertEqual(c.currsize, 3)
                with self.assertRaisesRegex(ValueError, 'value too large'):
                    c['huge'] = 4
                self.assertEqual(dict(c), {'a': 1, 'b': 2, 'zero': 0})


class Invalidate(unittest.TestCase):
    def test_variants(self):
        class Lock:
            active = False
            def __enter__(self): self.active = True
            def __exit__(self, *exc): self.active = False
        for info in [False, True]:
            for use_lock in [False, True]:
                for no_cache in [False, True]:
                    with self.subTest(info=info, lock=use_lock, no_cache=no_cache):
                        lock = Lock()
                        class Checked(dict):
                            def __delitem__(self, k):
                                if use_lock:
                                    assert lock.active, 'deletion outside lock'
                                return super().__delitem__(k)
                            def pop(self, *args):
                                if use_lock:
                                    assert lock.active, 'deletion outside lock'
                                return super().pop(*args)
                        store = None if no_cache else Checked()
                        calls = []
                        @cached(store, key=lambda x, scale=1: (x, scale),
                                lock=lock if use_lock else None, info=info)
                        def f(x, scale=1):
                            calls.append(x)
                            return x * scale
                        f(2, scale=3)
                        f(9)
                        before = f.cache_info() if info else None
                        self.assertIs(f.cache_invalidate(2, scale=3), not no_cache)
                        self.assertIs(f.cache_invalidate(2, scale=3), False)
                        self.assertEqual(calls, [2, 9])
                        if info:
                            after = f.cache_info()
                            self.assertEqual((after.hits, after.misses),
                                             (before.hits, before.misses))
                            self.assertEqual(after.maxsize, before.maxsize)
                            self.assertEqual(after.currsize, 0 if no_cache else 1)
                        if store is not None: self.assertEqual(store, {(9, 1): 9})
                        f(2, scale=3)
                        self.assertEqual(calls, [2, 9, 2])

    def test_convenience_decorators(self):
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', DeprecationWarning)
            for name in func.__all__:
                for maxsize in [0, 4, None]:
                    f = getattr(func, name)(maxsize=maxsize, typed=True)(lambda x: x)
                    f(1)
                    f(1.0)
                    before = f.cache_info()
                    self.assertIs(f.cache_invalidate(1), maxsize != 0)
                    self.assertEqual(f.cache_info().hits, before.hits)
                    self.assertEqual(f.cache_info().misses, before.misses)
                    self.assertIs(f.cache_invalidate(1.0), maxsize != 0)


class Peek(unittest.TestCase):
    def test_non_touching_read(self):
        class Custom(LRUCache):
            def __missing__(self, key):
                raise AssertionError('peek called __missing__')
        c = Custom(2)
        c['a'], c['b'] = None, 2
        sentinel = object()
        self.assertIs(c.peek('a', sentinel), None)
        self.assertIs(c.peek('missing', sentinel), sentinel)
        self.assertIsNone(c.peek('missing'))
        self.assertEqual(len(c), 2)
        c['c'] = 3
        self.assertNotIn('a', c)
        self.assertEqual(c.peek('b'), 2)
        self.assertEqual(c.get('b'), 2)
        c['d'] = 4
        self.assertNotIn('c', c)
        self.assertEqual(c['b'], 2)
        c['e'] = 5
        self.assertNotIn('d', c)


if __name__ == '__main__':
    case = sys.argv.pop(1)
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(
        {'negative-size': NegativeSize, 'invalidate': Invalidate, 'peek': Peek}[case])
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    sys.exit(not result.wasSuccessful())
