import * as React from "react";
import { Play } from "lucide-react";
import type { MaintenanceDto_Serialize, Settings_Serialize } from "../lib/bindings";
import { api } from "../lib/ipc";
import { Button, Switch } from "./ui";

type Props = {
  draft: Settings_Serialize;
  update: (key: "auto_recur" | "git_auto_commit", value: boolean) => void;
  onMessage: (message: string | null) => void;
  onError: (message: string | null) => void;
};

/**
 * The automation pass: confirm recurring entries, drop a monthly report into
 * `reports/`, commit the vault to git, and report budgets close to their limit.
 */
export function Automation({ draft, update, onMessage, onError }: Props) {
  const [busy, setBusy] = React.useState(false);
  const [last, setLast] = React.useState<MaintenanceDto_Serialize | null>(null);

  const run = async () => {
    setBusy(true);
    onError(null);
    onMessage(null);
    try {
      const outcome = await api.maintenanceRun();
      setLast(outcome);
      const parts: string[] = [];
      if (outcome.recur) parts.push(`定期 ${outcome.recur.written} 笔`);
      if (outcome.schedule) parts.push(`日程 ${outcome.schedule.written} 笔`);
      if (outcome.report) parts.push(outcome.report);
      if (outcome.git) parts.push(`git ${outcome.git}`);
      onMessage(parts.length > 0 ? `维护完成：${parts.join(" · ")}` : "维护完成，没有需要处理的内容");
    } catch (cause) {
      onError((cause as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
        <div>
          <div className="text-sm">自动记入定期交易与固定收支</div>
          <div className="text-[11px] text-[var(--color-muted)]">
            打开后，启动时会把到期的定期交易和 recurring.ledger 里的固定收支直接写入（仍可手工改）
          </div>
        </div>
        <Switch
          checked={draft.auto_recur}
          onChange={(next) => update("auto_recur", next)}
          label="自动记入定期交易与固定收支"
        />
      </div>

      <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
        <div>
          <div className="text-sm">账本自动提交到 git</div>
          <div className="text-[11px] text-[var(--color-muted)]">
            账本目录里有 <code>.git</code> 时，维护后提交一次快照
          </div>
        </div>
        <Switch
          checked={draft.git_auto_commit}
          onChange={(next) => update("git_auto_commit", next)}
          label="账本自动提交到 git"
        />
      </div>

      <div className="flex items-center gap-2">
        <Button variant="subtle" onClick={() => void run()} disabled={busy}>
          <Play className="mr-1 h-3.5 w-3.5" />
          立即维护
        </Button>
        <span className="text-[11px] text-[var(--color-muted)]">
          月度报告写到 <code>reports/YYYY-MM.md</code>，预算超过 90% 会在这里提示
        </span>
      </div>

      {last && last.alerts.length > 0 ? (
        <ul className="space-y-1 text-[11px] text-amber-600 dark:text-amber-400">
          {last.alerts.map((alert) => (
            <li key={alert}>· {alert}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
