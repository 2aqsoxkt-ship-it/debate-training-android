import type { ReactNode } from "react";

export function StatusBadge({
  tone = "neutral",
  children,
}: {
  tone?:
    "neutral" | "success" | "warning" | "danger" | "affirmative" | "negative";
  children: ReactNode;
}) {
  return <span className={`status-badge ${tone}`}>{children}</span>;
}

export function InlineNotice({
  tone = "info",
  title,
  children,
}: {
  tone?: "info" | "success" | "warning" | "danger";
  title?: string;
  children: ReactNode;
}) {
  return (
    <section
      className={`inline-notice ${tone}`}
      role={tone === "danger" ? "alert" : "status"}
    >
      {title ? <strong>{title}</strong> : null}
      <div>{children}</div>
    </section>
  );
}

export function ProgressBar({
  value,
  label,
}: {
  value: number;
  label: string;
}) {
  const safe = Math.min(100, Math.max(0, value));
  return (
    <div className="progress-block">
      <div className="progress-label">
        <span>{label}</span>
        <span>{Math.round(safe)}%</span>
      </div>
      <div
        className="progress-track"
        role="progressbar"
        aria-label={label}
        aria-valuenow={safe}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style={{ width: `${safe}%` }} />
      </div>
    </div>
  );
}

export function TeamClock({
  side,
  label,
  ms,
  active,
}: {
  side: "affirmative" | "negative";
  label: string;
  ms: number;
  active?: boolean;
}) {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return (
    <div className={`team-clock ${side} ${active ? "active" : ""}`}>
      <span>{label}</span>
      <strong>
        {String(minutes).padStart(2, "0")}:{String(remainder).padStart(2, "0")}
      </strong>
    </div>
  );
}

export function EmptyState({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <strong>{title}</strong>
      {children ? <div>{children}</div> : null}
    </div>
  );
}
