import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { RuleDto_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, money, shortAccount } from "../lib/format";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  Field,
  Input,
  Modal,
  Switch,
} from "../components/ui";

export function Rules({ onCapture }: { onCapture: () => void }) {
  const [editorOpen, setEditorOpen] = React.useState(false);
  const [selected, setSelected] = React.useState<RuleDto_Serialize | null>(null);
  const queryClient = useQueryClient();

  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.rules,
    queryFn: () => api.rules(),
    retry: 0,
  });

  const toggle = useMutation({
    mutationFn: (id: string) => api.ruleToggle(id),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.rules }),
    onError: (err: unknown) => window.alert(err instanceof Error ? err.message : String(err)),
  });

  const rules = data ?? [];

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-[var(--color-line)] px-4 py-3">
        <div>
          <div className="text-lg font-semibold">规则</div>
          <div className="text-xs text-[var(--color-muted)]">
            写时展开：命中即写入规范交易，之后改规则不会回改历史
          </div>
        </div>
        <div className="ml-auto flex gap-2">
          <Button variant="ghost" onClick={onCapture}>
            用快录建规则（= 开头）
          </Button>
          <Button variant="primary" onClick={() => setEditorOpen(true)}>
            新建规则
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
        {error ? (
          <EmptyState title="规则读取失败" hint={String((error as Error).message)} />
        ) : isLoading ? (
          <EmptyState title="正在读取 rules.ledger…" />
        ) : rules.length === 0 ? (
          <EmptyState
            title="还没有规则"
            hint="试试 = desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝"
            action={
              <Button variant="primary" className="mt-2" onClick={() => setEditorOpen(true)}>
                新建第一条规则
              </Button>
            }
          />
        ) : (
          <Card className="divide-y divide-[var(--color-line)]">
            {rules.map((rule) => (
              <div key={rule.id} className="flex items-start gap-3 px-3 py-2.5">
                <Switch
                  checked={rule.enabled}
                  onChange={() => toggle.mutate(rule.id)}
                  label={`启用 ${rule.id}`}
                />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs">{rule.query}</span>
                    <Badge>{rule.id}</Badge>
                    {rule.order !== null ? (
                      <span className="text-[10px] text-[var(--color-muted)]">#{rule.order}</span>
                    ) : null}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-1 text-[11px] text-[var(--color-muted)]">
                    {rule.postings.map((posting, index) => (
                      <span key={index} className="rounded bg-[var(--color-surface-2)] px-1.5 py-0.5">
                        {posting.is_source ? "← " : ""}
                        {shortAccount(posting.account)}
                        {posting.percent ? ` ${posting.percent}%` : ""}
                        {posting.amount ? ` ${money(posting.amount.value, posting.amount.display)}` : ""}
                      </span>
                    ))}
                  </div>
                  {rule.diagnostics.length > 0 ? (
                    <div className="mt-1 text-[11px] text-amber-600 dark:text-amber-400">
                      {rule.diagnostics[0].message}
                    </div>
                  ) : null}
                </div>
                <Button variant="ghost" onClick={() => setSelected(rule)}>
                  试算
                </Button>
              </div>
            ))}
          </Card>
        )}
      </div>

      <RuleEditor
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        onSaved={() => {
          setEditorOpen(false);
          void queryClient.invalidateQueries({ queryKey: queryKeys.rules });
        }}
      />

      {selected ? (
        <RuleTester
          rule={selected}
          onClose={() => setSelected(null)}
          onRecomputed={() => {
            setSelected(null);
            void queryClient.invalidateQueries();
          }}
        />
      ) : null}
    </div>
  );
}

function RuleEditor({
  open,
  onClose,
  onSaved,
}: {
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [raw, setRaw] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  const save = useMutation({
    mutationFn: () => api.ruleUpsert(raw.trim()),
    onSuccess: onSaved,
    onError: (err: unknown) => setError(err instanceof Error ? err.message : String(err)),
  });

  return (
    <Modal open={open} onClose={onClose} title="新建 / 更新规则">
      <div className="space-y-3">
        <Field
          label="规则（一行）"
          hint="= <查询> -> <账户>=<百分比>%, ... [from <资金账户>]；同一查询会覆盖旧规则"
        >
          <Input
            autoFocus
            value={raw}
            onChange={(e) => setRaw(e.target.value)}
            placeholder="= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝"
            className="font-mono"
          />
        </Field>
        <div className="text-[11px] text-[var(--color-muted)]">
          查询支持 <code>desc:</code> <code>acct:</code> <code>amt:</code> <code>tag:</code>，可用 and / or / not。
        </div>
        {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" disabled={!raw.trim() || save.isPending} onClick={() => save.mutate()}>
            保存
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function RuleTester({
  rule,
  onClose,
  onRecomputed,
}: {
  rule: RuleDto_Serialize;
  onClose: () => void;
  onRecomputed: () => void;
}) {
  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.ruleTest(rule.id),
    queryFn: () => api.ruleTest(rule.id, 30),
    retry: 0,
  });

  const recompute = useMutation({
    mutationFn: () => api.ruleRecompute((data?.matches ?? []).map((txn) => txn.id)),
    onSuccess: onRecomputed,
  });

  return (
    <Modal open onClose={onClose} title={`试算 ${rule.id}`} width="max-w-2xl">
      <div className="space-y-3">
        <div className="font-mono text-xs text-[var(--color-muted)]">{rule.query}</div>
        {isLoading ? (
          <div className="text-xs text-[var(--color-muted)]">正在匹配历史交易…</div>
        ) : error ? (
          <div className="text-xs text-[var(--color-danger)]">{String((error as Error).message)}</div>
        ) : (
          <>
            <div className="text-xs">
              命中 {data?.matched_count ?? 0} 笔
              {data?.expanded_for ? ` · 以 ${data.expanded_for.payee || data.expanded_for.id} 试算` : ""}
            </div>
            <div className="max-h-40 overflow-auto rounded-lg border border-[var(--color-line)]">
              {(data?.matches ?? []).map((txn) => (
                <div key={txn.id} className="flex items-center gap-2 border-b border-[var(--color-line)]/60 px-2 py-1 text-xs last:border-0">
                  <span className="text-[var(--color-muted)]">{txn.date}</span>
                  <span className="min-w-0 flex-1 truncate">{txn.payee}</span>
                  <span className="font-mono">{money(txn.base_value, txn.base_value_display)}</span>
                </div>
              ))}
              {(data?.matches ?? []).length === 0 ? (
                <div className="px-2 py-3 text-xs text-[var(--color-muted)]">没有命中任何历史交易</div>
              ) : null}
            </div>
            <div className="rounded-lg bg-[var(--color-surface-2)] p-2 font-mono text-[11px]">
              {(data?.expanded ?? []).map((posting, index) => (
                <div key={index} className="flex justify-between gap-2">
                  <span className={cn("truncate")}>{posting.account}</span>
                  <span>{posting.display}</span>
                </div>
              ))}
            </div>
            {(data?.notes ?? []).length > 0 ? (
              <div className="text-[11px] text-[var(--color-muted)]">
                {data?.notes.map((note, index) => <div key={index}>{note}</div>)}
              </div>
            ) : null}
          </>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            关闭
          </Button>
          <Button
            variant="primary"
            disabled={!data || data.matches.length === 0 || recompute.isPending}
            onClick={() => recompute.mutate()}
          >
            按当前规则重算这 {data?.matches.length ?? 0} 笔
          </Button>
        </div>
      </div>
    </Modal>
  );
}
