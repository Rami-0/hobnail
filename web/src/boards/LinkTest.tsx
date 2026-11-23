import { useState } from "react";
import type { IntegrationVerdict } from "../api/gen/IntegrationVerdict.ts";
import type { LinkTestInput } from "../api/gen/LinkTestInput.ts";
import { api } from "../api/session.ts";
import { Button } from "../ui/Button.tsx";
import { Note } from "../ui/Note.tsx";

export function LinkTest({ url }: { url: string }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ url: string; ok: boolean; message: string }>();
  return <>
    <Button busy={busy} disabled={!url} onClick={async () => {
      setBusy(true);
      const body: LinkTestInput = { url };
      const response = await api.post<IntegrationVerdict>("/api/v1/boards/test-link", body);
      const verdict = response.kind === "ok" ? response.data : { ok: false, message: response.message };
      setResult({ ...verdict, url });
      setBusy(false);
    }}>Test connection</Button>
    {result?.url === url && <Note kind={result.ok ? "ok" : "warn"}>{result.message}</Note>}
  </>;
}
