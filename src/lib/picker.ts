import { open } from "@tauri-apps/plugin-dialog";
import { isMobile } from "./platform";

/**
 * 所有「选一个路径」的入口都从这里走。
 *
 * 手机上没有文件夹选择器：Android 的 SAF 只能选文件，而且返回的是 `content://`
 * 地址，Rust 侧的 `std::fs` 打不开。所以手机上既不弹选择器，也不假装成功 —— 调用方
 * 用 `canPickPaths` 决定是给按钮还是给一句说明，函数本身返回 `null`。
 */
export const canPickPaths = !isMobile;

export const MOBILE_DIR_HINT =
  "手机上没有文件夹选择器，账本放在 App 私有目录里；桌面端打开同步后，这里会自动出现账本。";
export const MOBILE_FILE_HINT =
  "手机上选中的文件是 content:// 地址，App 读不到内容。请在桌面上导入对账单，账本会随同步到达手机。";

export async function pickDirectory(title: string): Promise<string | null> {
  if (!canPickPaths) return null;
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

export async function pickFile(extensions: string[]): Promise<string | null> {
  if (!canPickPaths) return null;
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "文件", extensions }],
  });
  return typeof picked === "string" ? picked : null;
}
