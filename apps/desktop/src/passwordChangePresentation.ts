export type PasswordChangeImpact =
  | "metadata"
  | "exposure_policy"
  | "references"
  | "locator"
  | "refresh"
  | "delete";

export type PasswordChangeDiffEntry = {
  path: string;
  label: string;
  before?: string | null;
  after?: string | null;
  impact: PasswordChangeImpact;
};

export type PasswordChangeItemDiff = {
  record_id: string;
  item_id: string;
  title: string;
  vaults?: string[];
  entries: PasswordChangeDiffEntry[];
};

export type GroupedPasswordChangeItem = {
  item_id: string;
  title: string;
  vaults: string[];
  record_ids: string[];
  entries: PasswordChangeDiffEntry[];
};

function entryKey(entry: PasswordChangeDiffEntry): string {
  return JSON.stringify([
    entry.path,
    entry.label,
    entry.before ?? null,
    entry.after ?? null,
    entry.impact,
  ]);
}

function displayTitle(item: PasswordChangeItemDiff): string {
  const changedTitle = item.entries.find(
    (entry) => entry.path === "/title" && entry.after?.trim(),
  )?.after;
  return changedTitle?.trim() || item.title;
}

export function groupPasswordChangeItems(
  items: PasswordChangeItemDiff[],
): GroupedPasswordChangeItem[] {
  const grouped = new Map<string, GroupedPasswordChangeItem>();
  const entryKeys = new Map<string, Set<string>>();

  for (const item of items) {
    const current = grouped.get(item.item_id);
    if (!current) {
      grouped.set(item.item_id, {
        item_id: item.item_id,
        title: displayTitle(item),
        vaults: [...(item.vaults ?? [])],
        record_ids: [item.record_id],
        entries: [...item.entries],
      });
      entryKeys.set(
        item.item_id,
        new Set(item.entries.map((entry) => entryKey(entry))),
      );
      continue;
    }

    current.record_ids.push(item.record_id);
    current.vaults = Array.from(
      new Set([...current.vaults, ...(item.vaults ?? [])]),
    );
    current.title = displayTitle(item);
    const seen = entryKeys.get(item.item_id);
    if (!seen) {
      throw new Error(`Missing diff-entry index for ${item.item_id}`);
    }
    for (const entry of item.entries) {
      const key = entryKey(entry);
      if (!seen.has(key)) {
        current.entries.push(entry);
        seen.add(key);
      }
    }
  }

  return Array.from(grouped.values());
}
