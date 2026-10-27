// Design-system primitives (SPEC §14.4): quiet, macOS-inspired controls.

import * as RSwitch from "@radix-ui/react-switch";
import * as RTooltip from "@radix-ui/react-tooltip";
import * as RPopover from "@radix-ui/react-popover";
import * as RDialog from "@radix-ui/react-dialog";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { forwardRef } from "react";

export function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

type Variant = "primary" | "secondary" | "plain";

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }>(
  function Button({ variant = "secondary", className, ...rest }, ref) {
    return (
      <button
        ref={ref}
        className={cx(
          "inline-flex h-7 items-center justify-center gap-1.5 rounded-(--radius-control) px-3 text-[13px] font-medium",
          "transition-colors duration-75 disabled:cursor-default disabled:opacity-40",
          variant === "primary" && "bg-accent text-accent-fg hover:brightness-110 active:brightness-95",
          variant === "secondary" && "border border-separator-strong bg-control text-text hover:bg-control-hover",
          variant === "plain" && "text-text-2 hover:bg-control-hover hover:text-text",
          className,
        )}
        {...rest}
      />
    );
  },
);

export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean; children: ReactNode }
>(function IconButton({ label, active, className, children, ...rest }, ref) {
  return (
    <Tip label={label}>
      <button
        ref={ref}
        aria-label={label}
        aria-pressed={active}
        className={cx(
          "no-drag inline-flex h-7 min-w-7 items-center justify-center rounded-(--radius-control) px-1.5 text-text-2",
          "transition-colors duration-75 hover:bg-control-hover hover:text-text disabled:opacity-40",
          active && "bg-accent-soft text-accent hover:bg-accent-soft hover:text-accent",
          className,
        )}
        {...rest}
      >
        {children}
      </button>
    </Tip>
  );
});

export function Tip({ label, children, side = "bottom" }: { label: string; children: ReactNode; side?: "top" | "bottom" | "left" | "right" }) {
  return (
    <RTooltip.Root delayDuration={500}>
      <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
      <RTooltip.Portal>
        <RTooltip.Content
          side={side}
          sideOffset={6}
          className="z-50 rounded-md bg-raised px-2 py-1 text-[12px] text-text shadow-(--shadow-float) ring-1 ring-separator-strong"
        >
          {label}
        </RTooltip.Content>
      </RTooltip.Portal>
    </RTooltip.Root>
  );
}

export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <RSwitch.Root
      checked={checked}
      onCheckedChange={onChange}
      aria-label={label}
      className={cx(
        "relative h-[18px] w-[30px] shrink-0 rounded-full transition-colors duration-150",
        checked ? "bg-accent" : "bg-control-hover ring-1 ring-separator-strong ring-inset",
      )}
    >
      <RSwitch.Thumb className="block h-[14px] w-[14px] translate-x-[2px] rounded-full bg-white shadow transition-transform duration-150 data-[state=checked]:translate-x-[14px]" />
    </RSwitch.Root>
  );
}

export interface SegOption<T extends string> {
  value: T;
  label: ReactNode;
  title?: string;
}

/** Pill segmented control (flag tiers, sort, overlay modes). */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  size = "md",
}: {
  options: SegOption<T>[];
  value: T;
  onChange: (v: T) => void;
  label: string;
  size?: "sm" | "md";
}) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex rounded-(--radius-control) bg-control-hover/60 p-0.5 ring-1 ring-separator ring-inset">
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={o.value === value}
          title={o.title}
          onClick={() => onChange(o.value)}
          className={cx(
            "rounded-[5px] font-medium transition-colors duration-75",
            size === "sm" ? "h-5 px-1.5 text-[11px]" : "h-6 px-2.5 text-[12px]",
            o.value === value ? "bg-control text-text shadow-sm ring-1 ring-separator-strong" : "text-text-2 hover:text-text",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Popover({ trigger, children, align = "start" }: { trigger: ReactNode; children: ReactNode; align?: "start" | "center" | "end" }) {
  return (
    <RPopover.Root>
      <RPopover.Trigger asChild>{trigger}</RPopover.Trigger>
      <RPopover.Portal>
        <RPopover.Content
          align={align}
          sideOffset={6}
          className="z-50 max-h-[70vh] overflow-auto rounded-(--radius-panel) bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong data-[state=open]:animate-[pop_180ms_var(--ease-spring)]"
        >
          {children}
        </RPopover.Content>
      </RPopover.Portal>
    </RPopover.Root>
  );
}

export const PopoverClose = RPopover.Close;

/** A sheet slides down from under the title bar (SPEC §14.4). */
export function Sheet({ open, onOpenChange, title, children }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; children: ReactNode }) {
  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <RDialog.Portal>
        <RDialog.Overlay className="fixed inset-0 z-40 bg-black/25" />
        <RDialog.Content className="fixed top-(--h-titlebar) left-1/2 z-50 w-[520px] max-w-[92vw] -translate-x-1/2 rounded-b-(--radius-sheet) bg-raised p-5 shadow-(--shadow-float) ring-1 ring-separator-strong data-[state=open]:animate-[sheet_220ms_var(--ease-spring)]">
          <RDialog.Title className="mb-3 text-[15px] font-semibold">{title}</RDialog.Title>
          {children}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

export function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <div className="text-text-3">{icon}</div>
      <div className="text-[13px] text-text-2">{title}</div>
      {children}
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded border border-separator-strong px-1 font-mono text-[10px] text-text-3">{children}</kbd>;
}

export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" className="animate-spin text-text-3" aria-hidden>
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="2" opacity="0.25" />
      <path d="M8 2a6 6 0 0 1 6 6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

/** Perceptual red→amber→green score colour (SPEC §14.4). */
export function scoreColor(v: number): string {
  if (!Number.isFinite(v)) return "var(--c-separator)";
  const t = Math.max(0, Math.min(1, v));
  return t < 0.5
    ? `color-mix(in oklch, var(--c-score-lo) ${100 - t * 200}%, var(--c-score-mid))`
    : `color-mix(in oklch, var(--c-score-mid) ${200 - t * 200}%, var(--c-score-hi))`;
}
