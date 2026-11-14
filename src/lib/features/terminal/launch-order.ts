/** The order the owner gave the launch menu. Presentation only: the files stay as they are. */
const KEY = 'wyss-console.launch-order';

/** Launches in the owner's order; any the order does not name follow, in the order given. */
export function applyOrder<T extends { id: string }>(
  items: readonly T[],
  order: readonly string[],
): T[] {
  const rank = new Map(order.map((id, index) => [id, index]));
  return items
    .map((item, index) => ({ item, index }))
    .sort((a, b) => {
      const left = rank.get(a.item.id) ?? Number.MAX_SAFE_INTEGER;
      const right = rank.get(b.item.id) ?? Number.MAX_SAFE_INTEGER;
      return left - right || a.index - b.index;
    })
    .map(({ item }) => item);
}

/** The whole order after `id` moves one place up (-1) or down (1) among `ids`. */
export function moveLaunch(
  ids: readonly string[],
  id: string,
  by: -1 | 1,
): string[] {
  const next = [...ids];
  const from = next.indexOf(id);
  const to = from + by;
  if (from < 0 || to < 0 || to >= next.length) return next;
  [next[from], next[to]] = [next[to], next[from]];
  return next;
}

export function parseOrder(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value)
      ? value.filter((id): id is string => typeof id === 'string')
      : [];
  } catch {
    return [];
  }
}

export function loadOrder(): string[] {
  try {
    return parseOrder(globalThis.localStorage?.getItem(KEY) ?? null);
  } catch {
    return [];
  }
}

export function saveOrder(order: readonly string[]): void {
  try {
    globalThis.localStorage?.setItem(KEY, JSON.stringify(order));
  } catch {
    /* An order that cannot be kept is not an error. */
  }
}

/** The order with a renamed launch in its old place, so a rename does not move it in the menu. */
export function renameInOrder(
  order: readonly string[],
  from: string,
  to: string,
): string[] {
  return order.includes(from)
    ? order.filter((id) => id !== to).map((id) => (id === from ? to : id))
    : [...order];
}
