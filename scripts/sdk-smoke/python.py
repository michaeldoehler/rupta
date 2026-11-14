"""Sends one of everything the Python SDK supports to SMOKE_DSN."""
import os
import time

import sentry_sdk
from sentry_sdk.crons import capture_checkin
from sentry_sdk.crons.consts import MonitorStatus

sentry_sdk.init(
    dsn=os.environ["SMOKE_DSN"],
    release="smoke-python@1.0.0",
    environment="smoke",
    traces_sample_rate=1.0,
    send_default_pii=True,
    server_name="smoke-host",
)

sentry_sdk.set_user({"id": "python-user", "email": "python@example.com"})
sentry_sdk.set_tag("smoke", "python")
sentry_sdk.add_breadcrumb(category="smoke", message="python breadcrumb", level="info")

scope = sentry_sdk.get_isolation_scope()
scope.start_session()


def failing_function():
    return 1 / 0


with sentry_sdk.new_scope() as s:
    s.add_attachment(bytes=b"hello from python", filename="python.txt", content_type="text/plain")
    try:
        failing_function()
    except ZeroDivisionError:
        sentry_sdk.capture_exception()
sentry_sdk.capture_message("smoke python message", level="warning")

config = {"schedule": {"type": "crontab", "value": "*/5 * * * *"}, "checkin_margin": 2, "max_runtime": 10, "timezone": "UTC"}
check_in_id = capture_checkin(monitor_slug="smoke-python-cron", status=MonitorStatus.IN_PROGRESS, monitor_config=config)
capture_checkin(monitor_slug="smoke-python-cron", check_in_id=check_in_id, status=MonitorStatus.OK, duration=0.25)

with sentry_sdk.start_transaction(name="smoke-python-transaction", op="task"):
    with sentry_sdk.start_span(op="db.query", name="child"):
        time.sleep(0.01)

scope.end_session()
sentry_sdk.flush(timeout=10)
print("python: sent")
