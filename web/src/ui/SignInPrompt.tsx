import { Button } from "./Button.tsx";

export function SignInPrompt({ retry, busy }: { retry?: () => void; busy?: boolean }) {
  const next = encodeURIComponent(window.location.pathname + window.location.search);
  return <>
    <a className="btn primary sm" data-testid="signin-newtab" href={`/app/login?next=${next}`} target="_blank" rel="noopener noreferrer">Sign in (new tab)</a>
    {retry && <Button className="sm" data-testid="retry" busy={busy} onClick={retry}>Retry</Button>}
  </>;
}
