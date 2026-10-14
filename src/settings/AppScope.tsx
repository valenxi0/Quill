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

type PickerOption = { value: string; label: string; app?: KnownApp; pinned?: boolean };

/** Options whose name or executable contains every typed word. Pinned options always show. */
function filterOptions(options: PickerOption[], query: string) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return options;
  return options.filter((o) => o.pinned || words.every((w) => o.label.toLowerCase().includes(w) || o.app?.exe.toLowerCase().includes(w)));
}

export function AppPicker({ label, value, options, onChange, placeholder }: {
  label: string;
  value: string;
  options: PickerOption[];
  onChange: (value: string) => void;
  /** Shown on the button while no option is chosen. */
  placeholder?: string;
}) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const selected = options.find((option) => option.value === value);
  const shown = filterOptions(options, query);
  const searchable = options.length > 6;
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
  useEffect(() => {
    if (open && searchable) search.current?.focus();
  }, [open, searchable]);
  const show = () => {
    setQuery("");
    setActive(Math.max(0, options.findIndex((option) => option.value === value)));
    setOpen(true);
  };
  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) trigger.current?.focus();
  };
  const choose = (option: PickerOption) => {
    onChange(option.value);
    close(true);
  };
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      // Home and End move the text cursor while typing a search.
      if (event.currentTarget === search.current && (event.key === "Home" || event.key === "End")) return;
      event.preventDefault();
      if (!open) { show(); return; }
      const n = shown.length;
      if (n === 0) return;
      setActive((index) => event.key === "Home" ? 0 : event.key === "End" ? n - 1 :
        (Math.min(index, n - 1) + (event.key === "ArrowDown" ? 1 : -1) + n) % n);
    } else if (event.key === "Escape" || event.key === "Tab") {
      if (open && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); }
      close(event.key === "Escape");
    } else if (open && (event.key === "Enter" || (event.key === " " && event.currentTarget !== search.current))) {
      event.preventDefault();
      const option = shown[Math.min(active, shown.length - 1)];
      if (option) choose(option);
    }
  };
  const activeIndex = Math.min(active, shown.length - 1);
  return (
    <div ref={root} className="relative">
      <span id={`${id}-label`} className="mb-1 block text-[12px] text-text-2">{label}</span>
      <button
        ref={trigger}
        type="button"
        role="combobox"
        aria-labelledby={`${id}-label`}
        aria-expanded={open}
        aria-controls={`${id}-options`}
        aria-haspopup="listbox"
        aria-activedescendant={open && !searchable && activeIndex >= 0 ? `${id}-${activeIndex}` : undefined}
        className="flex w-full items-center gap-2 rounded-[6px] border border-card-border bg-fill px-2.5 py-1.5 text-left text-[12.5px] text-text outline-none focus-visible:border-accent focus-visible:ring-1 focus-visible:ring-accent"
        onClick={() => open ? close(false) : show()}
        onKeyDown={onKeyDown}
      >
        {selected?.app && <AppIcon app={selected.app} />}
        <span className={cx("min-w-0 flex-1 truncate", !selected && !value && "text-text-3")}>{selected?.label ?? (value || placeholder)}</span>
        <svg aria-hidden="true" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="m4 6 4 4 4-4" /></svg>
      </button>
      {open && (
        <div className="absolute top-full right-0 left-0 z-50 mt-1 overflow-hidden rounded-[8px] border border-card-border bg-bg text-text shadow-xl" style={{ animation: "pop-in 0.12s ease-out" }}>
          {searchable && (
            <div className="flex items-center gap-2 border-b border-hairline px-2.5">
              <svg aria-hidden="true" width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" className="shrink-0 text-text-3"><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
              <input
                ref={search}
                role="searchbox"
                aria-label="Search apps"
                aria-controls={`${id}-options`}
                aria-activedescendant={activeIndex >= 0 ? `${id}-${activeIndex}` : undefined}
                value={query}
                placeholder="Search apps"
                onChange={(event) => { setQuery(event.target.value); setActive(0); }}
                onKeyDown={onKeyDown}
                className="selectable h-[32px] min-w-0 flex-1 bg-transparent text-[12.5px] outline-none placeholder:text-text-3 focus-visible:outline-none"
              />
            </div>
          )}
          <div id={`${id}-options`} role="listbox" aria-labelledby={`${id}-label`} className="max-h-[220px] overflow-y-auto p-1">
            {shown.map((option, index) => (
              <button
                type="button"
                key={option.value}
                id={`${id}-${index}`}
                role="option"
                aria-selected={option.value === value}
                tabIndex={-1}
                className={cx("flex w-full items-center gap-2 rounded-[5px] px-2 py-1.5 text-left text-[12.5px] text-text", activeIndex === index && "bg-accent-soft")}
                onMouseEnter={() => setActive(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(option)}
              >
                {option.app && <AppIcon app={option.app} />}
                <span className="min-w-0 flex-1 truncate">{option.label}</span>
                {option.value === value && <span aria-hidden="true" className="text-accent">✓</span>}
              </button>
            ))}
            {shown.length === 0 && <p className="px-2 py-1.5 text-[12px] text-text-3">No open or recent app matches "{query}".</p>}
          </div>
        </div>
      )}
    </div>
  );
}

/** Where an entry applies. `anyApp` offers "All apps"; without it, an app must be chosen. */
export function AppScope({ value, onChange, anyApp = true }: { value: string | null; onChange: (value: string | null) => void; anyApp?: boolean }) {
  const apps = useKnownApps();
  const [custom, setCustom] = useState(false);
  const selected = apps.find((app) => app.exe.toLowerCase() === value?.toLowerCase());
  const options: PickerOption[] = [
    ...(anyApp ? [{ value: "", label: "All apps", pinned: true }] : []),
    ...(value && !selected && !custom ? [{ value, label: value }] : []),
    ...apps.map((app) => ({ value: app.exe, label: app.name, app })),
    { value: "custom", label: "Other app…", pinned: true },
  ];
  return (
    <div className="mb-3">
      <AppPicker label={anyApp ? "Applies in" : "App"} placeholder="Choose an app" value={custom ? "custom" : selected?.exe ?? value ?? ""} options={options} onChange={(next) => {
        setCustom(next === "custom");
        onChange(next === "custom" ? "" : next || null);
      }} />
      {custom && <div className="mt-2"><TextField label="App executable" value={value ?? ""} onChange={(exe) => onChange(exe.trim().toLowerCase())} placeholder="Code.exe" /></div>}
      <p className="mt-1 text-[11.5px] text-text-3">
        {anyApp ? "App entries override matching global entries. " : ""}Quill uses the app you start dictating into.
      </p>
    </div>
  );
}
