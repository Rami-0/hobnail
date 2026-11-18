import { LogOutIcon, UserIcon } from "lucide-react";

import { api, useSession } from "../api/session.ts";
import { Menu, toast } from "../ui/index.ts";

async function signOut(): Promise<void> {
  const r = await api.post("/api/logout");
  if (r.kind !== "ok" && r.kind !== "sign_in") {
    toast(`Sign-out failed: ${r.message}`, { kind: "bad" });
    return;
  }
  const next = encodeURIComponent(window.location.pathname + window.location.search + window.location.hash);
  window.location.replace(`/app/login?next=${next}`);
}

export function Account() {
  const s = useSession();
  const user = s.session?.user;
  if (!user) return null;
  return (
    <div className="acct">
      <Menu
        label={`Account: ${user.name}`}
        testId="user"
        triggerClassName="acct-btn"
        trigger={
          <>
            <UserIcon aria-hidden="true" />
            <span className="acct-name">{user.name}</span>
            <span className="signed" aria-hidden="true" />
          </>
        }
        header={
          <>
            Signed in as <strong>{user.name}</strong>
          </>
        }
        items={[{ label: "Sign out", icon: <LogOutIcon aria-hidden="true" />, testId: "signout", onSelect: () => void signOut() }]}
      />
    </div>
  );
}
