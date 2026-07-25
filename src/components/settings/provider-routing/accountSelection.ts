export function toggleAccountSelection(current: Set<string>, accountId: string): Set<string> {
  const next = new Set(current);
  if (next.has(accountId)) next.delete(accountId);
  else next.add(accountId);
  return next;
}

export function selectionAfterDelete(current: Set<string>, deleted: readonly string[]): Set<string> {
  const next = new Set(current);
  deleted.forEach((id) => next.delete(id));
  return next;
}
