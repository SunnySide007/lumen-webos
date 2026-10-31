// SPDX-License-Identifier: GPL-3.0-or-later
//
// The UI's whole view layer: build elements once, then change the few nodes
// a state change touches. No framework: the page is served to a phone over
// Wi-Fi, and a screen here has a few dozen elements.

type Child = Node | string | number | null | undefined | false;
type Handler = (e: never) => void;
type Props = Record<string, string | number | boolean | null | undefined | Handler | Record<string, string>>;

/** Creates an element. Props: `class`, `text`, `vars` (CSS custom
 * properties, without the leading --), `on<event>` handlers, and attributes,
 * where true is present and false, null or undefined absent. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props | null = null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (props) apply(el, props);
  for (const c of children) if (c !== null && c !== undefined && c !== false) el.append(c instanceof Node ? c : String(c));
  return el;
}

function apply(el: HTMLElement, props: Props): void {
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null || v === false) continue;
    if (typeof v === "function") el.addEventListener(k.replace(/^on/, ""), v as EventListener);
    else if (typeof v === "object") setVars(el, v);
    else if (k === "class") el.className = String(v);
    else if (k === "text") el.textContent = String(v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
}

export function setVars(el: HTMLElement, vars: Record<string, string>): void {
  for (const [name, value] of Object.entries(vars)) el.style.setProperty(`--${name}`, value);
}

/** Sets text only when it differs, so an unchanged node isn't touched. */
export function setText(el: Node, text: string): void {
  if (el.textContent !== text) el.textContent = text;
}

/** Adds `cls` for `ms`, restarting it if it is already there: a one-off
 * flash, such as a pressed key or a number that changed. */
export function flash(el: Element, cls: string, ms: number): void {
  el.classList.remove(cls);
  // Reading layout restarts the CSS animation on the next add.
  void (el as HTMLElement).offsetWidth;
  el.classList.add(cls);
  const timers = flashTimers.get(el) ?? new Map<string, ReturnType<typeof setTimeout>>();
  clearTimeout(timers.get(cls));
  timers.set(cls, setTimeout(() => el.classList.remove(cls), ms));
  flashTimers.set(el, timers);
}
const flashTimers = new WeakMap<Element, Map<string, ReturnType<typeof setTimeout>>>();

/** Runs `fn` on a press, and again while held: after 400 ms, every 120 ms,
 * the shell's own repeat (look-and-feel.md, target 7). Keyboard activation
 * (Enter or Space on the focused button) presses once. */
export function holdRepeat(button: HTMLButtonElement, fn: () => void): void {
  let first: ReturnType<typeof setTimeout> | null = null;
  let every: ReturnType<typeof setInterval> | null = null;
  const stop = () => {
    if (first) clearTimeout(first);
    if (every) clearInterval(every);
    first = every = null;
  };
  button.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || button.disabled) return;
    stop();
    fn();
    first = setTimeout(() => (every = setInterval(fn, 120)), 400);
  });
  for (const type of ["pointerup", "pointerleave", "pointercancel", "blur"]) button.addEventListener(type, stop);
  button.addEventListener("click", (e) => {
    if (e.detail === 0) fn();
  });
  button.addEventListener("contextmenu", (e) => e.preventDefault());
}
