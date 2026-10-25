import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import type { Settings_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn } from "../lib/format";
import { ShortcutInput } from "../components/shortcut-input";
import { LlmSettings } from "../components/llm-settings";
import { Automation } from "../components/automation";
import { SyncSettings } from "../components/sync";
import { Badge, Button, Card, CardHeader, EmptyState, Field, Input, Select, Switch } from "../components/ui";

const THEMES = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

export function applyTheme(theme: string): void {
  const root = document.documentElement;
  const dark =
    theme === "dark" ||
    (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  root.classList.toggle("dark", dark);
}

export function SettingsView() {
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: queryKeys.settings, queryFn: () => api.settings() });
  const [draft, setDraft] = React.useState<Settings_Serialize | null>(null);
  const [message, setMessage] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const audit = useQuery({ queryKey: queryKeys.audit, queryFn: () => api.audit(30) });
  const version = useQuery({ queryKey: ["version"], queryFn: () => api.version() });
  const shortcut = useQuery({
    queryKey: queryKeys.shortcutStatus,
    queryFn: () => api.shortcutStatus(),
    retry: 0,
  });

  React.useEffect(() => {
    if (settings.data) setDraft(settings.data);
  }, [settings.data]);

  React.useEffect(() => {
    if (draft?.theme) applyTheme(draft.theme);
  }, [draft?.theme]);

  const save = useMutation({
    mutationFn: (next: Settings_Serialize) => api.saveSettings(next),
    onSuccess: (saved) => {
      setDraft(saved);
      setMessage("已保存");
      void queryClient.invalidateQueries();
    },
    onError: (err: unknown) => setError(err instanceof Error ? err.message : String(err)),
  });

  const pickVault = async () => {
    try {
      const picked = await open({ directory: true, multiple: false, title: "选择账本目录" });
      if (typeof picked === "string") {
        await api.vaultOpen(picked);
        setMessage(`已打开 ${picked}`);
        void queryClient.invalidateQueries();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  if (settings.isLoading || !draft) return <EmptyState title="正在读取设置…" />;

  const update = <K extends keyof Settings_Serialize>(key: K, value: Settings_Serialize[K]) => {
    // Functional update: several fields often change in one event (switching the
    // LLM provider resets protocol/base URL/model), and a spread of the render-time
    // draft would let the last call overwrite the earlier ones.
    setDraft((current) => (current ? { ...current, [key]: value } : current));
    setMessage(null);
  };

  // Plain functions on purpose: they sit after the loading early-return below, so
  // they must not be hooks (React error #310 otherwise).
  const patch = (partial: Partial<Settings_Serialize>) => {
    setDraft((current) => (current ? { ...current, ...partial } : current));
    setMessage(null);
  };

  const saveNow = () => save.mutateAsync(draft);

  return (
    <div className="flex h-full min-w-0 flex-col overflow-y-auto overflow-x-hidden">
      <div className="flex items-center justify-between border-b border-[var(--color-line)] px-4 py-3">
        <div>
          <div className="text-lg font-semibold">设置</div>
          <div className="text-xs text-[var(--color-muted)]">
            MoneyTracker {version.data ?? ""} · 账本是纯文本，随时可用 hledger 读取
          </div>
        </div>
        <div className="flex items-center gap-2">
          {message ? <span className="text-xs text-[var(--color-accent)]">{message}</span> : null}
          <Button variant="primary" onClick={() => save.mutate(draft)} disabled={save.isPending}>
            保存
          </Button>
        </div>
      </div>

      <div className="grid gap-3 p-4 lg:grid-cols-2">
        <Card>
          <CardHeader title="账本" hint="vault 是一个普通目录，可交给 git / iCloud 同步" />
          <div className="space-y-3 p-4">
            <Field label="当前账本路径">
              <div className="flex gap-2">
                <Input value={draft.vault_path ?? ""} readOnly placeholder="尚未打开账本" />
                <Button variant="subtle" onClick={pickVault}>
                  选择
                </Button>
              </div>
            </Field>
            <div className="flex flex-wrap gap-2">
              {draft.recent_vaults.map((path) => (
                <button
                  key={path}
                  className="rounded-md border border-[var(--color-line)] px-2 py-1 text-[11px] hover:bg-[var(--color-surface-2)]"
                  onClick={() => {
                    void api
                      .vaultOpen(path)
                      .then(() => {
                        setMessage(`已打开 ${path}`);
                        void queryClient.invalidateQueries();
                      })
                      .catch((err: unknown) =>
                        setError(err instanceof Error ? err.message : String(err)),
                      );
                  }}
                >
                  {path}
                </button>
              ))}
            </div>
            <div className="flex gap-2">
              <Button
                variant="subtle"
                onClick={() => {
                  void api.vaultClose().then(() => {
                    setMessage("已关闭账本");
                    void queryClient.invalidateQueries();
                  });
                }}
              >
                关闭账本
              </Button>
              <Button
                variant="subtle"
                onClick={() => {
                  void api
                    .createSampleVault()
                    .then((result) => {
                      setMessage(`已创建样例账本 ${result.info.path}`);
                      void queryClient.invalidateQueries();
                    })
                    .catch((err: unknown) =>
                      setError(err instanceof Error ? err.message : String(err)),
                    );
                }}
              >
                创建样例账本
              </Button>
            </div>
          </div>
        </Card>

        <Card>
          <CardHeader title="记账口径" hint="基准币种决定所有报表的合计口径" />
          <div className="grid grid-cols-2 gap-3 p-4">
            <Field label="基准币种">
              <Input
                value={draft.base_commodity}
                onChange={(e) => update("base_commodity", e.target.value.toUpperCase())}
              />
            </Field>
            <Field label="显示精度">
              <Input
                type="number"
                min={0}
                max={6}
                value={draft.base_precision}
                onChange={(e) => update("base_precision", Number(e.target.value))}
              />
            </Field>
            <Field label="默认资金账户" hint="快录未指定时使用">
              <Input
                value={draft.default_funding}
                onChange={(e) => update("default_funding", e.target.value)}
              />
            </Field>
            <Field label="未分类账户" hint="规则未命中时落这里并进 Inbox">
              <Input
                value={draft.default_unclassified}
                onChange={(e) => update("default_unclassified", e.target.value)}
              />
            </Field>
            <Field label="快照保留数量" hint="<vault>/.bak 下最多保留份数">
              <Input
                type="number"
                min={1}
                max={2000}
                value={draft.max_backups ?? 200}
                onChange={(e) => update("max_backups", Number(e.target.value))}
              />
            </Field>
            <Field label="主题">
              <Select
                value={draft.theme}
                onChange={(e) => update("theme", e.target.value)}
                className="w-full"
              >
                {THEMES.map((theme) => (
                  <option key={theme.value} value={theme.value}>
                    {theme.label}
                  </option>
                ))}
              </Select>
            </Field>
          </div>
        </Card>

        <Card>
          <CardHeader title="快捷键与汇率" hint="系统级快录键，切换后会立即重新注册" />
          <div className="space-y-3 p-4">
            <Field label="全局唤起快录" hint="点击后直接按下组合键，保存后立即生效">
              <ShortcutInput
                value={draft.shortcut}
                onChange={(next) => update("shortcut", next)}
                invalid={shortcut.data ? !shortcut.data.ok : false}
                onRecordingChange={(active) => {
                  // Release the live chord so pressing it cannot open the capture
                  // window mid-recording; restore it when the recorder closes.
                  void api
                    .shortcutRecording(active)
                    .then(() => queryClient.invalidateQueries({ queryKey: queryKeys.shortcutStatus }))
                    .catch(() => {
                      /* the recorder still works; the save path re-reports errors */
                    });
                }}
              />
            </Field>
            {shortcut.data && !shortcut.data.ok && shortcut.data.error ? (
              <div className="rounded-lg border border-[var(--color-danger)] px-3 py-2 text-[11px] text-[var(--color-danger)]">
                {shortcut.data.error}
              </div>
            ) : null}
            <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
              <div>
                <div className="text-sm">启用历史汇率</div>
                <div className="text-[11px] text-[var(--color-muted)]">
                  只读取 <code>prices.ledger</code> 的 P 指令，联网同步由你手动触发
                </div>
              </div>
              <Switch
                checked={draft.fx_enabled}
                onChange={(next) => update("fx_enabled", next)}
                label="启用历史汇率"
              />
            </div>
            <FxSync enabled={draft.fx_enabled} onMessage={setMessage} onError={setError} />
          </div>
        </Card>

        <Card>
          <CardHeader
            title="AI 记一笔"
            hint="可选：用一句自然语言记账，模型只挑字段，写入仍由本地引擎校验"
          />
          <LlmSettings
            draft={draft}
            persisted={settings.data}
            update={update}
            patch={patch}
            onSave={saveNow}
            onMessage={setMessage}
            onError={setError}
          />
        </Card>

        <Card>
          <CardHeader title="外观" hint="看板密度与每周起始日，都会立刻生效" />
          <div className="grid gap-3 p-3 sm:grid-cols-2">
            <Field label="看板密度" hint="紧凑会压缩卡片高度，一屏能看更多">
              <Select
                value={draft.density}
                onChange={(event) => update("density", event.target.value)}
                aria-label="看板密度"
                className="w-full"
              >
                <option value="comfortable">舒适</option>
                <option value="compact">紧凑</option>
              </Select>
            </Field>
            <Field label="每周起始" hint="影响列表里的按周分组">
              <Select
                value={String(draft.week_start ?? 1)}
                onChange={(event) => update("week_start", Number(event.target.value))}
                aria-label="每周起始"
                className="w-full"
              >
                <option value="1">周一</option>
                <option value="0">周日</option>
              </Select>
            </Field>
          </div>
        </Card>

        <Card>
          <CardHeader title="自动化" hint="定期交易、月度报告、git 快照与预算提醒" />
          <div className="p-3">
            <Automation
              draft={draft}
              update={update}
              onMessage={setMessage}
              onError={setError}
            />
          </div>
        </Card>

        <SyncSettings onMessage={setMessage} onError={setError} />

        <Card>
          <CardHeader title="写入审计" hint="每次改写账本都会记录 before/after" />
          <div className="max-h-72 overflow-auto p-2">
            {audit.data && audit.data.length > 0 ? (
              audit.data.map((entry, index) => (
                <div key={index} className="border-b border-[var(--color-line)]/60 px-2 py-1.5 text-[11px] last:border-0">
                  <div className="flex items-center gap-2">
                    <Badge tone="neutral">{entry.op}</Badge>
                    <span className="truncate text-[var(--color-muted)]">{entry.file}</span>
                    <span className="ml-auto text-[var(--color-muted)]">{entry.ts}</span>
                  </div>
                  {entry.after ? (
                    <pre className="mt-1 max-h-16 overflow-hidden whitespace-pre-wrap font-mono text-[10px] text-[var(--color-muted)]">
                      {entry.after.split("\n").slice(0, 3).join("\n")}
                    </pre>
                  ) : null}
                </div>
              ))
            ) : (
              <div className="p-3 text-xs text-[var(--color-muted)]">还没有写入记录。</div>
            )}
          </div>
        </Card>
      </div>

      {error ? (
        <div className={cn("px-4 pb-4 text-xs text-[var(--color-danger)]")}>{error}</div>
      ) : null}
    </div>
  );
}

function FxSync({
  enabled,
  onMessage,
  onError,
}: {
  enabled: boolean;
  onMessage: (message: string) => void;
  onError: (message: string) => void;
}) {
  const queryClient = useQueryClient();
  const missing = useQuery({
    queryKey: queryKeys.fxMissing,
    queryFn: () => api.fxMissing(),
    enabled,
    retry: 0,
  });

  const sync = useMutation({
    mutationFn: async () => {
      const rows: { date: string; from: string; to: string; rate: string }[] = [];
      const seen = new Set<string>();
      for (const item of missing.data ?? []) {
        const key = `${item.date}|${item.commodity}`;
        if (seen.has(key)) continue;
        seen.add(key);
        const url = `https://api.frankfurter.app/${item.date}?from=${item.commodity}&to=CNY`;
        try {
          const response = await fetch(url);
          if (!response.ok) continue;
          const payload = (await response.json()) as { rates?: Record<string, number> };
          const rate = payload.rates?.CNY;
          if (typeof rate === "number") {
            rows.push({ date: item.date, from: item.commodity, to: "CNY", rate: rate.toFixed(6) });
          }
        } catch {
          /* offline: skip this pair */
        }
      }
      return api.fxApplyRates(rows);
    },
    onSuccess: (result) => {
      onMessage(`写入 ${result.written.length} 条汇率，失败 ${result.failed.length} 条`);
      void queryClient.invalidateQueries();
    },
    onError: (err: unknown) => onError(err instanceof Error ? err.message : String(err)),
  });

  return (
    <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
      <div className="text-[11px] text-[var(--color-muted)]">
        {enabled
          ? missing.data && missing.data.length > 0
            ? `${missing.data.length} 笔缺少汇率`
            : "没有缺失的汇率"
          : "未启用"}
      </div>
      <Button
        variant="subtle"
        disabled={!enabled || sync.isPending || (missing.data?.length ?? 0) === 0}
        onClick={() => sync.mutate()}
      >
        {sync.isPending ? "同步中…" : "同步 ECB 汇率"}
      </Button>
    </div>
  );
}
