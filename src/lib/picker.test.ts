import { beforeEach, describe, expect, it, vi } from "vitest";

const openMock = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...args: unknown[]) => openMock(...args) }));

const state = { mobile: false };
vi.mock("./platform", () => ({
  get isMobile() {
    return state.mobile;
  },
}));

/** `canPickPaths` 是模块加载时算出来的，所以每个场景都要重新 import 一次。 */
async function load() {
  vi.resetModules();
  return import("./picker");
}

describe("picker", () => {
  beforeEach(() => {
    openMock.mockReset();
    state.mobile = false;
  });

  it("desktop asks the system dialog for one directory", async () => {
    const { canPickPaths, pickDirectory } = await load();
    openMock.mockResolvedValue("/Users/me/ledger");

    expect(canPickPaths).toBe(true);
    await expect(pickDirectory("选择目录")).resolves.toBe("/Users/me/ledger");
    expect(openMock).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "选择目录",
    });
  });

  it("desktop treats a cancelled dialog as “nothing picked”", async () => {
    const { pickDirectory } = await load();
    openMock.mockResolvedValue(null);
    await expect(pickDirectory("选择目录")).resolves.toBeNull();
  });

  it("desktop asks for a file with the given extensions", async () => {
    const { pickFile } = await load();
    openMock.mockResolvedValue("/tmp/账单.csv");
    await expect(pickFile(["csv", "txt"])).resolves.toBe("/tmp/账单.csv");
    expect(openMock).toHaveBeenCalledWith({
      multiple: false,
      directory: false,
      filters: [{ name: "文件", extensions: ["csv", "txt"] }],
    });
  });

  it("phones never reach the system picker, and say so through canPickPaths", async () => {
    state.mobile = true;
    const { canPickPaths, pickDirectory, pickFile } = await load();

    expect(canPickPaths).toBe(false);
    await expect(pickDirectory("选择目录")).resolves.toBeNull();
    await expect(pickFile(["csv"])).resolves.toBeNull();
    expect(openMock).not.toHaveBeenCalled();
  });
});
