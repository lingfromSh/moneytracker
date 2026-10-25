import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { CheckCheck, RefreshCw } from "lucide-react";
import type { ReconcilePreviewDto_Serialize } from "../lib/bindings";
import { api } from "../lib/ipc";
import { cn, money, today } from "../lib/format";
import { Badge, Button, EmptyState, Field, Input, Modal } from "./ui";

type Props = {
  open: boolean;
  accounts: string[];
  onClose: () => void;
};

/**
 * Reconciliation: the ledger marks a transaction as cleared with status `*`.
 * Pick an account, see what is still uncleared, and clear it against the
 * statement you are reading.
 */
export function ReconcileModal({ open, accounts, onClose }: Props) {
  const queryClient = useQueryClient();
  const [account, setAccount] = React.useState(accounts[0] ?? "assets:cash");
  const [date, setDate] = React.useState(today());
  const [preview, setPreview] = React.useState<ReconcilePreviewDto_Serialize | null>(null);
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [result, setResult] = React.useState<string | null>(null);

  const load = React.useCallback(
    async (target: string, asOf: string) => {
      setBusy(true);
      setError(null);
      try {
        const next = await api.reconcilePreview(target, asOf || null);
        setPreview(next);
        setSelected(
          new Set(next.rows.filter((row) => !row.cleared).map((row) => row.id)),
        );
      } catch (cause) {
        setError((cause as Error).message);
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  React.useEffect(() => {
    if (!open) return;
    setResult(null);
    const target = accounts.includes(account) ? account : (accounts[0] ?? account);
    if (target !== account) setAccount(target);
    if (target) void load(target, date);
  }, [open, account, accounts, date, load]);

  const clear = React.useCallback(async () => {
    if (selected.size === 0) return;
    setBusy(true);
    setError(null);
    try {
      const done = await api.reconcileClear([...selected]);
      setResult(`已对清 ${done} 笔`);
      await queryClient.invalidateQueries();
      await load(account, date);
    } catch (cause) {
      setError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  }, [selected, account, date, load, queryClient]);

  const toggle = (id: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const difference = preview?.difference?.value ?? null;

  return (
    <Modal open={open} onClose={onClose} title="对账" width="min(780px, 92vw)">
      <div className="space-y-3">
        <div className="grid gap-2 sm:grid-cols-[1fr_auto_auto]">
          <Field label="账户">
            <select
              className="w-full rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-1.5 font-mono text-xs"
              value={account}
              onChange={(event) => setAccount(event.target.value)}
              aria-label="对账账户"
            >
              {accounts.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="截止日期">
            <Input
              type="date"
              value={date}
              onChange={(event) => setDate(event.target.value)}
              aria-label="截止日期"
            />
          </Field>
          <div className="flex items-end">
            <Button variant="ghost" onClick={() => void load(account, date)} disabled={busy}>
              <RefreshCw className={cn("mr-1 h-3.5 w-3.5", busy && "animate-spin")} />
              重新比对
            </Button>
          </div>
        </div>

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

        {preview ? (
          <>
            <div className="grid gap-2 sm:grid-cols-4">
              <Figure label="已对清" value={money(preview.cleared.value, preview.cleared.display)} />
              <Figure
                label="未对清"
                value={money(preview.uncleared.value, preview.uncleared.display)}
              />
              <Figure label="账本余额" value={money(preview.balance.value, preview.balance.display)} />
              <Figure
                label="与余额断言差"
                value={
                  preview.difference
                    ? money(preview.difference.value, preview.difference.display)
                    : "—"
                }
                tone={difference !== null && Math.abs(difference) > 0.005 ? "bad" : undefined}
              />
            </div>
            {preview.note ? (
              <div className="text-[11px] text-[var(--color-muted)]">{preview.note}</div>
            ) : null}

            <div className="max-h-72 overflow-auto rounded-lg border border-[var(--color-line)]">
              {preview.rows.length === 0 ? (
                <EmptyState title="这个账户还没有交易" hint="记一笔之后再来对账" />
              ) : (
                <table className="w-full text-xs">
                  <thead className="sticky top-0 bg-[var(--color-surface-2)] text-[var(--color-muted)]">
                    <tr className="text-left">
                      <th className="w-8 px-2 py-1" />
                      <th className="px-2 py-1 font-normal">日期</th>
                      <th className="px-2 py-1 font-normal">摘要</th>
                      <th className="px-2 py-1 text-right font-normal">金额</th>
                      <th className="px-2 py-1 font-normal">状态</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preview.rows.map((row) => (
                      <tr key={row.id} className="border-t border-[var(--color-line)]/60">
                        <td className="px-2 py-1">
                          <input
                            type="checkbox"
                            checked={selected.has(row.id)}
                            onChange={() => toggle(row.id)}
                            aria-label={`选择 ${row.payee}`}
                          />
                        </td>
                        <td className="px-2 py-1 tabular-nums">{row.date}</td>
                        <td className="max-w-[18rem] truncate px-2 py-1">{row.payee}</td>
                        <td className="px-2 py-1 text-right font-mono tabular-nums">
                          {money(row.amount.value, row.amount.display)}
                        </td>
                        <td className="px-2 py-1">
                          {row.cleared ? <Badge tone="accent">已对清</Badge> : <Badge>未对清</Badge>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>

            <div className="flex items-center justify-end gap-2">
              <Button variant="ghost" onClick={onClose}>
                关闭
              </Button>
              <Button
                variant="primary"
                onClick={() => void clear()}
                disabled={busy || selected.size === 0}
              >
                <CheckCheck className="mr-1 h-3.5 w-3.5" />
                对清 {selected.size} 笔
              </Button>
            </div>
          </>
        ) : null}
      </div>
    </Modal>
  );
}

function Figure({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: "good" | "bad";
}) {
  return (
    <div className="rounded-lg border border-[var(--color-line)] px-3 py-2">
      <div className="text-[11px] text-[var(--color-muted)]">{label}</div>
      <div
        className={cn(
          "mt-0.5 font-mono text-sm",
          tone === "bad" && "text-[var(--color-danger)]",
          tone === "good" && "text-[var(--color-accent)]",
        )}
      >
        {value}
      </div>
    </div>
  );
}
