/**
 * Thin wrappers over the Tauri window API.
 *
 * Everything is guarded: the same bundle also runs in a plain browser during
 * `vite dev` and in unit tests, where no Tauri runtime exists.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isMobile } from "./platform";

export const PENDING_SEARCH = "moneytracker.pendingSearch";

export async function hideCaptureWindow(): Promise<void> {
  // On a phone the capture sheet is a route inside the only window — hiding it
  // would hide the whole app.
  if (isMobile) return;
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not running inside Tauri */
  }
}

export async function hideWindow(): Promise<void> {
  if (isMobile) return;
  try {
    await getCurrentWindow().hide();
  } catch {
    /* not running inside Tauri */
  }
}

/** Bring the main window forward, optionally handing it a search query. */
export async function focusMain(search?: string): Promise<void> {
  if (search) window.localStorage.setItem(PENDING_SEARCH, search);
  if (isMobile) return;
  try {
    const main = await WebviewWindow.getByLabel("main");
    await main?.show();
    await main?.setFocus();
    await hideCaptureWindow();
  } catch {
    /* not running inside Tauri */
  }
}
