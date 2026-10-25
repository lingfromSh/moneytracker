import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, onLedgerChanged, onParseDiagnostics, queryKeys } from "./lib/ipc";
import { cn } from "./lib/format";
import { Button, Modal, Input } from "./components/ui";
import { CaptureWindow } from "./routes/Capture";
import { Dashboard } from "./routes/Dashboard";
import { Transactions } from "./routes/Transactions";
import { Accounts } from "./routes/Accounts";
import { Categories } from "./routes/Categories";
import { Rules } from "./routes/Rules";
import { Import } from "./routes/Import";
import { Plan } from "./routes/Plan";
import { Schedule } from "./routes/Schedule";
import { Reports } from "./routes/Reports";
import { SettingsView } from "./routes/Settings";
import { Welcome } from "./routes/Welcome";
import { useShortcutLabel } from "./lib/shortcut";
import { isMobile } from "./lib/platform";

type Route =
  | "capture"
  | "dashboard"
  | "transactions"
  | "import"
  | "accounts"
  | "categories"
  | "rules"
  | "plan"
  | "schedule"
  | "reports"
  | "settings";

const NAV: { route: Route; label: string; hint: string }[] = [
  { route: "dashboard", label: "看板", hint: "⌘1" },
  { route: "transactions", label: "流水", hint: "⌘2" },
  { route: "import", label: "导入", hint: "⌘3" },
  { route: "accounts", label: "账户", hint: "⌘4" },
  { route: "categories", label: "分类", hint: "⌘5" },
  { route: "rules", label: "规则", hint: "⌘6" },
  { route: "plan", label: "规划", hint: "⌘7" },
  { route: "schedule", label: "日程", hint: "⌘8" },
  { route: "reports", label: "报表", hint: "⌘9" },
  { route: "settings", label: "设置", hint: "⌘0" },
];

function parseHash(): Route {
  const raw = window.location.hash.replace(/^#\/?/, "");
  const name = raw.split("?")[0];
  if (name === "capture") return "capture";
  const match = NAV.find((n) => n.route === name);
  return match ? match.route : "dashboard";
}

export function useRoute(): [Route, (next: Route) => void] {
  const [route, setRoute] = React.useState<Route>(parseHash);

  React.useEffect(() => {
    const onHash = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const go = React.useCallback((next: Route) => {
    window.location.hash = `#/${next}`;
    setRoute(next);
  }, []);

  return [route, go];
}

export function App() {
  const [route, go] = useRoute();
  if (route === "capture")
    return <CaptureWindow onDone={() => go("dashboard")} />;
  return <Shell route={route} go={go} />;
}

function Shell({ route, go }: { route: Route; go: (r: Route) => void }) {
  const queryClient = useQueryClient();
  const shortcutLabel = useShortcutLabel();
  const [paletteOpen, setPaletteOpen] = React.useState(false);
  const [paletteQuery, setPaletteQuery] = React.useState("");
  const [diagCount, setDiagCount] = React.useState(0);

  const info = useQuery({
    queryKey: queryKeys.snapshot,
    queryFn: () => api.vaultInfo(),
    retry: 0,
  });

  // Any ledger mutation (ours or an external editor) invalidates every projection.
  React.useEffect(() => {
    const off = onLedgerChanged(() => {
      void queryClient.invalidateQueries();
    });
    return () => {
      void off.then((unlisten) => unlisten());
    };
  }, [queryClient]);

  React.useEffect(() => {
    const off = onParseDiagnostics((event) =>
      setDiagCount(event.payload.count ?? 0),
    );
    return () => {
      void off.then((unlisten) => unlisten());
    };
  }, []);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }
      if (e.key.toLowerCase() === "n") {
        e.preventDefault();
        void api.showCapture();
        return;
      }
      if (e.key.toLowerCase() === "f") {
        e.preventDefault();
        go("transactions");
        return;
      }
      // Shortcuts are declared on each nav entry, so adding a page does not
      // silently shift every later ⌘number.
      const target = NAV.find((item) => item.hint === `⌘${e.key}`);
      if (target) {
        e.preventDefault();
        go(target.route);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go]);

  const pendingSearch = React.useMemo(() => {
    const value = window.localStorage.getItem("moneytracker.pendingSearch");
    if (value) window.localStorage.removeItem("moneytracker.pendingSearch");
    return value ?? "";
  }, [route]);

  const hasVault = Boolean(info.data);
  const mobile = isMobile;
  const capture = () => {
    if (mobile) go("capture");
    else void api.showCapture();
  };

  return (
    <div className={cn("flex h-full w-full", mobile && "flex-col")}>
      {mobile ? (
        <header className="flex shrink-0 items-center gap-2 border-b border-[var(--color-line)] bg-[var(--color-surface-2)]/60 px-3 py-2">
          <span className="grid h-6 w-6 place-items-center rounded-md bg-[var(--color-accent)] text-[11px] font-bold text-white">
            MT
          </span>
          <span className="text-sm font-semibold">
            {NAV.find((item) => item.route === route)?.label ?? "MoneyTracker"}
          </span>
          <Button variant="primary" className="ml-auto" onClick={capture}>
            记一笔
          </Button>
        </header>
      ) : (
        <aside className="flex w-48 shrink-0 flex-col gap-1 border-r border-[var(--color-line)] bg-[var(--color-surface-2)]/60 p-3">
          <div className="mb-3 flex items-center gap-2 px-1">
            <span className="grid h-6 w-6 place-items-center rounded-md bg-[var(--color-accent)] text-[11px] font-bold text-white">
              MT
            </span>
            <span className="text-sm font-semibold">MoneyTracker</span>
          </div>

          <Button
            variant="primary"
            className="mb-2 justify-start"
            onClick={capture}
          >
            快速记账
            <span className="ml-auto text-[11px] opacity-70">
              {shortcutLabel}
            </span>
          </Button>

          {NAV.map((item) => (
            <button
              key={item.route}
              onClick={() => go(item.route)}
              className={cn(
                "flex items-center justify-between rounded-lg px-2 py-1.5 text-sm transition",
                route === item.route
                  ? "bg-[var(--color-surface)] font-medium shadow-sm"
                  : "hover:bg-[var(--color-surface)]/70",
              )}
            >
              <span>{item.label}</span>
              <span className="text-[11px] text-[var(--color-muted)]">
                {item.hint}
              </span>
            </button>
          ))}

          <div className="mt-auto space-y-1 px-1 text-[11px] text-[var(--color-muted)]">
            {info.data ? (
              <>
                <div className="truncate" title={info.data.path}>
                  {info.data.base_commodity} ·{" "}
                  {info.data.transaction_count ?? 0} 笔
                </div>
                <div className="truncate">{info.data.path}</div>
              </>
            ) : (
              <div>未打开账本</div>
            )}
            {diagCount > 0 ? (
              <div className="text-amber-600 dark:text-amber-400">
                {diagCount} 条解析诊断
              </div>
            ) : null}
          </div>
        </aside>
      )}

      <main
        className={cn(
          "min-w-0 flex-1",
          mobile ? "overflow-y-auto" : "overflow-hidden",
        )}
      >
        {!hasVault && route !== "settings" ? (
          <Welcome />
        ) : (
          <>
            {route === "dashboard" && <Dashboard />}
            {route === "transactions" && (
              <Transactions initialQuery={pendingSearch} />
            )}
            {route === "import" && <Import />}
            {route === "accounts" && <Accounts />}
            {route === "categories" && <Categories />}
            {route === "rules" && <Rules onCapture={capture} />}
            {route === "plan" && <Plan />}
            {route === "schedule" && <Schedule />}
            {route === "reports" && <Reports />}
            {route === "settings" && <SettingsView />}
          </>
        )}
      </main>

      {mobile ? (
        <nav className="flex shrink-0 gap-1 overflow-x-auto border-t border-[var(--color-line)] bg-[var(--color-surface-2)]/60 px-2 py-1.5">
          {NAV.map((item) => (
            <button
              key={item.route}
              onClick={() => go(item.route)}
              className={cn(
                "shrink-0 rounded-lg px-3 py-1.5 text-xs transition",
                route === item.route
                  ? "bg-[var(--color-surface)] font-medium shadow-sm"
                  : "text-[var(--color-muted)]",
              )}
            >
              {item.label}
            </button>
          ))}
        </nav>
      ) : null}

      <Modal
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        title="命令面板"
      >
        <div className="space-y-2">
          <Input
            autoFocus
            value={paletteQuery}
            placeholder="输入查询语句，例如 acct:餐饮 date:2026-10"
            onChange={(e) => setPaletteQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== "Enter") return;
              const q = paletteQuery.trim();
              window.localStorage.setItem("moneytracker.pendingSearch", q);
              setPaletteOpen(false);
              go("transactions");
            }}
          />
          <div className="text-[11px] text-[var(--color-muted)]">
            支持 <code>acct:</code> <code>desc:</code> <code>date:</code>{" "}
            <code>tag:</code> <code>amt:</code>，可用 and / or / not 组合。
          </div>
        </div>
      </Modal>
    </div>
  );
}
