// Opens browser.html in headless Chrome and waits until the page is done.
// usage: node run-browser.mjs <chrome-binary> <page-url>
import puppeteer from "puppeteer-core";

const [chrome, url] = process.argv.slice(2);
const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ["--no-sandbox"] });
try {
  const page = await browser.newPage();
  page.on("console", (m) => console.log("browser console:", m.text()));
  await page.goto(url, { waitUntil: "load" });
  await page.waitForFunction(() => document.title === "done", { timeout: 30000 });
  // Let the session update and transaction go out.
  await new Promise((r) => setTimeout(r, 1000));
  console.log("browser: sent");
} finally {
  await browser.close();
}
