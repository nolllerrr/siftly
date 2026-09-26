import type { FileResult } from "../types";

export function togglePathSelection(
  current: ReadonlySet<string>,
  path: string,
): Set<string> {
  const next = new Set(current);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  return next;
}

export function selectAllFiles(files: readonly FileResult[]): Set<string> {
  return new Set(files.map((file) => file.path));
}

export function selectedFiles(
  files: readonly FileResult[],
  selectedPaths: ReadonlySet<string>,
): FileResult[] {
  return files.filter((file) => selectedPaths.has(file.path));
}
