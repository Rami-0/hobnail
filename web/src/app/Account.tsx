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
  // The initials are the button's visible text, so they are in its name too
  // ("Account: admin, AD"): a speech user can say what they see. (Not in
  // parentheses: name matching drops parentheticals, axe 2.5.3.)
  const initials = user.name.slice(0, 2).toUpperCase();
  return (
    <div className="acct">
      <Menu
        label={`Account: ${user.name}, ${initials}`}
        testId="user"
        triggerClassName="icon-btn"
        trigger={
          <>
            {/* The name first, so the button's text reads "admin AD" in the
                order its label names them. */}
            <span className="sr">{user.name}</span>
            <span className="account-avatar">{initials}</span>
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
