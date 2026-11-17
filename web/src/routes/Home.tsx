// /app: Phase 0 has no pages yet; an honest empty state points at the
// server-rendered UI, which still has every feature.
import { HammerIcon } from "lucide-react";

export function Home() {
  return (
    <div className="page narrow">
      <div className="panel">
        <div className="empty">
          <div className="empty-ic">
            <HammerIcon aria-hidden="true" />
          </div>
          <h1>The new client is being built</h1>
          <p>
            The current UI is at <a href="/">/</a>. It has every page and setting; this client gains them one by one.
          </p>
        </div>
      </div>
    </div>
  );
}
