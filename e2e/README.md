# Manual smoke checklist (macOS)

Automate what can be automated with `./scripts/smoke.sh`; the items below need a real window
server, a real tray or a real keyboard shortcut.

## Setup

```bash
pnpm install
pnpm tauri dev
```

Pick **创建样例账本** on the Welcome screen (or open `fixtures/vault/sample` directly, read-only if
you want to keep the fixture pristine — copy it first).

## Capture window

1. Press `⌘⇧Space` from another app. The 640×148 panel appears above the frontmost window.
2. Type `咖啡 4.5 @微信 +餐饮` — the preview shows two postings and a `:餐饮:` tag; `Enter` saves and
   keeps the panel open with an empty input.
3. Enter four more entries in a row: `工资 +12000 @招行 +收入`, `转 500 > @招行 > @现金`,
   `酒店 100usd@@720cny @旅行`, `星巴克 45 @微信`.
4. `⌘Enter` saves and hides. `Esc` hides without saving.

## Files on disk

```bash
cd <vault>
cat 2026/$(date +%Y-%m)-$(date +%d).ledger     # canonical blocks, one per entry
git status / git diff                          # only the day file changed
hledger -f index.ledger balance --flat --no-total   # matches Accounts / Dashboard
```

Each block must carry `; txn:<uuid7>`, and the four rule-driven entries must carry
`; rule:<id> query:<原文>` (or `rule:none` for the ones no rule matched).

## Editing

5. Transactions → click a row → change 描述 → 保存. `git diff` shows **only that block** changed.
6. Delete a row and confirm the block plus one blank line disappears and the rest of the file is
   byte-identical (`git diff --stat` shows a single file with a small delta).
7. Select two rows → 批量修改 → add a tag → both headers change, postings untouched.

## External changes

8. With the app open, edit a day file in `$EDITOR` and save. Within ~300 ms the list refreshes and
   the change appears; no dialog, no reload button needed.
9. Now edit the file again *while the app is showing a stale copy* (e.g. pause the watcher by
   writing from a script that touches the file twice, or simply edit the file the app is about to
   write) and try to save an edit: the app refuses with
   *“file was modified outside MoneyTracker — refresh and try again”*.

## Tray and settings

10. Left-click the tray icon: the capture panel is positioned at the tray (`TrayCenter`). Right
    click shows 快速记账 / 打开 MoneyTracker / 退出.
11. Settings → change the shortcut, press the new combination, confirm the old one stops working.
12. Settings → toggle 深色/浅色/跟随系统 and confirm the dashboard restyles without a restart.
13. Dashboard → drag and resize panels, restart the app, confirm the layout is restored (it is
    stored in the app config directory, not in the vault).

## Failure surfaces

14. Add a line the subset does not support (e.g. `expenses:stock  10 AAPL {150.00 USD}`). The
    diagnostics banner shows a `file:line:col: error[core.parse.unsupported_lot_price]` entry and
    that transaction is not editable — everything else keeps working.
15. Remove a `P` line so a USD transaction has no rate: the dashboard shows the grey 未折算 column,
    the transaction is flagged, and totals exclude it.

## Bundle

```bash
pnpm tauri build
open src-tauri/target/release/bundle/macos/MoneyTracker.app
```

Icons, identifier `dev.moneytracker.app`, and the `macos-private-api` flag (needed for the
transparent capture window) are configured in `src-tauri/tauri.conf.json`.
