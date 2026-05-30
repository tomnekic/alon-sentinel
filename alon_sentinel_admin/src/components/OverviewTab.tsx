import { useEffect, useState } from "react";
import { getSiteUptimeDaily, type AuthSession, type DailyUptimeBucket, type SiteMonitorInventory, type SiteMonitorType, type SiteSummary, type SiteUptime } from "../api";

function ArcGauge({ rate }: { rate: number | null }) {
  const r = 38;
  const cx = 48;
  const cy = 48;
  const circumference = Math.PI * r; // half-circle arc length
  const filled = rate !== null ? Math.max(0, Math.min(1, rate / 100)) * circumference : 0;

  const color =
    rate === null ? "var(--muted)"
    : rate >= 99 ? "#22c55e"
    : rate >= 95 ? "#38bdf8"
    : rate >= 80 ? "#f59e0b"
    : "var(--danger)";

  return (
    <svg width="96" height="56" viewBox="0 0 96 56" style={{ overflow: "visible" }}>
      {/* Track */}
      <path
        d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
        fill="none"
        stroke="rgba(255,255,255,0.08)"
        strokeWidth="6"
        strokeLinecap="round"
      />
      {/* Fill */}
      <path
        d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
        fill="none"
        stroke={color}
        strokeWidth="6"
        strokeLinecap="round"
        strokeDasharray={`${filled} ${circumference}`}
        style={{ transition: "stroke-dasharray 600ms ease, stroke 400ms ease" }}
      />
    </svg>
  );
}
import {
  formatMonitorTypeLabel,
  formatStateLabel,
  formatTimestamp,
  getCurrentStateTone,
  getMonitorPrimaryLabel,
  MONITOR_ORDER,
  StatusBadge,
  type DashboardTab,
  type StatusTone,
} from "./SiteDashboardShared";

function UptimeBars({ buckets }: { buckets: DailyUptimeBucket[] }) {
  if (buckets.length === 0) return null;
  return (
    <div style={{ display: "flex", gap: "2px", height: "28px", alignItems: "flex-end", marginTop: "0.5rem" }}>
      {buckets.map((b, i) => {
        const rate = b.uptime_percent;
        const bg =
          b.total_checks === 0 ? "rgba(255,255,255,0.08)"
          : rate !== null && rate >= 99 ? "rgba(34,197,94,0.65)"
          : rate !== null && rate >= 95 ? "rgba(251,191,36,0.65)"
          : "rgba(255,137,109,0.75)";
        const h = b.total_checks === 0 ? 6 : Math.max(8, (rate ?? 0) / 100 * 28);
        const label = b.total_checks === 0
          ? `${b.date}: no data`
          : `${b.date}: ${rate !== null ? rate.toFixed(1) : "–"}% (${b.successful_checks}/${b.total_checks})`;
        return (
          <div
            key={i}
            title={label}
            style={{
              flex: 1,
              height: `${h}px`,
              borderRadius: "2px",
              background: bg,
              transition: "height 300ms ease",
              cursor: "default",
            }}
          />
        );
      })}
    </div>
  );
}

function uptimeTone(rate: number | null): "success" | "warning" | "danger" | "muted" {
  if (rate === null) return "muted";
  if (rate >= 99) return "success";
  if (rate >= 95) return "warning";
  return "danger";
}

function averageUptime(buckets: DailyUptimeBucket[]) {
  const withData = buckets.filter((bucket) => bucket.total_checks > 0 && bucket.uptime_percent !== null);
  if (withData.length === 0) return null;
  return withData.reduce((sum, bucket) => sum + (bucket.uptime_percent ?? 0), 0) / withData.length;
}

function formatTrend(current: number | null, previous: number | null) {
  if (current === null || previous === null) return "No prior window";
  const delta = current - previous;
  const sign = delta > 0 ? "+" : "";
  return `${delta >= 0 ? "▲" : "▼"} ${sign}${delta.toFixed(2)}%`;
}

function UptimeSparkline({ buckets }: { buckets: DailyUptimeBucket[] }) {
  if (buckets.length === 0) return <div className="metric-sparkline metric-sparkline-empty" />;

  return (
    <div className="metric-sparkline" aria-hidden="true">
      {buckets.map((bucket) => {
        const rate = bucket.uptime_percent;
        const tone = uptimeTone(rate);
        const height = bucket.total_checks === 0 ? 18 : Math.max(24, Math.min(100, rate ?? 0));
        return (
          <span
            key={bucket.date}
            className={`metric-sparkline-bar metric-sparkline-bar-${tone}`}
            style={{ height: `${height}%` }}
            title={bucket.total_checks === 0 ? `${bucket.date}: no data` : `${bucket.date}: ${rate?.toFixed(1)}%`}
          />
        );
      })}
    </div>
  );
}

function UptimeMetricCard({
  label,
  uptime,
  buckets,
  previousBuckets,
}: {
  label: string;
  uptime: SiteUptime | null;
  buckets: DailyUptimeBucket[];
  previousBuckets: DailyUptimeBucket[];
}) {
  const rate = uptime?.uptime_percent ?? null;
  const previousRate = averageUptime(previousBuckets);
  const trend = formatTrend(rate, previousRate);
  const tone = uptimeTone(rate);
  const trendTone = rate !== null && previousRate !== null
    ? rate >= previousRate ? "positive" : "negative"
    : "neutral";

  return (
    <article className={`panel subpanel metric-card metric-card-${tone}`}>
      <div className="metric-card-head">
        <span className="panel-kicker">{label}</span>
        <span className={`metric-trend metric-trend-${trendTone}`}>{trend}</span>
      </div>
      <div className="metric-card-main">
        <span className="metric-card-value">
          {uptime ? (rate !== null ? `${rate.toFixed(2)}%` : "-") : "..."}
        </span>
        <span className="metric-card-label">Check-based uptime</span>
      </div>
      <UptimeSparkline buckets={buckets} />
      {uptime && (
        <div className="metric-card-footer">
          <span><strong>{uptime.successful_checks}</strong> passed</span>
          <span><strong>{uptime.failed_checks}</strong> failed</span>
          <span><strong>{uptime.total_checks}</strong> total</span>
        </div>
      )}
    </article>
  );
}

type OverviewTabProps = {
  summary: SiteSummary;
  inventory: SiteMonitorInventory | null;
  uptime7d: SiteUptime | null;
  uptime30d: SiteUptime | null;
  isMutating: boolean;
  session: AuthSession | null;
  canWrite: boolean;
  onConfigureMonitor: (type: SiteMonitorType) => void;
  onTabChange: (tab: DashboardTab) => void;
};

export function OverviewTab({
  summary,
  inventory,
  uptime7d,
  uptime30d,
  isMutating,
  session,
  canWrite,
  onConfigureMonitor,
  onTabChange,
}: OverviewTabProps) {
  const [dailyBuckets, setDailyBuckets] = useState<DailyUptimeBucket[]>([]);

  useEffect(() => {
    if (!session) return;
    let cancelled = false;
    getSiteUptimeDaily(session, summary.site.id, { days: 90 })
      .then(({ data }) => { if (!cancelled) setDailyBuckets(data.buckets); })
      .catch(() => { /* decorative — ignore silently */ });
    return () => { cancelled = true; };
  }, [summary.site.id, session]);
  const { current_state, incident_open, latest_check, recent_checks, site } = summary;
  const allMonitors = [
    ...(inventory?.http ?? []),
    ...(inventory?.ssl ?? []),
    ...(inventory?.heartbeat ?? []),
    ...(inventory?.tcp ?? []),
    ...(inventory?.dns ?? []),
  ];
  const activeCount = allMonitors.filter((m) => m.is_active).length;
  const totalCount = allMonitors.length;
  const last7Buckets = dailyBuckets.slice(-7);
  const previous7Buckets = dailyBuckets.slice(-14, -7);
  const last30Buckets = dailyBuckets.slice(-30);
  const previous30Buckets = dailyBuckets.slice(-60, -30);

  function renderMonitorCoverageTile(type: SiteMonitorType) {
    const monitors = inventory?.[type] ?? [];
    const primary = monitors[0];
    const isConfigured = monitors.length > 0;
    const isActive = monitors.some((m) => m.is_active);

    let tone: StatusTone = "muted";
    let statusLabel = "Not configured";
    if (isConfigured) {
      if (!isActive) {
        tone = "muted";
        statusLabel = "Paused";
      } else if (primary?.last_is_success === true) {
        tone = "success";
        statusLabel = "Healthy";
      } else if (primary?.last_is_success === false) {
        tone = "danger";
        statusLabel = "Failing";
      } else {
        tone = "warning";
        statusLabel = "Pending";
      }
    }

    let keyMetric: string | null = null;
    if (primary?.last_checked_at) {
      switch (primary.monitor_type) {
        case "http":
          if (primary.last_response_time_ms !== null) keyMetric = `${primary.last_response_time_ms} ms`;
          else if (primary.last_status_code !== null) keyMetric = `HTTP ${primary.last_status_code}`;
          break;
        case "ssl":
          if (primary.last_certificate_days_remaining !== null) keyMetric = `${primary.last_certificate_days_remaining}d cert`;
          break;
        case "heartbeat":
          if (primary.last_heartbeat_received_at) keyMetric = `beat ${formatTimestamp(primary.last_heartbeat_received_at)}`;
          break;
        case "tcp":
        case "dns":
          if (primary.last_response_time_ms !== null) keyMetric = `${primary.last_response_time_ms} ms`;
          break;
      }
    }

    return (
      <div key={type} className={`monitor-coverage-tile monitor-coverage-tile-${tone}`} data-monitor-type={type}>
        <div className="monitor-coverage-tile-header">
          <strong className="monitor-type-label">{formatMonitorTypeLabel(type)}</strong>
          <StatusBadge label={statusLabel} tone={tone} />
        </div>
        {isConfigured ? (
          <div className="monitor-coverage-tile-meta">
            <span className="monitor-coverage-tile-target">{getMonitorPrimaryLabel(primary!)}</span>
            <span>{formatTimestamp(primary?.last_checked_at)}</span>
            {keyMetric && <span className="monitor-coverage-metric">{keyMetric}</span>}
            {monitors.length > 1 && <span>{monitors.length} monitors</span>}
          </div>
        ) : (
          <div className="monitor-coverage-tile-meta">
            <span>No monitor configured</span>
          </div>
        )}
        {!isConfigured && canWrite ? (
          <button
            className="ghost-button monitor-coverage-tile-action"
            onClick={() => onConfigureMonitor(type)}
            disabled={isMutating}
            type="button"
          >
            Configure
          </button>
        ) : (
          <button className="text-button monitor-coverage-tile-action" onClick={() => onTabChange("monitors")} type="button">
            Manage →
          </button>
        )}
      </div>
    );
  }

  return (
    <div className="site-dashboard-tab-content">
      <div className="site-dashboard-overview-row">
        <article className="panel subpanel site-dashboard-state-panel">
          <span className="panel-kicker">Site Health</span>
          <div className="site-dashboard-state-hero">
            <StatusBadge label={formatStateLabel(current_state)} tone={getCurrentStateTone(current_state)} />
          </div>
          <div className="detail-list">
            <div>
              <span>Site</span>
              <strong>{site.name}</strong>
            </div>
            <div>
              <span>Base URL</span>
              <strong className="url-truncated">{site.base_url}</strong>
            </div>
            <div>
              <span>Status</span>
              <strong>{site.is_active ? "Active" : "Disabled"}</strong>
            </div>
          </div>
          {incident_open && (
            <button className="incident-open-banner" onClick={() => onTabChange("incidents")} type="button">
              ● Active incident — view details
            </button>
          )}
        </article>

        <article className="panel subpanel">
          <span className="panel-kicker">Recent Checks</span>
          <div style={{ position: "relative", display: "flex", flexDirection: "column", alignItems: "center", marginBottom: "0.25rem" }}>
            <ArcGauge rate={recent_checks.success_rate} />
            <div style={{ position: "absolute", bottom: "0", textAlign: "center" }}>
              <span className="site-dashboard-stat-big" style={{ fontSize: "1.5rem" }}>
                {recent_checks.success_rate !== null ? `${recent_checks.success_rate.toFixed(1)}%` : "–"}
              </span>
              <span className="site-dashboard-stat-label" style={{ display: "block" }}>Success Rate</span>
            </div>
          </div>
          <div className="site-dashboard-stats-row">
            <div>
              <span>{recent_checks.successful_checks}</span>
              <span className="stat-sub-label">Passed</span>
            </div>
            <div>
              <span>{recent_checks.failed_checks}</span>
              <span className="stat-sub-label">Failed</span>
            </div>
            <div>
              <span>{recent_checks.total_checks}</span>
              <span className="stat-sub-label">Total</span>
            </div>
            <div>
              <span>{activeCount}/{totalCount}</span>
              <span className="stat-sub-label">Monitors</span>
            </div>
          </div>
        </article>

        <article className="panel subpanel">
          <span className="panel-kicker">Latest Check</span>
          {latest_check ? (
            <div>
              <div className="site-dashboard-latest-check-head">
                <StatusBadge label={latest_check.is_success ? "Healthy" : "Failed"} tone={latest_check.is_success ? "success" : "danger"} />
                <span className="stat-sub-label">{formatTimestamp(latest_check.checked_at)}</span>
              </div>
              <div className="site-dashboard-detail-grid site-dashboard-detail-grid-three" style={{ marginTop: "0.75rem" }}>
                <div>
                  <span>Monitor</span>
                  <strong>{formatMonitorTypeLabel(latest_check.monitor_type)}</strong>
                </div>
                <div>
                  <span>Code</span>
                  <strong>{latest_check.status_code ?? "N/A"}</strong>
                </div>
                <div>
                  <span>Response</span>
                  <strong>{latest_check.response_time_ms !== null ? `${latest_check.response_time_ms} ms` : "N/A"}</strong>
                </div>
              </div>
              {!latest_check.is_success && (latest_check.failure_reason ?? latest_check.error_message) && (
                <p className="check-error-inline">{latest_check.failure_reason ?? latest_check.error_message}</p>
              )}
            </div>
          ) : (
            <div className="empty-state">No checks recorded yet.</div>
          )}
        </article>
      </div>

      <div className="site-dashboard-overview-row" style={{ marginTop: "0.7rem" }}>
        <UptimeMetricCard label="7d uptime" uptime={uptime7d} buckets={last7Buckets} previousBuckets={previous7Buckets} />
        <UptimeMetricCard label="30d uptime" uptime={uptime30d} buckets={last30Buckets} previousBuckets={previous30Buckets} />
      </div>
      {dailyBuckets.length > 0 && (
        <article className="panel subpanel">
          <div className="panel-header" style={{ marginBottom: "0.4rem" }}>
            <div>
              <div className="panel-kicker">90-Day Uptime</div>
            </div>
            <span style={{ fontSize: "0.75rem", color: "var(--muted)" }}>
              {dailyBuckets.length} days
            </span>
          </div>
          <UptimeBars buckets={dailyBuckets} />
          <div style={{ display: "flex", justifyContent: "space-between", marginTop: "0.3rem" }}>
            <span style={{ fontSize: "0.68rem", color: "var(--muted)" }}>{dailyBuckets[0]?.date}</span>
            <span style={{ fontSize: "0.68rem", color: "var(--muted)" }}>{dailyBuckets[dailyBuckets.length - 1]?.date}</span>
          </div>
        </article>
      )}

      <article className="panel">
        <div className="panel-header">
          <div>
            <div className="panel-kicker">Monitor Coverage</div>
            <h2>{totalCount === 0 ? "No monitors configured" : `${totalCount} configured · ${activeCount} active`}</h2>
          </div>
          <button className="ghost-button" onClick={() => onTabChange("monitors")} type="button">
            Manage Monitors
          </button>
        </div>
        {inventory ? (
          <div className="monitor-coverage-grid">{MONITOR_ORDER.map((type) => renderMonitorCoverageTile(type))}</div>
        ) : (
          <div className="empty-state">Loading monitor inventory...</div>
        )}
      </article>
    </div>
  );
}
