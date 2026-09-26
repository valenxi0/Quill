// Number and size formats used across Settings.

/** "12.3k" at 10,000 and above; otherwise grouped digits. */
export function compact(n: number): string {
  if (n >= 10_000) {
    const k = n / 1000;
    return `${k >= 100 ? Math.round(k) : k.toFixed(1).replace(/\.0$/, "")}k`;
  }
  return Math.round(n).toLocaleString("en-US");
}

/** "Nm" under an hour, "Hh Mm" otherwise. */
export function duration(seconds: number): string {
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${Math.round(n / 1e6)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} KB`;
  return `${n} B`;
}

/** "March 3" from "yyyy-MM-dd". */
export function monthDay(iso: string): string {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1).toLocaleDateString("en-US", { month: "long", day: "numeric" });
}

export function timeAgo(iso: string): string {
  const then = new Date(iso);
  return then.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}
