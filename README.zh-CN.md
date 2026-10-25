# MoneyTracker

**macOS 上的纯文本复式记账 —— 你的账本就是一份可以读、可以 grep、可以 diff、可以提交的文件。**

[![CI](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml/badge.svg)](https://github.com/lingfromSh/moneytracker/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust tests](https://img.shields.io/badge/rust_tests-229_passing-brightgreen.svg)](#测试)
[![Front-end tests](https://img.shields.io/badge/vitest-77_passing-brightgreen.svg)](#测试)
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
- [导入对账单](#导入对账单)
- [定期交易与自动化](#定期交易与自动化)
- [固定收支日程](#固定收支日程)
- [多端同步](#多端同步)
- [安卓](#安卓)
- [洞察与提问](#洞察与提问)
- [个性化](#个性化)
- [规划与报表](#规划与报表)
- [数据放在哪](#数据放在哪)
- [写入安全](#写入安全)
- [开发](#开发)
- [测试](#测试)
  - [示例账本](#示例账本)
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
| **导入** | 支付宝 / 微信支付 / 银行 CSV：自动识别列、指纹去重、先预览再写入、关键词→分类规则，并可对账户做对账 |
| **日程** | `recurring.ledger` 里手写 `~ monthly` 模板：工资（应发+公司缴纳+五险二金）、房贷本息、房租、订阅；日程页按 6/12/24 个月列出到期项，一键记入或跳过，看板有「固定收支」卡片 |
| **自动化** | 自动发现按月重复的收支（同摘要、同金额、同分录，≥3 个月），一键记入下个月并打 `recur:<id>@<月份>` 标签；每月生成 `reports/` 报告、可选提交 git、预算超 90% 提醒 |
| **同步** | 把账本镜像到任意 S3 兼容存储（MinIO / R2 / S3），靠 manifest 做三方比对；两边都改过的文件**停下来问你**，绝不静默覆盖 |
| **规划** | `goals.ledger` 驱动财富自由测算（SWR 3.0/3.5/4.0%）、储备月数、带目标日期的账户目标、负债还清日期（含“多还一点能省多少”）以及按 `class:` 标签的配置偏离 |
| **报表** | 资产负债表、任意月/季/年的损益表，以及把期末净资产对齐到「期初 + 收入 − 支出」的净值变动表 |
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
| `⌘1` … `⌘0` | 看板、流水、导入、账户、分类、规则、规划、日程、报表、设置 |
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

**模型永远不会直接写账本文本。** 它拿到你的句子、**当前账本自己的账户目录**（资产、负债、收入、
支出、权益，包括 `rules.ledger` 里已经打算使用的账户），以及用这些真实账户写成的示例，只返回一小段
JSON（日期、摘要、金额、币种、分类、资金来源、标签、置信度）；MoneyTracker 再把这段 JSON 变成一行
普通的快录语句，后面照旧走解析器、规则展开、复式校验、快照和审计——所以 LLM 记的账和手打的账一样
安全，而且写入前你总能看到那一行。

草稿在变成快录行之前还会被「落地」，因为模型很爱编账户：

- 不存在的账户会收敛到你真正拥有的账户——账本只有 `assets:wallet` 时，`assets:wallet:alipay`
  会变成 `assets:wallet`；
- **你句子里说的支付方式优先于模型的猜测**——「用微信付的」会选中微信钱包账户，没有子账户时用钱包父账户；
- `NONE` 这类占位币种会被丢弃，`45.0` 会写成 `45`。

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

## 导入对账单

支付宝、微信支付和银行导出的 CSV 往往很乱，**导入**（`⌘3`）把它们变成账本条目，不需要手输，
也不会把同一行写两遍：

1. 选择 CSV：自动识别编码（UTF-8/GBK）、分隔符、表头，以及日期 / 对方 / 金额列，并识别
   支付宝、微信支付、招商、工商等常见布局。
2. 确认识别结果：账户（`assets:bank:cmb` …）、各列映射，必要时填日期格式（如 `%Y/%m/%d`）。
3. 写分类规则，一行一条：`星巴克 => expenses:food:coffee :coffee`。首个命中的关键词（或
   `re:正则`）生效，没命中的进 `expenses:unclassified` 并打 `:inbox:`。
4. 预览：重复行用 `blake3(账户|日期|金额|币种|摘要)` 与账本里已有交易比对，所以重复导入
   同一段时间的账单是安全的。不需要的行可以取消勾选。
5. 导入。写入的行带 `import:<批次>` 和 `ref:<银行流水号>`，批次可追溯，重复导入不会重复记账。

**对账**（`账户` → **对账**）是另一半：选账户和截止日期，账本会区分已对清（`*`）和未对清；
把对账单上出现的交易标记为已对清。账户里写了 `= 余额` 断言时，同一面板还会告诉你账本余额
是否和银行一致。

## 定期交易与自动化

账本本身就知道哪些是重复的：同一摘要、同一金额、同一组分录，在至少三个不同月份出现，间隔
20–45 天。这些模式会显示在流水页顶部，可以 **全部记入**（写入下个月的那一笔）或 **本月不算**
（跳过这一期）。除非你在 设置 → 自动化 打开「自动记入定期交易与固定收支」，否则不会自动写入。

写入的每一笔都是普通交易，只多一个 `recur:<id>@<YYYY-MM>` 标签，批次可追溯，重复检测也不会
把同一个月记两遍。

设置 → 自动化 还负责维护任务（「立即维护」，开启后启动时也会跑一次）：

| 步骤 | 作用 |
| --- | --- |
| 定期交易 | 确认所有到期的定期交易 |
| 月度报告 | 每月写一次 `reports/YYYY-MM.md`（Markdown，数字与报表一致） |
| git 快照 | 账本目录里有 `.git` 时暂存并提交 |
| 预算提醒 | 在设置卡片里列出超过 90% 或已超支的预算 |

跳过的期数记录为 `recur.ledger` 中的 `; skip:<id>@<YYYY-MM>` 行——和 `rules.ledger` 一样，
`index.ledger` 不会 include 它。

## 固定收支日程

检测出来的重复（上一节）只能看见 **已经发生** 的事。还有一半是「我知道每月一定会发生」：工资、
房贷、房租、订阅。这些写进 `recurring.ledger`，语法就是 hledger 的周期块：

```ledger
~ monthly from 2025-08-05  ; id:salary-main label:工资 kind:salary gross:30000.00 personal:5250.00 deduct:1000.00 tax:expenses:个税
    income:salary                        -30,000.00 CNY
    income:benefit                       -9,900.00 CNY  ; 公司缴纳侧
    assets:cn:fund                         4,200.00 CNY  ; 公积金 个人 2,100 + 公司 2,100
    assets:cn:pension                      7,200.00 CNY  ; 养老 个人 2,400 + 公司 4,800
    expenses:insurance:medical             3,000.00 CNY  ; 医疗 个人 600 + 公司 2,400
    expenses:insurance:unemployment          300.00 CNY
    expenses:insurance:injury                150.00 CNY
    expenses:insurance:maternity             300.00 CNY
    assets:bank:cmb                       24,750.00 CNY  ; 到手（税前，现金腿）
```

几条约定：

- 头部注释里的 `id:` / `label:` / `kind:`（工资 / 还款 / 收入 / 支出）/ `enabled:` 就是元数据。
  「每月几号」由 `from` 后面的日期决定（`from 2026-01-31` 遇到小月自动收到月末），hledger 也是
  这么读的，所以文件在 App 外面同样有效。
- **最后一条 posting 是现金腿**，卡片上的「现金流」就取它；金额可以省略，由引擎补平。
- 五险一金的归属按「能不能取出来」分：公积金、个人养老账户是 `assets:cn:*`（钱还是你的），
  医疗、失业、工伤、生育是 `expenses:insurance:*`（统筹掉了）。公司缴纳的部分也一起入账，
  所以收入是 39,900，而不是 30,000。公积金按上海口径各付 7%，换城市就改
  `crates/ledger-engine/src/schedule.rs` 里那两个 `bps`。**企业年金不生成**：它只有公司建了
  年金计划才有，有的话自己加两条 `assets:cn:annuity` / `expenses:annuity`。
- **个税是估出来的，但记入前会让你确认。** 表头里的 `gross:` / `personal:` / `deduct:` /
  `tax:` 让「记入」能按累计预扣法算个税（3%–45% 七级税率、5,000 起征点、`deduct:` 里的专项附加
  扣除、再减去同一年前面几个月已经预缴的）。确认框里预填的就是估算值（月度应纳税所得额
  18,750.00 → 562.50），照工资条改成 562.50 之类也可以，改完写进 `expenses:个税`，下个月的
  「已预缴」就从账本里读回来。
- 只是写模板不会产生任何交易。「记入」才按普通交易写进日文件并打 `sched:<id>` 标签；打开
  设置 → 自动化 的「自动记入固定收支」后，每月维护任务会把当月的到期项补上。
- 不想自己算比例就用 **日程 → 工资模板**：输入应发工资和到手账户，它会按
  个人 8%/2%/0.5%/7%、公司 16%/8%/0.5%/0.5%/1%/7% 生成整段，预览后写入文件。

`recurring.ledger` 和 `rules.ledger`、`goals.ledger` 一样 **不被 `index.ledger` 包含**：它是计划，
不是已经发生的钱。否则 hledger、预算和现金流预测都会把同一个月算两遍。

## 多端同步

账本就是一个文本目录，同步账本＝同步文件。MoneyTracker 可以把它镜像到任意 S3 兼容存储
（MinIO / Cloudflare R2 / AWS S3 / B2 …），并记住每台设备上次一致时的哈希，于是「谁改了什么」
变成一个三方问题：

| 本机 | 云端 | 上次一致 | 结果 |
| --- | --- | --- | --- |
| 改过 | 没改 | — | 上传 |
| 没改 | 改过 | — | 下载 |
| 改过 | 改过 | 哈希相同 | 两边一致，什么都不做 |
| 改过 | 改过 | 哈希不同 | **冲突：这次同步停下来问你** |
| 改过 | 被删 | — | 对面也删掉 |
| 改过 | 改过 | 没有基线 | **冲突**（两边各自新建了同名文件） |

配置在「设置 → 同步」：端点、bucket、前缀（一个 bucket 可以放多个账本）、区域、MinIO 需要的
路径风格访问，以及两个密钥 —— 密钥写在 `settings.json` 旁边的 `sync.json`（`0600`），不会进账本。
**测试连接**只读 bucket，**预览差异**告诉你这次会做什么但不写任何东西，**立即同步**才真正执行。

- 只有 `*.ledger` 参与同步，`.bak/`、`.git/`、`.sync/` 和编辑器临时文件都留在本机。
- 文件先下载 blob 并校验 blake3 再落盘，且走和界面同一套原子写入，不会出现「同步了一半」的文件。
- 被删除的文件不直接 unlink，而是移到 `.sync/trash/<时间戳>/`。
- manifest 用 `If-Match` 替换：两台设备同时推送时是重试，而不是丢更新。任何冲突都不会自动裁决 ——
  逐个文件选「保留本机」或「用云端」，或者自己打开文件合并。
- 桶里是明文。请用私有 bucket + HTTPS + 只能访问该前缀的密钥。

打开「启动时自动拉取」后，启动会先合并云端改动（并通过 `sync-finished` 事件告诉界面），
手机上的改动在你开始记账之前就已经在了。

## 安卓

手机版不是复刻：还是同一份 `ledger-core` / `ledger-engine` / `ledger-sync`，只是把桌面专属的层（托盘图标、菜单栏定位、全局快捷键、置顶记账小窗）在编译期摘掉。

已经打好包的安装包在 [Releases](https://github.com/lingfromSh/moneytracker/releases/latest) 里，下载 `moneytracker-<版本>-arm64.apk` 直接装（需要允许「未知来源」）。


| 手机上 | 行为 |
| --- | --- |
| 首次启动 | 在 App 私有目录（`/data/data/dev.moneytracker.app/files/ledger`）自动建账本并打开，不需要选文件夹 |
| 记账 | 变成主窗口里的整页路由；侧边栏变底部标签栏 |
| 看板 | 卡片按你在桌面上排好的顺序堆成一列，并且禁用拖拽，让滚动优先 |
| 同步 | 完全一样：设置 → 同步 里填 endpoint / bucket / 密钥 |
| 托盘、全局快捷键 | 不可用 —— 手机上不存在这两个概念 |

打安装包：

```bash
scripts/android-apk.sh          # release / arm64 / 已签名
adb install -r build/android/moneytracker-*.apk
```

需要 Android SDK（`platform-tools`、`platforms;android-35`、`build-tools;35.0.0`、`ndk;27.2.12479018`）、JDK 17+ 和 `rustup target add aarch64-linux-android`。签名密钥在 `~/.moneytracker-android/moneytracker.jks`，**务必备份**：只有同一把钥匙签出来的新版本才能覆盖安装。口令不写进仓库：脚本读环境变量 `$KEYSTORE_PASS`，没有就退回到 `~/.moneytracker-android/pass`。

## 洞察与提问

看板的「洞察」面板完全由规则算出，不经过模型：超过 90% 的预算、相对上月变化最大的两个分类、
异常大额支出（超过六个月均值 3 倍）、储蓄率、净资产新高、缺少汇率的交易。离线、可解释。

「规划」页还有一个只读的「问一句」。配置了 LLM 时，模型只负责把问题翻成一份 **JSON 查询计划**
（口径、账户、标签、时间范围、分组、条数），随后每个字段都会对着当前账本校验（不存在的账户丢弃、
非法日期忽略、条数收敛），数字由引擎算，最后才允许模型基于这些算好的数字写一段话。关掉开关，
同样的问题仍然会得到事实表，只是没有那段话。

## 个性化

配置，不是代码。看板右上角「自定义」选择显示哪些卡片、顶部显示哪些指标；设置 → 外观 可以切换
「舒适 / 紧凑」密度、选择每周起始日。在流水页拼好的查询（含排序）可以存成带名字的视图，点一下
就套用，× 删除——它们和看板布局一起存在 `settings.json` 里，跟着这台机器，不跟着账本。

## 规划与报表

`goals.ledger` 是一个纯文本计划文件（和 `rules.ledger` 一样，**不会**被 `index.ledger` 引入，
hledger 永远不会看到它）。一行一条指令：

```ledger
assume  ; return:6% inflation:2.5% reserve:6          ; 名义收益、通胀、应急储备月数
pool    ; accounts:assets:invest,assets:cash           ; 什么算“可投资资产”
alloc   ; equity:60% bond:20% cash:20%                 ; 按 `class:` 标签的目标配置
goal fire 财富自由 ; swr:4%                            ; 标题数字使用的提取率
goal account 退休金 ; account:assets:invest:retirement target:1000000.00 CNY by:2040-01-01
debt 房贷 ; account:liabilities:mortgage rate:3.15% payment:8200.00 CNY
```

- **财富自由** = 年支出 ÷ 提取率。年支出取最近 12 个月（或 `expenses:annual`），资产池取
  `pool` 账户当期余额，达成时间用实际（扣通胀）收益与 6 个月平均储蓄推算。
  没写 `pool` 时，资产池 = **所有名字里不含 `房 / home / house / property / 车 / car` 的资产账户**：
  自住房不能拿来付水电，所以它进「净资产」但不进「可投资资产」。每个账户只算一次，父账户
  （`assets:现金`）和它的子账户（`assets:现金:招商银行`）不会重复相加。
- **储备月数** = 现金 ÷ 月均支出，目标来自 `assume` 的 `reserve:`。
- **负债进度** 按月摊还每条 `debt`：还清日期、账本内已付利息，以及**每月多还 500 / 1000 / 2000
  能省下多少**（只读，不写盘）。
- **配置偏离** 对比 `accounts.ledger` 里的 `class:` 标签（`; class:equity`）与 `alloc`，偏离
  超过 5 个百分点会标红。

**报表** 视图用同一份账本生成三张表：资产负债表（任何区间都满足
`资产 − 负债 = 权益 + 累计结余` 的复式恒等式）、任意月/季/年的损益表、净值变动表
（`期初 + 收入 − 支出 ± 估值 = 期末`）。`复制 Markdown` 会把当月报表放进剪贴板。

## 数据放在哪

```
<账本>/
  index.ledger                 # 唯一入口：选项、声明、include
  accounts.ledger              # 账户声明，带 `; type:A|L|E|R|X`
  prices.ledger                # `P` 指令 —— 汇率的唯一来源
  budgets.ledger               # `~ monthly` 预算信封
  goals.ledger                 # 规划：假设、财富自由目标、资产池、配置、负债（不被 index 引入）
  rules.ledger                 # MoneyTracker 规则 —— 故意不被 index.ledger include
  recurring.ledger             # 固定收支模板：工资、房贷、房租
  recur.ledger                 # `; skip:<id>@<月份>` 跳过记录（不 include）
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

**为什么 `recurring.ledger` 也不被 include。** 它是**计划**（`~` 模板），不是已经发生的钱。
include 进来以后，`hledger`、预算报表和现金流预测都会把同一个月算两遍，而且每一笔模板分录都会
被当成一个预算信封。

**预算与存钱目标。** `budgets.ledger` 里放 `~ monthly` 周期交易，**正数那一笔**就是信封：
`expenses:`（或任何 `X` 类）账户是花钱预算，`assets:` / `liabilities:` 账户是存钱目标，
“实际”取本月真正转进该账户的金额。负数那一笔只是为了让这个周期块平账。

```ledger
; 每月 500 的餐饮预算
~ monthly from 2026-01-01
    expenses:food        500.00 CNY
    assets:cash

; 每月存 500 养老
~ monthly from 2026-01-01
    assets:投资:个人养老金   500.00 CNY
    assets:cash
```

读取时是宽松的：`~`（以及 `=`）块下的分录如果丢了缩进，依然会被接受，并给一条
`core.parse.unindented_posting` 警告 —— hledger 本身要求缩进，所以这条警告是在提醒你文件
还不能直接交给 hledger。

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
cargo test --workspace                        # 229 个 Rust 测试，15 个 suite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm test                                     # 86 个 Vitest 测试
pnpm build                                    # tsc --noEmit && vite build
```

| 层 | 覆盖内容 |
| --- | --- |
| `crates/ledger-core/tests/corpus.rs` | `fixtures/grammar/*.ledger` 解析无错、打印→解析→打印逐字节一致；`fixtures/errors/*.ledger` 产出文档化的诊断码与行号 |
| `crates/ledger-engine` | include 图、通配符、循环、账户类型推断、汇率、规则、快录展开、报表、写入安全，以及仓库内的样例账本与示例账本端到端 |
| `crates/ledger-engine/tests/hledger_diff.rs` | `Vault::balances()` 与 `hledger balance --flat --no-total -O csv` 完全一致；`hledger check` 接受引擎刚写出的账本（无 `hledger` 时跳过） |
| `src-tauri/src/editing.rs` | 快录 → 落盘 → 重载、span 级修改、删除、批量修改，以及文件被外部改动时拒绝写入 |
| `src/**/*.test.tsx` | 快录面板、看板、流水列表（mock IPC 层） |

`scripts/smoke.sh` 会跑完上述全部检查，并在提交的 TypeScript 绑定过期时失败。

### 示例账本

仓库里带了两份账本，都是可以直接用 `hledger` 读的纯文本，也可以让应用打开
（**设置 → 账本 → 打开已有账本**）：

| 账本 | 内容 |
| --- | --- |
| `fixtures/vault/sample` | 7 笔：工资、房租、规则拆分的咖啡、一次美元旅行——小到可以逐行核对 |
| `fixtures/vault/full` | 16 个月、336 笔：现金、两张储蓄卡、电子钱包、信用卡、房贷与车贷、自住房与出租房、基金股票持仓、利息与分红、月度与年度预算信封，以及若干大额支出（装修 104,300、家电 58,200、购车）。净资产 2,384,394 CNY |

`fixtures/vault/full` 由 `scripts/gen-full-vault.py` 生成；每个账户/特性演示了什么，见
[`fixtures/vault/full/README.md`](fixtures/vault/full/README.md)。

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
crates/ledger-sync     S3 / MinIO 镜像：SigV4 签名、三方比对、冲突检测、回收站
src-tauri              窗口、托盘、全局快捷键、文件监听、IPC
src                    React 19 + Vite + Tailwind v4 + Recharts + d3-sankey
fixtures/              语法语料、错误语料、样例账本与示例账本
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
