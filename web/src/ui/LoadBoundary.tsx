import { Component, Suspense, type ReactNode } from "react";
import { Button } from "./Button.tsx";
import { Banner } from "./Note.tsx";

export function LoadError({ children }: { children: ReactNode }) {
  return <Banner kind="bad" action={<Button size="sm" onClick={() => location.reload()}>Reload</Button>}>{children}</Banner>;
}

// `failed` replaces the Reload banner (null for parts that may vanish);
// `onError` lets the owner reset the state that mounted the part.
export class LoadBoundary extends Component<{ children: ReactNode; fallback: ReactNode; failed?: ReactNode; onError?: () => void }> {
  override state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  override componentDidCatch() {
    this.props.onError?.();
  }

  override render() {
    const { children, fallback, failed = <LoadError>Could not load this content.</LoadError> } = this.props;
    return this.state.failed ? failed : <Suspense fallback={fallback}>{children}</Suspense>;
  }
}
