import * as React from "react";
import { cn } from "../lib/format";

type ButtonVariant = "primary" | "ghost" | "subtle" | "danger";

const BUTTON_STYLES: Record<ButtonVariant, string> = {
  primary:
    "bg-[var(--color-accent)] text-white hover:brightness-110 active:brightness-95 shadow-sm",
  ghost: "text-[var(--color-ink)] hover:bg-[var(--color-surface-2)]",
  subtle:
    "bg-[var(--color-surface-2)] text-[var(--color-ink)] hover:brightness-95 border border-[var(--color-line)]",
  danger: "bg-[var(--color-danger)] text-white hover:brightness-110",
};

export function Button({
  variant = "subtle",
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button
      {...props}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-lg px-3 py-1.5 text-sm font-medium transition",
        "min-h-10 sm:min-h-0",
        "disabled:cursor-not-allowed disabled:opacity-50",
        BUTTON_STYLES[variant],
        className,
      )}
    />
  );
}

export function Input({ className, ...props }: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cn(
        "w-full rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-1.5 text-sm",
        "min-h-10 sm:min-h-0",
        "placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent)] focus:outline-none",
        className,
      )}
    />
  );
}

export function Textarea({
  className,
  ...props
}: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={cn(
        "w-full rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2 text-sm",
        "min-h-20 sm:min-h-0",
        "focus:border-[var(--color-accent)] focus:outline-none",
        className,
      )}
    />
  );
}

export function Select({
  className,
  children,
  ...props
}: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cn(
        "rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-1.5 text-sm",
        "min-h-10 sm:min-h-0",
        "focus:border-[var(--color-accent)] focus:outline-none",
        className,
      )}
    >
      {children}
    </select>
  );
}

export function Card({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      {...props}
      className={cn(
        "rounded-[var(--radius-card)] border border-[var(--color-line)] bg-[var(--color-surface)] shadow-[0_1px_2px_rgba(0,0,0,0.04)]",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function CardHeader({
  title,
  hint,
  action,
}: {
  title: React.ReactNode;
  hint?: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3 px-4 pt-3">
      <div>
        <div className="text-sm font-semibold">{title}</div>
        {hint ? (
          <div className="mt-0.5 text-xs text-[var(--color-muted)]">{hint}</div>
        ) : null}
      </div>
      {action}
    </div>
  );
}

export function Badge({
  children,
  tone = "neutral",
  className,
}: {
  children: React.ReactNode;
  tone?: "neutral" | "accent" | "warn" | "danger";
  className?: string;
}) {
  const tones = {
    neutral: "bg-[var(--color-surface-2)] text-[var(--color-muted)]",
    accent: "bg-[color-mix(in_oklab,var(--color-accent)_18%,transparent)] text-[var(--color-accent)]",
    warn: "bg-amber-500/15 text-amber-600 dark:text-amber-400",
    danger: "bg-[var(--color-danger)]/15 text-[var(--color-danger)]",
  } as const;
  return (
    <span
      className={cn(
        "inline-flex items-center whitespace-nowrap rounded-md px-1.5 py-0.5 text-[11px] font-medium",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-5 w-9 rounded-full transition",
        checked ? "bg-[var(--color-accent)]" : "bg-[var(--color-line)]",
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all",
          checked ? "left-4.5" : "left-0.5",
        )}
      />
    </button>
  );
}

export function EmptyState({
  title,
  hint,
  action,
}: {
  title: string;
  hint?: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-16 text-center">
      <div className="text-sm font-medium">{title}</div>
      {hint ? (
        <div className="max-w-sm text-xs text-[var(--color-muted)]">{hint}</div>
      ) : null}
      {action}
    </div>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block space-y-1">
      <span className="block text-xs font-medium text-[var(--color-muted)]">{label}</span>
      {children}
      {hint ? <span className="block text-[11px] text-[var(--color-muted)]">{hint}</span> : null}
    </label>
  );
}

export function Modal({
  open,
  onClose,
  title,
  children,
  width = "max-w-lg",
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  width?: string;
}) {
  React.useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-end justify-center bg-black/30 backdrop-blur-sm sm:items-start sm:p-10">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={cn(
          "flex max-h-[92dvh] w-full flex-col overflow-hidden border border-[var(--color-line)] bg-[var(--color-surface)]",
          "rounded-t-2xl border-b-0 shadow-[0_-8px_24px_-12px_rgba(0,0,0,0.35)]",
          "sm:max-h-[85vh] sm:rounded-[var(--radius-card)] sm:border-b sm:shadow-[0_1px_2px_rgba(0,0,0,0.04)]",
          width,
        )}
      >
        {/* 手机上给一点“抓手”的暗示：这个面板是从底部拿上来的。 */}
        <div className="mx-auto mt-2 h-1 w-10 shrink-0 rounded-full bg-[var(--color-line)] sm:hidden" />
        <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-line)] px-4 py-2">
          <div className="text-sm font-semibold">{title}</div>
          <Button variant="ghost" onClick={onClose} aria-label="关闭">
            ✕
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-4 pb-[max(env(safe-area-inset-bottom),1rem)] sm:pb-4">
          {children}
        </div>
      </div>
    </div>
  );
}
