"""Worker processes for frames too large to hold many of at once.

concurrent.futures rather than multiprocessing.Pool: when a worker dies, for
example killed for memory, the pool raises BrokenProcessPool instead of
waiting forever for its result.
"""

import collections
import concurrent.futures
import os


def memory_bytes():
    """The machine's memory (in Docker, the VM's), or None if unknown."""
    try:
        return os.sysconf('SC_PAGE_SIZE') * os.sysconf('SC_PHYS_PAGES')
    except (AttributeError, OSError, ValueError):
        return None


def default_workers(bytes_per_worker, share=0.6, cap=8):
    """One worker per CPU, at most `cap`, and only as many as fit in `share` of memory."""
    workers = min(os.cpu_count() or 1, cap)
    total = memory_bytes()
    if total:
        workers = min(workers, max(1, int(total * share // bytes_per_worker)))
    return workers


def executor(workers, initializer=None, initargs=()):
    return concurrent.futures.ProcessPoolExecutor(workers, initializer=initializer, initargs=initargs)


def ordered(pool, func, items, window):
    """pool.map, in order, with at most `window` results waiting at a time."""
    pending = collections.deque()
    for item in items:
        pending.append(pool.submit(func, item))
        if len(pending) >= window:
            yield pending.popleft().result()
    while pending:
        yield pending.popleft().result()
