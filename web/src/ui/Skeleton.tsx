export function Skeleton({ className = "" }: { className?: string }) {
  return <span className={`sk ${className}`} aria-hidden="true" />;
}
