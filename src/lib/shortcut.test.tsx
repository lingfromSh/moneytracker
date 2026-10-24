import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_ACCELERATOR, formatAccelerator, useShortcutLabel } from "./shortcut";

const settingsMock = vi.fn();

vi.mock("./ipc", () => ({
  api: { settings: () => settingsMock() },
  queryKeys: { settings: ["settings"] },
}));

function wrapper({ children }: { children: React.ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("shortcut labels", () => {
  it("formats accelerators with macOS symbols", () => {
    expect(formatAccelerator(DEFAULT_ACCELERATOR)).toBe("⌘ ⇧ Space");
    expect(formatAccelerator("Ctrl+Alt+KeyK")).toBe("⌃ ⌥ K");
    expect(formatAccelerator("CmdOrCtrl+Shift+ArrowUp")).toBe("⌘ ⇧ → Up");
    expect(formatAccelerator("")).toBe("—");
  });

  it("follows the configured shortcut", async () => {
    settingsMock.mockResolvedValue({ shortcut: "CmdOrCtrl+Shift+KeyJ" });
    const { result } = renderHook(() => useShortcutLabel(), { wrapper });
    await waitFor(() => expect(result.current).toBe("⌘ ⇧ J"));
  });

  it("falls back to the default when settings are unavailable", async () => {
    settingsMock.mockRejectedValue(new Error("no vault"));
    const { result } = renderHook(() => useShortcutLabel(), { wrapper });
    await waitFor(() => expect(result.current).toBe("⌘ ⇧ Space"));
  });
});
