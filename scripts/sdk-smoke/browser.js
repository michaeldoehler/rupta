// Bundled with esbuild and loaded by browser.html in headless Chrome.
import * as Sentry from "@sentry/browser";

Sentry.init({
  dsn: window.SMOKE_DSN,
  release: "smoke-browser@1.0.0",
  environment: "smoke",
  integrations: [Sentry.browserTracingIntegration()],
  tracesSampleRate: 1.0,
  sendDefaultPii: true,
});
Sentry.setUser({ id: "browser-user", email: "browser@example.com" });
Sentry.setTag("smoke", "browser");
Sentry.addBreadcrumb({ category: "smoke", message: "browser breadcrumb", level: "info" });

function failingFunction() {
  throw new TypeError("smoke browser error");
}

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
setTimeout(async () => {
  await Sentry.flush(10000);
  document.title = "done";
}, 1500);
