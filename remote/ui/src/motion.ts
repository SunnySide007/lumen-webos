// SPDX-License-Identifier: GPL-3.0-or-later
//
// Motion: springs, the rim light that follows the pointer, tilt toward the
// pointer or the phone, and View Transitions between screens. Everything here
// moves only in answer to something (a pointer, the phone moving, an event
// from the TV) and stops when that stops: a settled page draws nothing
// (look-and-feel.md, principle 4). With prefers-reduced-motion it all stays
// still.
import { SPRINGS, springEasing } from "./spring.ts";
import { Store } from "./store.ts";

const reducedQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

/** Whether the system asks for reduced motion; follows changes. */
export const reducedMotion = new Store<boolean>(reducedQuery.matches);
reducedQuery.addEventListener("change", (e) => reducedMotion.set(e.matches));

const root = document.documentElement;

/** Sets --spring, --spring-ms and the soft pair as CSS custom properties.
 * Browsers without linear() keep the cubic-bezier stand-ins in the CSS. */
export function installSprings(): void {
  if (!CSS.supports("transition-timing-function", "linear(0, 1)")) return;
  for (const [name, spring] of Object.entries(SPRINGS)) {
    const { easing, ms } = springEasing(spring);
    root.style.setProperty(`--${name}`, easing);
    root.style.setProperty(`--${name}-ms`, `${ms}ms`);
  }
}

/** Marks the page when the engine refracts through an SVG filter in
 * backdrop-filter (Chromium: WebView2 and Chrome on the Pixel). */
export function detectRefraction(): void {
  const brands = (navigator as Navigator & { userAgentData?: { brands: { brand: string }[] } }).userAgentData
    ?.brands;
  if (brands?.some((b) => b.brand === "Chromium") && CSS.supports("backdrop-filter", "url(#refract)")) {
    root.classList.add("can-refract");
  }
}

type ViewTransitionDoc = Document & {
  startViewTransition?: (update: () => void) => { ready: Promise<void> };
};

/** Runs a DOM change as a View Transition, so named parts morph from their
 * old place to the new; with reduced motion, or no support, just runs it. */
export function morph(update: () => void): void {
  const doc = document as ViewTransitionDoc;
  if (!doc.startViewTransition || reducedMotion.get() || document.hidden) {
    update();
    return;
  }
  // A transition cut short by the next one still runs its update; only the
  // animation is skipped, which `ready` reports by rejecting.
  doc.startViewTransition(update).ready.catch(() => {});
}

// ---------------------------------------------------------------- rim light

const lit = new Set<HTMLElement>();
let pointer: { x: number; y: number } | null = null;
let rimFrame = 0;

function paintRims(): void {
  rimFrame = 0;
  if (!pointer) return;
  for (const el of lit) {
    const r = el.getBoundingClientRect();
    const x = pointer.x - r.left;
    const y = pointer.y - r.top;
    // Full strength over the panel, fading out within 160 px of its edge.
    const dx = Math.max(0, -x, x - r.width);
    const dy = Math.max(0, -y, y - r.height);
    const near = Math.max(0, 1 - Math.hypot(dx, dy) / 160);
    el.style.setProperty("--rim-x", `${x.toFixed(0)}px`);
    el.style.setProperty("--rim-y", `${y.toFixed(0)}px`);
    el.style.setProperty("--rim-o", near.toFixed(2));
  }
}

window.addEventListener(
  "pointermove",
  (e) => {
    if (e.pointerType === "touch") return;
    pointer = { x: e.clientX, y: e.clientY };
    if (!rimFrame) rimFrame = requestAnimationFrame(paintRims);
  },
  { passive: true },
);

/** Lets a glass panel's rim catch the pointer's light; returns the undo. */
export function rimLight(el: HTMLElement): () => void {
  lit.add(el);
  return () => lit.delete(el);
}

// ---------------------------------------------------------------- tilt

/** Leans `el` toward the pointer by up to `maxDeg`, and springs it back when
 * the pointer leaves. Sets --tilt-x and --tilt-y (degrees) and --glint-x/y
 * (where the light falls, in %); the CSS does the rest. */
export function tilt(el: HTMLElement, maxDeg = 8): void {
  let frame = 0;
  let last: PointerEvent | null = null;
  const paint = () => {
    frame = 0;
    if (!last) return;
    const r = el.getBoundingClientRect();
    const px = (last.clientX - r.left) / r.width - 0.5;
    const py = (last.clientY - r.top) / r.height - 0.5;
    el.style.setProperty("--tilt-x", `${(-py * 2 * maxDeg).toFixed(2)}deg`);
    el.style.setProperty("--tilt-y", `${(px * 2 * maxDeg).toFixed(2)}deg`);
    el.style.setProperty("--glint-x", `${((px + 0.5) * 100).toFixed(1)}%`);
    el.style.setProperty("--glint-y", `${((py + 0.5) * 100).toFixed(1)}%`);
  };
  el.addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch" || reducedMotion.get()) return;
    last = e;
    el.classList.add("tilting");
    if (!frame) frame = requestAnimationFrame(paint);
  });
  el.addEventListener("pointerleave", () => {
    last = null;
    el.classList.remove("tilting");
    for (const p of ["--tilt-x", "--tilt-y"]) el.style.removeProperty(p);
  });
}

// ---------------------------------------------------------------- the phone's motion

type OrientationPermission = { requestPermission?: () => Promise<"granted" | "denied"> };

/** Tilt from the phone's motion sensors: off by default, on only when the
 * person turns it on (and grants permission where the browser asks). */
export const deviceTilt = new Store<"off" | "on" | "denied" | "unsupported">(
  "DeviceOrientationEvent" in window ? "off" : "unsupported",
);

let base: { beta: number; gamma: number } | null = null;
let tiltFrame = 0;
let reading: { beta: number; gamma: number } | null = null;

function onOrientation(e: DeviceOrientationEvent): void {
  if (e.beta === null || e.gamma === null) return;
  reading = { beta: e.beta, gamma: e.gamma };
  base ??= reading;
  if (!tiltFrame) tiltFrame = requestAnimationFrame(paintDeviceTilt);
}

function paintDeviceTilt(): void {
  tiltFrame = 0;
  if (!reading || !base) return;
  // 20 degrees of the phone is the full 6 degrees of lean.
  const clamp = (v: number) => Math.max(-1, Math.min(1, v / 20));
  root.style.setProperty("--dev-x", `${(-clamp(reading.beta - base.beta) * 6).toFixed(2)}deg`);
  root.style.setProperty("--dev-y", `${(clamp(reading.gamma - base.gamma) * 6).toFixed(2)}deg`);
}

export async function enableDeviceTilt(): Promise<void> {
  const ctor = (window as unknown as { DeviceOrientationEvent?: OrientationPermission }).DeviceOrientationEvent;
  if (!ctor) return deviceTilt.set("unsupported");
  if (ctor.requestPermission) {
    try {
      if ((await ctor.requestPermission()) !== "granted") return deviceTilt.set("denied");
    } catch {
      return deviceTilt.set("denied");
    }
  }
  base = null;
  window.addEventListener("deviceorientation", onOrientation);
  deviceTilt.set("on");
}

export function disableDeviceTilt(): void {
  window.removeEventListener("deviceorientation", onOrientation);
  root.style.removeProperty("--dev-x");
  root.style.removeProperty("--dev-y");
  base = reading = null;
  deviceTilt.set("off");
}

reducedMotion.subscribe((reduced) => {
  if (reduced && deviceTilt.get() === "on") disableDeviceTilt();
});
