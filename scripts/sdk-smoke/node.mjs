// Sends one of everything the Node SDK supports to SMOKE_DSN: errors,
// messages, attachments, feedback, check-ins, sessions, spans, logs,
// metrics and (continuous) profiles.
import * as Sentry from "@sentry/node";
import { nodeProfilingIntegration } from "@sentry/profiling-node";

Sentry.init({
  dsn: process.env.SMOKE_DSN,
  release: "smoke-node@1.0.0",
  environment: "smoke",
  tracesSampleRate: 1.0,
  sendDefaultPii: true,
  serverName: "smoke-host",
  integrations: [nodeProfilingIntegration()],
  // Continuous profiling while spans are active.
  profileSessionSampleRate: 1.0,
  profileLifecycle: "trace",
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

// Busy work the profiler can sample.
function fibonacci(n) {
  return n < 2 ? n : fibonacci(n - 1) + fibonacci(n - 2);
}
function computeChecksum() {
  let total = 0;
  const end = Date.now() + 300;
  while (Date.now() < end) total += fibonacci(20);
  return total;
}

await Sentry.startSpan({ name: "smoke-node-transaction", op: "task" }, async () => {
  await Sentry.startSpan({ name: "child", op: "db.query" }, async () => new Promise((r) => setTimeout(r, 10)));
  await Sentry.startSpan({ name: "compute checksum", op: "function" }, async () => computeChecksum());
  // Logs inside the span carry its trace.
  Sentry.logger.info("smoke node log", { "smoke.kind": "node", "order.id": 42 });
  Sentry.logger.warn(Sentry.logger.fmt`smoke node low stock for ${"widget"}`);
  Sentry.logger.error("smoke node log error", { password: "hunter2" });
  Sentry.metrics.count("smoke.node.requests", 2, { attributes: { route: "/checkout" } });
  Sentry.metrics.gauge("smoke.node.queue", 7, { unit: "none" });
  Sentry.metrics.distribution("smoke.node.latency", 125.5, { unit: "millisecond", attributes: { route: "/checkout" } });
  Sentry.metrics.distribution("smoke.node.latency", 80, { unit: "millisecond", attributes: { route: "/checkout" } });
});

Sentry.endSession();
const ok = await Sentry.flush(10000);
if (!ok) {
  console.error("flush timed out");
  process.exit(1);
}
console.log("node: sent");
