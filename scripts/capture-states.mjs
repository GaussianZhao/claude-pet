/**
 * Captures a screenshot of each pet state by injecting state directly via JS.
 * Usage: node scripts/capture-states.mjs
 */

import puppeteer from "puppeteer-core";
import { spawn } from "child_process";
import { mkdir } from "fs/promises";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const OUT = join(ROOT, "docs", "states");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const PROJECT = "ai-coding-pet";
const TASK = "Building the feature";

const STATES = [
  { name: "running",   running: true,  status: "running",   task: "Generating dashboard components" },
  { name: "waiting",   running: true,  status: "waiting",   task: "Waiting to edit config.ts" },
  { name: "completed", running: true,  status: "completed", task: "Generating dashboard components" },
  { name: "error",     running: true,  status: "error",     task: "npm run build failed" },
  { name: "idle",      running: true,  status: "idle",      task: "" },
  { name: "offline",   running: false, status: "idle",      task: "" },
];

// Tight crop: glow top ≈ viewport-y 121, "!" bubble / note tops ≈ y 93, body
// bottom ≈ y 209.  y=80 gives 13px headroom above the highest element; h=140
// covers all the way to the body bottom with a small bottom margin.
const CLIP = { x: 0, y: 80, width: 160, height: 140 };

function makePetState(s) {
  const now = new Date().toISOString().slice(0, 16).replace("T", " ");
  return {
    running: s.running,
    status: s.status,
    sessions: s.running ? [{
      sessionId: "demo",
      project: PROJECT,
      taskName: s.task,
      status: s.status,
      cwd: "/demo",
      updatedAt: now,
    }] : [],
  };
}

async function main() {
  await mkdir(OUT, { recursive: true });

  console.log("Starting vite dev server…");
  const vite = spawn("npm", ["run", "dev"], {
    cwd: ROOT,
    stdio: ["ignore", "pipe", "pipe"],
  });

  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("vite start timeout")), 20000);
    const check = (d) => {
      if (d.toString().includes("localhost")) { clearTimeout(t); resolve(); }
    };
    vite.stdout.on("data", check);
    vite.stderr.on("data", check);
  });
  console.log("Vite ready.");

  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: true,
    args: ["--no-sandbox", "--disable-setuid-sandbox", "--disable-gpu"],
  });

  try {
    const page = await browser.newPage();
    // The pet canvas is 150×150 CSS px centred in a 150px-wide window.
    // We give the viewport some height margin for the glow + antenna overflow.
    await page.setViewport({ width: 160, height: 240, deviceScaleFactor: 2 });
    await page.goto("http://localhost:1420", { waitUntil: "networkidle0" });

    // Wait for PixiJS canvas to initialise (async init in Pet component).
    await page.waitForSelector("canvas", { timeout: 15000 });
    // Extra wait so the first animation frames have rendered.
    await sleep(2500);

    for (const s of STATES) {
      const petState = makePetState(s);

      // Inject state directly — no timing guesswork.
      await page.evaluate((state) => {
        const setter = window.__setMockState;
        if (setter) setter(state);
      }, petState);

      // Let the animation settle into a representative frame.
      await sleep(1200);

      const outPath = join(OUT, `${s.name}.png`);
      await page.screenshot({ path: outPath, clip: CLIP, omitBackground: true });
      console.log(`  ✓  ${s.name}.png`);
    }
  } finally {
    await browser.close();
    vite.kill();
  }

  console.log("\nAll state screenshots saved to docs/states/");
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

main().catch(e => { console.error(e); process.exit(1); });
