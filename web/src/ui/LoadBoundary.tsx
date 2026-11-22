import { Component, Suspense, type ReactNode } from "react";
import { Button } from "./Button.tsx";
import { Banner } from "./Note.tsx";

export function LoadError({ children }: { children: ReactNode }) {
  return <Banner kind="bad" action={<Button size="sm" onClick={() => location.reload()}>Reload</Button>}>{children}</Banner>;
}

export class LoadBoundary extends Component<{ children: ReactNode; fallback: ReactNode }> {
  override state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  override render() {
    const { children, fallback } = this.props;
    return this.state.failed
      ? <LoadError>Could not load this content.</LoadError>
      : <Suspense fallback={fallback}>{children}</Suspense>;
  }
}
