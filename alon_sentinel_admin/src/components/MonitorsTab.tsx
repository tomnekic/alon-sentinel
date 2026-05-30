import { useEffect, useState } from "react";
import { getSiteChecks, type AnySiteMonitor, type AuthSession, type SiteMonitorCheck, type SiteMonitorInventory, type SiteMonitorType } from "../api";
import {
  formatMonitorTypeLabel,
  formatTimestamp,
  getMonitorConfigSummary,
  getMonitorLatestStatusLabel,
  getMonitorLatestStatusTone,
  getMonitorPrimaryLabel,
  MONITOR_ORDER,
  StatusBadge,
} from "./SiteDashboardShared";

type MonitorsTabProps = {
  inventory: SiteMonitorInventory | null;
  isMutating: boolean;
  monitorActionError: string | null;
  session: AuthSession | null;
  siteId: number;
  canWrite: boolean;
  onConfigureMonitor: (type: SiteMonitorType) => void;
  onPauseMonitor: (type: SiteMonitorType, monitorId: number) => void;
  onResumeMonitor: (type: SiteMonitorType, monitorId: number) => void;
  onDeleteMonitor: (type: SiteMonitorType, monitorId: number) => void;
};

function ResponseSparkline({ checks }: { checks: SiteMonitorCheck[] }) {
  const [hovered, setHovered] = useState<number | null>(null);
  const points = checks.filter((c) => c.response_time_ms !== null);
  if (points.length < 2) return null;

  const W = 240;
  const H = 42;
  // Keep the threshold bands visible even when the service is healthy.
  const max = Math.max(...points.map((c) => c.response_time_ms!), 1000);
  const stepX = W / (points.length - 1);

  const toY = (ms: number) => H - (ms / max) * (H - 4) - 2;

  const coords = points.map((c, i) => ({
    x: i * stepX,
    y: toY(c.response_time_ms!),
    ok: c.is_success,
  }));

  const polyline = coords.map((p) => `${p.x},${p.y}`).join(" ");

  const topY = 2;
  const botY = H - 2;
  const y800 = toY(800);
  const y300 = toY(300);

  const hp = hovered !== null ? coords[hovered] : null;
  const hpData = hovered !== null ? points[hovered] : null;
  const hoveredTime = hpData ? formatTimestamp(hpData.checked_at) : null;

  function handleMouseMove(e: React.MouseEvent<SVGSVGElement>) {
    const rect = e.currentTarget.getBoundingClientRect();
    const ix = Math.round(((e.clientX - rect.left) / rect.width) * (points.length - 1));
    setHovered(Math.max(0, Math.min(points.length - 1, ix)));
  }

  return (
    <div className="monitor-chart">
      <div className="monitor-chart-head">
        <span>Latency</span>
        {hpData ? (
          <span className="monitor-chart-hover-label">
            <strong>{hpData.response_time_ms} ms</strong>
            <span>{hoveredTime}</span>
            {!hpData.is_success && <span className="monitor-chart-hover-fail">failed</span>}
          </span>
        ) : (
          <strong>{points[points.length - 1]?.response_time_ms} ms</strong>
        )}
      </div>
      <svg
        width="100%"
        height={H}
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        aria-hidden="true"
        onMouseMove={handleMouseMove}
        onMouseLeave={() => setHovered(null)}
        style={{ cursor: "crosshair", display: "block" }}
      >
        <rect x={0} y={topY} width={W} height={y800 - topY} fill="rgba(239,68,68,0.07)" />
        <rect x={0} y={y800} width={W} height={y300 - y800} fill="rgba(245,158,11,0.06)" />
        <rect x={0} y={y300} width={W} height={botY - y300} fill="rgba(34,197,94,0.06)" />

        <line x1={0} y1={y800} x2={W} y2={y800} stroke="rgba(239,68,68,0.3)" strokeWidth="0.5" strokeDasharray="3,4" vectorEffect="non-scaling-stroke" />
        <line x1={0} y1={y300} x2={W} y2={y300} stroke="rgba(245,158,11,0.3)" strokeWidth="0.5" strokeDasharray="3,4" vectorEffect="non-scaling-stroke" />

        {hp && <line x1={hp.x} y1={topY} x2={hp.x} y2={botY} stroke="rgba(148,163,184,0.24)" strokeWidth="1" vectorEffect="non-scaling-stroke" />}
        {hp && <line x1={0} y1={hp.y} x2={W} y2={hp.y} stroke="rgba(148,163,184,0.16)" strokeWidth="1" strokeDasharray="2,4" vectorEffect="non-scaling-stroke" />}

        <polyline
          points={polyline}
          fill="none"
          stroke="rgba(56,189,248,0.9)"
          strokeWidth="1.25"
          strokeLinejoin="round"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
          className="monitor-chart-line"
        />

        {coords.filter((p) => !p.ok).map((p, i) => (
          <line key={i} x1={p.x} y1={p.y - 5} x2={p.x} y2={p.y + 5} stroke="var(--danger)" strokeWidth="1.25" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
        ))}
      </svg>
    </div>
  );
}

function averageLatency(checks: SiteMonitorCheck[]) {
  const latencies = checks
    .map((check) => check.response_time_ms)
    .filter((value): value is number => value !== null);
  if (latencies.length === 0) return null;
  return latencies.reduce((sum, value) => sum + value, 0) / latencies.length;
}

function getLatencyTrend(checks: SiteMonitorCheck[]) {
  const latencyChecks = checks.filter((check) => check.response_time_ms !== null);
  if (latencyChecks.length < 4) return { label: "trend pending", tone: "neutral" as const };
  const midpoint = Math.floor(latencyChecks.length / 2);
  const previous = averageLatency(latencyChecks.slice(0, midpoint));
  const recent = averageLatency(latencyChecks.slice(midpoint));
  if (previous === null || recent === null) return { label: "trend pending", tone: "neutral" as const };
  const delta = recent - previous;
  if (Math.abs(delta) < 5) return { label: "→ stable", tone: "neutral" as const };
  return delta > 0
    ? { label: `↗ +${Math.round(delta)} ms`, tone: "bad" as const }
    : { label: `↘ ${Math.round(delta)} ms`, tone: "good" as const };
}

function MonitorTags({ monitor, checks }: { monitor: AnySiteMonitor; checks: SiteMonitorCheck[] }) {
  const trend = getLatencyTrend(checks);
  return (
    <div className="monitor-tags">
      <span data-monitor-type={monitor.monitor_type}>{formatMonitorTypeLabel(monitor.monitor_type)}</span>
      <span>Probe local</span>
      <span>Every {monitor.check_interval_seconds}s</span>
      <span className={`monitor-tag-trend monitor-tag-trend-${trend.tone}`}>{trend.label}</span>
    </div>
  );
}

type TimelineTone = "up" | "degraded" | "down" | "empty";

function getTimelineTone(checks: SiteMonitorCheck[]): TimelineTone {
  if (checks.length === 0) return "empty";
  const passed = checks.filter((check) => check.is_success).length;
  if (passed === checks.length) return "up";
  if (passed === 0) return "down";
  return "degraded";
}

function buildTimelineSegments(checks: SiteMonitorCheck[], windowHours: number, segmentCount: number) {
  const now = Date.now();
  const start = now - windowHours * 60 * 60 * 1000;
  const segmentMs = (now - start) / segmentCount;

  return Array.from({ length: segmentCount }, (_, index) => {
    const segmentStart = start + index * segmentMs;
    const segmentEnd = segmentStart + segmentMs;
    const segmentChecks = checks.filter((check) => {
      const checkedAt = new Date(check.checked_at).getTime();
      return checkedAt >= segmentStart && checkedAt < segmentEnd;
    });
    const tone = getTimelineTone(segmentChecks);
    const failed = segmentChecks.filter((check) => !check.is_success).length;
    const title = segmentChecks.length === 0
      ? "No checks"
      : `${segmentChecks.length - failed}/${segmentChecks.length} checks passed`;

    return { key: `${windowHours}-${index}`, tone, title };
  });
}

function OperationalTimeline({ checks }: { checks: SiteMonitorCheck[] }) {
  const windows = [
    { label: "24h", hours: 24, segments: 24 },
    { label: "7d", hours: 24 * 7, segments: 28 },
    { label: "30d", hours: 24 * 30, segments: 30 },
    { label: "90d", hours: 24 * 90, segments: 45 },
  ];

  return (
    <div className="monitor-timeline">
      <div className="monitor-timeline-head">
        <span>Operational timeline</span>
        <span>green healthy · yellow partial · red incident</span>
      </div>
      <div className="monitor-timeline-grid">
        {windows.map((window) => (
          <div key={window.label} className="monitor-timeline-row">
            <span className="monitor-timeline-label">{window.label}</span>
            <div className="monitor-timeline-track" aria-label={`${window.label} monitor timeline`}>
              {buildTimelineSegments(checks, window.hours, window.segments).map((segment) => (
                <span
                  key={segment.key}
                  className={`monitor-timeline-segment monitor-timeline-segment-${segment.tone}`}
                  title={`${window.label}: ${segment.title}`}
                />
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function MonitorsTab({
  inventory,
  isMutating,
  monitorActionError,
  session,
  siteId,
  canWrite,
  onConfigureMonitor,
  onPauseMonitor,
  onResumeMonitor,
  onDeleteMonitor,
}: MonitorsTabProps) {
  const [checksByMonitorId, setChecksByMonitorId] = useState<Map<number, SiteMonitorCheck[]>>(new Map());

  useEffect(() => {
    if (!session) return;
    let cancelled = false;
    getSiteChecks(session, siteId, { limit: 2000 })
      .then(({ checks }) => {
        if (cancelled) return;
        const map = new Map<number, SiteMonitorCheck[]>();
        for (const c of checks) {
          const list = map.get(c.site_monitor_id) ?? [];
          list.push(c);
          map.set(c.site_monitor_id, list);
        }
        // keep chronological order (oldest first) for sparkline and timeline left-to-right
        map.forEach((v, k) => map.set(k, v.slice().reverse()));
        setChecksByMonitorId(map);
      })
      .catch(() => { /* silently ignore — sparklines are decorative */ });
    return () => { cancelled = true; };
  }, [siteId, session]);

  function renderMonitorCard(monitor: AnySiteMonitor, type: SiteMonitorType) {
    const monitorChecks = checksByMonitorId.get(monitor.id) ?? [];
    const details: { label: string; value: string }[] = [
      { label: "Last checked", value: formatTimestamp(monitor.last_checked_at) },
      { label: "Last success", value: formatTimestamp(monitor.last_successful_check_at) },
    ];

    switch (monitor.monitor_type) {
      case "http":
        if (monitor.last_status_code !== null) details.push({ label: "Status code", value: String(monitor.last_status_code) });
        if (monitor.last_response_time_ms !== null) details.push({ label: "Response time", value: `${monitor.last_response_time_ms} ms` });
        if (monitor.ssl_certificate_checks_enabled) {
          if (monitor.last_certificate_days_remaining !== null) details.push({ label: "SSL cert", value: `${monitor.last_certificate_days_remaining} days` });
          if (monitor.last_certificate_issuer) details.push({ label: "Issuer", value: monitor.last_certificate_issuer });
        }
        break;
      case "ssl":
        if (monitor.last_certificate_days_remaining !== null) details.push({ label: "Cert expires", value: `${monitor.last_certificate_days_remaining} days` });
        if (monitor.last_certificate_issuer) details.push({ label: "Issuer", value: monitor.last_certificate_issuer });
        break;
      case "heartbeat":
        if (monitor.last_heartbeat_received_at) details.push({ label: "Last heartbeat", value: formatTimestamp(monitor.last_heartbeat_received_at) });
        break;
      case "tcp":
      case "dns":
        if (monitor.last_response_time_ms !== null) details.push({ label: "Response time", value: `${monitor.last_response_time_ms} ms` });
        break;
    }

    if (monitor.last_error_message) details.push({ label: "Last error", value: monitor.last_error_message });

    const assertionTags: string[] = [];
    if (monitor.monitor_type === "http") {
      if (monitor.max_response_time_ms) assertionTags.push(`Max ${monitor.max_response_time_ms}ms`);
      if (monitor.body_must_contain) assertionTags.push(`Contains "${monitor.body_must_contain}"`);
      if (monitor.body_must_not_contain) assertionTags.push(`Not "${monitor.body_must_not_contain}"`);
      const extraBodyCount = (monitor.body_must_contain_texts?.length ?? 0) + (monitor.body_must_not_contain_texts?.length ?? 0);
      if (extraBodyCount > 0) assertionTags.push(`${extraBodyCount} body rule${extraBodyCount !== 1 ? "s" : ""}`);
      if (monitor.required_header_name) assertionTags.push(`Header: ${monitor.required_header_name}`);
      const extraHeaderCount = monitor.header_assertions?.length ?? 0;
      if (extraHeaderCount > 0) assertionTags.push(`${extraHeaderCount} header rule${extraHeaderCount !== 1 ? "s" : ""}`);
      const jsonCount = (monitor.json_path_exists?.length ?? 0) + (monitor.json_path_equals?.length ?? 0) + (monitor.json_path_not_equals?.length ?? 0);
      if (jsonCount > 0) assertionTags.push(`${jsonCount} JSON rule${jsonCount !== 1 ? "s" : ""}`);
      if (monitor.http_check_timeout_seconds_override) assertionTags.push(`Timeout ${monitor.http_check_timeout_seconds_override}s`);
      if (monitor.http_check_max_attempts_override) assertionTags.push(`${monitor.http_check_max_attempts_override} attempts`);
    }

    return (
      <div key={monitor.id} className={`monitor-card${monitor.is_active ? "" : " monitor-card-paused"}`}>
        <div className="monitor-card-header">
          <div>
            <strong className="monitor-card-target">{getMonitorPrimaryLabel(monitor)}</strong>
            <p className="monitor-card-meta">{getMonitorConfigSummary(monitor)}</p>
            <MonitorTags monitor={monitor} checks={monitorChecks} />
          </div>
          <StatusBadge label={getMonitorLatestStatusLabel(monitor)} tone={getMonitorLatestStatusTone(monitor)} />
        </div>
        <div className="monitor-card-body">
          <div>
            <div className="monitor-detail-grid">
              {details.map((d) => (
                <div key={d.label}>
                  <span>{d.label}</span>
                  <strong>{d.value}</strong>
                </div>
              ))}
            </div>
            {assertionTags.length > 0 && (
              <div className="monitor-assertion-tags">
                {assertionTags.map((tag) => <span key={tag} className="monitor-assertion-tag">{tag}</span>)}
              </div>
            )}
          </div>
          <ResponseSparkline checks={monitorChecks} />
        </div>
        <OperationalTimeline checks={monitorChecks} />
        {canWrite && (
          <div className="monitor-card-actions">
            {(inventory?.[type] ?? []).length <= 1 && (
              <button className="text-button" disabled={isMutating} onClick={() => onConfigureMonitor(type)} type="button">
                Edit
              </button>
            )}
            {monitor.is_active ? (
              <button className="ghost-button" disabled={isMutating} onClick={() => onPauseMonitor(type, monitor.id)} type="button">
                Pause
              </button>
            ) : (
              <button className="ghost-button" disabled={isMutating} onClick={() => onResumeMonitor(type, monitor.id)} type="button">
                Resume
              </button>
            )}
            <button className="ghost-button ghost-button-danger" disabled={isMutating} onClick={() => onDeleteMonitor(type, monitor.id)} type="button">
              Delete
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="site-dashboard-tab-content">
      {monitorActionError && <div className="inline-alert inline-alert-danger">{monitorActionError}</div>}
      {MONITOR_ORDER.map((type) => {
        const monitors = inventory?.[type] ?? [];
        return (
          <article key={type} className="panel subpanel" data-monitor-type={type}>
            <div className="panel-header">
              <div>
                <div className="panel-kicker" data-monitor-type={type}>{formatMonitorTypeLabel(type)}</div>
                <h2>{monitors.length === 0 ? "Not configured" : `${monitors.length} monitor${monitors.length > 1 ? "s" : ""}`}</h2>
              </div>
              {canWrite && (
                <button className="ghost-button" disabled={isMutating} onClick={() => onConfigureMonitor(type)} type="button">
                  {monitors.length === 0 ? "Configure" : "+ Add"}
                </button>
              )}
            </div>
            {monitors.length === 0 ? (
              <div className="empty-state">No {formatMonitorTypeLabel(type)} monitor configured for this site.</div>
            ) : (
              <div className="monitor-card-list">{monitors.map((monitor) => renderMonitorCard(monitor, type))}</div>
            )}
          </article>
        );
      })}
    </div>
  );
}
