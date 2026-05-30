import { useEffect, useRef, useState } from "react";
import {
  type PublicResolvedIncident,
  type PublicStatusPage,
  type PublicUptimeDayBucket,
  fetchPublicStatusPage,
} from "../api";
import { formatTimestamp } from "../utils";

type Props = { slug: string };

const TYPE_LABELS: Record<string, string> = {
  http: "HTTP",
  ssl: "SSL",
  heartbeat: "Heartbeat",
  tcp: "TCP",
  dns: "DNS",
};

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const mins = Math.floor(seconds / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  const remMins = mins % 60;
  return remMins > 0 ? `${hours}h ${remMins}m` : `${hours}h`;
}

function uptimeSquareColor(total: number, success: number): string {
  if (total === 0) return "rgba(255,255,255,0.06)";
  const pct = success / total;
  if (pct >= 0.999) return "#4ade80";
  if (pct >= 0.95) return "#f59e0b";
  return "#ff896d";
}

function UptimeSquares({ history }: { history: PublicUptimeDayBucket[] }) {
  return (
    <div className="sp-uptime-squares">
      {history.map((b, i) => {
        const pctStr =
          b.total > 0 ? `${((b.success / b.total) * 100).toFixed(1)}%` : null;
        const title = pctStr
          ? `${b.date}: ${pctStr} (${b.success}/${b.total} checks)`
          : `${b.date}: No data`;
        return (
          <div
            key={i}
            className="sp-uptime-square"
            style={{ background: uptimeSquareColor(b.total, b.success) }}
            title={title}
          />
        );
      })}
    </div>
  );
}

function StatusHeroIcon({ status }: { status: string }) {
  if (status === "operational") {
    return (
      <svg width="60" height="60" viewBox="0 0 60 60" fill="none" style={{ flexShrink: 0 }}>
        <circle cx="30" cy="30" r="30" fill="rgba(74,222,128,0.1)" />
        <circle cx="30" cy="30" r="22" fill="rgba(74,222,128,0.16)" />
        <path d="M19 30l8.5 8.5L41 21" stroke="#4ade80" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }
  if (status === "outage") {
    return (
      <svg width="60" height="60" viewBox="0 0 60 60" fill="none" style={{ flexShrink: 0 }}>
        <circle cx="30" cy="30" r="30" fill="rgba(255,137,109,0.1)" />
        <circle cx="30" cy="30" r="22" fill="rgba(255,137,109,0.16)" />
        <path d="M21 21l18 18M39 21L21 39" stroke="#ff896d" strokeWidth="3.5" strokeLinecap="round" />
      </svg>
    );
  }
  if (status === "degraded") {
    return (
      <svg width="60" height="60" viewBox="0 0 60 60" fill="none" style={{ flexShrink: 0 }}>
        <circle cx="30" cy="30" r="30" fill="rgba(245,158,11,0.1)" />
        <circle cx="30" cy="30" r="22" fill="rgba(245,158,11,0.16)" />
        <path d="M30 19v14M30 37v3" stroke="#f59e0b" strokeWidth="3.5" strokeLinecap="round" />
      </svg>
    );
  }
  return (
    <svg width="60" height="60" viewBox="0 0 60 60" fill="none" style={{ flexShrink: 0 }}>
      <circle cx="30" cy="30" r="30" fill="rgba(148,163,184,0.07)" />
      <circle cx="30" cy="30" r="22" fill="rgba(148,163,184,0.1)" />
      <path d="M30 20v13M30 37v3" stroke="#94a3b8" strokeWidth="3.5" strokeLinecap="round" />
    </svg>
  );
}

function UptimeBar({ percent, label }: { percent: number | null; label: string }) {
  const pct = percent ?? 0;
  const color =
    percent === null
      ? "rgba(148,163,184,0.4)"
      : pct >= 99
        ? "#4ade80"
        : pct >= 95
          ? "#f59e0b"
          : "#ff896d";
  return (
    <div className="sp-uptime-bar-item">
      <div className="sp-uptime-bar-header">
        <span className="sp-uptime-bar-label">{label}</span>
        <span className="sp-uptime-bar-pct" style={{ color }}>
          {percent !== null ? `${percent.toFixed(2)}%` : "-"}
        </span>
      </div>
      <div className="sp-uptime-bar-track">
        <div
          className="sp-uptime-bar-fill"
          style={{ width: `${Math.min(100, Math.max(0, pct))}%`, background: color }}
        />
      </div>
    </div>
  );
}

function statusLabel(status: string) {
  if (status === "up") return "Operational";
  if (status === "down") return "Down";
  if (status === "degraded") return "Degraded";
  if (status === "paused") return "Paused";
  return "Pending";
}

function brandInitials(title: string) {
  return title
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("") || "AS";
}

function groupMonitors(monitors: PublicStatusPage["monitors"]) {
  return monitors.reduce<Record<string, PublicStatusPage["monitors"]>>((acc, monitor) => {
    const key = TYPE_LABELS[monitor.monitor_type] ?? monitor.monitor_type.toUpperCase();
    acc[key] = acc[key] ?? [];
    acc[key].push(monitor);
    return acc;
  }, {});
}

function ResponseTimeGraph({ monitors }: { monitors: PublicStatusPage["monitors"] }) {
  const withResponse = monitors.filter((monitor) => monitor.response_time_ms !== null);
  if (withResponse.length === 0) return null;
  const max = Math.max(...withResponse.map((monitor) => monitor.response_time_ms ?? 0), 1);

  return (
    <section className="sp-card sp-response-card">
      <div className="sp-card-header">
        <h2 className="sp-section-title">Response Time</h2>
        <span>{withResponse.length} active probes</span>
      </div>
      <div className="sp-response-graph">
        {withResponse.map((monitor, index) => {
          const value = monitor.response_time_ms ?? 0;
          const width = Math.max(4, (value / max) * 100);
          return (
            <div key={`${monitor.label}-${index}`} className="sp-response-row">
              <span>{monitor.label}</span>
              <div className="sp-response-track">
                <div style={{ width: `${width}%` }} data-status={monitor.status} />
              </div>
              <strong>{value} ms</strong>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function IncidentHistorySection({ incidents }: { incidents: PublicResolvedIncident[] }) {
  if (incidents.length === 0) return null;
  return (
    <section className="sp-card sp-incident-history-card">
      <h2 className="sp-section-title">Incident History</h2>
      <ul className="sp-incident-list sp-incident-history-list">
        {incidents.map((inc, i) => (
          <li key={i} className="sp-incident-item sp-incident-item-resolved">
            <div className="sp-incident-resolved-dot" />
            <div className="sp-incident-body">
              <span className="sp-incident-label">{inc.monitor_label}</span>
              <div className="sp-incident-meta-row">
                <span className="sp-incident-type-pill" data-type={inc.monitor_type}>
                  {TYPE_LABELS[inc.monitor_type] ?? inc.monitor_type.toUpperCase()}
                </span>
                {inc.downtime_seconds !== null && inc.downtime_seconds > 0 && (
                  <span className="sp-incident-duration">
                    {formatDuration(inc.downtime_seconds)} outage
                  </span>
                )}
                <span className="sp-muted">
                  {formatTimestamp(inc.opened_at)} to {formatTimestamp(inc.resolved_at)}
                </span>
              </div>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function StatusPage({ slug }: Props) {
  const [page, setPage] = useState<PublicStatusPage | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  function load() {
    abortRef.current?.abort();
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    fetchPublicStatusPage(slug, ctrl.signal)
      .then((r) => {
        if (r === null) setNotFound(true);
        else setPage(r);
      })
      .catch((e) => {
        if ((e as Error).name !== "AbortError")
          setError((e as Error).message ?? "Failed to load status page.");
      });
  }

  useEffect(() => {
    load();
    const t = setInterval(load, 60_000);
    return () => {
      clearInterval(t);
      abortRef.current?.abort();
    };
  }, [slug]);

  if (notFound) {
    return (
      <div className="sp-shell">
        <div
          className="sp-card"
          style={{ textAlign: "center", padding: "3rem 2rem", maxWidth: 480, margin: "0 auto" }}
        >
          <h1 className="sp-not-found-title">Status page not found</h1>
          <p className="sp-muted">
            This status page may have been disabled or the link may be incorrect.
          </p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="sp-shell">
        <div
          className="sp-card"
          style={{
            color: "var(--danger)",
            textAlign: "center",
            padding: "2rem",
            maxWidth: 480,
            margin: "0 auto",
          }}
        >
          {error}
        </div>
      </div>
    );
  }

  if (!page) {
    return (
      <div className="sp-shell">
        <div
          className="sp-card sp-muted"
          style={{ textAlign: "center", padding: "2rem", maxWidth: 480, margin: "0 auto" }}
        >
          Loading...
        </div>
      </div>
    );
  }

  const isOperational = page.overall_status === "operational";
  const isOutage = page.overall_status === "outage";
  const operationalCount = page.monitors.filter((m) => m.status === "up").length;
  const totalCount = page.monitors.length;
  const groupedMonitors = groupMonitors(page.monitors);
  const responseMonitors = page.monitors.filter((m) => m.response_time_ms !== null);
  const avgResponseMs = responseMonitors.length > 0
    ? Math.round(responseMonitors.reduce((sum, monitor) => sum + (monitor.response_time_ms ?? 0), 0) / responseMonitors.length)
    : null;

  const heroHeadline = isOperational
    ? "All Systems Operational"
    : isOutage
      ? "Service Disruption"
      : page.overall_status === "degraded"
        ? "Degraded Performance"
        : "Status Unknown";

  return (
    <div className="sp-shell">
      <div className="sp-container">
        <section className="sp-hero" data-status={page.overall_status}>
          <div className="sp-brand-mark">{brandInitials(page.page_title)}</div>
          <div className="sp-hero-text">
            <span className="sp-brand-name">{page.page_title}</span>
            <h1 className="sp-hero-headline" data-status={page.overall_status}>
              {heroHeadline}
            </h1>
            <p className="sp-hero-sub">Live component health, uptime, incidents, and response times.</p>
          </div>
          <StatusHeroIcon status={page.overall_status} />
        </section>

        <section className="sp-trust-grid" aria-label="Current trust metrics">
          <div className="sp-trust-card">
            <span>Components</span>
            <strong>{operationalCount}/{totalCount}</strong>
            <small>operational</small>
          </div>
          <div className="sp-trust-card">
            <span>30d uptime</span>
            <strong>{page.uptime_30d !== null ? `${page.uptime_30d.toFixed(2)}%` : "-"}</strong>
            <small>visible monitors</small>
          </div>
          <div className="sp-trust-card">
            <span>Response</span>
            <strong>{avgResponseMs !== null ? `${avgResponseMs} ms` : "-"}</strong>
            <small>current average</small>
          </div>
          <div className="sp-trust-card" data-alert={page.open_incidents.length > 0 ? "true" : undefined}>
            <span>Incidents</span>
            <strong>{page.open_incidents.length}</strong>
            <small>currently open</small>
          </div>
        </section>

        {page.open_incidents.length > 0 && (
          <section className="sp-card sp-incidents-section">
            <div className="sp-incidents-header">
              <h2 className="sp-section-title sp-section-title-danger">
                <span className="sp-incident-live-dot" />
                Active incidents
              </h2>
              <span className="sp-incident-count">{page.open_incidents.length}</span>
            </div>
            <ul className="sp-incident-list">
              {page.open_incidents.map((inc, i) => (
                <li key={i} className="sp-incident-item sp-incident-item-open">
                  <div className="sp-incident-timeline-dot" />
                  <div className="sp-incident-body">
                    <span className="sp-incident-label">{inc.monitor_label}</span>
                    <div className="sp-incident-meta-row">
                      <span className="sp-incident-type-pill" data-type={inc.monitor_type}>
                        {TYPE_LABELS[inc.monitor_type] ?? inc.monitor_type.toUpperCase()}
                      </span>
                      <span className="sp-muted">since {formatTimestamp(inc.opened_at)}</span>
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          </section>
        )}

        {page.show_monitor_details && page.monitors.length > 0 && (
          <section className="sp-card sp-component-summary-card">
            <div className="sp-card-header">
              <h2 className="sp-section-title">Component Groups</h2>
              <span>{Object.keys(groupedMonitors).length} groups</span>
            </div>
            <div className="sp-component-summary-grid">
              {Object.entries(groupedMonitors).map(([group, monitors]) => (
                <div key={group} className="sp-component-summary">
                  <span>{group}</span>
                  <strong>{monitors.filter((m) => m.status === "up").length}/{monitors.length}</strong>
                  <small>{monitors.map((m) => statusLabel(m.status)).join(" / ")}</small>
                </div>
              ))}
            </div>
          </section>
        )}

        {page.show_monitor_details && page.monitors.length > 0 && (
          <section className="sp-card sp-monitors-card">
            <div className="sp-monitors-header">
              <h2 className="sp-section-title">Monitors</h2>
              <span className="sp-monitors-summary">
                <span className={operationalCount === totalCount ? "sp-summary-ok" : "sp-summary-warn"}>
                  {operationalCount}/{totalCount}
                </span>{" "}
                operational
              </span>
            </div>
            <ul className="sp-monitor-list">
              {page.monitors.map((m, i) => (
                <li key={i} className="sp-monitor-row" data-status={m.status}>
                  <div className="sp-monitor-info">
                    <span className="sp-monitor-dot" data-status={m.status} />
                    <span className="sp-monitor-label">{m.label}</span>
                    <span className="sp-monitor-type-badge" data-type={m.monitor_type}>
                      {TYPE_LABELS[m.monitor_type] ?? m.monitor_type.toUpperCase()}
                    </span>
                    <div className="sp-monitor-right">
                      {m.response_time_ms !== null && (
                        <span className="sp-monitor-rt">{m.response_time_ms} ms</span>
                      )}
                      {m.last_checked_at && (
                        <span className="sp-monitor-checked">
                          {formatTimestamp(m.last_checked_at)}
                        </span>
                      )}
                      <span className="sp-monitor-status-text" data-status={m.status}>
                        {m.status === "up"
                          ? "Operational"
                          : m.status === "down"
                            ? "Down"
                            : m.status === "degraded"
                              ? "Degraded"
                              : m.status === "paused"
                                ? "Paused"
                                : "Pending"}
                      </span>
                    </div>
                  </div>
                  {m.uptime_history.length > 0 && (
                    <div className="sp-monitor-history">
                      <UptimeSquares history={m.uptime_history} />
                      <span className="sp-monitor-uptime-pct">
                        {m.uptime_30d !== null ? `${m.uptime_30d.toFixed(2)}%` : "-"}
                      </span>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          </section>
        )}

        {page.show_uptime_percentages && (
          <section className="sp-card">
            <h2 className="sp-section-title">Overall Uptime</h2>
            <div className="sp-uptime-bars">
              <UptimeBar percent={page.uptime_7d} label="Last 7 days" />
              <UptimeBar percent={page.uptime_30d} label="Last 30 days" />
            </div>
          </section>
        )}

        <ResponseTimeGraph monitors={page.monitors} />

        <IncidentHistorySection incidents={page.incident_history} />

        <footer className="sp-footer">
          <span className="sp-muted">Updated {formatTimestamp(page.last_updated)} / refreshes every 60s</span>
          <span className="sp-powered-by">Alon Sentinel</span>
        </footer>
      </div>
    </div>
  );
}
