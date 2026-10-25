import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarClock, Pencil, Play, Plus, Wallet } from "lucide-react";
import type {
  SalaryTemplateDto_Serialize,
  ScheduleOverviewDto_Serialize,
} from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, dayLabel, money, monthLabel } from "../lib/format";
import {
  OccurrenceStatus,
  cadenceText,
  cashOf,
  kindText,
  kindTone,
} from "../components/schedule";
import {
  Badge,
  Button,
  Card,
  CardHeader,
  EmptyState,
  Field,
  Input,
  Modal,
  Select,
  Switch,
  Textarea,
} from "../components/ui";

/**
 * The calendar of declared money: `recurring.ledger` lists the templates, this
 * page shows what they will cost or pay each month and writes the transactions
 * only when asked (or when 设置 → 自动化 turned auto-recur on).
 */
export function Schedule() {
  const queryClient = useQueryClient();
  const [months, setMonths] = React.useState(12);
  const [editing, setEditing] = React.useState(false);
  const [draft, setDraft] = React.useState("");
  const [salaryOpen, setSalaryOpen] = React.useState(false);
  const [pending, setPending] = React.useState<PendingCommit | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [message, setMessage] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const listed = useQuery({
    queryKey: queryKeys.schedule(months),
    queryFn: () => api.scheduleOverview(months),
    retry: 0,
  });

  const refresh = React.useCallback(async () => {
    await queryClient.invalidateQueries();
  }, [queryClient]);

  const run = React.useCallback(
    async (work: () => Promise<string>) => {
      setBusy(true);
      setError(null);
      setMessage(null);
      try {
        setMessage(await work());
        await refresh();
      } catch (cause) {
        setError((cause as Error).message);
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const data = listed.data;

  const commitOne = (id: string, month: string, tax: number | null = null) =>
    run(async () => {
      const result = await api.scheduleCommit(id, month, tax);
      return Number(result.written ?? 0) > 0 ? "已记入账本" : "这个月已经记过了";
    });

  /** 带个税的模板先问一句：个税是估的，允许按工资条改。 */
  const askCommit = (id: string, month: string) => {
    const template = data?.templates.find((item) => item.id === id);
    const occurrence = data?.occurrences.find(
      (item) => item.id === id && item.month === month && !item.recorded,
    );
    if (template?.tax_account && occurrence) {
      setPending({
        id,
        month,
        label: template.label,
        base: Number(occurrence.tax_base.value ?? 0),
        tax: Number(occurrence.tax.value ?? 0),
        settle: Number(occurrence.settle.value ?? 0),
      });
      return;
    }
    void commitOne(id, month, null);
  };

  const commitDue = () =>
    run(async () => {
      const result = await api.scheduleCommitDue();
      if (result.errors.length > 0) {
        return `记入 ${result.written ?? 0} 笔，${result.errors.length} 条失败：${result.errors[0]}`;
      }
      return Number(result.written ?? 0) > 0 ? `已记入 ${result.written ?? 0} 笔` : "本月没有待记的条目";
    });

  const skip = (id: string, month: string) =>
    run(async () => {
      await api.scheduleSkip(id, month);
      return "这个月不再提醒";
    });

  const startEditing = () => {
    setDraft(data?.src ?? "");
    setError(null);
    setEditing(true);
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.scheduleSave(draft);
      await refresh();
      setEditing(false);
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (listed.isLoading) {
    return <div className="p-6 text-sm text-[var(--color-muted)]">正在读取 recurring.ledger…</div>;
  }
  if (listed.error) {
    return (
      <div className="p-6 text-sm text-[var(--color-danger)]">
        读取日程失败：{(listed.error as Error).message}
      </div>
    );
  }
  if (!data) return null;

  const waiting = data.month_totals.reduce((sum, month) => sum + Number(month.pending ?? 0), 0);

  return (
    <div className="flex h-full min-w-0 flex-col">
      <div className="flex flex-col gap-2 px-4 pb-3 pt-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="hidden text-lg font-semibold sm:block">日程</div>
          <div className="text-xs text-[var(--color-muted)]">
            来自 <code>{data.file}</code> · {data.templates.length} 条固定收支
            {waiting > 0 ? ` · ${waiting} 笔待记` : ""}
            {data.auto_commit ? " · 自动记入已开启" : " · 只提醒，不写入"}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2 sm:justify-end">
          <Select
            value={String(months)}
            onChange={(event) => setMonths(Number(event.target.value))}
            aria-label="时间范围"
          >
            <option value="6">6 个月</option>
            <option value="12">12 个月</option>
            <option value="24">24 个月</option>
          </Select>
          <Button variant="subtle" onClick={() => setSalaryOpen(true)}>
            <Plus className="mr-1 h-3.5 w-3.5" />
            工资模板
          </Button>
          <Button variant="subtle" onClick={startEditing}>
            <Pencil className="mr-1 h-3.5 w-3.5" />
            编辑文件
          </Button>
          <Button variant="primary" onClick={() => void commitDue()} disabled={busy || waiting === 0}>
            <Play className="mr-1 h-3.5 w-3.5" />
            记入本月待办
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden px-4 pb-6">
        {message ? <div className="text-xs text-[var(--color-accent)]">{message}</div> : null}
        {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}

        {!data.has_schedule ? (
          <Card className="p-4">
            <EmptyState
              title="还没有固定收支"
              hint="写一份 recurring.ledger，把工资、房贷、订阅这类每月重复的钱一次性声明出来。它不会被 index.ledger 引用，所以不会影响报表和预算。"
              action={
                <div className="flex gap-2">
                  <Button variant="primary" onClick={() => setSalaryOpen(true)}>
                    生成工资模板
                  </Button>
                  <Button variant="subtle" onClick={startEditing}>
                    手写文件
                  </Button>
                </div>
              }
            />
          </Card>
        ) : null}

        {data.diagnostics.length > 0 ? (
          <Card className="p-3">
            <div className="mb-1 text-xs font-medium text-amber-600 dark:text-amber-400">
              {data.file} 有 {data.diagnostics.length} 处问题
            </div>
            <ul className="space-y-1 text-xs text-[var(--color-muted)]">
              {data.diagnostics.map((diagnostic, index) => (
                <li key={`${diagnostic.code}-${index}`}>
                  {diagnostic.line ? `第 ${diagnostic.line} 行：` : ""}
                  {diagnostic.message}
                </li>
              ))}
            </ul>
          </Card>
        ) : null}

        {data.templates.length > 0 ? (
          <Templates data={data} busy={busy} onCommit={askCommit} onSkip={skip} />
        ) : null}

        <div className="space-y-2">
          {data.months.map((month) => {
            const totals = data.month_totals.find((entry) => entry.month === month);
            const rows = data.occurrences.filter((occurrence) => occurrence.month === month);
            return (
              <Card key={month}>
                <CardHeader
                  title={
                    <span className="flex items-center gap-2">
                      {monthLabel(month)}
                      {totals && Number(totals.pending ?? 0) > 0 ? (
                        <Badge tone="warn">{totals.pending} 笔待记</Badge>
                      ) : null}
                    </span>
                  }
                  hint={
                    <span className="flex flex-wrap items-center gap-x-3 gap-y-0.5 font-mono text-[11px]">
                      <span className="text-[var(--color-accent)]">
                        收 {money(totals?.income.value, totals?.income.display)}
                      </span>
                      <span>支 {money(totals?.expense.value, totals?.expense.display)}</span>
                      <span>结余 {money(Number(totals?.income.value ?? 0) - Number(totals?.expense.value ?? 0))}</span>
                    </span>
                  }
                />
                {rows.length === 0 ? (
                  <div className="px-3 pb-2 text-[11px] text-[var(--color-muted)]">没有固定收支</div>
                ) : (
                  <ul className="divide-y divide-[var(--color-line)]">
                    {rows.map((occurrence) => (
                      <li
                        key={`${occurrence.id}@${occurrence.month}`}
                        className="flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-1.5 text-xs"
                      >
                        <CalendarClock className="h-3.5 w-3.5 shrink-0 text-[var(--color-muted)]" />
                        <span className="shrink-0 font-mono text-[11px] text-[var(--color-muted)]">
                          {dayLabel(occurrence.date)}
                        </span>
                        <span className="min-w-0 flex-1 truncate">{occurrence.label}</span>
                        <Badge tone={kindTone(occurrence.kind)}>{kindText(occurrence.kind)}</Badge>
                        <OccurrenceStatus occurrence={occurrence} />
                        <span className="ml-auto shrink-0 text-right font-mono tabular-nums">
                          {cashOf(occurrence)}
                        </span>
                        <span className="flex shrink-0 justify-end gap-1">
                          {!occurrence.recorded && !occurrence.skipped ? (
                            <>
                              <Button
                                variant="subtle"
                                className="px-2 py-0.5 text-[11px]"
                                disabled={busy}
                                onClick={() => askCommit(occurrence.id, occurrence.month)}
                              >
                                记入
                              </Button>
                              <Button
                                variant="ghost"
                                className="px-2 py-0.5 text-[11px]"
                                disabled={busy}
                                onClick={() => void skip(occurrence.id, occurrence.month)}
                              >
                                跳过
                              </Button>
                            </>
                          ) : null}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </Card>
            );
          })}
        </div>
      </div>

      <Modal open={editing} onClose={() => setEditing(false)} title={`日程文件 ${data.file}`} width="max-w-3xl">
        <div className="space-y-3">
          <Field
            label="文件内容"
            hint="一条模板就是一个 hledger 定期交易块（~ 月 频率 从 日期 号），头部的 ; 注释里写 id: label: kind:。"
          >
            <Textarea
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              className="h-80 font-mono text-xs"
              aria-label="日程文件内容"
            />
          </Field>
          {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setEditing(false)}>
              取消
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy}>
              {busy ? "保存中…" : "保存"}
            </Button>
          </div>
        </div>
      </Modal>

      {pending ? (
        <TaxModal
          pending={pending}
          busy={busy}
          onCancel={() => setPending(null)}
          onConfirm={(tax) => {
            setPending(null);
            void commitOne(pending.id, pending.month, tax);
          }}
        />
      ) : null}

      <SalaryModal
        open={salaryOpen}
        onClose={() => setSalaryOpen(false)}
        base={data.src}
        onSaved={async (text) => {
          await api.scheduleSave(text);
          await refresh();
          setSalaryOpen(false);
          setMessage("工资模板已写入 recurring.ledger");
        }}
        onError={setError}
      />
    </div>
  );
}

function Templates({
  data,
  busy,
  onCommit,
  onSkip,
}: {
  data: ScheduleOverviewDto_Serialize;
  busy: boolean;
  onCommit: (id: string, month: string) => void;
  onSkip: (id: string, month: string) => void;
}) {
  return (
    <Card>
      <CardHeader title="固定收支模板" hint={`${data.templates.length} 条 · 点击“记入”写入当月账单`} />
      <div className="overflow-x-auto">
        <table className="w-full min-w-[640px] text-xs">
          <thead className="text-[11px] text-[var(--color-muted)]">
            <tr className="border-b border-[var(--color-line)]">
              <th className="px-3 py-1.5 text-left font-medium">名称</th>
              <th className="px-2 py-1.5 text-left font-medium">类型</th>
              <th className="px-2 py-1.5 text-left font-medium">频率</th>
              <th className="px-2 py-1.5 text-left font-medium">下次</th>
              <th className="px-2 py-1.5 text-right font-medium">现金流</th>
              <th className="px-2 py-1.5 text-right font-medium">收 / 支 / 资产 / 负债</th>
              <th className="px-3 py-1.5 text-right font-medium">操作</th>
            </tr>
          </thead>
          <tbody>
            {data.templates.map((template) => {
              const month = data.today.slice(0, 7);
              return (
                <tr key={template.id} className="border-b border-[var(--color-line)]/60 last:border-0">
                  <td className="px-3 py-1.5">
                    <div className="flex items-center gap-1.5">
                      {template.kind === "salary" ? (
                        <Wallet className="h-3.5 w-3.5 text-[var(--color-accent)]" />
                      ) : null}
                      <span className="truncate">{template.label}</span>
                      {template.enabled ? null : <Badge>已停用</Badge>}
                    </div>
                  </td>
                  <td className="px-2 py-1.5">
                    <Badge tone={kindTone(template.kind)}>{kindText(template.kind)}</Badge>
                  </td>
                  <td className="px-2 py-1.5 text-[var(--color-muted)]">{cadenceText(template)}</td>
                  <td className="px-2 py-1.5 font-mono text-[11px]">{template.next_date ?? "—"}</td>
                  <td
                    className={cn(
                      "px-2 py-1.5 text-right font-mono tabular-nums",
                      (template.settle.value ?? 0) > 0 ? "text-[var(--color-accent)]" : undefined,
                    )}
                  >
                    {cashOf(template)}
                  </td>
                  <td className="px-2 py-1.5 text-right font-mono text-[11px] text-[var(--color-muted)]">
                    {money(template.income.value)} / {money(template.expense.value)} /{" "}
                    {money(template.assets.value)} / {money(template.debt.value)}
                  </td>
                  <td className="px-3 py-1.5 text-right">
                    <span className="flex justify-end gap-1">
                      <Button
                        variant="subtle"
                        className="px-2 py-0.5 text-[11px]"
                        disabled={busy || !template.enabled}
                        onClick={() => onCommit(template.id, month)}
                      >
                        记入本月
                      </Button>
                      <Button
                        variant="ghost"
                        className="px-2 py-0.5 text-[11px]"
                        disabled={busy}
                        onClick={() => onSkip(template.id, month)}
                      >
                        跳过
                      </Button>
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

const SALARY_DEFAULTS: SalaryTemplateDto_Serialize = {
  label: "工资",
  gross: "30000.00",
  deduct: "1000.00",
  day: 5,
  start: null,
  net_account: "assets:bank:cmb",
  salary_account: "income:salary",
  benefit_account: "income:benefit",
  prefix: "assets:cn",
  employer: true,
};

type PendingCommit = {
  id: string;
  month: string;
  label: string;
  /** 本月应纳税所得额（应发 − 5,000 − 个人代扣 − 专项附加扣除）。 */
  base: number;
  /** 估算的本月个税。 */
  tax: number;
  /** 模板的现金流（税前到手）。 */
  settle: number;
};

/** 记入前确认个税：累计预扣法只是估算，工资条上的数字才是真的。 */
function TaxModal({
  pending,
  busy,
  onCancel,
  onConfirm,
}: {
  pending: PendingCommit;
  busy: boolean;
  onCancel: () => void;
  onConfirm: (tax: number) => void;
}) {
  const [tax, setTax] = React.useState(pending.tax.toFixed(2));

  React.useEffect(() => {
    setTax(pending.tax.toFixed(2));
  }, [pending]);

  const value = Number(tax);
  const safe = Number.isFinite(value) ? value : 0;
  const fixed = (number: number) =>
    number.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

  return (
    <Modal
      open
      onClose={onCancel}
      title={`记入 ${pending.label} · ${pending.month}`}
      width="max-w-md"
    >
      <div className="space-y-3">
        <div className="space-y-1 rounded-lg border border-[var(--color-line)] px-3 py-2 text-xs">
          <div className="flex justify-between">
            <span className="text-[var(--color-muted)]">本月应纳税所得额</span>
            <span className="font-mono tabular-nums">{fixed(pending.base)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-[var(--color-muted)]">现金流（税前到手）</span>
            <span className="font-mono tabular-nums">{fixed(pending.settle)}</span>
          </div>
          <div className="flex justify-between border-t border-[var(--color-line)] pt-1 font-medium">
            <span>记入后到手</span>
            <span className="font-mono tabular-nums">{fixed(pending.settle - safe)}</span>
          </div>
        </div>

        <Field label="本月个税" hint="按累计预扣法估算，可以改成工资条上的数字">
          <Input
            value={tax}
            onChange={(event) => setTax(event.target.value)}
            className="font-mono"
            inputMode="decimal"
          />
        </Field>

        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onCancel}>
            取消
          </Button>
          <Button variant="primary" onClick={() => onConfirm(safe)} disabled={busy}>
            记入
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function nextMonthStart(): string {
  const now = new Date();
  const next = new Date(now.getFullYear(), now.getMonth() + 1, 1);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${next.getFullYear()}-${pad(next.getMonth() + 1)}-01`;
}

/**
 * 应发工资 + 五险一金的骨架生成器：个人部分从工资里扣，公司部分再记一笔收益，
 * 一半进资产（可提取），一半进支出（不可提取）。个税由 App 在记入时按累计预扣法
 * 估算并记到 `expenses:个税`。生成的是文本，用户看过再保存。
 */
function SalaryModal({
  open,
  onClose,
  base,
  onSaved,
  onError,
}: {
  open: boolean;
  onClose: () => void;
  base: string;
  onSaved: (text: string) => Promise<void>;
  onError: (message: string | null) => void;
}) {
  const [form, setForm] = React.useState<SalaryTemplateDto_Serialize>({
    ...SALARY_DEFAULTS,
    start: nextMonthStart(),
  });
  const [preview, setPreview] = React.useState("");
  const [busy, setBusy] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setForm((current) => ({ ...current, start: current.start ?? nextMonthStart() }));
      setPreview("");
    }
  }, [open]);

  const generate = async () => {
    setBusy(true);
    onError(null);
    try {
      setPreview(await api.scheduleSalaryPreview(form));
    } catch (cause) {
      onError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    setBusy(true);
    onError(null);
    try {
      const trimmed = base.replace(/\s+$/, "");
      await onSaved(`${trimmed}${trimmed.length > 0 ? "\n\n" : ""}${preview}`);
    } catch (cause) {
      onError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const update = <K extends keyof SalaryTemplateDto_Serialize>(
    key: K,
    value: SalaryTemplateDto_Serialize[K],
  ) => setForm((current) => ({ ...current, [key]: value }));

  return (
    <Modal open={open} onClose={onClose} title="生成工资模板" width="max-w-3xl">
      <div className="space-y-3">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field label="名称">
            <Input value={form.label} onChange={(event) => update("label", event.target.value)} />
          </Field>
          <Field label="应发工资" hint="税前、公司缴纳之外的个人工资">
            <Input
              value={form.gross}
              onChange={(event) => update("gross", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="专项附加扣除" hint="房贷利息 1000、子女教育 2000…按月，影响个税估算">
            <Input
              value={form.deduct ?? ""}
              onChange={(event) => update("deduct", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="发薪日">
            <Input
              type="number"
              min={1}
              max={28}
              value={form.day ?? 5}
              onChange={(event) => update("day", Number(event.target.value))}
              className="font-mono"
            />
          </Field>
          <Field label="开始月份" hint="从 1 号起算，填 2026-11 或 2026-11-01">
            <Input
              value={form.start ?? ""}
              onChange={(event) => update("start", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="到账账户" hint="实发工资进哪个账户">
            <Input
              value={form.net_account}
              onChange={(event) => update("net_account", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="工资收入账户" hint="应发工资记到这里">
            <Input
              value={form.salary_account ?? ""}
              onChange={(event) => update("salary_account", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="公司缴纳账户" hint="公司缴的那部分记成收入">
            <Input
              value={form.benefit_account ?? ""}
              onChange={(event) => update("benefit_account", event.target.value)}
              className="font-mono"
            />
          </Field>
          <Field label="账户前缀" hint="assets:cn → assets:cn:fund、assets:cn:pension…">
            <Input
              value={form.prefix}
              onChange={(event) => update("prefix", event.target.value)}
              className="font-mono"
            />
          </Field>
        </div>

        <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
          <div>
            <div className="text-sm">把公司缴纳侧也入账</div>
            <div className="text-[11px] text-[var(--color-muted)]">
              关闭后只记个人到手部分，总收入等于应发工资
            </div>
          </div>
          <Switch
            checked={form.employer}
            onChange={(next) => update("employer", next)}
            label="把公司缴纳侧也入账"
          />
        </div>

        {preview ? (
          <Field label="预览" hint="会追加到 recurring.ledger 末尾，可以再手改">
            <Textarea
              value={preview}
              onChange={(event) => setPreview(event.target.value)}
              className="h-64 font-mono text-xs"
              aria-label="工资模板预览"
            />
          </Field>
        ) : null}

        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button variant="subtle" onClick={() => void generate()} disabled={busy}>
            生成预览
          </Button>
          <Button variant="primary" onClick={() => void apply()} disabled={busy || !preview}>
            写入文件
          </Button>
        </div>
      </div>
    </Modal>
  );
}
