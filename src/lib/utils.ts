/** Joins class names, skipping empty ones. The dither engine imports it as `@/lib/utils`. */
export function cn(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}
