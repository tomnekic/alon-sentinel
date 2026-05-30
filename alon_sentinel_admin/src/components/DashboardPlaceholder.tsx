import { useEffect, useRef, useState } from "react";
import type { DashboardSiteEntry, GlobalDashboardStats, GlobalIncident } from "../api";
import { formatTimestamp } from "../utils";

function LiveDuration({ openedAt }: { openedAt: string }) {
  const [elapsed, setElapsed] = useState(0);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  useEffect(() => {
    const start = new Date(openedAt).getTime();
    const update = () => setElapsed(Math.floor((Date.now() - start) / 1000));
    update();
    intervalRef.current = setInterval(update, 1000);
    return () => { if (intervalRef.current) clearInterval(intervalRef.current); };
  }, [openedAt]);
  const s = elapsed % 60, m = Math.floor(elapsed / 60) % 60, h = Math.floor(elapsed / 3600);
  const label = h > 0
    ? `${h}h ${m}m`
    : m > 0
    ? `${m}m ${s}s`
    : `${s}s`;
  return <span style={{ fontVariantNumeric: "tabular-nums", color: "var(--danger)", fontWeight: 700 }}>{label}</span>;
}

type StatTone = "success" | "danger" | "neutral" | "muted";

function ActivityTrace({ tone, values }: { tone: StatTone; values: number[] }) {
  return (
    <div className="metric-mini-trace" aria-hidden="true">
      {values.map((value, index) => (
        <span
          key={index}
          className={`metric-mini-trace-bar metric-mini-trace-bar-${tone}`}
          style={{ height: `${Math.max(18, Math.min(100, value))}%` }}
        />
      ))}
    </div>
  );
}

function HeroStatTile({
  label,
  value,
  tone,
  sub,
  movement,
  trace,
}: {
  label: string;
  value: number | string | null | undefined;
  tone: StatTone;
  sub?: string;
  movement?: string;
  trace: number[];
}) {
  const accentMap: Record<StatTone, string> = {
    success: "#22c55e",
    danger: "var(--danger)",
    neutral: "var(--accent)",
    muted: "var(--muted)",
  };
  const bgMap: Record<StatTone, string> = {
    success: "rgba(34, 197, 94, 0.06)",
    danger: "rgba(255, 137, 109, 0.07)",
    neutral: "rgba(24, 183, 255, 0.05)",
    muted: "transparent",
  };
  const accent = accentMap[tone];
  const bg = bgMap[tone];
  return (
    <div
      className={`hero-stat-tile hero-stat-tile-${tone}`}
      style={{
        borderLeft: `3px solid ${accent}`,
        background: bg,
      }}
    >
      <div className="hero-stat-tile-head">
        <span>{label}</span>
        {movement && <strong>{movement}</strong>}
      </div>
      <strong className="hero-stat-tile-value" style={{ color: accent }}>{value ?? "-"}</strong>
      {sub && <span className="hero-stat-tile-sub">{sub}</span>}
      <ActivityTrace tone={tone} values={trace} />
    </div>
  );
}

type DashboardPlaceholderProps = {
  isRefreshing: boolean;
  stats: GlobalDashboardStats | null;
  sites: DashboardSiteEntry[];
  onOpenSite: (siteId: number) => void;
  openIncidents: Array<{ incident: GlobalIncident }>;
};

function SiteStatusBadge({ status, hasOpenIncident }: { status: DashboardSiteEntry["status"]; hasOpenIncident: boolean }) {
  const map: Record<DashboardSiteEntry["status"], { cls: string; label: string }> = {
    up: { cls: "status-badge-success", label: "Up" },
    down: { cls: "status-badge-danger", label: "Down" },
    paused: { cls: "status-badge-muted", label: "Paused" },
    unchecked: { cls: "status-badge-muted", label: "Unchecked" },
    inactive: { cls: "status-badge-muted", label: "Inactive" },
  };
  const { cls, label } = map[status] ?? { cls: "status-badge-muted", label: status };
  return (
    <span style={{ display: "flex", alignItems: "center", gap: "0.4rem", flexWrap: "wrap" }}>
      <span className={`status-badge ${cls}`}>{label}</span>
      {hasOpenIncident && (
        <span
          className="monitor-type-pill"
          style={{ color: "var(--danger)", background: "rgba(255,137,109,0.12)" }}
        >
          INCIDENT
        </span>
      )}
    </span>
  );
}

function MonitorsCell({ active, total }: { active: number; total: number }) {
  if (total === 0) return <span style={{ color: "var(--muted)" }}>None</span>;
  if (active === 0) return <span style={{ color: "var(--muted)" }}>{total} paused</span>;
  if (active === total) return <>{active} active</>;
  return <>{active}/{total} active</>;
}

export function DashboardPlaceholder({
  isRefreshing,
  stats,
  sites,
  onOpenSite,
  openIncidents,
}: DashboardPlaceholderProps) {
  const openIncidentCount = stats?.open_incidents ?? openIncidents.length;
  const hasIncidents = openIncidentCount > 0;
  const sitesTotal = stats?.sites_total ?? 0;
  const activeTotal = stats?.sites_active ?? 0;
  const upRatio = sitesTotal > 0 && stats ? (stats.sites_up / sitesTotal) * 100 : 0;
  const downRatio = sitesTotal > 0 && stats ? (stats.sites_down / sitesTotal) * 100 : 0;
  const incidentRatio = sitesTotal > 0 ? (openIncidentCount / sitesTotal) * 100 : 0;
  const monitorCoverage = activeTotal > 0 && stats ? (stats.sites_with_monitor / activeTotal) * 100 : 0;

  return (
    <section className="page-panel">
      {/* Global Health Stats */}
      <article className="panel subpanel" style={{ marginBottom: "0.7rem" }} data-incident={hasIncidents ? "true" : undefined}>
        <div className="panel-header">
          <div>
            <div className="panel-kicker">Global Health</div>
            <h2>{hasIncidents ? "Incident Active" : "All Systems Healthy"}</h2>
          </div>
          <span className={`tag ${hasIncidents ? "tag-failing" : "tag-active"}`}>
            {isRefreshing ? "Syncing..." : "Live"}
          </span>
        </div>

        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: "0.55rem" }}>
          <HeroStatTile
            label="Sites Up"
            value={stats?.sites_up ?? null}
            tone={stats && stats.sites_up > 0 ? "success" : "neutral"}
            sub={`of ${stats?.sites_total ?? "-"} total`}
            movement={`${upRatio.toFixed(0)}%`}
            trace={[62, 70, 76, 82, 88, 92, upRatio]}
          />
          <HeroStatTile
            label="Sites Down"
            value={stats?.sites_down ?? null}
            tone={stats && stats.sites_down > 0 ? "danger" : "neutral"}
            sub={stats && stats.sites_paused > 0 ? `${stats.sites_paused} paused` : undefined}
            movement={stats && stats.sites_down > 0 ? "Live" : "Clear"}
            trace={[18, 22, 20, 24, 28, 26, downRatio]}
          />
          <HeroStatTile
            label="Open Incidents"
            value={openIncidentCount}
            tone={hasIncidents ? "danger" : "neutral"}
            sub={hasIncidents ? "Requires attention" : "All clear"}
            movement={hasIncidents ? "Active" : "Stable"}
            trace={[12, 16, 18, 22, 30, 36, incidentRatio]}
          />
          <HeroStatTile
            label="Active Monitors"
            value={stats?.monitors_active ?? null}
            tone="neutral"
            sub={stats && stats.sites_without_monitor > 0 ? `${stats.sites_without_monitor} unmonitored` : "Full coverage"}
            movement={`${monitorCoverage.toFixed(0)}%`}
            trace={[48, 58, 64, 72, 78, 86, monitorCoverage]}
          />
        </div>
      </article>

      {/* Site Table + Incidents */}
      <div className="global-dashboard-grid">
        {/* Site Status Table */}
        <article className="panel subpanel">
          <div className="panel-header">
            <div>
              <div className="panel-kicker">Fleet</div>
              <h2>Site Status</h2>
            </div>
          </div>

          {sites.length === 0 ? (
            <div className="empty-state">
              {stats === null ? "Loading site data..." : "No sites configured yet."}
            </div>
          ) : (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Site</th>
                    <th>Status</th>
                    <th>Monitors</th>
                    <th>Response</th>
                    <th>Last Checked</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {sites.map((site) => (
                    <tr key={site.id}>
                      <td>
                        <div className="table-primary">{site.name}</div>
                        <div className="table-url">{site.base_url}</div>
                      </td>
                      <td>
                        <SiteStatusBadge status={site.status} hasOpenIncident={site.has_open_incident} />
                      </td>
                      <td>
                        <MonitorsCell active={site.monitors_active} total={site.monitors_total} />
                      </td>
                      <td>
                        {site.last_response_time_ms !== null
                          ? `${site.last_response_time_ms} ms`
                          : <span style={{ color: "var(--muted)" }}>-</span>
                        }
                      </td>
                      <td className="check-col-time">
                        {site.last_checked_at
                          ? formatTimestamp(site.last_checked_at)
                          : <span style={{ color: "var(--muted)" }}>Never</span>
                        }
                      </td>
                      <td>
                        <button
                          className="ghost-button"
                          style={{ padding: "0.26rem 0.62rem", fontSize: "0.78rem" }}
                          type="button"
                          onClick={() => onOpenSite(site.id)}
                        >
                          Open
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </article>

        {/* Active Incidents */}
        <article className="panel subpanel">
          <div className="panel-header">
            <div>
              <div className="panel-kicker">Incidents</div>
              <h2>Active Incidents</h2>
            </div>
            {openIncidentCount > 0 && (
              <span className="tag tag-failing">{openIncidentCount}</span>
            )}
          </div>

          {openIncidents.length === 0 ? (
            <div className="empty-state">No open incidents right now.</div>
          ) : (
            <div className="incident-list">
              {openIncidents.slice(0, 10).map(({ incident }) => (
                <div key={incident.id} className="incident-card incident-card-open">
                  <div className="incident-card-header">
                    <div>
                      <span className="monitor-type-pill" data-type={incident.monitor_type}>{incident.monitor_type.toUpperCase()}</span>
                      <strong className="incident-card-target">{incident.site_name}</strong>
                    </div>
                  </div>
                  <div className="incident-card-meta">
                    <div>
                      <span>Started</span>
                      <strong>{formatTimestamp(incident.opened_at)}</strong>
                    </div>
                    <div>
                      <span>Down for</span>
                      <LiveDuration openedAt={incident.opened_at} />
                    </div>
                    <div>
                      <span>Failures</span>
                      <strong>{incident.failure_count}</strong>
                    </div>
                    <div>
                      <span>Acknowledged</span>
                      <strong>{incident.acknowledged_at ? "Yes" : "No"}</strong>
                    </div>
                  </div>
                  <div className="incident-card-actions">
                    <button
                      className="ghost-button"
                      style={{ padding: "0.26rem 0.62rem", fontSize: "0.78rem" }}
                      type="button"
                      onClick={() => onOpenSite(incident.site_id)}
                    >
                      Open Site
                    </button>
                  </div>
                </div>
              ))}
              {openIncidents.length > 10 && (
                <div className="empty-state" style={{ marginTop: "0.35rem" }}>
                  +{openIncidents.length - 10} more - open each site for details.
                </div>
              )}
            </div>
          )}
        </article>
      </div>

      {/* Coverage Gap notice */}
      {stats && stats.sites_without_monitor > 0 && (
        <article className="panel subpanel" style={{ marginTop: "0.7rem" }}>
          <div className="panel-header">
            <div>
              <div className="panel-kicker">Coverage</div>
              <h2>Missing Monitoring</h2>
            </div>
            <span className="tag tag-disabled">
              {stats.sites_without_monitor} site{stats.sites_without_monitor !== 1 ? "s" : ""}
            </span>
          </div>
          <div className="incident-list">
            {sites
              .filter((s) => s.monitors_total === 0)
              .map((s) => (
                <div key={s.id} className="incident-card">
                  <div className="incident-card-header">
                    <div>
                      <strong className="incident-card-target">{s.name}</strong>
                      <span style={{ marginLeft: "0.5rem", color: "var(--muted)", fontSize: "0.82rem" }}>
                        {s.base_url}
                      </span>
                    </div>
                  </div>
                  <div className="incident-card-actions">
                    <button
                      className="ghost-button"
                      style={{ padding: "0.26rem 0.62rem", fontSize: "0.78rem" }}
                      type="button"
                      onClick={() => onOpenSite(s.id)}
                    >
                      Add Monitor
                    </button>
                  </div>
                </div>
              ))}
          </div>
        </article>
      )}
    </section>
  );
}
