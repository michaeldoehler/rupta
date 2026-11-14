// Drives browser.html in headless Chromium with Playwright: clicks a
// button, navigates (history API), triggers errors, and flushes the SDK.
// The page is served with `Document-Policy: js-profiling` so the browser
// profiler (JS Self-Profiling API) is available.
// usage: node run-browser.cjs <page-url>
const { chromium } = require("playwright");

(async () => {
  const url = process.argv[2];
  const browser = await chromium.launch({ headless: true, args: [
      "--no-sandbox",
      // Talk to the local server directly, not through an HTTP proxy from the environment.
      "--no-proxy-server",
      // The page and the server are both on 127.0.0.1 but on different ports; recent
      // Chromium asks the user before a page may reach a loopback address.
      "--disable-features=LocalNetworkAccessChecks",
    ] });
  try {
    const page = await browser.newPage();
    page.on("console", (m) => console.log("browser console:", m.text()));
    await page.route("**/index.html", async (route) => {
      const response = await route.fetch();
      await route.fulfill({ response, headers: { ...response.headers(), "document-policy": "js-profiling" } });
    });
    await page.goto(url, { waitUntil: "load" });
    await page.waitForTimeout(500);
    await page.click("#buy");
    await page.waitForTimeout(300);
    await page.click("#next");
    await page.waitForTimeout(300);
    await page.evaluate(() => window.smokeErrors());
    // Replays shorter than five seconds are not sent.
    await page.waitForTimeout(5000);
    const replayId = await page.evaluate(() => window.smokeFinish());
    console.log("browser: replay " + replayId);
    // Let the last transport requests finish.
    await page.waitForTimeout(1500);
    console.log("browser: sent");
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
