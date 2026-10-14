// Small shared building blocks for the Settings window.
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  CaretRight,
  Check,
  Copy,
  Fire,
  Microphone,
  Plus,
  Trash,
  Warning as WarningIcon,
  Waveform,
} from "@phosphor-icons/react";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export function Card({ children, className, dim }: { children: ReactNode; className?: string; dim?: boolean }) {
  return (
    <section
      className={cx(
        "rounded-[10px] bg-card px-4 py-3.5 shadow-panel transition-opacity",
        dim && "opacity-60",
        className,
      )}
    >
      {children}
    </section>
  );
}

export function CardHeader({ title, detail, right, icon }: { title: ReactNode; detail?: ReactNode; right?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      {icon && <span className="mt-[1px] flex h-[20px] w-[20px] shrink-0 items-center justify-center text-text-2">{icon}</span>}
      <div className="min-w-0 flex-1">
        <h3 className="text-[13.5px] font-semibold">{title}</h3>
        {detail && <p className="mt-0.5 text-[12px] leading-[1.45] text-text-2">{detail}</p>}
      </div>
      {right && <div className="flex shrink-0 items-center gap-2">{right}</div>}
    </div>
  );
}

/** A labelled group of cards, like a section in Windows Settings. */
export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-[6px]">
      <h2 className="px-0.5 pt-1 text-[12px] font-semibold text-text-2">{title}</h2>
      {children}
    </section>
  );
}

export function Divider({ className }: { className?: string }) {
  return <div className={cx("my-3 h-px bg-hairline", className)} />;
}

export function Switch({
  checked,
  onChange,
  disabled,
  large,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  large?: boolean;
  label: string;
}) {
  const w = large ? 46 : 38;
  const h = large ? 26 : 20;
  const knob = h - 6;
  return (
    <button
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative shrink-0 rounded-full border transition-[background-color,border-color] duration-150 disabled:opacity-40",
        checked ? "border-accent bg-accent" : "border-text-3 bg-transparent",
      )}
      style={{ width: w, height: h }}
    >
      <span
        className={cx("absolute top-[2px] rounded-full transition-[left,background-color] duration-150 ease-out", checked ? "bg-white" : "bg-text-2")}
        style={{ width: knob, height: knob, left: checked ? w - knob - 4 : 2 }}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  width,
  disabled,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  width?: number;
  disabled?: boolean;
}) {
  const index = Math.max(0, options.findIndex((o) => o.value === value));
  return (
    <div
      className={cx("relative flex rounded-[8px] bg-fill p-[3px]", disabled && "pointer-events-none opacity-50")}
      style={{ width }}
      role="radiogroup"
    >
      <span
        className="absolute top-[3px] bottom-[3px] rounded-[6px] shadow-sm transition-[left] duration-200 ease-out"
        style={{
          left: `calc(${(index / options.length) * 100}% + 3px)`,
          width: `calc(${100 / options.length}% - 6px)`,
          background: "var(--fill-strong)",
        }}
      />
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          disabled={disabled}
          aria-checked={o.value === value}
          onClick={() => onChange(o.value)}
          className={cx(
            "relative z-10 flex-1 rounded-[6px] px-2 py-[5px] text-[12.5px]",
            o.value === value ? "font-semibold" : "text-text-2",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

type ButtonKind = "default" | "primary" | "destructive" | "plain";

export function Button({
  children,
  onClick,
  kind = "default",
  disabled,
  small,
  title,
  className,
}: {
  children: ReactNode;
  onClick?: () => void;
  kind?: ButtonKind;
  disabled?: boolean;
  small?: boolean;
  title?: string;
  className?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={cx(
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-[6px] font-medium transition-[background-color,scale,filter] duration-100 active:scale-[0.96] disabled:opacity-40 disabled:active:scale-100",
        small ? "h-[24px] px-2.5 text-[12px]" : "h-[28px] px-3 text-[12.5px]",
        kind === "default" && "bg-fill shadow-[0_0_0_1px_var(--card-border)] hover:bg-fill-strong",
        kind === "primary" && "bg-accent text-white hover:brightness-110",
        kind === "destructive" && "bg-fill text-danger shadow-[0_0_0_1px_var(--card-border)] hover:bg-fill-strong",
        kind === "plain" && "text-accent hover:bg-fill",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function IconButton({ onClick, label, children }: { onClick: () => void; label: string; children: ReactNode }) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      title={label}
      className="inline-flex h-[24px] w-[24px] shrink-0 items-center justify-center rounded-[6px] text-text-2 hover:bg-fill hover:text-text"
    >
      {children}
    </button>
  );
}

export function Dot({ color }: { color: "green" | "yellow" | "orange" | "gray" | "red" }) {
  const map = { green: "bg-ok", yellow: "bg-warn", orange: "bg-alert", gray: "bg-text-3", red: "bg-danger" };
  return <span className={cx("inline-block h-[9px] w-[9px] shrink-0 rounded-full", map[color])} />;
}

export function Capsule({ children, accent }: { children: ReactNode; accent?: boolean }) {
  return (
    <span
      className={cx(
        "inline-flex items-center rounded-full px-2 py-[1px] text-[11px] font-medium",
        accent ? "bg-accent-soft text-accent" : "bg-fill text-text-2",
      )}
    >
      {children}
    </span>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return (
    <span
      className="inline-block shrink-0 rounded-full border-2 border-text-3 border-t-accent"
      style={{ width: size, height: size, animation: "spin 0.8s linear infinite" }}
    />
  );
}

export function Progress({ fraction }: { fraction: number }) {
  return (
    <div className="h-[4px] w-full overflow-hidden rounded-full bg-fill-strong">
      <div className="h-full rounded-full bg-accent transition-[width] duration-200" style={{ width: `${fraction * 100}%` }} />
    </div>
  );
}

export function Warning({ children }: { children: ReactNode }) {
  return (
    <p className="mt-2 flex items-start gap-1.5 text-[12px] text-alert">
      <Icon.Warning />
      <span>{children}</span>
    </p>
  );
}

/** A modal sheet over the window. */
export function Sheet({
  title,
  children,
  onClose,
  footer,
  width = 460,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  footer: ReactNode;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    ref.current?.querySelector<HTMLElement>("input, textarea")?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-6" onMouseDown={onClose}>
      <div
        ref={ref}
        role="dialog"
        aria-label={title}
        className="max-h-full overflow-auto rounded-[12px] border border-card-border p-5 shadow-2xl"
        style={{ width, background: "var(--bg)", animation: "pop-in 0.16s ease-out" }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2 className="mb-3 text-[15px] font-semibold">{title}</h2>
        {children}
        <div className="mt-4 flex justify-end gap-2">{footer}</div>
      </div>
    </div>
  );
}

export function TextField({
  label,
  value,
  onChange,
  placeholder,
  multiline,
  mono,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  multiline?: boolean;
  mono?: boolean;
}) {
  const cls = cx(
    "selectable w-full rounded-[6px] border border-card-border bg-fill px-2.5 py-1.5 text-[12.5px] outline-none focus:border-accent",
    mono && "font-mono",
  );
  return (
    <label className="mb-3 block">
      <span className="mb-1 block text-[12px] text-text-2">{label}</span>
      {multiline ? (
        <textarea className={cx(cls, "min-h-[90px] resize-y")} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />
      ) : (
        <input className={cls} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />
      )}
    </label>
  );
}

export function useCopied(): [boolean, () => void] {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(t);
  }, [copied]);
  return [copied, () => setCopied(true)];
}

export const Icon = {
  Waveform: ({ size = 16 }: { size?: number }) => <Waveform size={size} weight="bold" />,
  Mic: ({ size = 16 }: { size?: number }) => <Microphone size={size} />,
  Check: ({ size = 16 }: { size?: number }) => <Check size={size} weight="bold" />,
  Chevron: ({ open }: { open: boolean }) => (
    <CaretRight size={14} weight="bold" style={{ transform: open ? "rotate(90deg)" : undefined, transition: "transform 0.15s ease-out" }} />
  ),
  Copy: () => <Copy size={15} />,
  Trash: () => <Trash size={15} />,
  Plus: () => <Plus size={13} weight="bold" />,
  Warning: () => <WarningIcon size={15} className="mt-[1px] shrink-0" />,
  Flame: () => <Fire size={16} weight="fill" />,
};
