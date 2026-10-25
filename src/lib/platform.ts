/**
 * Which shell are we rendering in?
 *
 * Tauri injects `__TAURI_INTERNALS__` into every webview, phone or desktop, so
 * the platform has to come from the user agent. iPadOS 13+ reports a desktop UA,
 * hence the touch-point + viewport-width fallback — it also keeps the wide
 * layout in the `vite dev` browser preview on a small window.
 */
export const isMobile = (() => {
  if (typeof navigator === "undefined") return false;
  if (/Android|iPhone|iPod/i.test(navigator.userAgent)) return true;
  return (
    typeof window !== "undefined" &&
    navigator.maxTouchPoints > 1 &&
    window.innerWidth < 820
  );
})();
