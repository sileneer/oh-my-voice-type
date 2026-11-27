/** Short, locale-aware timestamp: "Today 13:45", "Yesterday 07:45", or a date. */
export function formatWhen(ms: number, locale: string, now: Date = new Date()): string {
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  const time = date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const dayOffset = Math.floor((ms - startOfToday) / 86_400_000);
  if (dayOffset === 0 || dayOffset === -1) {
    const day = new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(dayOffset, "day");
    return `${day.charAt(0).toLocaleUpperCase(locale)}${day.slice(1)} ${time}`;
  }
  const sameYear = date.getFullYear() === now.getFullYear();
  const day = date.toLocaleDateString(locale, sameYear
    ? { month: "short", day: "numeric" }
    : { year: "numeric", month: "short", day: "numeric" });
  return `${day} ${time}`;
}
