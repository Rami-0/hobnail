// Match the server's '*' and '?' pattern vocabulary.
export function globMatches(pattern: string, name: string): boolean {
  const parts = [...pattern];
  const chars = [...name];
  let part = 0;
  let char = 0;
  let star = -1;
  let restart = 0;
  while (char < chars.length) {
    if (parts[part] === "?" || parts[part] === chars[char]) {
      part++;
      char++;
    } else if (parts[part] === "*") {
      star = part++;
      restart = char;
    } else if (star >= 0) {
      part = star + 1;
      char = ++restart;
    } else return false;
  }
  while (parts[part] === "*") part++;
  return part === parts.length;
}
