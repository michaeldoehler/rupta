"""Continuous profiling with the Python SDK: a profiler session that
follows the transaction lifecycle sends profile_chunk items."""
import os
import time

import sentry_sdk

sentry_sdk.init(
    dsn=os.environ["SMOKE_DSN"],
    release="smoke-python@1.0.0",
    environment="smoke",
    traces_sample_rate=1.0,
    profile_session_sample_rate=1.0,
    profile_lifecycle="trace",
)


def fibonacci(n):
    return n if n < 2 else fibonacci(n - 1) + fibonacci(n - 2)


def busy_loop():
    end = time.monotonic() + 0.5
    while time.monotonic() < end:
        fibonacci(15)


with sentry_sdk.start_transaction(name="smoke-python-continuous", op="task"):
    busy_loop()

# The profiler sends its chunk when the session stops or the chunk is full.
time.sleep(1.5)
sentry_sdk.flush(timeout=10)
print("python (continuous profiling): sent")
