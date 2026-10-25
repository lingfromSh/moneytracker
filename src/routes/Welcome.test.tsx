import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const vaultDefaultDir = vi.fn(async () => "/data/user/0/dev.moneytracker.app/files/ledger");
const vaultCreate = vi.fn(async () => ({ info: { path: "/vault" } }));
const vaultOpen = vi.fn(async () => ({ info: { path: "/vault" } }));
const createSampleVault = vi.fn(async () => ({ info: { path: "/vault" } }));
const settings = vi.fn(async () => ({ shortcut: "CmdOrCtrl+Shift+Space" }));

vi.mock("../lib/ipc", () => ({
  api: {
    vaultDefaultDir: () => vaultDefaultDir(),
    vaultCreate: (...args: unknown[]) => vaultCreate(...(args as [])),
    vaultOpen: (...args: unknown[]) => vaultOpen(...(args as [])),
    createSampleVault: () => createSampleVault(),
    settings: () => settings(),
  },
  queryKeys: { settings: ["settings"] },
}));

const state = { mobile: false };
vi.mock("../lib/platform", () => ({
  get isMobile() {
    return state.mobile;
  },
}));

/** `canPickPaths` is derived at module load, so every scenario imports fresh. */
async function show(mobile: boolean) {
  state.mobile = mobile;
  vi.resetModules();
  const { Welcome } = await import("./Welcome");
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <Welcome />
    </QueryClientProvider>,
  );
}

describe("Welcome", () => {
  beforeEach(() => {
    vaultDefaultDir.mockClear();
  });

  it("offers the folder picker on the desktop and leaves the path empty", async () => {
    await show(false);
    expect(screen.getAllByRole("button", { name: "选择" }).length).toBeGreaterThan(0);
    expect(screen.getByPlaceholderText("~/Documents/ledger")).toHaveValue("");
    expect(vaultDefaultDir).not.toHaveBeenCalled();
  });

  it("fills in the app-private directory on a phone, where no picker exists", async () => {
    await show(true);

    expect(screen.queryByRole("button", { name: "选择" })).toBeNull();
    expect(screen.queryByRole("button", { name: "选择目录" })).toBeNull();
    await waitFor(() =>
      expect(screen.getByPlaceholderText("~/Documents/ledger")).toHaveValue(
        "/data/user/0/dev.moneytracker.app/files/ledger",
      ),
    );
    expect(screen.getByText(/手机上没有文件夹选择器/)).toBeTruthy();
  });
});
