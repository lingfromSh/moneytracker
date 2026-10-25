import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureParseDto_Serialize, SnapshotDto_Serialize } from "../lib/bindings";

const parseMock = vi.fn();
const commitMock = vi.fn();
const ruleMock = vi.fn();
const snapshotMock = vi.fn();
const hideMock = vi.fn();
const focusMainMock = vi.fn();
const llmStatusMock = vi.fn();
const llmDraftMock = vi.fn();

vi.mock("../lib/ipc", () => ({
  api: {
    captureParse: (text: string) => parseMock(text),
    captureCommit: (text: string) => commitMock(text),
    ruleUpsert: (text: string) => ruleMock(text),
    snapshot: () => snapshotMock(),
    llmStatus: () => llmStatusMock(),
    llmDraft: (text: string) => llmDraftMock(text),
  },
  queryKeys: { snapshot: ["snapshot"], llmStatus: ["llm-status"] },
}));

vi.mock("../lib/window", () => ({
  hideCaptureWindow: () => hideMock(),
  focusMain: (search?: string) => focusMainMock(search),
}));

const { CaptureWindow } = await import("./Capture");

function preview(overrides: Partial<CaptureParseDto_Serialize> = {}): CaptureParseDto_Serialize {
  return {
    kind: "transaction",
    canonical_text: "",
    postings: [
      {
        account: "expenses:餐饮",
        amount: null,
        commodity: "CNY",
        quantity: "4.50",
        display: "4.50 CNY",
        price: null,
        price_kind: null,
        assertion: null,
        comment: null,
        virtual_kind: "real",
        tags: [],
      },
      {
        account: "assets:微信",
        amount: null,
        commodity: "CNY",
        quantity: "-4.50",
        display: "-4.50 CNY",
        price: null,
        price_kind: null,
        assertion: null,
        comment: null,
        virtual_kind: "real",
        tags: [],
      },
    ],
    warnings: [],
    errors: [],
    rule_id: null,
    rule_preview: null,
    summary: "expenses:餐饮  →  assets:微信",
    date: "2026-10-25",
    payee: "咖啡",
    amount_display: "4.50 CNY",
    ...overrides,
  };
}

function renderCapture() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: 0 } } });
  return render(
    <QueryClientProvider client={client}>
      <CaptureWindow />
    </QueryClientProvider>,
  );
}

describe("CaptureWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    llmStatusMock.mockResolvedValue({ enabled: false, ready: false, reason: "还没有填写 API Key" });
    snapshotMock.mockResolvedValue({ accounts: [] } as unknown as SnapshotDto_Serialize);
    commitMock.mockResolvedValue({ file: "2026/2026-10-25.ledger" });
  });

  it("previews the expanded postings while typing", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 4.5 @微信");

    await waitFor(() => expect(parseMock).toHaveBeenCalledWith("咖啡 4.5 @微信"));
    expect(await screen.findByText("expenses:餐饮")).toBeInTheDocument();
    expect(screen.getByText("4.50 CNY")).toBeInTheDocument();
  });

  it("commits on Enter and keeps the window open", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    const input = screen.getByLabelText("快录输入");
    await userEvent.type(input, "咖啡 4.5 @微信");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(commitMock).toHaveBeenCalledWith("咖啡 4.5 @微信"));
    expect(hideMock).not.toHaveBeenCalled();
    expect(input).toHaveValue("");
  });

  it("hides the window on Cmd+Enter", async () => {
    parseMock.mockResolvedValue(preview());
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 4.5 @微信");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Meta>}{Enter}{/Meta}");

    await waitFor(() => expect(hideMock).toHaveBeenCalled());
  });

  it("shows parser errors instead of a preview", async () => {
    parseMock.mockResolvedValue(
      preview({
        postings: [],
        errors: [
          {
            severity: "error",
            code: "core.parse.bad_amount",
            message: "金额不合法",
            file: "<capture>",
            line: 1,
            col: 5,
          },
        ],
      }),
    );
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "咖啡 ??");
    expect(await screen.findByText("金额不合法")).toBeInTheDocument();
  });

  it("saves rules through rule_upsert", async () => {
    parseMock.mockResolvedValue(preview({ kind: "rule", postings: [] }));
    ruleMock.mockResolvedValue({ id: "r-abc123" });
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "= desc:星巴克 -> 餐饮=100%");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(ruleMock).toHaveBeenCalled());
    expect(commitMock).not.toHaveBeenCalled();
    expect(await screen.findByText("规则 r-abc123 已保存")).toBeInTheDocument();
  });

  it("sends / searches to the main window", async () => {
    parseMock.mockResolvedValue(preview({ kind: "search", postings: [] }));
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "/acct:餐饮");
    await waitFor(() => expect(parseMock).toHaveBeenCalled());
    await userEvent.keyboard("{Enter}");

    await waitFor(() => expect(focusMainMock).toHaveBeenCalledWith("acct:餐饮"));
  });
});

describe("CaptureWindow natural-language mode", () => {
  const draft = (line: string) => ({
    line,
    payee: "咖啡",
    date: "2026-10-25",
    category: "expenses:food:coffee",
    funding: "assets:wallet:alipay",
    confidence: 0.9,
    notes: "识别为餐饮支出",
    preview: preview(),
    elapsed_ms: 320,
  });

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    snapshotMock.mockResolvedValue({ accounts: [] } as unknown as SnapshotDto_Serialize);
    commitMock.mockResolvedValue({ file: "2026/2026-10-25.ledger" });
    llmStatusMock.mockResolvedValue({
      enabled: true,
      ready: true,
      reason: null,
      has_key: true,
      key_required: true,
      provider: "openai",
      protocol: "openai",
      model: "gpt-4o-mini",
      base_url: "https://api.openai.com/v1",
    });
  });

  it("switches with Tab and commits the drafted line, not the prose", async () => {
    llmDraftMock.mockResolvedValue(
      draft("咖啡 45 @expenses:food:coffee @assets:wallet:alipay"),
    );
    renderCapture();

    const input = screen.getByLabelText("快录输入");
    await userEvent.type(input, "{Tab}");
    expect(screen.getByLabelText("输入模式")).toHaveTextContent("自然语言");
    expect(input).toHaveAttribute("placeholder", "昨天和同事在星巴克花了 45，支付宝付的");

    await userEvent.type(input, "星巴克 45");
    await waitFor(() => expect(llmDraftMock).toHaveBeenCalledWith("星巴克 45"), {
      timeout: 4000,
    });
    expect(await screen.findByText("将写入", {}, { timeout: 4000 })).toBeInTheDocument();
    expect(parseMock).not.toHaveBeenCalled();

    await userEvent.keyboard("{Enter}");
    await waitFor(() =>
      expect(commitMock).toHaveBeenCalledWith(
        "咖啡 45 @expenses:food:coffee @assets:wallet:alipay",
      ),
    );
    expect(hideMock).not.toHaveBeenCalled();
  });

  it("explains the missing configuration instead of silently doing nothing", async () => {
    llmStatusMock.mockResolvedValue({
      enabled: false,
      ready: false,
      reason: "还没有填写 API Key",
      has_key: false,
      key_required: true,
      provider: "openai",
      protocol: "openai",
      model: "gpt-4o-mini",
      base_url: "https://api.openai.com/v1",
    });
    renderCapture();

    await userEvent.type(screen.getByLabelText("快录输入"), "{Tab}");
    expect(await screen.findByText("自然语言模式还没启用。", {}, { timeout: 4000 })).toBeInTheDocument();
    expect(llmDraftMock).not.toHaveBeenCalled();
  });

  it("keeps account completion on Option+Tab", async () => {
    snapshotMock.mockResolvedValue({
      accounts: [{ full: "assets:cash" }, { full: "assets:wallet:alipay" }],
    } as unknown as SnapshotDto_Serialize);
    parseMock.mockResolvedValue(preview());
    renderCapture();

    const input = screen.getByLabelText("快录输入");
    await userEvent.type(input, "咖啡 4.5 @alipay");
    await userEvent.keyboard("{Alt>}{Tab}{/Alt}");

    expect(screen.getByLabelText("输入模式")).toHaveTextContent("Hledger");
    await waitFor(() => expect(input).toHaveValue("咖啡 4.5 @assets:wallet:alipay "));
  });
});
