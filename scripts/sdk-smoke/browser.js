// Bundled with esbuild and loaded by browser.html in headless Chromium,
// driven by run-browser.cjs: tracing, session replay, profiling, logs,
// metrics, errors and feedback.
import * as Sentry from "@sentry/browser";

Sentry.init({
  dsn: window.SMOKE_DSN,
  release: "smoke-browser@1.0.0",
  environment: "smoke",
  integrations: [
    Sentry.browserTracingIntegration(),
    Sentry.replayIntegration({ maskAllText: false, blockAllMedia: false }),
    Sentry.browserProfilingIntegration(),
  ],
  tracesSampleRate: 1.0,
  replaysSessionSampleRate: 1.0,
  replaysOnErrorSampleRate: 1.0,
  profileSessionSampleRate: 1.0,
  profileLifecycle: "trace",
  sendDefaultPii: true,
});
Sentry.setUser({ id: "browser-user", email: "browser@example.com" });
Sentry.setTag("smoke", "browser");
Sentry.addBreadcrumb({ category: "smoke", message: "browser breadcrumb", level: "info" });

document.getElementById("buy").addEventListener("click", () => {
  document.getElementById("status").textContent = "bought";
  Sentry.logger.info("smoke browser log", { "smoke.kind": "browser" });
  Sentry.metrics.count("smoke.browser.clicks", 1, { attributes: { button: "buy" } });
});
document.getElementById("next").addEventListener("click", (e) => {
  e.preventDefault();
  history.pushState({}, "", "/checkout");
  document.getElementById("heading").textContent = "Checkout";
});

function failingFunction() {
  throw new TypeError("smoke browser error");
}

// Called by the driver after the clicks, so the errors happen during the replay.
window.smokeErrors = () => {
  try {
    failingFunction();
  } catch (e) {
    const eventId = Sentry.captureException(e);
    Sentry.captureFeedback({ message: "smoke browser feedback", name: "Browser User", email: "browser@example.com", associatedEventId: eventId });
  }
  Sentry.captureMessage("smoke browser message");
  setTimeout(() => {
    throw new Error("smoke browser unhandled");
  }, 0);
};

window.smokeFinish = async () => {
  const replay = Sentry.getReplay();
  if (replay) await replay.flush();
  await Sentry.flush(10000);
  return replay ? replay.getReplayId() : null;
};
