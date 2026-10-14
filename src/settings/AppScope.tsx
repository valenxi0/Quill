import { useEffect, useId, useRef, useState } from "react";
import { api, type KnownApp } from "../lib/api";
import { cx, TextField } from "./ui";

export function useKnownApps() {
  const [apps, setApps] = useState<KnownApp[]>([]);
  useEffect(() => {
    let current = true;
    api.getKnownApps().then((items) => { if (current) setApps(items); }).catch(() => {});
    return () => { current = false; };
  }, []);
  return apps;
}

export function scopeName(exe: string | null | undefined, apps: KnownApp[]) {
  return exe ? apps.find((a) => a.exe.toLowerCase() === exe.toLowerCase())?.name ?? exe : "All apps";
}

export function AppIcon({ app }: { app: KnownApp }) {
  const [failed, setFailed] = useState<string[]>([]);
  const source = app.icon && !failed.includes(app.icon) ? app.icon : null;
  if (source) {
    return <img src={source} alt="" onError={() => setFailed((items) => [...items, source])} className="h-[22px] w-[22px] shrink-0 object-contain" />;
  }
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="h-[22px] w-[22px] shrink-0 text-text-3" fill="none" stroke="currentColor" strokeWidth="1.5">
      <rect x="3" y="4" width="18" height="16" rx="3" />
      <path d="M3 9h18M6 6.5h1m2 0h1" />
    </svg>
  );
}

type PickerOption = { value: string; label: string; app?: KnownApp };

export function AppPicker({ label, value, options, onChange }: {
  label: string;
  value: string;
  options: PickerOption[];
  onChange: (value: string) => void;
}) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const selected = options.find((option) => option.value === value);
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open]);
  useEffect(() => {
    if (open) document.getElementById(`${id}-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [active, id, open]);
  const show = () => {
    setActive(Math.max(0, options.findIndex((option) => option.value === value)));
    setOpen(true);
  };
  const choose = (option: PickerOption) => {
    onChange(option.value);
    setOpen(false);
  };
  return (
    <div ref={root} className="relative">
      <span id={`${id}-label`} className="mb-1 block text-[12px] text-text-2">{label}</span>
      <button
        type="button"
        role="combobox"
        aria-labelledby={`${id}-label`}
        aria-expanded={open}
        aria-controls={`${id}-options`}
        aria-haspopup="listbox"
        aria-activedescendant={open ? `${id}-${active}` : undefined}
        className="flex w-full items-center gap-2 rounded-[6px] border border-card-border bg-fill px-2.5 py-1.5 text-left text-[12.5px] text-text outline-none focus-visible:border-accent focus-visible:ring-1 focus-visible:ring-accent"
        onClick={() => open ? setOpen(false) : show()}
        onKeyDown={(event) => {
          if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
            event.preventDefault();
            if (!open) { show(); return; }
            setActive((index) => event.key === "Home" ? 0 : event.key === "End" ? options.length - 1 :
              (index + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
          } else if (event.key === "Escape" || event.key === "Tab") {
            if (open && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); }
            setOpen(false);
          } else if (open && (event.key === "Enter" || event.key === " ")) {
            event.preventDefault();
            const option = options[active];
            if (option) choose(option);
          }
        }}
      >
        {selected?.app && <AppIcon app={selected.app} />}
        <span className="min-w-0 flex-1 truncate">{selected?.label ?? value}</span>
        <svg aria-hidden="true" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="m4 6 4 4 4-4" /></svg>
      </button>
      {open && (
        <div id={`${id}-options`} role="listbox" aria-labelledby={`${id}-label`} className="absolute top-full right-0 left-0 z-50 mt-1 max-h-[220px] overflow-y-auto rounded-[8px] border border-card-border bg-bg p-1 text-text shadow-xl">
          {options.map((option, index) => (
            <button
              type="button"
              key={option.value}
              id={`${id}-${index}`}
              role="option"
              aria-selected={option.value === value}
              tabIndex={-1}
              className={cx("flex w-full items-center gap-2 rounded-[5px] px-2 py-1.5 text-left text-[12.5px] text-text", active === index && "bg-accent-soft")}
              onMouseEnter={() => setActive(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(option)}
            >
              {option.app && <AppIcon app={option.app} />}
              <span className="min-w-0 flex-1 truncate">{option.label}</span>
              {option.value === value && <span aria-hidden="true" className="text-accent">✓</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function AppScope({ value, onChange }: { value: string | null; onChange: (value: string | null) => void }) {
  const apps = useKnownApps();
  const [custom, setCustom] = useState(false);
  const selected = apps.find((app) => app.exe.toLowerCase() === value?.toLowerCase());
  const options: PickerOption[] = [
    { value: "", label: "All apps" },
    ...(value && !selected && !custom ? [{ value, label: value }] : []),
    ...apps.map((app) => ({ value: app.exe, label: app.name, app })),
    { value: "custom", label: "Other app..." },
  ];
  return (
    <div className="mb-3">
      <AppPicker label="Applies in" value={custom ? "custom" : selected?.exe ?? value ?? ""} options={options} onChange={(next) => {
        setCustom(next === "custom");
        onChange(next === "custom" ? "" : next || null);
      }} />
      {custom && <div className="mt-2"><TextField label="App executable" value={value ?? ""} onChange={(exe) => onChange(exe.trim().toLowerCase())} placeholder="Code.exe" /></div>}
      <p className="mt-1 text-[11.5px] text-text-3">App entries override matching global entries. Quill uses the app you start dictating into.</p>
    </div>
  );
}
