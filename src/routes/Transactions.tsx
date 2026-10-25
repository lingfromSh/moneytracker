import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { TxnDto_Serialize } from "../lib/bindings";
import { api, queryKeys } from "../lib/ipc";
import { cn, dayLabel, money, shortAccount, weekday } from "../lib/format";
import {
  Badge,
  Button,
  EmptyState,
  Field,
  Input,
  Modal,
  Select,
  Textarea,
} from "../components/ui";
import { RecurringBanner } from "../components/recurring";
import { useShortcutLabel } from "../lib/shortcut";
import { addView, parseViews, removeView, serializeViews } from "../lib/views";

const PAGE = 200;
const ROW_HEIGHT = 56;
const VIEWPORT = 800;
const OVERSCAN = 6;

const ORDERS = [
  { value: "date_desc", label: "最新在前" },
  { value: "date_asc", label: "最早在前" },
  { value: "amount_desc", label: "金额从大到小" },
  { value: "amount_asc", label: "金额从小到大" },
];

type Selection = Set<string>;

export function Transactions({ initialQuery = "" }: { initialQuery?: string }) {
  const [query, setQuery] = React.useState(initialQuery);
  const [order, setOrder] = React.useState("date_desc");
  const [offset, setOffset] = React.useState(0);
  const [editing, setEditing] = React.useState<TxnDto_Serialize | null>(null);
  const shortcutLabel = useShortcutLabel();
  const [selection, setSelection] = React.useState<Selection>(new Set());
  const [bulkOpen, setBulkOpen] = React.useState(false);
  const [scrollTop, setScrollTop] = React.useState(0);
  const [viewName, setViewName] = React.useState<string | null>(null);
  const queryClient = useQueryClient();

  const settings = useQuery({
    queryKey: queryKeys.settings,
    queryFn: () => api.settings(),
    retry: 0,
  });
  const savedViews = parseViews(settings.data?.saved_views);

  const persistViews = async (next: ReturnType<typeof parseViews>) => {
    if (!settings.data) return;
    await api.saveSettings({ ...settings.data, saved_views: serializeViews(next) });
    await queryClient.invalidateQueries({ queryKey: queryKeys.settings });
  };

  const request = React.useMemo(
    () => ({ query: query.trim() || null, offset, limit: PAGE, order }),
    [offset, order, query],
  );

  const page = useQuery({
    queryKey: queryKeys.transactions(request),
    queryFn: () => api.transactions(request),
    retry: 0,
  });

  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries();
  }, [queryClient]);

  const items = page.data?.items ?? [];
  const total = page.data?.total ?? items.length;

  // Small windowing implementation: enough for 100k rows without a dependency.
  const visible = React.useMemo(() => {
    const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
    const count = Math.ceil(VIEWPORT / ROW_HEIGHT) + OVERSCAN * 2;
    return { start, rows: items.slice(start, start + count) };
  }, [items, scrollTop]);

  const toggle = (id: string) => {
    setSelection((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const groups = React.useMemo(() => {
    const map = new Map<string, TxnDto_Serialize[]>();
    for (const txn of items) {
      const list = map.get(txn.date) ?? [];
      list.push(txn);
      map.set(txn.date, list);
    }
    return map;
  }, [items]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-[var(--color-line)] px-4 py-3">
        <Input
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setOffset(0);
          }}
          placeholder="acct:餐饮 date:2026-10 tag:coffee amt:>100"
          aria-label="查询"
        />
        <Select
          value={order}
          onChange={(e) => setOrder(e.target.value)}
          aria-label="排序"
          className="shrink-0"
        >
          {ORDERS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </Select>
        <Button variant="ghost" onClick={refresh} title="重新读取账本">
          刷新
        </Button>
        <Button variant="primary" onClick={() => void api.showCapture()}>
          记一笔
        </Button>
      </div>

      <RecurringBanner />

      {savedViews.length > 0 || query.trim().length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5 border-b border-[var(--color-line)] px-4 py-1.5 text-[11px]">
          <span className="text-[var(--color-muted)]">视图</span>
          {savedViews.map((view) => (
            <span
              key={view.name}
              className="inline-flex items-center gap-1 rounded-full border border-[var(--color-line)] pl-2 pr-1"
            >
              <button
                className="py-0.5 hover:text-[var(--color-accent)]"
                onClick={() => {
                  setQuery(view.query);
                  if (view.order) setOrder(view.order);
                  setOffset(0);
                }}
              >
                {view.name}
              </button>
              <button
                aria-label={`删除视图 ${view.name}`}
                className="px-1 text-[var(--color-muted)] hover:text-[var(--color-danger)]"
                onClick={() => void persistViews(removeView(savedViews, view.name))}
              >
                ×
              </button>
            </span>
          ))}
          {query.trim().length > 0 ? (
            <button
              className="rounded-full border border-dashed border-[var(--color-line)] px-2 py-0.5 text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-ink)]"
              onClick={() => setViewName(query.trim())}
            >
              ＋ 保存当前
            </button>
          ) : null}
        </div>
      ) : null}

      <Modal
        open={viewName !== null}
        onClose={() => setViewName(null)}
        title="保存视图"
        width="max-w-sm"
      >
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            const name = (viewName ?? "").trim();
            if (name.length === 0) return;
            void persistViews(addView(savedViews, { name, query, order })).then(() =>
              setViewName(null),
            );
          }}
        >
          <Field label="名称" hint="同名视图会被覆盖">
            <Input
              value={viewName ?? ""}
              onChange={(event) => setViewName(event.target.value)}
              aria-label="视图名称"
              autoFocus
            />
          </Field>
          <div className="text-[11px] text-[var(--color-muted)]">查询：{query}</div>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={() => setViewName(null)}>
              取消
            </Button>
            <Button type="submit" variant="primary">
              保存
            </Button>
          </div>
        </form>
      </Modal>

      {selection.size > 0 ? (
        <div className="flex items-center gap-2 border-b border-[var(--color-line)] bg-[var(--color-surface-2)]/60 px-4 py-2 text-xs">
          <span>已选 {selection.size} 笔</span>
          <Button variant="subtle" onClick={() => setBulkOpen(true)}>
            批量修改
          </Button>
          <Button variant="ghost" onClick={() => setSelection(new Set())}>
            取消选择
          </Button>
        </div>
      ) : null}

      <div
        className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden"
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
      >
        {page.isLoading ? (
          <EmptyState title="正在读取流水…" />
        ) : items.length === 0 ? (
          <EmptyState
            title="没有匹配的交易"
            hint={query ? `查询 ${query} 没有结果` : `按 ${shortcutLabel} 记第一笔`}
            action={
              <Button variant="primary" className="mt-2" onClick={() => void api.showCapture()}>
                快速记账
              </Button>
            }
          />
        ) : (
          <div style={{ height: items.length * ROW_HEIGHT, position: "relative" }}>
            <div style={{ transform: `translateY(${visible.start * ROW_HEIGHT}px)` }}>
              {visible.rows.map((txn) => {
                const group = groups.get(txn.date) ?? [];
                const first = group[0]?.id === txn.id;
                return (
                  <React.Fragment key={txn.id}>
                    {first ? (
                      <div className="flex items-center gap-2 bg-[var(--color-surface-2)]/40 px-4 py-1 text-[11px] text-[var(--color-muted)]">
                        <span className="font-medium">{dayLabel(txn.date)}</span>
                        <span>周{weekday(txn.date)}</span>
                      </div>
                    ) : null}
                    <TxnRow
                      txn={txn}
                      selected={selection.has(txn.id)}
                      onToggle={() => toggle(txn.id)}
                      onOpen={() => setEditing(txn)}
                    />
                  </React.Fragment>
                );
              })}
            </div>
          </div>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-[var(--color-line)] px-4 py-2 text-xs text-[var(--color-muted)]">
        <span>
          {total} 笔 · 显示 {offset + 1}–{Math.min(offset + items.length, total)}
        </span>
        <div className="flex gap-2">
          <Button
            variant="ghost"
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - PAGE))}
          >
            上一页
          </Button>
          <Button
            variant="ghost"
            disabled={offset + PAGE >= total}
            onClick={() => setOffset(offset + PAGE)}
          >
            下一页
          </Button>
        </div>
      </div>

      {editing ? (
        <TxnEditor
          txn={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            refresh();
          }}
        />
      ) : null}

      <BulkEditor
        open={bulkOpen}
        ids={Array.from(selection)}
        onClose={() => setBulkOpen(false)}
        onDone={() => {
          setBulkOpen(false);
          setSelection(new Set());
          refresh();
        }}
      />
    </div>
  );
}

function TxnRow({
  txn,
  selected,
  onToggle,
  onOpen,
}: {
  txn: TxnDto_Serialize;
  selected: boolean;
  onToggle: () => void;
  onOpen: () => void;
}) {
  const category = txn.postings.find((p) => p.account.startsWith("expenses") || p.account.startsWith("income"));
  const funding = txn.postings.find((p) => p.account.startsWith("assets") || p.account.startsWith("liabilities"));
  const inbox = txn.postings.some((p) => p.account === "expenses:unclassified");
  return (
    <div
      className={cn(
        "group flex h-14 items-center gap-2 border-b border-[var(--color-line)]/60 px-4 text-sm hover:bg-[var(--color-surface-2)]/50",
        selected && "bg-[var(--color-accent)]/5",
      )}
      style={{ height: ROW_HEIGHT }}
    >
      <input
        type="checkbox"
        checked={selected}
        onChange={onToggle}
        aria-label={`选择 ${txn.payee}`}
        className="accent-[var(--color-accent)]"
      />
      <button onClick={onOpen} className="flex min-w-0 flex-1 items-center gap-3 text-left">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate font-medium">{txn.payee || "(无描述)"}</span>
            {txn.status === "pending" ? <Badge tone="warn">待定</Badge> : null}
            {!txn.balanced ? <Badge tone="danger">不平</Badge> : null}
            {txn.fx_missing ? <Badge tone="warn">缺汇率</Badge> : null}
            {inbox ? <Badge tone="accent">Inbox</Badge> : null}
          </div>
          <div className="truncate text-[11px] text-[var(--color-muted)]">
            {category ? shortAccount(category.account) : "—"}
            {funding ? ` · ${shortAccount(funding.account)}` : ""}
            {txn.tags.length > 0 ? ` · ${txn.tags.map((t) => t.key).join(" ")}` : ""}
          </div>
        </div>
        <div className="text-right font-mono text-sm">
          {money(txn.base_value, txn.base_value_display)}
        </div>
      </button>
    </div>
  );
}

/** The engine owns `txn:`/`rule:`/`query:`/`recomputed:` tokens; never show or
 * re-submit them from the editor. */
const SYSTEM_COMMENT = /\b(?:txn|rule|query|recomputed):\S+/g;

export function userComment(comment: string | null | undefined): string {
  return (comment ?? "").replace(SYSTEM_COMMENT, "").replace(/\s+/g, " ").trim();
}

function TxnEditor({
  txn,
  onClose,
  onSaved,
}: {
  txn: TxnDto_Serialize;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [payee, setPayee] = React.useState(txn.payee);
  const [date, setDate] = React.useState(txn.date);
  const [status, setStatus] = React.useState(txn.status ?? "");
  const [comment, setComment] = React.useState(userComment(txn.comment));
  const [tags, setTags] = React.useState(
    txn.tags
      .filter((t) => t.key !== "txn")
      .map((t) => `${t.key}${t.value ? `:${t.value}` : ""}`)
      .join(" "),
  );
  const [postings, setPostings] = React.useState(
    txn.postings.map((p) => ({ account: p.account, amount: p.amount?.quantity ?? p.quantity, comment: p.comment ?? "" })),
  );
  const [error, setError] = React.useState<string | null>(null);

  const save = useMutation({
    mutationFn: () =>
      api.txnUpdate(txn.id, {
        date,
        payee,
        status: status || null,
        comment: comment || null,
        tags: tags
          .split(/\s+/)
          .filter(Boolean)
          .map((token) => {
            const [key, value] = token.split(":");
            return value ? { key, value } : { key, value: null };
          }),
        postings: postings.map((p) => ({
          account: p.account,
          amount: p.amount,
          comment: p.comment || null,
        })),
      }),
    onSuccess: onSaved,
    onError: (err: unknown) => setError(err instanceof Error ? err.message : String(err)),
  });

  const remove = useMutation({
    mutationFn: () => api.txnDelete(txn.id),
    onSuccess: onSaved,
    onError: (err: unknown) => setError(err instanceof Error ? err.message : String(err)),
  });

  return (
    <Modal open onClose={onClose} title="编辑交易" width="max-w-2xl">
      <div className="space-y-3">
        <div className="grid grid-cols-3 gap-2">
          <Field label="日期">
            <Input value={date} onChange={(e) => setDate(e.target.value)} />
          </Field>
          <Field label="描述">
            <Input value={payee} onChange={(e) => setPayee(e.target.value)} />
          </Field>
          <Field label="状态">
            <Select
              value={status}
              onChange={(e) => setStatus(e.target.value)}
              className="w-full"
            >
              <option value="">未标记</option>
              <option value="cleared">已清算</option>
              <option value="pending">待定</option>
            </Select>
          </Field>
        </div>

        <div className="space-y-1">
          <div className="text-xs font-medium text-[var(--color-muted)]">分录</div>
          {postings.map((posting, index) => (
            <div key={index} className="grid grid-cols-[1fr_120px_28px] gap-2">
              <Input
                value={posting.account}
                aria-label={`账户 ${index + 1}`}
                onChange={(e) =>
                  setPostings((rows) =>
                    rows.map((row, i) => (i === index ? { ...row, account: e.target.value } : row)),
                  )
                }
              />
              <Input
                value={posting.amount}
                aria-label={`金额 ${index + 1}`}
                onChange={(e) =>
                  setPostings((rows) =>
                    rows.map((row, i) => (i === index ? { ...row, amount: e.target.value } : row)),
                  )
                }
              />
              <Button
                variant="ghost"
                aria-label="删除分录"
                onClick={() => setPostings((rows) => rows.filter((_, i) => i !== index))}
              >
                ✕
              </Button>
            </div>
          ))}
          <Button
            variant="ghost"
            onClick={() => setPostings((rows) => [...rows, { account: "", amount: "", comment: "" }])}
          >
            + 添加分录
          </Button>
        </div>

        <Field label="标签" hint="空格分隔；foo:bar 写为键值标签">
          <Input value={tags} onChange={(e) => setTags(e.target.value)} />
        </Field>
        <Field label="注释">
          <Textarea rows={2} value={comment} onChange={(e) => setComment(e.target.value)} />
        </Field>

        <div className="flex items-center gap-2 text-[11px] text-[var(--color-muted)]">
          <span className="truncate">{txn.file}</span>
          <span>{txn.id}</span>
          {!txn.editable ? <span className="text-[var(--color-danger)]">不可编辑</span> : null}
        </div>

        {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}

        <div className="flex justify-between">
          <Button variant="danger" onClick={() => remove.mutate()} disabled={remove.isPending}>
            删除
          </Button>
          <div className="flex gap-2">
            <Button variant="ghost" onClick={onClose}>
              取消
            </Button>
            <Button variant="primary" onClick={() => save.mutate()} disabled={save.isPending}>
              {save.isPending ? "保存中…" : "保存"}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
}

function BulkEditor({
  open,
  ids,
  onClose,
  onDone,
}: {
  open: boolean;
  ids: string[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [addTags, setAddTags] = React.useState("");
  const [removeTags, setRemoveTags] = React.useState("");
  const [from, setFrom] = React.useState("");
  const [to, setTo] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  const apply = useMutation({
    mutationFn: () =>
      api.txnBulkUpdate(ids, {
        add_tags: addTags.split(/\s+/).filter(Boolean),
        remove_tags: removeTags.split(/\s+/).filter(Boolean),
        shift_account: from && to ? [from, to] : null,
      }),
    onSuccess: onDone,
    onError: (err: unknown) => setError(err instanceof Error ? err.message : String(err)),
  });

  return (
    <Modal open={open} onClose={onClose} title={`批量修改 ${ids.length} 笔`}>
      <div className="space-y-3">
        <Field label="添加标签" hint="空格分隔">
          <Input value={addTags} onChange={(e) => setAddTags(e.target.value)} />
        </Field>
        <Field label="移除标签">
          <Input value={removeTags} onChange={(e) => setRemoveTags(e.target.value)} />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Field label="账户前缀（原）">
            <Input value={from} onChange={(e) => setFrom(e.target.value)} placeholder="expenses:餐饮" />
          </Field>
          <Field label="账户前缀（新）">
            <Input value={to} onChange={(e) => setTo(e.target.value)} placeholder="expenses:food" />
          </Field>
        </div>
        {error ? <div className="text-xs text-[var(--color-danger)]">{error}</div> : null}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" onClick={() => apply.mutate()} disabled={apply.isPending}>
            应用
          </Button>
        </div>
      </div>
    </Modal>
  );
}
