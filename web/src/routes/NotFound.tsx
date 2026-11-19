// An /app path the client does not know.
import { appHref } from "../app/router.ts";

export function NotFound({ path }: { path: string }) {
  return (
    <div className="page narrow">
      <div className="panel">
        <div className="empty">
          <h1>No such page</h1>
          <p>
            No page exists at <code>{path}</code>. Go to <a href={appHref("/")}>its home page</a> or to{" "}
            <a href="/">the current UI</a>.
          </p>
        </div>
      </div>
    </div>
  );
}
