import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { FileUp, Upload } from "lucide-react";
import type { ImportPreviewDto_Serialize, ImportProbeDto_Serialize } from "../lib/bindings";
import { api } from "../lib/ipc";
import { cn, money } from "../lib/format";
import { Badge, Button, Card, CardHeader, EmptyState, Field, Input, Switch } from "../components/ui";

type ProfileShape = {
  path: string;
  delimiter: string | null;
  encoding: string | null;
  header_row: boolean;
  date_col: string | null;
  payee_col: string | null;
  memo_col: string | null;
  amount_col: string | null;
  outflow_col: string | null;
  inflow_col: string | null;
  account: string;
  commodity: string | null;
  date_format: string | null;
  flip_sign: boolean;
  skip_patterns: string[];
  mapping: { pattern: string; category: string; tags: string[] }[];
  include_lines: number[];
};

function emptyProfile(path: string): ProfileShape {
  return {
    path,
    delimiter: null,
    encoding: null,
    header_row: true,
    date_col: null,
    payee_col: null,
    memo_col: null,
    amount_col: null,
    outflow_col: null,
    inflow_col: null,
    account: "assets:bank",
    commodity: null,
    date_format: null,
    flip_sign: false,
    skip_patterns: [] as string[],
    mapping: [] as { pattern: string; category: string; tags: string[] }[],
    include_lines: [] as number[],
  };
}

function profileFromProbe(probe: ImportProbeDto_Serialize): ProfileShape {
  return {
    ...emptyProfile(probe.path),
    delimiter: probe.delimiter,
    encoding: probe.encoding,
    date_col: probe.guess.date_col,
    payee_col: probe.guess.payee_col,
    memo_col: probe.guess.memo_col,
    amount_col: probe.guess.amount_col,
    outflow_col: probe.guess.outflow_col,
    inflow_col: probe.guess.inflow_col,
    account: probe.guess.account ?? "assets:bank",
  };
}

/** One line per rule: `关键词 => 分类 [:tag ...]`, `re:正则` is accepted too. */
function parseRules(text: string) {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.includes("=>"))
    .map((line) => {
      const [pattern, rest] = line.split("=>");
      const parts = (rest ?? "").trim().split(/\s+/);
      return {
        pattern: pattern.trim(),
        category: parts[0] ?? "expenses:unclassified",
        tags: parts.slice(1).map((tag) => tag.replace(/^[:+]/, "").replace(/:$/, "")),
      };
    })
    .filter((rule) => rule.pattern.length > 0);
}

export function Import() {
  const queryClient = useQueryClient();
  const [probe, setProbe] = React.useState<ImportProbeDto_Serialize | null>(null);
  const [profile, setProfile] = React.useState<ProfileShape | null>(null);
  const [preview, setPreview] = React.useState<ImportPreviewDto_Serialize | null>(null);
  const [selected, setSelected] = React.useState<Set<number>>(new Set());
  const [rules, setRules] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [result, setResult] = React.useState<string | null>(null);

  const update = React.useCallback((patch: Partial<ProfileShape>) => {
    setProfile((current) => (current ? { ...current, ...patch } : current));
  }, []);

  const choose = React.useCallback(async () => {
    setError(null);
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "对账单", extensions: ["csv", "txt", "tsv"] }],
      });
      if (typeof picked !== "string") return;
      setBusy(true);
      const next = await api.importProbe(picked);
      setProbe(next);
      setProfile(profileFromProbe(next));
      setPreview(null);
      setResult(null);
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  }, []);

  const runPreview = React.useCallback(async () => {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      const next = await api.importPreview({ ...profile, mapping: parseRules(rules) });
      setPreview(next);
      setSelected(new Set(next.rows.filter((row) => row.selected).map((row) => Number(row.line))));
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  }, [profile, rules]);

  const commit = React.useCallback(async () => {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      const next = await api.importCommit({
        ...profile,
        mapping: parseRules(rules),
        include_lines: [...selected],
      });
      setResult(
        `批次 ${next.batch_id}：写入 ${next.written} 笔，跳过 ${next.skipped} 笔（${next.files.length} 个文件）`,
      );
      await queryClient.invalidateQueries();
      setPreview(null);
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  }, [profile, rules, selected, queryClient]);

  const toggle = (line: number) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(line)) next.delete(line);
      else next.add(line);
      return next;
    });
  };

  const columns = probe?.headers ?? [];
  const columnSelect = (label: string, value: string | null, onPick: (next: string | null) => void) => (
    <Field label={label}>
      <select
        className="w-full rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-1.5 text-xs"
        value={value ?? ""}
        onChange={(event) => onPick(event.target.value || null)}
        aria-label={label}
      >
        <option value="">（不使用）</option>
        {columns.map((column) => (
          <option key={column} value={column}>
            {column}
          </option>
        ))}
      </select>
    </Field>
  );

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex items-center justify-between px-4 pb-3 pt-4">
        <div>
          <div className="text-lg font-semibold">导入对账单</div>
          <div className="text-xs text-[var(--color-muted)]">
            支付宝 / 微信 / 银行导出的 CSV，先预览、去重，再写入账本
          </div>
        </div>
        <Button variant="subtle" onClick={() => void choose()} disabled={busy}>
          <FileUp className="mr-1 h-3.5 w-3.5" />
          {probe ? "换一个文件" : "选择文件"}
        </Button>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden px-4 pb-8">
        {error ? (
          <div className="rounded-lg border border-[var(--color-danger)]/40 bg-[var(--color-danger)]/10 px-3 py-2 text-xs text-[var(--color-danger)]">
            {error}
          </div>
        ) : null}
        {result ? (
          <div className="rounded-lg border border-[var(--color-accent)]/50 bg-[var(--color-accent)]/10 px-3 py-2 text-xs">
            {result}
          </div>
        ) : null}

        {!probe ? (
          <Card className="p-4">
            <EmptyState
              title={busy ? "正在读取…" : "选择一份对账单"}
              hint="支持 UTF-8 与 GBK 编码，自动识别分隔符、日期与金额列；重复导入会被指纹识别并跳过。"
              action={
                <Button variant="primary" onClick={() => void choose()} disabled={busy}>
                  选择 CSV 文件
                </Button>
              }
            />
          </Card>
        ) : null}

        {probe && profile ? (
          <>
            <Card className="p-4">
              <CardHeader
                title={probe.name}
                hint={`${probe.encoding} · 分隔符「${probe.delimiter === "\t" ? "TAB" : probe.delimiter}」 · ${probe.row_count} 行`}
                action={
                  probe.guess.preset ? <Badge tone="accent">{probe.guess.preset}</Badge> : null
                }
              />
              <div className="mt-3 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                <Field label="账号（钱从/到哪里）" hint="导入的每笔都会记到这个账户">
                  <Input
                    value={profile.account}
                    onChange={(event) => update({ account: event.target.value })}
                    aria-label="账号"
                    className="font-mono text-xs"
                  />
                </Field>
                {columnSelect("日期列", profile.date_col, (next) => update({ date_col: next }))}
                {columnSelect("对方/商户列", profile.payee_col, (next) => update({ payee_col: next }))}
                {columnSelect("备注列", profile.memo_col, (next) => update({ memo_col: next }))}
                {columnSelect("金额列", profile.amount_col, (next) => update({ amount_col: next }))}
                {columnSelect("支出列", profile.outflow_col, (next) => update({ outflow_col: next }))}
                {columnSelect("收入列", profile.inflow_col, (next) => update({ inflow_col: next }))}
                <Field label="日期格式" hint="留空自动识别，例如 %Y/%m/%d">
                  <Input
                    value={profile.date_format ?? ""}
                    onChange={(event) => update({ date_format: event.target.value || null })}
                    placeholder="自动"
                    aria-label="日期格式"
                    className="font-mono text-xs"
                  />
                </Field>
                <Field label="跳过包含…的行" hint="逗号分隔，用于过滤「合计」「小计」">
                  <Input
                    value={profile.skip_patterns.join(",")}
                    onChange={(event) =>
                      update({
                        skip_patterns: event.target.value
                          .split(/[,，]/)
                          .map((item) => item.trim())
                          .filter(Boolean),
                      })
                    }
                    aria-label="跳过包含"
                    className="text-xs"
                  />
                </Field>
                <Field label="反转正负" hint="导出把支出写成负数时打开">
                  <div className="flex h-8 items-center gap-2">
                    <Switch
                      checked={profile.flip_sign}
                      onChange={(next) => update({ flip_sign: next })}
                      label="反转正负"
                    />
                    <span className="text-xs text-[var(--color-muted)]">
                      {profile.flip_sign ? "已反转" : "按原样"}
                    </span>
                  </div>
                </Field>
              </div>

              <Field
                label="分类规则"
                hint="每行：关键词 => 分类 :标签；也支持 re:正则。首个匹配生效。"
              >
                <textarea
                  value={rules}
                  onChange={(event) => setRules(event.target.value)}
                  placeholder={"星巴克 => expenses:food:coffee\n滴滴 => expenses:transport :taxi"}
                  aria-label="分类规则"
                  className="h-24 w-full rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-1.5 font-mono text-xs"
                />
              </Field>

              <div className="mt-3 flex items-center justify-end gap-2">
                <Button variant="ghost" onClick={() => void runPreview()} disabled={busy}>
                  预览
                </Button>
                <Button
                  variant="primary"
                  onClick={() => void commit()}
                  disabled={busy || !preview || selected.size === 0}
                >
                  <Upload className="mr-1 h-3.5 w-3.5" />
                  导入 {preview ? `${selected.size} 笔` : ""}
                </Button>
              </div>
            </Card>

            {preview ? (
              <Card className="p-4">
                <CardHeader
                  title="预览"
                  hint={`${preview.rows.length} 行 · 重复 ${preview.duplicates} · 合计 ${money(preview.total.value, preview.total.display)}`}
                />
                {preview.errors.length > 0 ? (
                  <ul className="mt-2 space-y-1 text-xs text-amber-600 dark:text-amber-400">
                    {preview.errors.slice(0, 8).map((diagnostic, index) => (
                      <li key={`${diagnostic.code}-${index}`}>{diagnostic.message}</li>
                    ))}
                  </ul>
                ) : null}
                <div className="mt-2 max-h-[26rem] overflow-auto">
                  <table className="w-full min-w-[640px] text-xs">
                    <thead className="sticky top-0 bg-[var(--color-surface)] text-[var(--color-muted)]">
                      <tr className="text-left">
                        <th className="w-8 py-1" />
                        <th className="py-1 font-normal">日期</th>
                        <th className="py-1 font-normal">对方</th>
                        <th className="py-1 font-normal">分类</th>
                        <th className="py-1 text-right font-normal">金额</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.rows.map((row) => {
                        const line = Number(row.line);
                        const checked = selected.has(line);
                        return (
                          <tr
                            key={`${line}-${row.fingerprint}`}
                            className={cn(
                              "border-t border-[var(--color-line)]/60",
                              row.duplicate && "text-[var(--color-muted)]",
                            )}
                          >
                            <td className="py-1">
                              <input
                                type="checkbox"
                                checked={checked}
                                onChange={() => toggle(line)}
                                aria-label={`选择第 ${line} 行`}
                              />
                            </td>
                            <td className="py-1 tabular-nums">{row.date}</td>
                            <td className="py-1">
                              <div className="flex items-center gap-1.5">
                                <span className="truncate">{row.payee}</span>
                                {row.duplicate ? <Badge tone="warn">重复</Badge> : null}
                              </div>
                              {row.memo ? (
                                <div className="truncate text-[10px] text-[var(--color-muted)]">
                                  {row.memo}
                                </div>
                              ) : null}
                            </td>
                            <td className="py-1">
                              {row.category ? (
                                <span className="font-mono text-[11px]">{row.category}</span>
                              ) : (
                                <span className="text-[11px] text-[var(--color-muted)]">
                                  {row.amount.value && row.amount.value < 0
                                    ? "income:unclassified"
                                    : "expenses:unclassified"}
                                </span>
                              )}
                              {row.tags.length > 0 ? (
                                <span className="ml-1 text-[10px] text-[var(--color-muted)]">
                                  {row.tags.map((tag) => `:${tag}:`).join("")}
                                </span>
                              ) : null}
                            </td>
                            <td className="py-1 text-right tabular-nums">
                              {money(row.amount.value, row.amount.display)}
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              </Card>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}
