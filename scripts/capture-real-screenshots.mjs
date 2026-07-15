#!/usr/bin/env node
// Captures REAL screenshots of a running `azork crawl --serve` Dungeon
// Crawler map against a real Azure subscription, using Playwright.
//
// Unlike scripts/capture-screenshots.sh (which drives the *mock* backend as
// a quick local documentation aid and is never run in CI), this script is a
// one-off, manually-invoked tool for regenerating docs/images/crawl-*.png
// against a REAL, already-running server (started separately with
// `azork crawl --backend az --serve --port <PORT>`). It is not part of the
// build, test, or CI pipeline.
//
// Usage:
//   AZORK_MAP_URL=http://127.0.0.1:8791 node scripts/capture-real-screenshots.mjs
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = join(__dirname, "..", "docs", "images");
const BASE_URL = process.env.AZORK_MAP_URL || "http://127.0.0.1:8791";

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1200 } });

  console.log(`Loading ${BASE_URL} ...`);
  await page.goto(BASE_URL, { waitUntil: "networkidle" });

  const roomCount = await page.locator(".room").count();
  const resourceCount = await page.locator(".resource").count();
  console.log(`Loaded map: ${roomCount} rooms, ${resourceCount} resources.`);
  if (roomCount === 0 || resourceCount === 0) {
    throw new Error("Map has no rooms/resources — refusing to capture an empty map.");
  }

  // The server-rendered SVG already declares explicit pixel width/height on
  // its root <svg> (see src/dungeon/render.rs), so no viewBox injection is
  // needed for it to size correctly. We do, however, make sure the page
  // itself is scrolled to the top-left before the overview capture.
  await page.evaluate(() => window.scrollTo(0, 0));

  // --- (a) Full map overview ---------------------------------------------
  const svgBox = await page.locator("svg").first().boundingBox();
  await page.setViewportSize({
    width: Math.min(Math.ceil(svgBox.width) + 40, 8000),
    height: Math.min(Math.ceil(svgBox.height) + 40, 8000),
  });
  await page.screenshot({ path: join(OUT_DIR, "crawl-map-overview.png") });
  console.log("Captured crawl-map-overview.png");

  // --- (b) Zoomed-in region with legible icons/labels ---------------------
  // Zoom into a region with several adjacent rooms so resource icons and
  // room (resource-group) labels are legible at native resolution.
  await page.setViewportSize({ width: 1400, height: 1000 });
  const rooms = page.locator(".room");
  const zoomRoom = rooms.nth(Math.min(3, roomCount - 1));
  await zoomRoom.scrollIntoViewIfNeeded();
  const zoomBox = await zoomRoom.boundingBox();
  const clip = {
    x: Math.max(0, zoomBox.x - 60),
    y: Math.max(0, zoomBox.y - 60),
    width: Math.min(900, zoomBox.width + 420),
    height: Math.min(700, zoomBox.height + 320),
  };
  await page.screenshot({ path: join(OUT_DIR, "crawl-map-zoom.png"), clip });
  console.log("Captured crawl-map-zoom.png");

  // --- (c) Interactive resource popup -------------------------------------
  // Click a real .resource element and wait for the server-backed #detail
  // panel to be populated via the client's fetch('/api/v1/resources/<id>').
  await page.setViewportSize({ width: 1000, height: 700 });
  const resourceEl = page.locator(".resource").first();
  const resId = await resourceEl.getAttribute("data-resource-id");
  console.log(`Clicking resource ${resId} ...`);
  await resourceEl.scrollIntoViewIfNeeded();
  await resourceEl.click();

  const detail = page.locator("#detail");
  await detail.waitFor({ state: "visible", timeout: 15000 });
  // Wait until the detail panel actually contains the portal link and an
  // az command, not just the empty shell — the fetch is async.
  await page.waitForFunction(
    () => {
      const el = document.getElementById("detail");
      return el && el.querySelector("a") && el.querySelector("code");
    },
    { timeout: 15000 },
  );

  const detailText = await detail.innerText();
  console.log(`Popup detail panel contents:\n${detailText}`);

  await detail.scrollIntoViewIfNeeded();
  // Screenshot the #detail element directly (Playwright's element
  // screenshot handles scrolling/positioning itself, avoiding manual
  // clip-box math that can land outside the viewport after a scroll).
  await detail.screenshot({ path: join(OUT_DIR, "crawl-resource-popup.png") });
  console.log("Captured crawl-resource-popup.png");

  await browser.close();

  console.log(
    JSON.stringify({ roomCount, resourceCount, clickedResourceId: resId }, null, 2),
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
