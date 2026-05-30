import { useEffect, useState } from "react";
import { getSiteChecks, type AuthSession, type SiteMonitorCheck } from "../api";
import { CHECKS_PAGE_SIZE, formatMonitorTypeLabel, formatTimestamp, StatusBadge } from "./SiteDashboardShared";

type ChecksFilter = "all" | "success" | "failure";

type HistoryTabProps = {
  siteId: number;
  session: AuthSession | null;
};

function getCheckGroupLabel(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const checkDay = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const dayDiff = Math.round((today - checkDay) / 86_400_000);
  if (dayDiff === 0) return "Today";
  if (dayDiff === 1) return "Yesterday";
  return date.toLocaleDateString();
}

function getResponseHeat(value: number | null) {
  if (value === null) return "empty";
  if (value < 150) return "cool";
  if (value < 400) return "warm";
  return "hot";
}

export function HistoryTab({ siteId, session }: HistoryTabProps) {
  const [checks, setChecks] = useState<SiteMonitorCheck[]>([]);
  const [checksNextCursor, setChecksNextCursor] = useState<string | null>(null);
  const [checksFilter, setChecksFilter] = useState<ChecksFilter>("all");
  const [isLoadingChecks, setIsLoadingChecks] = useState(false);
  const [checksError, setChecksError] = useState<string | null>(null);

  async function loadChecks(reset: boolean, filter: ChecksFilter = checksFilter) {
    if (isLoadingChecks) return;
    if (!session) {
      setChecksError("Admin session has expired. Log in again.");
      return;
    }
    setIsLoadingChecks(true);
    setChecksError(null);
    try {
      const result = await getSiteChecks(session, siteId, {
        filter: filter === "all" ? undefined : filter,
        cursor: reset ? undefined : (checksNextCursor ?? undefined),
        limit: CHECKS_PAGE_SIZE,
      });
      setChecks(reset ? result.checks : (prev) => [...prev, ...result.checks]);
      setChecksNextCursor(result.nextCursor);
    } catch (e) {
      setChecksError(e instanceof Error ? e.message : "Failed to load check history.");
    } finally {
      setIsLoadingChecks(false);
    }
  }

  function handleChecksFilterChange(filter: ChecksFilter) {
    setChecksFilter(filter);
    setChecks([]);
    setChecksNextCursor(null);
    void loadChecks(true, filter);
  }

  function renderCheckRows() {
    let previousGroup: string | null = null;

    return checks.flatMap((check) => {
      const group = getCheckGroupLabel(check.checked_at);
      const rows = [];
      if (group !== previousGroup) {
        rows.push(
          <tr key={`group-${group}-${check.id}`} className="check-group-row">
            <td colSpan={7}>{group}</td>
          </tr>
        );
        previousGroup = group;
      }
      rows.push(
        <tr key={check.id} className={check.is_success ? "" : "check-row-failed"}>
          <td className="check-col-time">{formatTimestamp(check.checked_at)}</td>
          <td><span className="monitor-type-pill" data-type={check.monitor_type}>{formatMonitorTypeLabel(check.monitor_type)}</span></td>
          <td className="check-col-target" title={check.url_checked}>{check.url_checked}</td>
          <td><StatusBadge label={check.is_success ? "OK" : "Fail"} tone={check.is_success ? "success" : "danger"} /></td>
          <td>{check.status_code ?? "-"}</td>
          <td>
            <span className={`response-heat response-heat-${getResponseHeat(check.response_time_ms)}`}>
              {check.response_time_ms !== null ? `${check.response_time_ms} ms` : "-"}
            </span>
          </td>
          <td className="check-col-error" title={check.error_message ?? check.failure_reason ?? undefined}>
            {check.error_message ?? check.failure_reason ?? "-"}
          </td>
        </tr>
      );
      return rows;
    });
  }

  useEffect(() => {
    setChecks([]);
    setChecksNextCursor(null);
    setChecksFilter("all");
    setChecksError(null);
    void loadChecks(true, "all");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [siteId, session]);

  return (
    <div className="site-dashboard-tab-content">
      <article className="panel subpanel">
        <div className="panel-header history-toolbar">
          <div>
            <div className="panel-kicker">Check History</div>
            <h2>Recent Checks</h2>
          </div>
          <div className="inline-actions">
            <select
              value={checksFilter}
              onChange={(e) => handleChecksFilterChange(e.target.value as ChecksFilter)}
              style={{ width: "auto", padding: "0.5rem 0.75rem" }}
            >
              <option value="all">All results</option>
              <option value="success">Successful only</option>
              <option value="failure">Failed only</option>
            </select>
            <button
              className="ghost-button"
              disabled={isLoadingChecks}
              onClick={() => {
                setChecks([]);
                setChecksNextCursor(null);
                void loadChecks(true);
              }}
              type="button"
            >
              {isLoadingChecks ? "Loading..." : "Refresh"}
            </button>
          </div>
        </div>

        {checksError && <div className="inline-alert inline-alert-danger">{checksError}</div>}

        {checks.length === 0 && !isLoadingChecks ? (
          <div className="empty-state">
            {checksFilter === "all" ? "No check history available." : `No ${checksFilter} checks found.`}
          </div>
        ) : (
          <>
            <div className="checks-table-wrapper">
              <table className="checks-table">
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Type</th>
                    <th>Target</th>
                    <th>Result</th>
                    <th>Code</th>
                    <th>Response</th>
                    <th>Error</th>
                  </tr>
                </thead>
                <tbody>
                  {renderCheckRows()}
                </tbody>
              </table>
            </div>
            {(checksNextCursor || isLoadingChecks) && (
              <div className="load-more-row">
                <button className="ghost-button" disabled={isLoadingChecks} onClick={() => void loadChecks(false)} type="button">
                  {isLoadingChecks ? "Loading..." : "Load more"}
                </button>
              </div>
            )}
          </>
        )}
      </article>
    </div>
  );
}
