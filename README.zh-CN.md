# MoneyTracker

**macOS 上的纯文本复式记账 —— 你的账本就是一份可以读、可以 grep、可以 diff、可以提交的文件。**

[![CI](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml/badge.svg)](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust tests](https://img.shields.io/badge/rust_tests-130_passing-brightgreen.svg)](#测试)
[![Front-end tests](https://img.shields.io/badge/vitest-37_passing-brightgreen.svg)](#测试)
[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/tauri-2.x-24C8DB.svg)](https://tauri.app)
[![Platform](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#环境要求)

[English](README.md) · **简体中文**

MoneyTracker 是一个数据库就是 [hledger](https://hledger.org) 兼容纯文本账本的桌面记账应用：
全局热键快录、规则、多币种折算、预算和丰富看板 —— **没有任何私有存储格式**。删掉应用，账本还在；
它写出的每一个字节，`hledger` 都能直接读。

```ledger
2026-10-04 * 星巴克  ; txn:01JA3KQ70SEED0005 rule:r-star query:desc:星巴克
    expenses:food:coffee         31.50 CNY
    expenses:food                13.50 CNY
    assets:wallet:alipay        -45.00 CNY
```

---

## 目录

- [为什么再造一个记账应用](#为什么再造一个记账应用)
- [功能](#功能)
- [环境要求](#环境要求)
- [安装](#安装)
- [使用](#使用)
- [快录语法](#快录语法)
- [自然语言记一笔（可选）](#自然语言记一笔可选)
- [数据放在哪](#数据放在哪)
- [写入安全](#写入安全)
- [开发](#开发)
- [测试](#测试)
- [项目结构](#项目结构)
- [已知限制](#已知限制)
- [参与贡献](#参与贡献)
- [许可](#许可)

## 为什么再造一个记账应用

大多数记账应用把你的历史锁进只有它能打开的数据库。MoneyTracker 反过来：文件是唯一事实源，
界面只是它的快速编辑器。

- **账本就是一个 git 仓库。** 每笔交易是带 `txn:<id>` 标签的稳定文本块，编辑只重写那一块的字节
  span，diff 小而可审查。
- **天然可互操作。** `hledger balance`、`hledger register`、`hledger check` 都能直接作用在同一份
  账本上 —— 这一点由针对真实 `hledger` 二进制的差分测试保证。
- **记录要快，但不牺牲正确性。** 浮动窗口里一行话，变成一笔平衡、带标签、已展开规则的交易。

## 功能

| | |
| --- | --- |
| **快录** | 全局热键唤出 640×208 悬浮窗，每次按键都在 Rust 侧解析；`Enter` 保存并留在窗口里连续录入 |
| **规则** | `= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝`，写入时展开并记录 `; rule:` 来源；Decimal 精确分摊，余数归最后一笔 |
| **多币种** | 汇率只来自 `P` 指令；缺汇率会被标记、置灰并排除在合计之外 —— 绝不按 1:1 假设 |
| **看板** | KPI 与环比、收支柱状、净资产曲线、分类环图、堆叠面积、日历热力图、预算 vs 实际、预测、桑基图；布局可拖拽缩放并按用户持久化 |
| **流水** | 窗口化列表、行内编辑、批量改标签/账户、保存的查询（`acct:`、`desc:`、`date:`、`tag:`、`amt:` 支持 `and`/`or`/`not`） |
| **账户与分类** | 账户树显示自身/合计余额与类型推断；分类视图可按账户或按标签统计 |
| **双语界面文案** | 当前界面为简体中文；账本内容与代码注释保持中立 |
| **安全性** | 写前快照、原子替换、外部改动检测、JSONL 写入审计 |

## 环境要求

- macOS 13+（Apple Silicon 或 Intel）。快录窗口用 `macos-private-api` 实现透明背景。
- 从源码构建时需要 Rust 1.91+（stable）与 Node.js 22+ / pnpm 10+。

## 安装

发布版本会附带上架用的 `.app`（见 [releases](https://github.com/lingfromSh/moneytracker/releases)）。
自己构建：

```bash
git clone https://github.com/lingfromSh/moneytracker.git
cd moneytracker
pnpm install
pnpm tauri build                 # → src-tauri/target/release/bundle/macos/MoneyTracker.app
```

开发模式：

```bash
pnpm tauri dev                   # Vite :1420 + 带热更新的 Tauri 外壳
```

## 使用

首次启动进入 **Welcome**：新建账本、打开已有目录，或创建样例账本（内容同 `fixtures/vault/sample`）。

| 快捷键 | 动作 |
| --- | --- |
| `⌘⇧Space` | 全局快录窗口（可在设置里改） |
| `⌘K` | 命令面板 |
| `⌘N` | 快录窗口 |
| `⌘F` | 流水 / 搜索 |
| `⌘1` … `⌘6` | 看板、流水、账户、分类、规则、设置 |
| `Enter` / `⌘Enter` / `Esc` | 快录：保存并留下 / 保存并隐藏 / 隐藏 |

托盘图标会在菜单栏旁弹出快录面板；设置里可以调整主题、快捷键、本位币、默认资金账户、
未分类账户、备份数量与汇率同步。

## 快录语法

```
[#日期] [*|!] 描述 金额 [@账户]... [+标签]... [; 备注]
```

| 输入 | 结果 |
| --- | --- |
| `咖啡 4.5 @微信 +餐饮` | `expenses:餐饮 4.50 CNY` / `assets:微信 -4.50 CNY`，标签 `:餐饮:` |
| `工资 +12000 @招行 +收入` | `assets:招行 12000.00 CNY` / `income:工资 -12000.00 CNY` |
| `转 500 > @招行 > @现金` | `assets:招行 -500` / `assets:现金 500` |
| `酒店 100usd@@720cny @旅行` | `expenses:旅行 100 USD @@ 720 CNY` / `assets:<默认> -720 CNY` |
| `火锅 168 @微信 100 @现金 68` | 一笔支出，来自两个账户 |
| `超市 200 @餐饮 120 @日用 80 @现金` | 一笔支出拆到两个分类，资金端自动补齐 |
| `聚餐 300 @餐饮 70% @娱乐 30% @招行` | 按总额百分比拆分（余数给最后一个） |
| `转 500 > @招行 > @现金 200 > @支付宝 300` | 一笔转账同时到两个账户 |
| `"iPhone 15" 8999 @数码` | 描述保留引号，`8999` 是金额 |
| `#昨天 咖啡 4.5` | 记为昨天（也支持 `#前天`、`#+3d`、`#2026-10-01`） |
| `= desc:星巴克 -> 餐饮=70%, 咖啡豆=30% from 支付宝` | 新建或更新规则 |
| `/acct:餐饮` | 带着该查询跳到流水列表 |
| `:settings` | 打开命令面板 |

符号约定：不带符号或 `-` 表示**支出**（钱从资金账户流出），金额前 `+` 表示**收入**。
写一个 `@账户` 时另一侧按账户类型判定（`A`/`L` → 资金端，`R`/`X` → 分类端），或交给命中的规则；
写两个 `@` 账户按 `分类 资金` 理解。既没命中规则也没有分类时，落到 `expenses:unclassified`
并打上 `:inbox:` 标签。

**多个分录。** 任何 `@账户` 后面都可以直接跟金额，这条分录就记这个数，而不是整笔金额：写
`@微信 100 @现金 68` 就是一笔支出由两个钱包出钱；写 `@餐饮 120 @日用 80` 就是把同一笔支出拆到两个
分类。每个分录属于哪一侧仍由账户类型决定，所以两侧可以同时拆。最多只能留一个账户不写金额，剩下的由
程序补平；`@餐饮 70%` 这样的百分比按总额计算，除不尽的余数给最后一个百分比。各分录加起来和总额不
一致时，会在写入之前就报错。

## 自然语言记一笔（可选）

快录窗口有两种模式：**Hledger 语法**（默认）和**自然语言**。按 <kbd>Tab</kbd> 切换，选择会被记住。

```
昨天和朋友吃火锅花了 168，微信付的
        ↓ Tab
将写入   火锅 168 @微信 +餐饮
```

**模型永远不会直接写账本文本。** 它拿到你的句子和一份账户清单，只返回一小段 JSON（日期、
摘要、金额、币种、分类、资金来源、标签、置信度）；MoneyTracker 再把这段 JSON 变成一行普通的
快录语句，后面照旧走解析器、规则展开、复式校验、快照和审计——所以 LLM 记的账和手打的账一样安全，
而且写入前你总能看到那一行。

在**设置 → AI 记一笔**里配置：

| 配置 | 说明 |
| --- | --- |
| 服务商 | OpenAI、Anthropic Claude、DeepSeek、Ollama/本地模型，或自定义端点 |
| Base URL | 留空使用预设；需要网关或代理时再填 |
| 模型 | 自由填写，端点支持哪个就填哪个 |
| 协议 | `OpenAI chat/completions` 或 `Anthropic messages`，只有自定义端点才需要选 |
| API Key | 只写入应用配置目录下的 `llm.json`（权限 `0600`），不写进账本，也不写进 `settings.json` |

默认关闭。没有 Key、没有模型时该模式保持禁用，快录窗口会说明缺什么。"测试连接"会发一次极小的
请求，方便你在依赖它之前确认凭证可用。离开你机器的只有你输入的那句话和账户名，而且只在你按
<kbd>Tab</kbd> 切到自然语言模式时才会发生。

## 数据放在哪

```
<账本>/
  index.ledger                 # 唯一入口：选项、声明、include
  accounts.ledger              # 账户声明，带 `; type:A|L|E|R|X`
  prices.ledger                # `P` 指令 —— 汇率的唯一来源
  budgets.ledger               # `~ monthly` 预算信封
  rules.ledger                 # MoneyTracker 规则 —— 故意不被 index.ledger include
  2026/
    2026.ledger                # include 各月索引
    2026-10.ledger             # include 各日文件
    2026-10-04.ledger          # 一天的交易
  .bak/                        # 写前快照（已被 git 忽略）
```

全部是标准 hledger 语法：`include` / `!include`（支持通配符与循环检测）、带 `type:` 标签的
`account`、`commodity`、`D`、`Y`、`P`、`~` 周期块、`=` 自动分录、`comment`/`end comment`、
`;` `#` `%` `|` 注释、虚拟分录 `(a)` / `[a]`、`@` / `@@` 价格、余额断言，以及标签
（`:a:b:` 与 `key:value`）。

超出这个子集的写法 —— 金额表达式、批次成本 `{}`、`C` / `N` 转换、timeclock、`csv:` 规则 ——
会带着 `文件:行:列` 与稳定的诊断码明确报错。应用永远不会悄悄丢弃看不懂的行。
细节见 [`docs/vault-format.md`](docs/vault-format.md)（技术文档为英文）。

**为什么 `rules.ledger` 不被 include。** 规则在**写入时**展开，结果以普通分录落盘，并用
`; rule:<id> query:<原始输入>` 记录来源。若把它 include 进来，`hledger --auto` 会再应用一次，
数字就不一样了。

## 写入安全

所有写操作都经过 `crates/ledger-engine/src/writer.rs`：

1. 目标文件必须解析无错误；
2. 先把快照复制到 `<账本>/.bak/<相对路径>.<时间戳>`（默认保留 200 份）；
3. 新内容写到同目录临时文件 → `fsync` → `rename` → 目录 `fsync`；
4. 解析时记录的 `(mtime, size, blake3)` 必须仍然匹配，否则拒绝写入并提示
   *“file was modified outside MoneyTracker — refresh and try again”*；
5. 操作追加到 `<appSupport>/dev.moneytracker.app/write-audit.jsonl`。

编辑交易时只重写**那一笔的字节 span** —— 引擎测试逐字节断言文件其余部分不变。`notify` 监听器
带 300 ms 去抖，外部改动（`git pull`、`vim`）会即时出现在界面上。

## 开发

```bash
pnpm install
pnpm tauri dev                   # 带热更新的应用
pnpm bindings                    # 由 Rust 命令重新生成 src/lib/bindings.ts
./scripts/smoke.sh               # 完整验证流程（见下）
```

前置：Rust stable 1.91+、Node 22+、pnpm 10+；差分测试需要 `hledger`（`brew install hledger`）。

## 测试

```bash
cargo test --workspace                        # 110 个 Rust 测试，12 个 suite
cargo clippy --workspace --all-targets -D warnings
cargo fmt --all -- --check
pnpm test                                     # 15 个 Vitest 测试
pnpm build                                    # tsc --noEmit && vite build
```

| 层 | 覆盖内容 |
| --- | --- |
| `crates/ledger-core/tests/corpus.rs` | `fixtures/grammar/*.ledger` 解析无错、打印→解析→打印逐字节一致；`fixtures/errors/*.ledger` 产出文档化的诊断码与行号 |
| `crates/ledger-engine` | include 图、通配符、循环、账户类型推断、汇率、规则、快录展开、报表、写入安全，以及仓库内的样例账本端到端 |
| `crates/ledger-engine/tests/hledger_diff.rs` | `Vault::balances()` 与 `hledger balance --flat --no-total -O csv` 完全一致；`hledger check` 接受引擎刚写出的账本（无 `hledger` 时跳过） |
| `src-tauri/src/editing.rs` | 快录 → 落盘 → 重载、span 级修改、删除、批量修改，以及文件被外部改动时拒绝写入 |
| `src/**/*.test.tsx` | 快录面板、看板、流水列表（mock IPC 层） |

`scripts/smoke.sh` 会跑完上述全部检查，并在提交的 TypeScript 绑定过期时失败。

性能（M 系列笔记本，`cargo run -q --release -p ledger-engine --example bench_load -- 100000`
生成 10 万笔交易、5183 个文件）：

| 阶段 | 耗时 |
| --- | --- |
| 冷启动 `Vault::load`（读取 + 解析 + 建索引） | ~760 ms |
| `snapshot()`（账户树、余额） | ~70 ms |
| `dashboard(12 个月)` | ~100 ms |

## 项目结构

```
crates/ledger-core     AST、字节 span 解析器、打印器、诊断
crates/ledger-query    hledger 查询子集：acct/desc/date/tag/amt，and/or/not
crates/ledger-engine   include 图、索引、余额、快录、规则、汇率、报表、安全写入
src-tauri              窗口、托盘、全局快捷键、文件监听、IPC
src                    React 19 + Vite + Tailwind v4 + Recharts + d3-sankey
fixtures/              语法语料、错误语料、样例账本
docs/                 架构与账本格式
```

类型全部由代码生成，绝不手写：`src-tauri/examples/export_bindings.rs` 通过 `tauri-specta`
把每个命令与事件导出到 `src/lib/bindings.ts`。模块划分、命令清单与快录状态机见
[`docs/architecture.md`](docs/architecture.md)。

## 已知限制

- 有意不做：银行导入/对账、附件、多用户同步、预算自动结转、云存储（请自己把账本放进 iCloud 或 git）。
- 支持的方言就是上面那个子集；其余写法一律报错，而不是近似处理。
- 不内置 `hledger`，只有它在 `PATH` 上时才做交叉核对。
- 目前只支持 macOS：Rust 核心是可移植的，窗口/托盘层不是。
- `specta` 2.0.0-rc.26 以 `vendor/specta` 形式内置并打了两行补丁，因为 stable Rust 没有它使用的
  `fmt::from_fn`，而 `tauri-specta` 又精确锁定了这个版本。

## 参与贡献

欢迎 bug 报告、语料样本与 PR —— 见 [CONTRIBUTING.md](CONTRIBUTING.md)。提 PR 前请先跑
`./scripts/smoke.sh`；版本记录在 [CHANGELOG.md](CHANGELOG.md)。

## 许可

[MIT](LICENSE) © 2026 lingfromSh

---

**[English documentation →](README.md)**
