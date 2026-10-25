import { api } from "./ipc";
import { isMobile } from "./platform";

/**
 * 打开快录。桌面端是独立的浮窗（全局快捷键把同一个窗口叫出来），手机上只有一个
 * 窗口，所以「打开快录」就是把页面切到 #/capture。
 */
export function startCapture(): void {
  if (isMobile) {
    window.location.hash = "#/capture";
    return;
  }
  void api.showCapture();
}
