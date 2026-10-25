import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const syncConfig = vi.fn();
const syncSave = vi.fn();
const syncSetKeys = vi.fn();
const syncTest = vi.fn();
const syncPreview = vi.fn();
const syncRun = vi.fn();
vi.mock("../lib/ipc", () => ({
  api: {
    syncConfig: (...args: unknown[]) => syncConfig(...args),
    syncSave: (...args: unknown[]) => syncSave(...args),
    syncSetKeys: (...args: unknown[]) => syncSetKeys(...args),
    syncTest: (...args: unknown[]) => syncTest(...args),
    syncPreview: (...args: unknown[]) => syncPreview(...args),
    syncRun: (...args: unknown[]) => syncRun(...args),
  },
}));

const { SyncSettings } = await import("./sync");

const config = (over: Record<string, unknown> = {}) => ({
  enabled: true,
  endpoint: "https://s3.example.com:9000",
  region: "us-east-1",
  bucket: "ledger",
  prefix: "moneytracker",
  path_style: true,
  auto: false,
  device: "Mac",
  has_keys: true,
  access_key_hint: "AKIA…X1Z",
  configured: true,
  last_sync: "2026-11-20T02:46:00Z",
  has_ledger: true,
  ...over,
});

const conflict = (rel: string) => ({
  rel,
  local_hash: "aaa",
  remote_hash: "bbb",
  base_hash: "ccc",
  local_mtime: "2026-11-20 10:30",
  remote_mtime: "2026-11-19 22:10",
  local_bytes: 2100,
  remote_bytes: 1980,
});

function setup(props: Partial<{ onMessage: (m: string | null) => void; onError: (m: string | null) => void }> = {}) {
  const onMessage = props.onMessage ?? vi.fn();
  const onError = props.onError ?? vi.fn();
  render(<SyncSettings onMessage={onMessage} onError={onError} />);
  return { onMessage, onError };
}

beforeEach(() => {
  vi.clearAllMocks();
  syncConfig.mockResolvedValue(config());
  syncSave.mockResolvedValue(config());
  syncSetKeys.mockResolvedValue(config());
  syncTest.mockResolvedValue("连接正常：bucket 里已有 12 个文件");
  syncPreview.mockResolvedValue({
    uploads: ["2026/2026-11.ledger"],
    downloads: [],
    delete_remote: [],
    delete_local: [],
    conflicts: [],
    files_local: 12,
    files_remote: 12,
  });
  syncRun.mockResolvedValue({
    pushed: ["2026/2026-11.ledger"],
    pulled: [],
    removed_local: [],
    removed_remote: [],
    conflicts: [],
    bytes_up: 2048,
    bytes_down: 0,
    last_sync: "2026-11-20T03:00:00Z",
    complete: true,
    summary: "上传 1 个文件",
  });
});

describe("SyncSettings", () => {
  it("shows the last sync time and the stored key hint", async () => {
    setup();
    expect(await screen.findByText(/上次同步/)).toBeTruthy();
    expect(screen.getAllByPlaceholderText("留空表示不修改")).toHaveLength(2);
    expect(screen.getByText("已保存 AKIA…X1Z")).toBeTruthy();
  });

  it("tests the connection and reports what the bucket holds", async () => {
    const { onMessage } = setup();
    await screen.findByText(/上次同步/);
    await userEvent.click(screen.getByRole("button", { name: /测试连接/ }));
    await waitFor(() => expect(syncTest).toHaveBeenCalled());
    await waitFor(() =>
      expect(onMessage).toHaveBeenCalledWith("连接正常：bucket 里已有 12 个文件"),
    );
  });

  it("previews the difference without writing", async () => {
    const { onMessage } = setup();
    await screen.findByText(/上次同步/);
    await userEvent.click(screen.getByRole("button", { name: /预览差异/ }));
    await waitFor(() => expect(syncPreview).toHaveBeenCalled());
    await waitFor(() => expect(onMessage).toHaveBeenCalledWith("上传 1 · 下载 0"));
  });

  it("stops on a conflict and lets the user pick a side", async () => {
    syncRun
      .mockResolvedValueOnce({
        pushed: [],
        pulled: [],
        removed_local: [],
        removed_remote: [],
        conflicts: [conflict("2026/2026-11.ledger")],
        bytes_up: 0,
        bytes_down: 0,
        last_sync: null,
        complete: false,
        summary: "1 个文件两边都改过，需要你来决定",
      })
      .mockResolvedValueOnce({
        pushed: [],
        pulled: ["2026/2026-11.ledger"],
        removed_local: [],
        removed_remote: [],
        conflicts: [],
        bytes_up: 0,
        bytes_down: 2048,
        last_sync: "2026-11-20T03:10:00Z",
        complete: true,
        summary: "下载 1 个文件",
      });
    const { onMessage } = setup();
    await screen.findByText(/上次同步/);

    await userEvent.click(screen.getByRole("button", { name: /立即同步/ }));
    expect(await screen.findByText("选择保留哪一边")).toBeTruthy();
    expect(screen.getByText("2026/2026-11.ledger")).toBeTruthy();
    expect(onMessage).toHaveBeenCalledWith("有文件两边都改过，请选择保留哪一边");

    await userEvent.click(screen.getByRole("button", { name: "保留本机" }));
    await userEvent.click(screen.getByRole("button", { name: "应用选择" }));

    await waitFor(() =>
      expect(syncRun).toHaveBeenLastCalledWith([{ rel: "2026/2026-11.ledger", side: "local" }]),
    );
    await waitFor(() => expect(onMessage).toHaveBeenCalledWith("下载 1 个文件"));
  });

  it("is inert before the switch is on", async () => {
    syncConfig.mockResolvedValue(config({ enabled: false, configured: false }));
    setup();
    expect(await screen.findByText("未开启")).toBeTruthy();
    const button = screen.getByRole("button", { name: /立即同步/ }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
