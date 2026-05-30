import { useEffect, useRef, useState } from "react";
import { acknowledgeIncident, getSiteIncidents, type AuthSession, type SiteIncident } from "../api";
import { formatDuration, formatMonitorTypeLabel, formatTimestamp, INCIDENTS_PAGE_SIZE, StatusBadge } from "./SiteDashboardShared";

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

  return <strong style={{ color: "var(--danger)", fontVariantNumeric: "tabular-nums" }}>{formatDuration(elapsed)}</strong>;
}

type IncidentsTabProps = {
  siteId: number;
  session: AuthSession | null;
};

export function IncidentsTab({ siteId, session }: IncidentsTabProps) {
  const [incidents, setIncidents] = useState<SiteIncident[]>([]);
  const [incidentsNextCursor, setIncidentsNextCursor] = useState<string | null>(null);
  const [isLoadingIncidents, setIsLoadingIncidents] = useState(false);
  const [incidentsError, setIncidentsError] = useState<string | null>(null);
  const [acknowledgingId, setAcknowledgingId] = useState<number | null>(null);

  const openIncidents = incidents.filter((i) => i.status === "open");
  const resolvedIncidents = incidents.filter((i) => i.status === "resolved");

  async function loadIncidents(reset: boolean) {
    if (isLoadingIncidents) return;
    if (!session) {
      setIncidentsError("Admin session has expired. Log in again.");
      return;
    }
    setIsLoadingIncidents(true);
    setIncidentsError(null);
    try {
      const result = await getSiteIncidents(session, siteId, {
        cursor: reset ? undefined : (incidentsNextCursor ?? undefined),
        limit: INCIDENTS_PAGE_SIZE,
      });
      setIncidents(reset ? result.incidents : (prev) => [...prev, ...result.incidents]);
      setIncidentsNextCursor(result.nextCursor);
    } catch (e) {
      setIncidentsError(e instanceof Error ? e.message : "Failed to load incidents.");
    } finally {
      setIsLoadingIncidents(false);
    }
  }

  async function handleAcknowledge(incidentId: number) {
    if (!session) {
      setIncidentsError("Admin session has expired. Log in again.");
      return;
    }
    setAcknowledgingId(incidentId);
    setIncidentsError(null);
    try {
      await acknowledgeIncident(session, siteId, incidentId);
      setIncidents((prev) =>
        prev.map((inc) => inc.id === incidentId ? { ...inc, acknowledged_at: new Date().toISOString() } : inc)
      );
    } catch (e) {
      setIncidentsError(e instanceof Error ? e.message : "Failed to acknowledge incident.");
    } finally {
      setAcknowledgingId(null);
    }
  }

  useEffect(() => {
    setIncidents([]);
    setIncidentsNextCursor(null);
    setIncidentsError(null);
    void loadIncidents(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteId, session]);

  function renderIncidentCard(incident: SiteIncident) {
    const isOpen = incident.status === "open";
    const duration = incident.resolved_at && incident.opened_at
      ? Math.round((new Date(incident.resolved_at).getTime() - new Date(incident.opened_at).getTime()) / 1000)
      : null;

    return (
      <div key={incident.id} className={`incident-card${isOpen ? " incident-card-open" : " incident-card-resolved"}`}>
        <div className="incident-card-header">
          <div>
            <span className="monitor-type-pill" data-type={incident.monitor_type}>{formatMonitorTypeLabel(incident.monitor_type)}</span>
            <strong className="incident-card-target">{incident.target_url}</strong>
          </div>
          <StatusBadge label={isOpen ? "Open" : "Resolved"} tone={isOpen ? "danger" : "success"} />
        </div>

        <div className="incident-card-meta">
          <div><span>Started</span><strong>{formatTimestamp(incident.opened_at)}</strong></div>
          {incident.resolved_at && <div><span>Resolved</span><strong>{formatTimestamp(incident.resolved_at)}</strong></div>}
          {isOpen
            ? <div><span>Duration</span><LiveDuration openedAt={incident.opened_at} /></div>
            : duration !== null && <div><span>Duration</span><strong>{formatDuration(duration)}</strong></div>
          }
          <div><span>Failures</span><strong>{incident.failure_count}</strong></div>
          {incident.last_status_code !== null && <div><span>Last code</span><strong>HTTP {incident.last_status_code}</strong></div>}
          {incident.resolved_reason && <div><span>Reason</span><strong>{incident.resolved_reason.replace(/_/g, " ")}</strong></div>}
          {incident.downtime_seconds !== null && <div><span>Downtime</span><strong>{formatDuration(incident.downtime_seconds)}</strong></div>}
          {incident.acknowledged_at && <div><span>Acknowledged</span><strong>{formatTimestamp(incident.acknowledged_at)}</strong></div>}
        </div>

        {incident.last_error_message && <p className="incident-card-error">{incident.last_error_message}</p>}

        {isOpen && !incident.acknowledged_at && (
          <div className="incident-card-actions">
            <button className="ghost-button" disabled={acknowledgingId === incident.id} onClick={() => void handleAcknowledge(incident.id)} type="button">
              {acknowledgingId === incident.id ? "Acknowledging..." : "Acknowledge"}
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="site-dashboard-tab-content">
      {incidentsError && <div className="inline-alert inline-alert-danger">{incidentsError}</div>}

      <article className="panel subpanel">
        <div className="panel-header">
          <div>
            <div className="panel-kicker">Active</div>
            <h2>
              Open Incidents
              {openIncidents.length > 0 && <span className="incident-count-badge">{openIncidents.length}</span>}
            </h2>
          </div>
          <button
            className="ghost-button"
            disabled={isLoadingIncidents}
            onClick={() => {
              setIncidents([]);
              setIncidentsNextCursor(null);
              void loadIncidents(true);
            }}
            type="button"
          >
            {isLoadingIncidents ? "Loading..." : "Refresh"}
          </button>
        </div>
        {openIncidents.length === 0 ? (
          <div className="empty-state">{isLoadingIncidents ? "Loading incidents..." : "No open incidents - all monitors healthy."}</div>
        ) : (
          <div className="incident-list">{openIncidents.map((inc) => renderIncidentCard(inc))}</div>
        )}
      </article>

      <article className="panel subpanel">
        <div className="panel-header">
          <div>
            <div className="panel-kicker">History</div>
            <h2>Resolved Incidents</h2>
          </div>
        </div>
        {resolvedIncidents.length === 0 ? (
          <div className="empty-state">{isLoadingIncidents ? "Loading..." : "No resolved incidents found."}</div>
        ) : (
          <div className="incident-list">{resolvedIncidents.map((inc) => renderIncidentCard(inc))}</div>
        )}
        {incidentsNextCursor && (
          <div className="load-more-row">
            <button className="ghost-button" disabled={isLoadingIncidents} onClick={() => void loadIncidents(false)} type="button">
              {isLoadingIncidents ? "Loading..." : "Load more"}
            </button>
          </div>
        )}
      </article>
    </div>
  );
}
