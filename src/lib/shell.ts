/**
 * Which shell is this webview?
 *
 * The capture palette lives in its own always-on-top window. Tauri resolves
 * `index.html#/capture` to an asset URL, and the `#` can end up percent-encoded,
 * so the hash is *not* a reliable signal — the window label is. The hash is kept
 * as a fallback for the mobile shell (capture is a route there) and for the
 * browser preview, where no Tauri runtime exists.
 */

type Internals = {
  metadata?: { currentWindow?: { label?: string } };
};

export function isCaptureWindow(): boolean {
  if (typeof window === "undefined") return false;
  if (window.location.hash.startsWith("#/capture")) return true;
  try {
    const internals = (window as unknown as { __TAURI_INTERNALS__?: Internals })
      .__TAURI_INTERNALS__;
    return internals?.metadata?.currentWindow?.label === "capture";
  } catch {
    return false;
  }
}
