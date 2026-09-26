/** Display only: keep the original path for selections and native file operations. */
export function displayPath(path: string): string {
  if (path.slice(0, 8).toUpperCase() === "\\\\?\\UNC\\") return "\\\\" + path.slice(8);
  if (/^\\\\\?\\[a-z]:\\/i.test(path)) return path.slice(4);
  return path;
}
