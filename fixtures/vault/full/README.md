# 示例账本 · Showcase vault

一个可以直接用 `hledger` 打开、也可以在 MoneyTracker 里打开的完整家庭账本样本：
**2025-07 → 2026-10 共 16 个月、336 笔交易**，包含现金、两张储蓄卡、电子钱包、信用卡、
房贷、车贷、自住房与出租房、投资持仓、工资/年终奖/利息/分红/租金/投资兑现，
以及月度与年度预算、装修购车旅行等大额支出。

A complete household ledger you can open with `hledger` or MoneyTracker:
16 months (2025-07 → 2026-10), 336 entries.

```
assets=3,970,334.00 CNY    liabilities=1,585,940.00 CNY    net worth=2,384,394.00 CNY
income=590,470.00 CNY      expenses=468,772.00 CNY        (16 months)
```

## 账本里有什么

| 内容 | 账户 / 位置 | 想验证的东西 |
| --- | --- | --- |
| 现金 | `assets:cash` | 取现、菜市场零钱，同一账户多币种无关的最简形式 |
| 储蓄账户 | `assets:bank:icbc`（工资卡）、`assets:bank:cmb`（消费/收租卡） | 两条资金流、账户余额从不为负、`资金归集` 内部转账 |
| 电子钱包 | `assets:wallet:alipay`、`assets:wallet:wechat` | 非银行支付渠道，日常小额高频 |
| 信用卡 | `liabilities:credit:cmb` | 刷卡形成负债、次月 20 号全额还款，账单余额始终是「本月未还」 |
| 房贷 | `liabilities:mortgage:home` + `expenses:home:mortgage_interest` | 月供 8,200 拆成本金（负债减少）与利息（费用） |
| 车贷 | `liabilities:loan:car` + `expenses:transport:loan_interest` | 3 年车贷，利息逐月下降 |
| 自住房 | `assets:property:home` | 首付 + 贷款购房一次入账，资产与负债同时上升 |
| 出租房 | `assets:property:rental` + `income:rent` | `equity:gift` 继承入账，之后每月 4,200 租金收入 |
| 车辆 | `assets:vehicle:car` | 首付 + 车贷的资产化，`expenses:vehicle:maintenance` 养护 |
| 投资 | `assets:invest:fund`、`assets:invest:stock` | 每月定投、卖基金兑现收益、股票分红 |
| 利息 | `income:interest` | 季度结息，货币基金/存款的被动收入 |
| 工资与奖金 | `income:salary`、`income:bonus` | 每月 28,000，7 月与 1 月加薪，12 月年终奖 78,000 |
| 日常消费 | `expenses:food:*`、`expenses:transport:*`、`expenses:shopping*` … | 分类层级、`@` 账户角色推断、标签（`:car:`、`:travel:` …） |
| 大额支出 | `expenses:home:renovation`（104,300）、`expenses:home:appliance`（58,200）、`expenses:shopping:electronics`、`expenses:tax` | 大额一次性支出对现金流与预算的冲击 |
| 年度预算 | `budgets.ledger` 的 `~ yearly` | 旅行 30,000 / 保险 12,000 / 车辆养护 6,000 分摊到每月 |
| 月度预算 | `budgets.ledger` 的 `~ monthly` | 餐饮、交通、水电、购物、娱乐、医疗等信封 |
| 储蓄目标 | `assets:savings:emergency` 信封（`kind=A`） | 每月 1,500 存入应急金，按「已存 / 还差」显示 |
| 汇率 | `prices.ledger` | 东京旅行 `3,200.00 USD @@ 22,976.00 CNY`，按交易日汇率折算 |
| 规则 | `rules.ledger` | 星巴克 70/30 拆分、盒马/加油/租金四条规则（只影响新录入） |
| 多分录 | `2025-08-01 购房首付`、`2026-06-18 购车`、`2026-03-01 装修` | 一笔交易里资产、负债、支出三条腿同时变化 |

## 怎么用

在 MoneyTracker 里：

1. **设置 → 账本 → 打开已有账本**，选择这个目录（`fixtures/vault/full`）。
2. 看板会显示净资产曲线、分类占比、桑基图、预算 vs 实际与 12 个月预测。
3. 想改着玩，先复制一份：`cp -r fixtures/vault/full ~/Documents/示例账本`，再打开副本。

在命令行里：

```bash
hledger -f fixtures/vault/full/index.ledger check
hledger -f fixtures/vault/full/index.ledger balance --flat --no-total
hledger -f fixtures/vault/full/index.ledger reg expenses:food --monthly
```

## 约定

- `index.ledger` 只包含 `accounts / prices / budgets` 与年份文件；
  `rules.ledger` **故意不被包含**，否则 hledger 会把自动分录再算一次。
- 月份文件里直接放交易（没有拆成日文件）；MoneyTracker 新记账时会自动建日文件并接到
  年 → 月链路上，两种形态可以共存。
- 金额能省略就省略（`assets:bank:icbc` 不写金额），由程序补平，这样也顺便演示了
  “每笔交易最多一个省略金额”的规则。
- 由 `scripts/gen-full-vault.py` 生成；改数字请改脚本再重新生成，别手改生成物。
