/**
 * Relative time formatting and date grouping for version history.
 */

/** Format a Unix timestamp (seconds) as a relative time string. */
export function relativeTime(timestampSecs: number): string {
  if (timestampSecs <= 0) return '';
  const now = Date.now() / 1000;
  const diff = now - timestampSecs;

  if (diff < 60) return 'just now';
  if (diff < 3600) {
    const mins = Math.floor(diff / 60);
    return `${mins}m ago`;
  }
  if (diff < 86400) {
    const hours = Math.floor(diff / 3600);
    return `${hours}h ago`;
  }
  if (diff < 172800) return 'yesterday';

  // Fall back to short time format
  return formatShortTime(timestampSecs);
}

/** Format a Unix timestamp (seconds) as a short time like "2:30p". */
export function formatShortTime(timestampSecs: number): string {
  if (timestampSecs <= 0) return '';
  const date = new Date(timestampSecs * 1000);
  let hours = date.getHours();
  const minutes = date.getMinutes().toString().padStart(2, '0');
  const ampm = hours >= 12 ? 'p' : 'a';
  hours = hours % 12 || 12;
  return `${hours}:${minutes}${ampm}`;
}

/** Format a session duration in a human-readable way. */
export function formatDuration(startSecs: number, endSecs: number): string {
  const diff = endSecs - startSecs;
  if (diff < 60) return '< 1 min';
  if (diff < 3600) {
    const mins = Math.floor(diff / 60);
    return `${mins} min`;
  }
  const hours = Math.floor(diff / 3600);
  const mins = Math.floor((diff % 3600) / 60);
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
}

/**
 * Group items by date label.
 * Returns an ordered list of [label, items[]] tuples.
 */
export function groupByDate<T>(
  items: T[],
  getTimestamp: (item: T) => number,
): Array<[string, T[]]> {
  const groups = new Map<string, T[]>();

  for (const item of items) {
    const label = dateGroupLabel(getTimestamp(item));
    const existing = groups.get(label);
    if (existing) {
      existing.push(item);
    } else {
      groups.set(label, [item]);
    }
  }

  return Array.from(groups.entries());
}

/** Get a date group label for a Unix timestamp (seconds). */
function dateGroupLabel(timestampSecs: number): string {
  if (timestampSecs <= 0) return 'recent';
  const date = new Date(timestampSecs * 1000);
  const now = new Date();

  const dateStr = date.toDateString();
  const todayStr = now.toDateString();

  if (dateStr === todayStr) return 'today';

  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (dateStr === yesterday.toDateString()) return 'yesterday';

  // Same year: "march 22"
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString('en-US', { month: 'long', day: 'numeric' }).toLowerCase();
  }

  // Different year: "dec 14, 2025"
  return date
    .toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })
    .toLowerCase();
}
