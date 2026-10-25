#!/usr/bin/env python3
"""Generate the showcase vault at fixtures/vault/full/.

The output is a plain hledger-compatible ledger. Everything is written with
elided amounts wherever the value is implied, so the file also demonstrates the
"exactly one missing amount" rule the engine uses.
"""
import math
import os
import shutil
from datetime import date

ROOT = os.path.join(os.path.dirname(__file__), "..", "fixtures", "vault", "full")
ROOT = os.path.abspath(ROOT)

MONTHS = [
    (2025, 7), (2025, 8), (2025, 9), (2025, 10), (2025, 11), (2025, 12),
    (2026, 1), (2026, 2), (2026, 3), (2026, 4), (2026, 5), (2026, 6),
    (2026, 7), (2026, 8), (2026, 9), (2026, 10),
]

# ---------------------------------------------------------------- accounts
ACCOUNTS = """\
; MoneyTracker showcase vault — account declarations.
; `type:` tags drive the dashboard (A assets, L liabilities, R income,
; X expenses). Anything without a tag falls back to the top-level name.

account assets:cash                        ; type:A
account assets:bank:icbc                   ; type:A
account assets:bank:cmb                    ; type:A
account assets:wallet:alipay               ; type:A
account assets:wallet:wechat               ; type:A
account assets:savings:emergency           ; type:A
account assets:invest:fund                 ; type:A
account assets:invest:stock                ; type:A
account assets:property:home               ; type:A
account assets:property:rental             ; type:A
account assets:vehicle:car                 ; type:A

; 五险二金：可提取的部分是资产，统筹掉的部分是当期支出。
account assets:cn:fund                     ; type:A
account assets:cn:pension                  ; type:A
account assets:cn:annuity                  ; type:A

account liabilities:credit:cmb             ; type:L
account liabilities:mortgage:home          ; type:L
account liabilities:loan:car               ; type:L

account equity:opening                     ; type:E
account equity:gift                        ; type:E

account income:salary                      ; type:R
account income:bonus                       ; type:R
account income:interest                    ; type:R
account income:dividend                    ; type:R
account income:rent                        ; type:R
account income:invest:gain                 ; type:R
account income:other                       ; type:R
account income:benefit                     ; type:R

account expenses:food:groceries            ; type:X
account expenses:food:restaurant           ; type:X
account expenses:food:coffee               ; type:X
account expenses:home:mortgage_interest    ; type:X
account expenses:home:utilities            ; type:X
account expenses:home:property_fee         ; type:X
account expenses:home:repair               ; type:X
account expenses:home:renovation           ; type:X
account expenses:home:appliance            ; type:X
account expenses:transport:fuel            ; type:X
account expenses:transport:transit         ; type:X
account expenses:transport:parking         ; type:X
account expenses:transport:loan_interest   ; type:X
account expenses:vehicle:maintenance       ; type:X
account expenses:shopping                  ; type:X
account expenses:shopping:clothing         ; type:X
account expenses:shopping:electronics      ; type:X
account expenses:health:clinic             ; type:X
account expenses:health:pharmacy           ; type:X
account expenses:education:books           ; type:X
account expenses:education:course          ; type:X
account expenses:entertainment             ; type:X
account expenses:travel                    ; type:X
account expenses:insurance                 ; type:X
account expenses:insurance:medical         ; type:X
account expenses:insurance:unemployment    ; type:X
account expenses:insurance:injury          ; type:X
account expenses:insurance:maternity       ; type:X
account expenses:fees:bank                 ; type:X
account expenses:tax                       ; type:X
account expenses:unclassified              ; type:X
"""

PRICES = """\
; Exchange rates. MoneyTracker only trusts `P` lines: the rate for a
; transaction is the latest one on or before its date.
P 2025-07-01 USD 7.16 CNY
P 2025-10-01 USD 7.12 CNY
P 2026-01-01 USD 7.30 CNY
P 2026-04-01 USD 7.24 CNY
P 2026-07-01 USD 7.18 CNY
P 2026-10-01 USD 7.21 CNY
"""

BUDGETS = """\
; Envelope budgets. `~` blocks belong here, not in the day files.
; expenses:* envelopes cap spending; assets:*/liabilities:* envelopes are
; savings goals and count money moved towards them.

~ monthly from 2025-07-01
    expenses:food                      4,500.00 CNY
    expenses:transport                 1,800.00 CNY
    expenses:home:utilities              800.00 CNY
    expenses:home:mortgage_interest    6,400.00 CNY
    expenses:shopping                  1,500.00 CNY
    expenses:entertainment             1,000.00 CNY
    expenses:health                      600.00 CNY
    assets:savings:emergency           1,500.00 CNY

~ yearly from 2025-07-01
    expenses:travel                   30,000.00 CNY
    expenses:insurance                12,000.00 CNY
    expenses:vehicle:maintenance       6,000.00 CNY
"""

RULES = """; MoneyTracker rules — read by the app, never included by index.ledger.
; New entries typed into the capture window pick the first rule whose query
; matches; percentages always add up to the whole amount.

= desc:星巴克  ; rule:r-star order:10 enabled:true from:assets:wallet:alipay
    expenses:food:coffee      70.00%
    expenses:food             30.00%
    assets:wallet:alipay

= desc:盒马  ; rule:r-hema order:20 enabled:true from:assets:wallet:alipay
    expenses:food:groceries  100.00%
    assets:wallet:alipay

= desc:加油  ; rule:r-fuel order:30 enabled:true from:assets:wallet:wechat
    expenses:transport:fuel  100.00%
    assets:wallet:wechat

= desc:房租收入 and not desc:押金  ; rule:r-rent order:40 enabled:true from:assets:bank:cmb
    income:rent              100.00%
    assets:bank:cmb
"""

RECURRING = """; MoneyTracker 固定收支 — 由日程页读取，**不**被 index.ledger 包含。
;
; 这些是计划和模板，不是已经发生的交易：文件被 include 的话，hledger 与账本
; 的预算/现金流会把同一个月算两遍。所以它和 rules.ledger、goals.ledger 一样，
; 独立存在。
;
; 语法：一个 hledger 的 ~ 周期块就是一条模板。
;   ~ monthly from <起始月> day:<1-31>  ; id:<唯一 id> label:<名称> kind:<类型>
; 头部注释里的 id/label/kind/settle/enabled 都是空格分隔的 key:value；
; day 超过当月天数时会自动收到月末（2 月按 28/29 号算）。
; 最后一条 posting 是现金腿 —— 它决定卡片上的「现金流」，也可以省略金额，
; 由引擎按「恰好一条缺失」的规则补平。
; kind 只影响界面上的标签：salary 工资 / loan 还款 / income 收入 / expense 支出。

; 工资：应发 30,000.00 + 公司缴 16,422.00，个人代扣 9,460.50，到手 26,239.50。
; 个人与公司两侧同账户合并成一行，注释里写清楚拆分。
~ monthly from 2025-08-05 day:5  ; id:salary-main label:工资 kind:salary
    income:salary                        -30,000.00 CNY
    income:benefit                       -16,422.00 CNY  ; 公司缴纳侧合计
    assets:cn:fund                         7,200.00 CNY  ; 公积金 个人 3,600 + 公司 3,600
    assets:cn:pension                      7,200.00 CNY  ; 养老 个人 2,400 + 公司 4,800
    assets:cn:annuity                      3,600.00 CNY  ; 年金 个人 1,200 + 公司 2,400
    expenses:insurance:medical             3,000.00 CNY  ; 医疗 个人 714 + 公司 2,400
    expenses:insurance:unemployment          300.00 CNY  ; 失业 个人 150.00 + 公司 150.00
    expenses:insurance:injury                150.00 CNY  ; 工伤（公司）
    expenses:insurance:maternity             300.00 CNY  ; 生育（公司）
    assets:bank:cmb                       26,239.50 CNY  ; 到手

; 房贷：每月 8,200.00，其中 2,130.00 是本金（冲减负债），剩下是利息。
~ monthly from 2025-08-20 day:20  ; id:loan-home label:房贷 kind:loan
    expenses:home:mortgage_interest        6,070.00 CNY
    liabilities:mortgage:home              2,130.00 CNY
    assets:bank:icbc                      -8,200.00 CNY

; 房租收入：每月 3,600.00。
~ monthly from 2025-08-01 day:1  ; id:rent-flat label:房租收入 kind:income
    income:rent                           -3,600.00 CNY
    assets:bank:icbc                       3,600.00 CNY

; 订阅：现金腿省略金额，引擎会补 -30.00。
~ monthly from 2025-08-15 day:15  ; id:sub-cloud label:云存储 kind:expense
    expenses:entertainment                    30.00 CNY
    assets:wallet:alipay
"""


def txn(day: date, payee, postings, tags=(), status="*", comment=None):
    """Build a canonical transaction block; `postings` is [(account, amount|None)]."""
    head = f"{day.isoformat()} {status} {payee}"
    tail = []
    if comment:
        tail.append(comment)
    if tags:
        tail.append("".join(f":{t.strip(':')}:" if ":" not in t else t for t in tags))
    if tail:
        head += "  ; " + " ".join(tail)
    lines = [head]
    for account, amount in postings:
        lines.append(f"    {account}" + (f"    {amount}" if amount else ""))
    return "\n".join(lines) + "\n"


def cny(value):
    return f"{value:,.2f} CNY"


FLOOR = 10_000.0
BANKS = ("assets:bank:icbc", "assets:bank:cmb")
balances = {name: None for name in BANKS}  # filled from the opening entry


def deltas(postings):
    """Per-account amounts, resolving the single elided posting."""
    explicit = [(acc, amount_value(amt)) for acc, amt in postings if amt]
    missing = [acc for acc, amt in postings if not amt]
    out = {acc: value for acc, value in explicit}
    if len(missing) == 1:
        out[missing[0]] = -sum(value for _acc, value in explicit)
    elif len(missing) > 1:
        for acc in missing:  # commodity-different legs: approximate, they are tiny
            out[acc] = 0.0
    return out


def sweep(entries, year, month):
    """Insert `资金归集` transfers so neither bank account dips below FLOOR."""
    if balances["assets:bank:icbc"] is None:
        balances.update({name: float(assets[name]) for name in BANKS})
    out = []
    for entry in entries:
        delta = deltas(entry[2])
        for account in BANKS:
            if account not in delta:
                continue
            projected = balances[account] + delta[account]
            if projected >= FLOOR:
                continue
            other = next(name for name in BANKS if name != account)
            spare = balances[other] - FLOOR
            need = FLOOR + 20_000.0 - projected
            move = math.ceil(min(max(need, 10_000.0), max(spare, 0.0)) / 1_000) * 1_000
            if move <= 0:
                continue
            out.append(
                (entry[0], "资金归集", [(account, cny(move)), (other, None)], ["transfer"])
            )
            balances[account] += move
            balances[other] -= move
        for account, value in delta.items():
            if account in balances:
                balances[account] += value
        out.append(entry)
    return out


blocks = []


def add(day, payee, postings, tags=(), status="*", comment=None):
    blocks.append(txn(day, payee, postings, tags, status, comment))


# ------------------------------------------------------------------ opening
assets = {
    "assets:bank:icbc": 320_000,
    "assets:bank:cmb": 280_000,
    "assets:savings:emergency": 260_000,
    "assets:invest:fund": 320_000,
    "assets:invest:stock": 180_000,
    "assets:cash": 3_000,
    "assets:wallet:alipay": 1_200,
    "assets:wallet:wechat": 800,
}
opening_liabilities = {"liabilities:credit:cmb": 2_400}
opening_total = sum(assets.values()) - sum(opening_liabilities.values())
add(
    date(2025, 7, 1),
    "期初余额",
    [(k, cny(v)) for k, v in assets.items()]
    + [(k, cny(-v)) for k, v in opening_liabilities.items()]
    + [("equity:opening", cny(-opening_total))],
    tags=["opening"],
    comment="2025 年 7 月的起点：现金、两张银行卡、基金和股票持仓",
)

# ---------------------------------------------------------------- one-offs
one_off = {
    (2025, 7): [
        (3, "收到上年度报销", [("assets:bank:icbc", cny(2_800)), ("income:other", None)], ["2024-colleague-trip"]),
        (18, "体检", [("expenses:health:clinic", cny(1_680)), ("liabilities:credit:cmb", None)], ["family"]),
    ],
    (2025, 8): [
        (1, "购房首付", [("assets:property:home", cny(1_800_000)), ("liabilities:mortgage:home", cny(-1_500_000)), ("assets:bank:icbc", cny(-300_000))], ["home", "one-off"],
         "总价 180 万，首付 30 万，公积金+商贷 150 万"),
        (1, "购房契税与手续费", [("expenses:tax", cny(27_000)), ("expenses:fees:bank", cny(380)), ("assets:bank:icbc", None)], ["home"]),
        (12, "出售部分指数基金凑首付", [("assets:invest:fund", cny(-180_000)), ("assets:bank:cmb", cny(200_000)), ("income:invest:gain", cny(-20_000))], ["invest"]),
    ],
    (2025, 12): [
        (28, "家电与家具", [("expenses:home:appliance", cny(48_600)), ("liabilities:credit:cmb", None)], ["home", "big"]),
    ],
    (2026, 1): [
        (10, "年度重疾险+医疗险", [("expenses:insurance", cny(9_800)), ("assets:bank:icbc", None)], ["insurance"]),
    ],
    (2026, 2): [
        (15, "春节红包", [("expenses:entertainment", cny(6_400)), ("assets:wallet:alipay", None)], ["spring-festival"]),
    ],
    (2026, 3): [
        (1, "厨房与卫生间改造", [("expenses:home:renovation", cny(96_000)), ("assets:bank:cmb", cny(-60_000)), ("liabilities:credit:cmb", cny(-36_000))], ["home", "big"],
         "装修总价 9.6 万，先付 6 万，信用卡分期 3.6 万"),
        (25, "瓷砖与五金补货", [("expenses:home:renovation", cny(8_300)), ("liabilities:credit:cmb", None)], ["home"]),
    ],
    (2026, 4): [
        (2, "车辆年检与保养", [("expenses:vehicle:maintenance", cny(2_600)), ("liabilities:credit:cmb", None)], ["car"]),
    ],
    (2026, 5): [
        (26, "继承父母房产", [("assets:property:rental", cny(900_000)), ("equity:gift", cny(-900_000))], ["rental", "gift"],
         "老房子过户，无贷款；此后每月收租"),
        (28, "出租房简单翻新", [("expenses:home:repair", cny(14_500)), ("assets:bank:cmb", None)], ["rental"]),
    ],
    (2026, 6): [
        (18, "购车", [("assets:vehicle:car", cny(168_000)), ("liabilities:loan:car", cny(-120_000)), ("assets:bank:icbc", cny(-48_000))], ["car", "one-off"],
         "落地 16.8 万，首付 4.8 万，三年车贷 12 万"),
        (18, "车辆购置税与上牌", [("expenses:tax", cny(14_300)), ("expenses:fees:bank", cny(520)), ("assets:bank:icbc", None)], ["car"]),
    ],
    (2026, 7): [
        (5, "东京家庭旅行", [("expenses:travel", "3,200.00 USD @@ 22,976.00 CNY"), ("assets:bank:icbc", cny(-22_976))], ["travel", "fx"],
         "机票+酒店+购物，刷卡按 7.18 结算"),
        (20, "取现（旅行备用金）", [("assets:cash", cny(4_000)), ("assets:bank:icbc", None)], ["travel"]),
    ],
    (2026, 8): [
        (8, "更换笔记本电脑", [("expenses:shopping:electronics", cny(18_800)), ("liabilities:credit:cmb", None)], ["work", "big"]),
    ],
    (2026, 9): [
        (9, "车辆保险续保", [("expenses:insurance", cny(6_240)), ("assets:bank:icbc", None)], ["car"]),
        (22, "牙科治疗", [("expenses:health:clinic", cny(6_800)), ("liabilities:credit:cmb", None)], ["health", "big"]),
    ],
    (2026, 10): [
        (12, "父母住院陪护", [("expenses:health:clinic", cny(12_400)), ("assets:bank:icbc", None)], ["family"]),
        (18, "给家里换空调", [("expenses:home:appliance", cny(9_600)), ("liabilities:credit:cmb", None)], ["home"]),
    ],
}

# ------------------------------------------------------------- monthly loop
DAILY = {
    # (payee, account, amount, tags, day)
    "groceries": [
        ("盒马鲜生", "expenses:food:groceries", 320, ["home"]),
        ("菜市场", "expenses:food:groceries", 120, ["cash"]),
        ("永辉超市", "expenses:food:groceries", 260, ["home"]),
    ],
    "dining": [
        ("星巴克", "expenses:food:coffee", 45, ["office"]),
        ("公司和府捞面", "expenses:food:restaurant", 68, ["office"]),
        ("海底捞", "expenses:food:restaurant", 380, ["family"]),
    ],
    "transport": [
        ("地铁充值", "expenses:transport:transit", 100, ["commute"]),
        ("加油", "expenses:transport:fuel", 420, ["car"]),
        ("停车费", "expenses:transport:parking", 60, ["car"]),
    ],
}

money_account = {
    "expenses:food:groceries": "assets:wallet:alipay",
    "expenses:food:coffee": "assets:wallet:alipay",
    "expenses:food:restaurant": "liabilities:credit:cmb",
    "expenses:transport:transit": "assets:wallet:alipay",
    "expenses:transport:fuel": "assets:wallet:wechat",
    "expenses:transport:parking": "assets:wallet:wechat",
}

CARD = "liabilities:credit:cmb"


def amount_value(text):
    return float(text.replace(",", "").split()[0])


prev_card = 2_400.0  # opening balance sits on the card
for index, (year, month) in enumerate(MONTHS):
    # Mortgage: interest falls as the principal is paid down.
    interest = 6_250 - 12 * index
    principal = 8_200 - interest
    salary = 28_000 + (2_000 if month in (1, 7) else 0)

    if (year, month) >= (2026, 7):
        car_interest = 900 - 20 * (index - 12)
        car_principal = 4_100 - car_interest
        month_blocks = [
            (6, "工资", [("assets:bank:icbc", cny(salary)), ("income:salary", None)], ["salary"]),
            (8, "房贷还款", [("expenses:home:mortgage_interest", cny(interest)), ("liabilities:mortgage:home", cny(principal)), ("assets:bank:icbc", None)], ["home"]),
            (8, "车贷还款", [("expenses:transport:loan_interest", cny(car_interest)), ("liabilities:loan:car", cny(car_principal)), ("assets:bank:icbc", None)], ["car"]),
            (10, "物业费", [("expenses:home:property_fee", cny(380)), ("assets:bank:icbc", None)], ["home"]),
            (12, "水电燃气", [("expenses:home:utilities", cny(430 + (month % 5) * 40)), ("assets:bank:icbc", None)], ["home"]),
            (13, "指数基金定投", [("assets:invest:fund", cny(3_000)), ("assets:bank:icbc", None)], ["invest"]),
            (14, "应急金存入", [("assets:savings:emergency", cny(1_500)), ("assets:bank:icbc", None)], ["saving"]),
        ]
    else:
        month_blocks = [
            (6, "工资", [("assets:bank:icbc", cny(salary)), ("income:salary", None)], ["salary"]),
            (8, "房贷还款", [("expenses:home:mortgage_interest", cny(interest)), ("liabilities:mortgage:home", cny(principal)), ("assets:bank:icbc", None)], ["home"]),
            (10, "物业费", [("expenses:home:property_fee", cny(380)), ("assets:bank:icbc", None)], ["home"]),
            (12, "水电燃气", [("expenses:home:utilities", cny(430 + (month % 5) * 40)), ("assets:bank:icbc", None)], ["home"]),
            (13, "指数基金定投", [("assets:invest:fund", cny(3_000)), ("assets:bank:icbc", None)], ["invest"]),
            (14, "应急金存入", [("assets:savings:emergency", cny(1_500)), ("assets:bank:icbc", None)], ["saving"]),
        ]

    month_blocks.append((3, "充值电子钱包", [("assets:wallet:alipay", cny(1_500)), ("assets:wallet:wechat", cny(600)), ("assets:bank:icbc", None)], ["wallet"]))

    # Credit card repayment on the 20th clears whatever was charged last month,
    # so the liability stays near zero instead of drifting.
    month_blocks.append((20, "信用卡还款", [("liabilities:credit:cmb", cny(round(prev_card))), ("assets:bank:icbc", None)], ["card"] if prev_card else ["card"]))

    # Rental income from 2026-05.
    if (year, month) >= (2026, 5):
        month_blocks.append((25, "出租房收租", [("assets:bank:cmb", cny(4_200)), ("income:rent", None)], ["rental"]))
        month_blocks.append((26, "出租房物业与维修", [("expenses:home:repair", cny(260 + (month % 3) * 90)), ("assets:bank:cmb", None)], ["rental"]))

    # Quarterly interest, semi-annual dividends.
    if month in (3, 6, 9, 12):
        month_blocks.append((21, "存款利息", [("assets:savings:emergency", cny(430 + index * 12)), ("income:interest", None)], ["interest"]))
    if month in (6, 12):
        month_blocks.append((22, "股票分红", [("assets:invest:stock", cny(3_600 + index * 40)), ("income:dividend", None)], ["dividend"]))
    if month == 12:
        month_blocks.append((28, "年终奖", [("assets:bank:icbc", cny(78_000)), ("income:bonus", None)], ["bonus"]))

    # Everyday spending, spread over the month.
    for day, payee, account, amount, tags in [
        (4, "星巴克", "expenses:food:coffee", 45, ["office"]),
        (7, "盒马鲜生", "expenses:food:groceries", 320 + (index % 3) * 40, ["home"]),
        (11, "地铁充值", "expenses:transport:transit", 100, ["commute"]),
        (16, "海底捞", "expenses:food:restaurant", 380 + (index % 2) * 60, ["family"]),
        (19, "菜市场", "expenses:food:groceries", 120 + (index % 4) * 20, ["cash"]),
        (23, "加油", "expenses:transport:fuel", 420, ["car"]),
        (24, "停车费", "expenses:transport:parking", 60, ["car"]),
        (27, "永辉超市", "expenses:food:groceries", 260, ["home"]),
        (28, "京东日用品", "expenses:shopping", 180 + (index % 5) * 60, ["home"]),
    ]:
        if payee == "菜市场":
            source = "assets:cash"
        else:
            source = money_account.get(account) or (
                "liabilities:credit:cmb" if account.startswith("expenses:shopping") else "assets:wallet:alipay"
            )
        month_blocks.append((day, payee, [(account, cny(amount)), (source, None)], tags))

    # A couple of month-specific extras to keep the categories alive.
    if month in (2, 8):
        month_blocks.append((17, "购书", [("expenses:education:books", cny(320)), ("liabilities:credit:cmb", None)], ["study"]))
    if month in (4, 10):
        month_blocks.append((15, "线上课程", [("expenses:education:course", cny(1_280)), ("liabilities:credit:cmb", None)], ["study"]))
    if month in (1, 5, 9):
        month_blocks.append((29, "看电影与展览", [("expenses:entertainment", cny(460)), ("assets:wallet:alipay", None)], ["fun"]))
    if month in (3, 7, 11):
        month_blocks.append((30, "药店", [("expenses:health:pharmacy", cny(180)), ("assets:wallet:alipay", None)], ["health"]))
    if month in (5, 11):
        month_blocks.append((14, "添置衣物", [("expenses:shopping:clothing", cny(1_260)), ("liabilities:credit:cmb", None)], ["clothes"]))

    month_entries = month_blocks + one_off.get((year, month), [])
    month_entries.sort(key=lambda entry: entry[0])
    month_entries = sweep(month_entries, year, month)
    charges = 0.0
    for entry in month_entries:
        day, payee, postings, tags = entry[:4]
        comment = entry[4] if len(entry) > 4 else None
        add(date(year, month, day), payee, postings, tags, comment=comment)
        # A charge is either an explicit negative posting on the card or, when the
        # card side is elided, the positive total on the other side.
        card_elided = any(acc == CARD and amt is None for acc, amt in postings)
        if card_elided:
            charges += sum(
                amount_value(amt)
                for acc, amt in postings
                if acc != CARD and amt and amount_value(amt) > 0
            )
        else:
            charges += sum(
                -amount_value(amt) for acc, amt in postings if acc == CARD and amt and amount_value(amt) < 0
            )
    prev_card = charges

# ---------------------------------------------------------------- write out
# README.md 是手写的说明，不属于生成物，重建目录时保留下来。
readme_path = os.path.join(ROOT, "README.md")
readme = None
if os.path.isfile(readme_path):
    with open(readme_path) as handle:
        readme = handle.read()
if os.path.isdir(ROOT):
    shutil.rmtree(ROOT)
os.makedirs(ROOT)
if readme is not None:
    with open(readme_path, "w") as handle:
        handle.write(readme)

by_month = {}
for block in blocks:
    key = block[:7]  # YYYY-MM
    by_month.setdefault(key, []).append(block)
for key in by_month:
    by_month[key].sort(key=lambda block: block[:10])  # stable: keeps same-day order


def write(rel, text):
    path = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as handle:
        handle.write(text)


write("accounts.ledger", ACCOUNTS)
write("prices.ledger", PRICES)
write("budgets.ledger", BUDGETS)
write("rules.ledger", RULES)
write("recurring.ledger", RECURRING)
write(".gitignore", ".bak/\n")

years = sorted({key[:4] for key in by_month})
index = [
    "; MoneyTracker showcase vault — index.",
    "; Everything a transaction needs is reachable from here: index → year → month.",
    "; rules.ledger is deliberately NOT included: the app expands rules when it",
    "; writes an entry, so including it would make hledger apply them a second time.",
    "; recurring.ledger and goals.ledger are not included either: they describe",
    "; plans (`~` templates, savings goals), not money that moved.",
    "",
    "commodity 1,000.00 CNY",
    "commodity 1,000.00 USD",
    "",
    "include accounts.ledger",
    "include prices.ledger",
    "include budgets.ledger",
    "",
]
for year in years:
    index.append(f"include {year}/{year}.ledger")
write("index.ledger", "\n".join(index) + "\n")

for year in years:
    months = [key for key in sorted(by_month) if key.startswith(year)]
    lines = [f"; {year} — month files, newest last.", ""]
    for key in months:
        lines.append(f"include {key}.ledger")
    write(f"{year}/{year}.ledger", "\n".join(lines) + "\n")
    for key in months:
        body = "\n".join(by_month[key])
        header = f"; {key} — {len(by_month[key])} entries. Generated by scripts/gen-full-vault.py.\n\n"
        write(f"{year}/{key}.ledger", header + body)

count = sum(len(v) for v in by_month.values())
print(f"{ROOT}: {count} transactions in {len(by_month)} month files across {len(years)} years")
