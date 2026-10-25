import type { AdvisorTurnDto } from "./bindings";

const KEY = "moneytracker.advisor.seed";

/**
 * 「问一句」和顾问共用一套上下文：从看板点「接着聊」时，把刚才的问答当成本轮对话的
 * 开头带过去，顾问就不用把同一个问题再问一遍（也省一次调用）。
 *
 * 用 sessionStorage 是为了只跨一个页面跳转：刷新之后就不该再重复塞进去。
 */
export function seedAdvisor(question: string, answer: string | null): void {
  const turns: AdvisorTurnDto[] = [{ role: "user", content: question, at: new Date().toISOString() }];
  if (answer && answer.trim()) {
    turns.push({ role: "assistant", content: answer, at: new Date().toISOString() });
  }
  try {
    sessionStorage.setItem(KEY, JSON.stringify(turns));
  } catch {
    /* 隐私模式下 sessionStorage 可能不可写，忽略即可 */
  }
}

export function takeAdvisorSeed(): AdvisorTurnDto[] {
  try {
    const raw = sessionStorage.getItem(KEY);
    if (!raw) return [];
    sessionStorage.removeItem(KEY);
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as AdvisorTurnDto[]) : [];
  } catch {
    return [];
  }
}

/** 顾问的快捷问题：都是「基于账本数字做判断」的问法，而不是查某一笔。 */
export const ADVISOR_EXAMPLES = [
  "这个月可以支出 5000 买手机吗？",
  "我的储备还够几个月？",
  "按现在的结余，多久能到 1/4 自由目标？",
  "超支最厉害的两个分类，怎么压？",
];
