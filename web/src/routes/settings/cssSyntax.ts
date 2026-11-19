// Advisory syntax diagnostics for the appearance editor.
export function cssSyntax(t: string): string {
    const open: number[] = []; let line = 1, q = "", qAt = 0, cAt = 0;
    for (let i = 0; i < t.length; i++) {
      const c = t[i];
      if (c === "\n") { line++; if (q) return `line ${qAt}: a string that does not close.`; continue; }
      if (cAt) { if (c === "*" && t[i + 1] === "/") { cAt = 0; i++; } continue; }
      if (q) { if (c === "\\") i++; else if (c === q) q = ""; continue; }
      if (c === "/" && t[i + 1] === "*") { cAt = line; i++; }
      else if (c === '"' || c === "'") { q = c; qAt = line; }
      else if (c === "{") open.push(line);
      else if (c === "}") { if (!open.length) return `line ${line}: a } without its {.`; open.pop(); }
    }
    if (cAt) return `line ${cAt}: a comment (/*) that does not close.`;
    if (q) return `line ${qAt}: a string that does not close.`;
    return open.length ? `line ${open[open.length - 1]}: a { that does not close.` : "";
  }
