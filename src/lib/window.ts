/**
 * Thin wrappers over the Tauri window API.
 *
 * Everything is guarded: the same bundle also runs in a plain browser during
 * `vite dev` and in unit tests, where no Tauri runtime exists.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

export const PENDING_SEARCH = "moneytracker.pendingSearch";

export async function hideCaptureWindow(): Promise<void> {
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not running inside Tauri */
  }
}

export async function hideWindow(): Promise<void> {
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not running inside Tauri */
  }
}

/** Bring the main window forward, optionally handing it a search query. */
export async function focusMain(search?: string): Promise<void> {
  try {
    if (search) window.localStorage.setItem(PENDING_SEARCH, search);
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await hideCaptureWindow();
  } catch {
    /* not running inside Tauri */
  }
}
