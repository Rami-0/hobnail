/// <reference lib="webworker" />
// Regex evaluation stays off the renderer, including a single match that
// cannot yield. The pane terminates this worker if it misses its deadline.
const worker = globalThis as unknown as DedicatedWorkerGlobalScope;
worker.addEventListener("message", ({ data }: MessageEvent<{ query: string; sensitive: boolean; rows: [number, string][] }>) => {
  let expression: RegExp;
  try { expression = new RegExp(data.query, data.sensitive ? "g" : "gi"); }
  catch { worker.postMessage({ error: true }, []); return; }

  const hits: [number, [number, number][]][] = [];
  const started = performance.now();
  let cursor = 0;
  function slice() {
    const deadline = performance.now() + 8;
    while (cursor < data.rows.length && performance.now() < deadline) {
      const row = data.rows[cursor++];
      if (!row) continue;
      expression.lastIndex = 0;
      const ranges: [number, number][] = [];
      let matched = false;
      for (const hit of row[1].matchAll(expression)) {
        matched = true;
        if (hit[0].length) ranges.push([hit.index, hit.index + hit[0].length]);
        if (ranges.length === 100 || performance.now() >= deadline) break;
      }
      if (matched) hits.push([row[0], ranges]);
    }
    if (cursor === data.rows.length) worker.postMessage({ hits }, []);
    else if (performance.now() - started > 80) worker.postMessage({ error: true }, []);
    else setTimeout(slice, 0);
  }
  slice();
}, { once: true });
