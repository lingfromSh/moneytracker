import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { Settings_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn } from "../lib/format";
import { Badge, Button, Field, Input, Select, Switch } from "../components/ui";

/**
 * Mirrors `src-tauri/src/llm.rs::PRESETS`. Only used for placeholders and the
 * "reset to defaults" behaviour — the backend always has the final say.
 */
const PRESETS = [
  {
    id: "openai",
    label: "OpenAI",
    protocol: "openai",
    baseUrl: "https://api.openai.com/v1",
    model: "gpt-4o-mini",
    keyRequired: true,
  },
  {
    id: "anthropic",
    label: "Anthropic Claude",
    protocol: "anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    model: "claude-3-5-haiku-latest",
    keyRequired: true,
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    protocol: "openai",
    baseUrl: "https://api.deepseek.com/v1",
    model: "deepseek-chat",
    keyRequired: true,
  },
  {
    id: "ollama",
    label: "Ollama / 本地模型",
    protocol: "openai",
    baseUrl: "http://localhost:11434/v1",
    model: "qwen2.5:7b",
    keyRequired: false,
  },
  {
    id: "custom",
    label: "自定义（OpenAI / Anthropic 兼容）",
    protocol: "openai",
    baseUrl: "",
    model: "",
    keyRequired: false,
  },
] as const;

const preset = (id: string) => PRESETS.find((p) => p.id === id) ?? PRESETS[0];

/**
 * 设置 → AI 记一笔: provider, endpoint, model and the API key.
 *
 * The key is not part of `Settings` — it is stored by its own backend command so
 * it never travels through the settings document and never comes back to the UI.
 */
const LLM_FIELDS = ["llm_enabled", "llm_provider", "llm_protocol", "llm_base_url", "llm_model"] as const;

export function LlmSettings({
  draft,
  persisted,
  update,
  patch,
  onSave,
  onMessage,
  onError,
}: {
  draft: Settings_Serialize;
  persisted?: Settings_Serialize;
  update: <K extends keyof Settings_Serialize>(key: K, value: Settings_Serialize[K]) => void;
  patch: (partial: Partial<Settings_Serialize>) => void;
  onSave: () => Promise<unknown>;
  onMessage: (message: string) => void;
  onError: (message: string) => void;
}) {
  const queryClient = useQueryClient();
  const status = useQuery({
    queryKey: queryKeys.llmStatus,
    queryFn: () => api.llmStatus(),
    retry: 0,
  });
  const [editingKey, setEditingKey] = React.useState(false);
  const [keyDraft, setKeyDraft] = React.useState("");
  const [working, setWorking] = React.useState<"key" | "test" | null>(null);
  const [testResult, setTestResult] = React.useState<string | null>(null);

  const current = preset(draft.llm_provider);
  const hasKey = status.data?.has_key ?? false;
  const dirty =
    persisted !== undefined && LLM_FIELDS.some((key) => draft[key] !== persisted[key]);

  /** Persist the field edits before anything is tested, so what you see is what runs. */
  const persist = async () => {
    if (!dirty) return;
    await onSave();
    await queryClient.invalidateQueries({ queryKey: queryKeys.llmStatus });
  };

  const saveKey = async (value: string) => {
    setWorking("key");
    try {
      await api.llmSetKey(value);
      setKeyDraft("");
      setEditingKey(false);
      setTestResult(null);
      await queryClient.invalidateQueries({ queryKey: queryKeys.llmStatus });
      onMessage(value.trim() ? "凭证已保存" : "凭证已清除");
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setWorking(null);
    }
  };

  const runTest = async () => {
    setWorking("test");
    setTestResult(null);
    try {
      await persist();
      const result = await api.llmTest();
      setTestResult(`${result.model} 回应「${result.message}」· ${Math.round(result.elapsed_ms ?? 0)} ms`);
      onMessage("连接成功");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setTestResult(null);
      onError(message);
    } finally {
      setWorking(null);
    }
  };

  return (
    <div className="space-y-3 p-4">
      <div className="flex items-start justify-between gap-3 rounded-lg border border-[var(--color-line)] px-3 py-2">
        <div>
          <div className="text-sm">启用自然语言记一笔</div>
          <div className="text-[11px] text-[var(--color-muted)]">
            快录窗口里按 <kbd className="font-mono">Tab</kbd>{" "}
            切换「Hledger 语法 / 自然语言」。那句话会发送给你配置的服务商，账本本身只写本地。
          </div>
        </div>
        <Switch
          checked={draft.llm_enabled}
          onChange={(next) => update("llm_enabled", next)}
          label="启用自然语言记一笔"
        />
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="服务商">
          <Select
            className="w-full"
            value={draft.llm_provider}
            onChange={(e) => {
              const next = preset(e.target.value);
              // Blank means “use the preset default”, so switching clears overrides.
              patch({
                llm_provider: next.id,
                llm_protocol: next.id === "custom" ? next.protocol : "",
                llm_base_url: "",
                llm_model: "",
              });
              setTestResult(null);
            }}
            aria-label="服务商"
          >
            {PRESETS.map((item) => (
              <option key={item.id} value={item.id}>
                {item.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="模型" hint={`默认 ${current.model || "自定义"}`}>
          <Input
            value={draft.llm_model}
            onChange={(e) => update("llm_model", e.target.value)}
            placeholder={current.model || "gpt-4o-mini"}
            className="font-mono"
            aria-label="模型"
          />
        </Field>
        <Field
          label="Base URL"
          hint={`留空使用默认 ${current.baseUrl || "（必填）"}；可以填自建网关或代理`}
        >
          <Input
            value={draft.llm_base_url}
            onChange={(e) => update("llm_base_url", e.target.value)}
            placeholder={current.baseUrl || "https://your-gateway/v1"}
            className="font-mono"
            aria-label="Base URL"
          />
        </Field>
        <Field label="协议" hint="自定义端点需要指定它兼容哪一种请求格式">
          <Select
            className="w-full"
            value={draft.llm_protocol || current.protocol}
            onChange={(e) => update("llm_protocol", e.target.value)}
            disabled={draft.llm_provider !== "custom"}
            aria-label="协议"
          >
            <option value="openai">OpenAI chat/completions</option>
            <option value="anthropic">Anthropic messages</option>
          </Select>
        </Field>
      </div>

      <Field
        label="API Key"
        hint={
          current.keyRequired
            ? "保存在用户目录下的 llm.json（权限 0600），不会写进账本或设置文件"
            : "该服务商通常不需要 Key；如需鉴权再填写"
        }
      >
        <div className="flex items-center gap-2">
          {editingKey ? (
            <>
              <Input
                autoFocus
                type="password"
                value={keyDraft}
                onChange={(e) => setKeyDraft(e.target.value)}
                placeholder="sk-…"
                className="font-mono"
                aria-label="API Key"
                onKeyDown={(e) => {
                  if (e.key === "Enter") void saveKey(keyDraft);
                  if (e.key === "Escape") {
                    setEditingKey(false);
                    setKeyDraft("");
                  }
                }}
              />
              <Button
                variant="primary"
                aria-label="保存"
                onClick={() => void saveKey(keyDraft)}
                disabled={working === "key"}
              >
                保存
              </Button>
              <Button
                variant="ghost"
                aria-label="取消"
                onClick={() => {
                  setEditingKey(false);
                  setKeyDraft("");
                }}
              >
                取消
              </Button>
            </>
          ) : (
            <>
              <div className="flex-1 rounded-lg border border-[var(--color-line)] px-3 py-2 font-mono text-sm text-[var(--color-muted)]">
                {hasKey ? "••••••••••••••••（已保存）" : "未设置"}
              </div>
              <Button aria-label={hasKey ? "更换" : "填写"} onClick={() => setEditingKey(true)}>
                {hasKey ? "更换" : "填写"}
              </Button>
              {hasKey ? (
                <Button
                  variant="ghost"
                  aria-label="清除"
                  onClick={() => void saveKey("")}
                  disabled={working === "key"}
                >
                  清除
                </Button>
              ) : null}
            </>
          )}
        </div>
      </Field>

      <div className="flex flex-wrap items-center gap-2">
        <Button aria-label="测试连接" onClick={() => void runTest()} disabled={working === "test"}>
          {working === "test" ? "测试中…" : "测试连接"}
        </Button>
        <Badge tone={status.data?.ready && !dirty ? "accent" : "warn"}>
          {dirty ? "未保存" : status.data?.ready ? "已就绪" : "未就绪"}
        </Badge>
        <span className="text-[11px] text-[var(--color-muted)]">
          {status.data?.protocol === "anthropic" ? "Anthropic 协议" : "OpenAI 协议"} ·{" "}
          {status.data?.model || current.model || "未指定模型"}
        </span>
        {dirty ? (
          <Button
            aria-label="保存 AI 设置"
            onClick={() => {
              void persist()
                .then(() => onMessage("已保存 AI 设置"))
                .catch((err: unknown) => onError(err instanceof Error ? err.message : String(err)));
            }}
          >
            保存 AI 设置
          </Button>
        ) : null}
        {!dirty && status.data?.reason ? (
          <span className="text-[11px] text-amber-600 dark:text-amber-400">{status.data.reason}</span>
        ) : null}
      </div>

      {testResult ? (
        <div className={cn("rounded-lg border border-[var(--color-line)] px-3 py-2 text-[11px]")}>
          {testResult}
        </div>
      ) : null}
    </div>
  );
}
