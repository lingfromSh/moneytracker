/**
 * Presentation helpers. Amounts arrive as strings (exact decimals from Rust) and
 * are never parsed into floats unless a chart needs geometry.
 */

export function cn(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

/** `display` already carries the commodity and precision; fall back to a number. */
export function money(value: number | null | undefined, display?: string | null): string {
  if (display) return display;
  if (value === null || value === undefined) return "—";
  return value.toFixed(2);
}

export function signed(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  const sign = value > 0 ? "+" : "";
  return `${sign}${value.toFixed(2)}`;
}

export function percent(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || Number.isNaN(value)) return "—";
  return `${(value * 100).toFixed(digits)}%`;
}

export function monthLabel(key: string): string {
  const [year, month] = key.split("-");
  if (!month) return key;
  return `${year.slice(2)}年${Number(month)}月`;
}

export function dayLabel(iso: string): string {
  const [, month, day] = iso.split("-");
  if (!day) return iso;
  return `${Number(month)}月${Number(day)}日`;
}

export function weekday(iso: string): string {
  const date = new Date(`${iso}T00:00:00`);
  if (Number.isNaN(date.getTime())) return "";
  return "日一二三四五六"[date.getDay()];
}

export function today(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** Account colour ramp shared by the dashboard and the category views. */
const PALETTE = [
  "#3f9e6a",
  "#4c8bd8",
  "#d98b3f",
  "#b45bb0",
  "#d05b5b",
  "#5fb3c4",
  "#8a7bd8",
  "#c4a049",
  "#6f9f3f",
  "#b76c8e",
];

export function palette(index: number): string {
  return PALETTE[index % PALETTE.length];
}

export function shortAccount(account: string): string {
  const parts = account.split(":");
  return parts.length > 1 ? parts.slice(1).join(":") : account;
}

export function kindLabel(kind: string): string {
  switch (kind) {
    case "A":
      return "资产";
    case "L":
      return "负债";
    case "E":
      return "权益";
    case "R":
      return "收入";
    case "X":
      return "支出";
    default:
      return "未分类";
  }
}
