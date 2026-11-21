import { needsSecret } from "../boards/document.ts";
import { Button } from "../ui/Button.tsx";
import { Field, TextInput } from "../ui/Field.tsx";
import { Note } from "../ui/Note.tsx";
import { Switch } from "../ui/Switch.tsx";

export function HttpConnectionFields({ id, url, secret, bound, enabled, fields, busy, setUrl, setSecret, setEnabled, clearSecret }: {
  id: string;
  url: string;
  secret: string;
  bound?: string | null;
  enabled: boolean;
  fields?: Record<string, string>;
  busy?: boolean;
  setUrl: (value: string) => void;
  setSecret: (value: string) => void;
  setEnabled: (value: boolean) => void;
  clearSecret?: () => void;
}) {
  return <>
    <Field id={`${id}-url`} label="Service URL" error={fields?.base_url}>
      <TextInput name="base_url" type="url" required maxLength={2048} value={url} onChange={(e) => setUrl(e.target.value)} />
    </Field>
    <Field id={`${id}-secret`} label="Bearer credential" error={fields?.secret}
      hint={bound ? `A credential is saved for ${bound}. Path changes keep the binding.` : "Optional. The credential is bound to the origin (scheme, host, and port)."}>
      <TextInput name="secret" type="password" autoComplete="new-password" maxLength={4096} value={secret} onChange={(e) => setSecret(e.target.value)} />
    </Field>
    {secret && /^http:\/\//i.test(url.trim()) && <Note kind="warn">This HTTP connection sends the credential unencrypted.</Note>}
    {needsSecret(bound, url) && <Note kind="warn">The origin changed. Enter the credential again for this destination.</Note>}
    {bound && clearSecret && <Button busy={busy} onClick={clearSecret}>Clear saved credential</Button>}
    <Switch checked={enabled} onChange={setEnabled} label="Enabled" />
  </>;
}
