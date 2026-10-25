import * as React from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings_Serialize } from "../lib/bindings";

const READY = {
  enabled: true,
  ready: true,
  reason: null,
  has_key: true,
  key_required: true,
  provider: "openai",
  protocol: "openai",
  model: "gpt-4o-mini",
  base_url: "https://api.openai.com/v1",
};

const statusMock = vi.fn(async () => READY);
const setKeyMock = vi.fn();
const testMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    llmStatus: () => statusMock(),
    llmSetKey: (key: string) => setKeyMock(key),
    llmTest: () => testMock(),
  },
  queryKeys: { llmStatus: ["llm-status"] },
}));

const { LlmSettings } = await import("./llm-settings");

function settings(overrides: Partial<Settings_Serialize> = {}): Settings_Serialize {
  return {
    vault_path: null,
    recent_vaults: [],
    theme: "system",
    shortcut: "CmdOrCtrl+Shift+Space",
    base_commodity: "CNY",
    base_precision: 2,
    default_funding: "assets:cash",
    default_unclassified: "expenses:unclassified",
    fx_enabled: false,
    max_backups: 200,
    dashboard_layout: null,
    llm_enabled: false,
    llm_provider: "openai",
    llm_protocol: "",
    llm_base_url: "",
    llm_model: "",
    auto_recur: false,
    git_auto_commit: false,
    ...overrides,
  };
}

function renderSettings(initial: Settings_Serialize) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  const updates: Array<[string, unknown]> = [];
  const patchCalls: Array<Partial<Settings_Serialize>> = [];
  const onMessage = vi.fn();
  const onError = vi.fn();
  const onSave = vi.fn(async () => undefined);

  function Harness() {
    const [draft, setDraft] = React.useState(initial);
    return (
      <LlmSettings
        draft={draft}
        persisted={initial}
        update={(key, value) => {
          updates.push([String(key), value]);
          setDraft((current) => ({ ...current, [key]: value }));
        }}
        patch={(partial) => {
          patchCalls.push(partial);
          setDraft((current) => ({ ...current, ...partial }));
        }}
        onSave={onSave}
        onMessage={onMessage}
        onError={onError}
      />
    );
  }

  render(
    <QueryClientProvider client={client}>
      <Harness />
    </QueryClientProvider>,
  );
  return { updates, patchCalls, onMessage, onError, onSave };
}

describe("LlmSettings", () => {
  beforeEach(() => {
    statusMock.mockClear();
    setKeyMock.mockClear();
    testMock.mockClear();
    statusMock.mockImplementation(async () => READY);
  });

  it("never renders the stored key, only that one exists", async () => {
    renderSettings(settings());
    expect(await screen.findByText(/已保存/)).toBeInTheDocument();
    // The key is only ever reachable through the explicit 更换 → input → 保存 flow.
    expect(screen.queryByLabelText("API Key")).toBeNull();
    expect(document.body.textContent).not.toMatch(/sk-/);
  });

  it("saves a key through its own command and clears it again", async () => {
    setKeyMock.mockResolvedValue({});
    const { onMessage } = renderSettings(settings());

    expect(await screen.findByText(/已保存/, {}, { timeout: 3000 })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "更换" }));
    await userEvent.type(await screen.findByLabelText("API Key"), "sk-test-123");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(setKeyMock).toHaveBeenCalledWith("sk-test-123"));
    expect(onMessage).toHaveBeenCalledWith("凭证已保存");

    await userEvent.click(screen.getByRole("button", { name: "清除" }));
    await waitFor(() => expect(setKeyMock).toHaveBeenLastCalledWith(""));
    expect(onMessage).toHaveBeenLastCalledWith("凭证已清除");
  });

  it("switches the provider and clears its overrides in one edit", async () => {
    const { patchCalls } = renderSettings(
      settings({ llm_base_url: "https://old/v1", llm_model: "old-model" }),
    );

    await userEvent.selectOptions(await screen.findByLabelText("服务商"), "deepseek");

    // All four fields travel as one patch: four separate updates would each be
    // spread from the same render-time draft and only the last one would survive.
    expect(patchCalls).toContainEqual({
      llm_provider: "deepseek",
      llm_protocol: "",
      llm_base_url: "",
      llm_model: "",
    });
    expect(await screen.findByLabelText("服务商")).toHaveValue("deepseek");
  });

  it("saves unsaved edits before testing the connection", async () => {
    testMock.mockResolvedValue({ model: "liquid/lfm2.5-1.2b", message: "OK", elapsed_ms: 12 });
    const { onSave } = renderSettings(settings({ llm_model: "gpt-4o-mini" }));

    await userEvent.clear(await screen.findByLabelText("模型"));
    await userEvent.type(screen.getByLabelText("模型"), "liquid/lfm2.5-1.2b");
    expect(screen.getByText("未保存")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "测试连接" }));

    expect(onSave).toHaveBeenCalled();
    expect(await screen.findByText(/liquid\/lfm2.5-1.2b 回应「OK」/)).toBeInTheDocument();
  });

  it("reports the result of a connection test", async () => {
    testMock.mockResolvedValue({ model: "gpt-4o-mini", message: "OK", elapsed_ms: 480 });
    const { onMessage } = renderSettings(settings());

    await userEvent.click(await screen.findByRole("button", { name: "测试连接" }));

    expect(await screen.findByText(/gpt-4o-mini 回应「OK」· 480 ms/)).toBeInTheDocument();
    expect(onMessage).toHaveBeenCalledWith("连接成功");
  });

  it("surfaces a rejected test instead of pretending it worked", async () => {
    testMock.mockRejectedValue(new Error("HTTP 401：invalid api key"));
    const { onError } = renderSettings(settings());

    await userEvent.click(await screen.findByRole("button", { name: "测试连接" }));

    await waitFor(() => expect(onError).toHaveBeenCalledWith("HTTP 401：invalid api key"));
  });
});
