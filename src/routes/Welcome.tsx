import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/ipc";
import { Button, Card, Field, Input } from "../components/ui";
import { useShortcutLabel } from "../lib/shortcut";

/**
 * First-run screen: create a vault, open an existing one, or drop in the sample
 * ledger. Everything here writes plain files — nothing is locked inside the app.
 */
export function Welcome() {
  const shortcutLabel = useShortcutLabel();
  const queryClient = useQueryClient();
  const [path, setPath] = React.useState("");
  const [commodity, setCommodity] = React.useState("CNY");
  const [funding, setFunding] = React.useState("assets:cash");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const refresh = () => void queryClient.invalidateQueries();

  const pick = async () => {
    try {
      const picked = await open({ directory: true, multiple: false, title: "选择目录" });
      if (typeof picked === "string") setPath(picked);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const run = async (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      refresh();
      window.location.hash = `#/dashboard`;
      void done;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-full max-w-3xl space-y-4">
        <div className="space-y-1 text-center">
          <div className="text-2xl font-semibold">MoneyTracker</div>
          <div className="text-sm text-[var(--color-muted)]">
            明文复式记账。账本就是目录里的 .ledger 文件，hledger 可以直接读。
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-3">
          <Card className="space-y-3 p-4">
            <div className="text-sm font-semibold">新建账本</div>
            <Field label="目录">
              <div className="flex gap-2">
                <Input value={path} onChange={(e) => setPath(e.target.value)} placeholder="~/Documents/ledger" />
                <Button variant="subtle" onClick={pick}>
                  选择
                </Button>
              </div>
            </Field>
            <div className="grid grid-cols-2 gap-2">
              <Field label="基准币种">
                <Input value={commodity} onChange={(e) => setCommodity(e.target.value.toUpperCase())} />
              </Field>
              <Field label="默认资金账户">
                <Input value={funding} onChange={(e) => setFunding(e.target.value)} />
              </Field>
            </div>
            <Button
              variant="primary"
              className="w-full"
              disabled={!path.trim() || busy}
              onClick={() =>
                run(
                  () =>
                    api.vaultCreate(path.trim(), {
                      base_commodity: commodity,
                      default_funding: funding,
                      default_unclassified: "expenses:unclassified",
                      template: true,
                    }),
                  "created",
                )
              }
            >
              创建
            </Button>
          </Card>

          <Card className="space-y-3 p-4">
            <div className="text-sm font-semibold">打开已有账本</div>
            <div className="text-[11px] text-[var(--color-muted)]">
              目录里需要有 <code>index.ledger</code>；已有 hledger 账本可直接打开。
            </div>
            <Button variant="subtle" className="w-full" disabled={busy} onClick={pick}>
              选择目录
            </Button>
            <Button
              variant="subtle"
              className="w-full"
              disabled={!path.trim() || busy}
              onClick={() => run(() => api.vaultOpen(path.trim()), "opened")}
            >
              打开输入的路径
            </Button>
          </Card>

          <Card className="space-y-2 p-4">
            <div className="text-sm font-semibold">先看看样例</div>
            <div className="text-[11px] text-[var(--color-muted)]">
              生成一个带工资、房租和咖啡的小账本，随时可以删掉。
            </div>
            <Button
              variant="primary"
              className="w-full"
              disabled={busy}
              onClick={() => run(() => api.createSampleVault(), "sample")}
            >
              创建样例账本
            </Button>
          </Card>
        </div>

        {error ? <div className="text-center text-xs text-[var(--color-danger)]">{error}</div> : null}
        <div className="text-center text-[11px] text-[var(--color-muted)]">
          {shortcutLabel} 随时唤起快录窗口
        </div>
      </div>
    </div>
  );
}
