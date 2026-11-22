import { LogOutIcon, UserIcon } from "lucide-react";

import { api, useSession } from "../api/session.ts";
import { navigate } from "./router.ts";
import { Menu } from "../ui/Menu.tsx";
import { toast } from "../ui/toast.ts";

export async function signOut(): Promise<void> {
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
        triggerClassName="icon-btn"
        trigger={
          <>
            <span className="account-avatar" data-i={user.name.slice(0, 2).toUpperCase()} aria-hidden="true" />
            <span className="sr">{user.name}</span>
          </>
        }
        header={
          <>
            Signed in as <strong>{user.name}</strong>
          </>
        }
        items={[{ label: "Account", icon: <UserIcon aria-hidden="true" />, onSelect: () => navigate("/app/account") }, { label: "Sign out", danger: true, separator: true, icon: <LogOutIcon aria-hidden="true" />, testId: "signout", onSelect: () => void signOut() }]}
      />
    </div>
  );
}
