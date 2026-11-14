// Sends one of everything the Node SDK supports to SMOKE_DSN.
import * as Sentry from "@sentry/node";

Sentry.init({
  dsn: process.env.SMOKE_DSN,
  release: "smoke-node@1.0.0",
  environment: "smoke",
  tracesSampleRate: 1.0,
  sendDefaultPii: true,
  serverName: "smoke-host",
});

Sentry.setUser({ id: "node-user", email: "node@example.com" });
Sentry.setTag("smoke", "node");
Sentry.addBreadcrumb({ category: "smoke", message: "node breadcrumb", level: "info" });

// Release health: one session that sees an error.
Sentry.startSession();

function failingFunction() {
  throw new Error("smoke node error");
}

let eventId;
Sentry.withScope((scope) => {
  scope.addAttachment({ filename: "node.txt", data: "hello from node", contentType: "text/plain" });
  try {
    failingFunction();
  } catch (e) {
    eventId = Sentry.captureException(e);
  }
});
Sentry.captureMessage("smoke node message", "warning");
Sentry.captureFeedback({ message: "smoke node feedback", name: "Node User", email: "node@example.com", associatedEventId: eventId });

const checkInId = Sentry.captureCheckIn(
  { monitorSlug: "smoke-node-cron", status: "in_progress" },
  { schedule: { type: "crontab", value: "*/5 * * * *" }, checkinMargin: 2, maxRuntime: 10, timezone: "UTC" },
);
Sentry.captureCheckIn({ checkInId, monitorSlug: "smoke-node-cron", status: "ok", duration: 0.25 });

await Sentry.startSpan({ name: "smoke-node-transaction", op: "task" }, async () => {
  await Sentry.startSpan({ name: "child", op: "db.query" }, async () => new Promise((r) => setTimeout(r, 10)));
});

Sentry.endSession();
const ok = await Sentry.flush(10000);
if (!ok) {
  console.error("flush timed out");
  process.exit(1);
}
console.log("node: sent");
