import { Application, Container, Graphics } from "pixi.js";
import { STATUS_COLOR, type TaskStatus } from "../types";

// Canvas is sized tightly around the drawing so the window — and thus the
// clickable area — stays small and doesn't swallow desktop clicks.
const SIZE = 150;
const BODY = 0xede9fe;
const OUTLINE = 0x6d28d9;
const ACCENT = 0x8b5cf6;

/**
 * A self-contained procedural pixel pet rendered with PixiJS.
 *
 * Each Claude state drives a distinct animation (PRD F1):
 *   idle      → gentle standing bob
 *   running   → fast "typing" arms (hammering down)
 *   waiting   → one arm waving + "!" bubble
 *   completed → dancing sway with floating notes
 *   error     → slumped posture with falling tears
 */
export class PixelPet {
  app!: Application;
  private root = new Container();
  private glow = new Graphics();
  private body = new Container();
  private leftArm = new Graphics();
  private rightArm = new Graphics();
  private bodyShape = new Graphics();
  private face = new Container();
  private leftPupil = new Graphics();
  private rightPupil = new Graphics();
  private eyeL = new Container();
  private eyeR = new Container();
  private mouth = new Graphics();
  private cheeks = new Graphics();
  private fx = new Graphics(); // per-frame effects (tears, notes, bubble)

  private status: TaskStatus = "idle";
  private dimmed = false;
  private t = 0;
  private blink = 0;
  private nextBlink = 2 + Math.random() * 3;
  private ready = false;
  private destroyed = false;
  private pending: { status: TaskStatus; running: boolean } = {
    status: "idle",
    running: false,
  };

  async init(parent: HTMLElement) {
    this.app = new Application();
    await this.app.init({
      width: SIZE,
      height: SIZE,
      backgroundAlpha: 0,
      antialias: true,
      resolution: window.devicePixelRatio || 1,
      autoDensity: true,
      preference: "webgl",
    });
    if (this.destroyed) {
      try { this.app.destroy(true); } catch { /* ignore */ }
      return;
    }
    parent.appendChild(this.app.canvas);
    this.build();
    this.ready = true;
    this.applyStatus(this.pending.status, this.pending.running);
    this.app.ticker.add((ticker) => this.update(ticker.deltaMS / 1000));
  }

  destroy() {
    this.destroyed = true;
    if (this.ready && this.app) {
      try { this.app.destroy(true, { children: true }); } catch { /* ignore */ }
    }
  }

  private build() {
    this.root.position.set(SIZE / 2, SIZE / 2 + 4);
    this.app.stage.addChild(this.root);
    // Glow sits below everything else.
    this.root.addChild(this.glow);
    this.root.addChild(this.body);

    // Arms drawn first so they appear behind the body shape.
    this.body.addChild(this.leftArm, this.rightArm);
    this.leftArm.position.set(-32, -4);
    this.rightArm.position.set(32, -4);
    this.drawArm(this.leftArm);
    this.drawArm(this.rightArm);

    // Body blob + antenna.
    this.bodyShape
      .roundRect(-36, -40, 72, 80, 26)
      .fill(BODY)
      .stroke({ width: 3, color: OUTLINE, alpha: 0.9 });
    this.bodyShape.moveTo(0, -40).lineTo(0, -52).stroke({ width: 3, color: OUTLINE });
    this.bodyShape.circle(0, -56, 4).fill(ACCENT);
    this.body.addChild(this.bodyShape);

    // Face.
    this.body.addChild(this.face);
    this.cheeks.position.set(0, 0);
    this.face.addChild(this.cheeks);
    this.buildEye(this.eyeL, this.leftPupil, -14);
    this.buildEye(this.eyeR, this.rightPupil, 14);
    this.face.addChild(this.eyeL, this.eyeR);
    this.face.addChild(this.mouth);

    this.root.addChild(this.fx);
  }

  private buildEye(eye: Container, pupil: Graphics, x: number) {
    eye.position.set(x, -6);
    const white = new Graphics();
    white.circle(0, 0, 9).fill(0xffffff).stroke({ width: 2, color: OUTLINE });
    pupil.circle(0, 0, 4).fill(0x312e81);
    eye.addChild(white, pupil);
  }

  private drawArm(arm: Graphics) {
    arm.clear();
    arm.roundRect(-5, 0, 10, 26, 5).fill(BODY).stroke({ width: 3, color: OUTLINE });
  }

  private setMouth(status: TaskStatus) {
    const m = this.mouth;
    m.clear();
    switch (status) {
      case "running":
        m.ellipse(0, 12, 4, 5).fill(0x312e81);
        break;
      case "waiting":
        m.arc(0, 8, 8, 0.2 * Math.PI, 0.8 * Math.PI).stroke({ width: 3, color: 0x312e81 });
        break;
      case "completed":
        m.moveTo(-10, 9).arc(0, 9, 10, 0, Math.PI).fill(0x312e81);
        m.ellipse(0, 16, 4, 3).fill(0xf472b6);
        break;
      case "error":
        m.arc(0, 20, 8, 1.2 * Math.PI, 1.8 * Math.PI).stroke({ width: 3, color: 0x312e81 });
        break;
      case "idle":
      default:
        m.arc(0, 10, 7, 0.15 * Math.PI, 0.85 * Math.PI).stroke({ width: 3, color: 0x312e81 });
        break;
    }
  }

  private setCheeks(color: number) {
    this.cheeks.clear();
    this.cheeks.circle(-22, 6, 5).fill({ color, alpha: 0.45 });
    this.cheeks.circle(22, 6, 5).fill({ color, alpha: 0.45 });
  }

  /** Public entry: safe to call before init() resolves (stores until ready). */
  setStatus(status: TaskStatus, running: boolean) {
    this.pending = { status, running };
    if (this.ready && !this.destroyed) this.applyStatus(status, running);
  }

  private applyStatus(status: TaskStatus, running: boolean) {
    this.status = status;
    this.dimmed = !running;
    const color = STATUS_COLOR[status];

    this.setMouth(status);
    this.setCheeks(color);

    // Always reset body transform fully to avoid stale values from previous
    // animation states (e.g. completed sets body.x = ±6, error sets body.x).
    this.body.x = 0;
    this.body.y = 0;
    this.body.rotation = 0;
    this.leftArm.rotation = 0;
    this.rightArm.rotation = 0;

    this.app.ticker.maxFPS = !running || status === "idle" ? 10 : 30;
    this.root.alpha = running ? 1 : 0.55;
  }

  private update(dt: number) {
    if (!this.ready || this.destroyed) return;
    this.t += dt;
    const t = this.t;

    // Blink.
    this.blink = Math.max(0, this.blink - dt * 8);
    this.nextBlink -= dt;
    if (this.nextBlink <= 0) {
      this.blink = 1;
      this.nextBlink = 2.5 + Math.random() * 3.5;
    }
    const eyeScale = this.blink > 0.5 ? lerp(1, 0.1, (this.blink - 0.5) * 2) : 1;
    this.eyeL.scale.y = eyeScale;
    this.eyeR.scale.y = eyeScale;

    // Soft status glow that breathes behind the body.
    const pulse = 0.5 + 0.5 * Math.sin(t * 2);
    const color = STATUS_COLOR[this.status];
    this.glow.clear();
    this.glow
      .circle(0, -2, 46)
      .fill({ color, alpha: this.dimmed ? 0.06 : 0.1 + 0.12 * pulse });

    this.fx.clear();

    switch (this.status) {
      case "running":   this.animateRunning(t);   break;
      case "waiting":   this.animateWaiting(t);   break;
      case "completed": this.animateCompleted(t);  break;
      case "error":     this.animateError(t);      break;
      case "idle":
      default:          this.animateIdle(t);       break;
    }
  }

  private animateIdle(t: number) {
    this.body.y = Math.sin(t * 2) * 3;
    this.body.x = 0;
    this.leftArm.rotation  =  0.18 + Math.sin(t * 2) * 0.04;
    this.rightArm.rotation = -0.18 - Math.sin(t * 2) * 0.04;
    this.lookAt(0, Math.sin(t * 2) * 1.5);
  }

  private animateRunning(t: number) {
    // Body bounces slightly; arms hammer alternately.
    this.body.x = 0;
    this.body.y = Math.abs(Math.sin(t * 6)) * 1.5;
    this.leftArm.rotation  =  0.5 + Math.sin(t * 16) * 0.28;
    this.rightArm.rotation = -0.5 - Math.sin(t * 16 + Math.PI) * 0.28;
    this.lookAt(0, 4);
  }

  private animateWaiting(t: number) {
    this.body.x = 0;
    this.body.y = Math.sin(t * 3) * 2;
    this.leftArm.rotation  =  0.18;
    // Right arm raised and waving.
    this.rightArm.rotation = -2.1 + Math.sin(t * 9) * 0.35;
    this.lookAt(0, -2);

    // "!" speech bubble above the head.
    const by = -64 + Math.sin(t * 3) * 2;
    this.fx
      .roundRect(20, by - 12, 26, 24, 8)
      .fill(0xffffff)
      .stroke({ width: 2, color: STATUS_COLOR.waiting });
    this.fx.poly([24, by + 8, 30, by + 8, 26, by + 16]).fill(0xffffff);
    this.fx.rect(31, by - 7, 4, 11).fill(STATUS_COLOR.waiting);
    this.fx.circle(33, by + 8, 2.2).fill(STATUS_COLOR.waiting);
  }

  private animateCompleted(t: number) {
    this.body.x = Math.sin(t * 6) * 6;
    this.body.y = -Math.abs(Math.sin(t * 12)) * 4;
    this.body.rotation = Math.sin(t * 6) * 0.14;
    this.leftArm.rotation  =  0.6 + Math.sin(t * 12) * 0.5;
    this.rightArm.rotation = -0.6 - Math.sin(t * 12 + Math.PI) * 0.5;
    this.lookAt(Math.sin(t * 6) * 2, -2);

    // Floating notes.
    for (let i = 0; i < 3; i++) {
      const phase = (t * 0.9 + i * 0.6) % 1;
      const nx = (i - 1) * 30 + Math.sin((t + i) * 3) * 4;
      const ny = -30 - phase * 46;
      const alpha = 1 - phase;
      this.fx.circle(nx, ny, 3).fill({ color: STATUS_COLOR.completed, alpha });
      this.fx.rect(nx + 2.5, ny - 10, 1.6, 10).fill({ color: STATUS_COLOR.completed, alpha });
    }
  }

  private animateError(t: number) {
    this.body.x = Math.sin(t * 22) * 0.4;
    this.body.y = 4 + Math.sin(t * 20) * 0.4;
    this.leftArm.rotation  =  0.05;
    this.rightArm.rotation = -0.05;
    this.lookAt(0, 3);

    // Tears.
    for (let s = -1; s <= 1; s += 2) {
      const phase = (t * 1.4 + (s + 1) * 0.25) % 1;
      const alpha = 1 - phase * 0.7;
      this.fx.ellipse(s * 14, 2 + phase * 40, 2.4, 3.4).fill({ color: 0x38bdf8, alpha });
    }
  }

  private lookAt(dx: number, dy: number) {
    const cx = Math.max(-2.5, Math.min(2.5, dx));
    const cy = Math.max(-2.5, Math.min(2.5, dy));
    this.leftPupil.position.set(cx, cy);
    this.rightPupil.position.set(cx, cy);
  }
}

function lerp(a: number, b: number, t: number) { return a + (b - a) * t; }
