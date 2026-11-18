import { useSession } from "../../api/session.ts";
import { Sessions } from "../account/index.tsx";
import { FailureNote, useSettings } from "./shared.tsx";

type Login = { id: number; ts: number; kind: string; subject: string; detail: string };
export default function Security() {
  const user = useSession().session?.user;
  const { data, failure, load } = useSettings<{ recent_logins: Login[] }>("security");
  return <>
    <div className="settings-actions">
      <a className="btn" href="/app/account">Account and password</a>
      {user?.role === "owner" && <a className="btn" href="/app/settings/users">Manage users</a>}
    </div>
    <Sessions />
    <section className="settings-section">
      <h2>Recent sign-ins</h2>
      <FailureNote failure={failure} retry={() => { void load(); }} />
      {data?.recent_logins.length === 0 && <p className="muted">No recent sign-ins.</p>}
      <div className="settings-list">{data?.recent_logins.map((login) => <div className="settings-row" key={login.id}>
        <div><strong>{login.subject}</strong><p>{login.kind === "auth.login_failed" ? "Failed sign-in" : "Signed in"} · {login.detail}</p></div>
        <time dateTime={new Date(login.ts * 1000).toISOString()}>{new Date(login.ts * 1000).toLocaleString()}</time>
      </div>)}</div>
    </section>
  </>;
}
