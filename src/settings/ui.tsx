// Small shared building blocks for the Settings window.
import { useEffect, useRef, useState, type ReactNode } from "react";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

export function Card({ children, className, dim }: { children: ReactNode; className?: string; dim?: boolean }) {
  return (
    <section
      className={cx(
        "rounded-[12px] border border-card-border bg-card px-4 py-3.5 transition-opacity",
        dim && "opacity-60",
        className,
      )}
    >
      {children}
    </section>
  );
}

export function CardHeader({ title, detail, right }: { title: ReactNode; detail?: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <h3 className="text-[13.5px] font-semibold">{title}</h3>
        {detail && <p className="mt-0.5 text-[12px] leading-[1.45] text-text-2">{detail}</p>}
      </div>
      {right && <div className="flex shrink-0 items-center gap-2">{right}</div>}
    </div>
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
        "relative shrink-0 rounded-full border transition-colors duration-150 disabled:opacity-40",
        checked ? "border-accent bg-accent" : "border-text-3 bg-transparent",
      )}
      style={{ width: w, height: h }}
    >
      <span
        className={cx("absolute top-[2px] rounded-full transition-all duration-150", checked ? "bg-white" : "bg-text-2")}
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
        className="absolute top-[3px] bottom-[3px] rounded-[6px] bg-card shadow-sm ring-1 ring-card-border transition-all duration-200"
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
        "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-[6px] font-medium transition-colors disabled:opacity-40",
        small ? "h-[24px] px-2.5 text-[12px]" : "h-[28px] px-3 text-[12.5px]",
        kind === "default" && "border border-card-border bg-fill hover:bg-fill-strong",
        kind === "primary" && "bg-accent text-white hover:brightness-110",
        kind === "destructive" && "border border-card-border bg-fill text-danger hover:bg-fill-strong",
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

const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round", strokeLinejoin: "round" } as const;

export const Icon = {
  Waveform: ({ size = 16 }: { size?: number }) => (
    <svg width={size} height={size} viewBox="0 0 16 16" {...stroke}>
      <path d="M2 7v2M5 4.5v7M8 2v12M11 4.5v7M14 7v2" />
    </svg>
  ),
  Mic: ({ size = 16 }: { size?: number }) => (
    <svg width={size} height={size} viewBox="0 0 16 16" {...stroke}>
      <rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5" />
      <path d="M3 8a5 5 0 0 0 10 0M8 13v1.8" />
    </svg>
  ),
  Check: ({ size = 16 }: { size?: number }) => (
    <svg width={size} height={size} viewBox="0 0 16 16" {...stroke} strokeWidth={2}>
      <path d="M3.5 8.5l3 3 6-7" />
    </svg>
  ),
  Chevron: ({ open }: { open: boolean }) => (
    <svg width={14} height={14} viewBox="0 0 16 16" {...stroke} style={{ transform: open ? "rotate(90deg)" : undefined, transition: "transform 0.15s" }}>
      <path d="M6 3.5l4.5 4.5L6 12.5" />
    </svg>
  ),
  Copy: () => (
    <svg width={14} height={14} viewBox="0 0 16 16" {...stroke}>
      <rect x="5" y="5" width="8.5" height="8.5" rx="1.5" />
      <path d="M11 5V3.5A1.5 1.5 0 0 0 9.5 2h-6A1.5 1.5 0 0 0 2 3.5v6A1.5 1.5 0 0 0 3.5 11H5" />
    </svg>
  ),
  Trash: () => (
    <svg width={14} height={14} viewBox="0 0 16 16" {...stroke}>
      <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4" />
    </svg>
  ),
  Plus: () => (
    <svg width={13} height={13} viewBox="0 0 16 16" {...stroke} strokeWidth={1.9}>
      <path d="M8 3v10M3 8h10" />
    </svg>
  ),
  Warning: () => (
    <svg width={14} height={14} viewBox="0 0 16 16" {...stroke} className="mt-[1px] shrink-0">
      <path d="M8 1.8l6.5 11.7h-13L8 1.8zM8 6.5v3.2M8 11.6v.1" />
    </svg>
  ),
  Flame: () => (
    <svg width={16} height={16} viewBox="0 0 16 16" {...stroke}>
      <path d="M8 14.5c2.8 0 4.5-1.8 4.5-4.3 0-3.2-3-4.3-3.4-8.2C6.6 3.6 5.8 5.6 6.2 7.4 5.3 7 4.9 6.1 4.8 5.3 3.9 6.3 3.5 7.5 3.5 10.2c0 2.5 1.7 4.3 4.5 4.3z" />
    </svg>
  ),
};
