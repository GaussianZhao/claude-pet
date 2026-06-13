/**
 * Renders the menu-bar (tray) icon SVG to a 256x256 template PNG.
 * Template image => pure black shapes on a transparent background; macOS
 * recolors it to match the menu bar. Eyes and mouth are cut out (transparent)
 * so they read as the bar color, like the original.
 *
 * Usage: node scripts/make-tray-icon.mjs
 */
import puppeteer from "puppeteer-core";
import { writeFile } from "fs/promises";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT = join(__dirname, "..", "src-tauri", "icons", "tray.png");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

// Bigger rounded-square head, short antenna, eyes well above a separate smile.
const SVG = `
<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
  <defs>
    <mask id="face">
      <rect x="0" y="0" width="256" height="256" fill="white"/>
      <!-- eyes (cut out) -->
      <circle cx="98" cy="120" r="20" fill="black"/>
      <circle cx="158" cy="120" r="20" fill="black"/>
      <!-- smile (cut out), clearly below the eyes -->
      <path d="M92 168 Q128 200 164 168" fill="none" stroke="black"
            stroke-width="14" stroke-linecap="round"/>
    </mask>
  </defs>
  <!-- short antenna: stalk + dot -->
  <rect x="122" y="40" width="12" height="26" rx="6" fill="black"/>
  <circle cx="128" cy="30" r="14" fill="black"/>
  <!-- head -->
  <rect x="30" y="62" width="196" height="170" rx="46" fill="black" mask="url(#face)"/>
</svg>`;

const browser = await puppeteer.launch({
  executablePath: CHROME,
  args: ["--no-sandbox", "--force-color-profile=srgb"],
});
const page = await browser.newPage();
await page.setViewport({ width: 256, height: 256, deviceScaleFactor: 1 });
await page.setContent(
  `<style>html,body{margin:0;background:transparent}</style>${SVG}`,
  { waitUntil: "networkidle0" },
);
const el = await page.$("svg");
const buf = await el.screenshot({ omitBackground: true });
await writeFile(OUT, buf);
await browser.close();
console.log("wrote", OUT);
