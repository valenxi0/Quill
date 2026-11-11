// One 30 Hz timer shared by every animated meter. A timer, not
// requestAnimationFrame, so a 240 Hz display doesn't wake the page 240 times
// a second for 30 drawn frames.
import { useSyncExternalStore } from "react";

const TICK_MS = 33;
const listeners = new Set<() => void>();
let timer: number | undefined;

export function onTick(listener: () => void) {
  listeners.add(listener);
  timer ??= window.setInterval(() => listeners.forEach((l) => l()), TICK_MS);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.clearInterval(timer);
      timer = undefined;
    }
  };
}

const subscribeToVisibility = (onChange: () => void) => {
  document.addEventListener("visibilitychange", onChange);
  return () => document.removeEventListener("visibilitychange", onChange);
};

/** Whether the window is visible; animations pause while it isn't. */
export function useDocumentVisible() {
  return useSyncExternalStore(subscribeToVisibility, () => document.visibilityState === "visible");
}
