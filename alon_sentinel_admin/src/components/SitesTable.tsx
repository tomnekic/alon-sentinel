import { type FormEvent, useState } from "react";
import type { Site } from "../api";

type SitesTableProps = {
  canCreateSite: boolean;
  canDeleteSite: boolean;
  canUpdateSite: boolean;
  onCreateSite: (payload: {
    name: string;
    base_url: string;
  }) => Promise<boolean>;
  onUpdateSite: (
    siteId: number,
    payload: { name: string; base_url: string; is_active: boolean }
  ) => Promise<boolean>;
  onDeleteSite: (siteId: number) => Promise<boolean>;
  isRefreshing: boolean;
  onOpenDashboard: (site: Site) => void;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
  onSearchQueryChange: (value: string) => void;
  sites: Site[];
  page: number;
  pageSize: number;
  onRefresh: () => void;
  searchQuery: string;
  totalCount: number;
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

type SortKey = "name" | "base_url" | "health" | "monitor" | "status" | "updated_at";
type SortDirection = "asc" | "desc";

function getSiteOrigin(value: string) {
  try {
    return new URL(value).origin;
  } catch {
    return null;
  }
}

function getSiteInitials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("") || "?";
}

function getHealthRank(site: Site) {
  if (site.has_open_incident) return 4;
  if (site.current_state === "failing") return 3;
  if (site.current_state === "pending_first_check") return 2;
  if (site.current_state === "healthy") return 1;
  return 0;
}

function getRowTone(site: Site) {
  if (!site.is_active) return "disabled";
  if (site.has_open_incident || site.current_state === "failing") return "incident";
  if (site.current_state === "pending_first_check") return "pending";
  if (site.current_state === "healthy") return "healthy";
  return "muted";
}

function getHealthLabel(site: Site) {
  if (site.has_open_incident) return "Incident";
  if (site.current_state === "healthy") return "Healthy";
  if (site.current_state === "failing") return "Failing";
  if (site.current_state === "pending_first_check") return "Pending";
  if (site.current_state === "not_configured") return "Not configured";
  return "Disabled";
}

function compareSites(a: Site, b: Site, sortKey: SortKey) {
  switch (sortKey) {
    case "name": return a.name.localeCompare(b.name);
    case "base_url": return a.base_url.localeCompare(b.base_url);
    case "health": return getHealthRank(a) - getHealthRank(b);
    case "monitor": return a.http_monitor_status.localeCompare(b.http_monitor_status);
    case "status": return Number(a.is_active) - Number(b.is_active);
    case "updated_at": return new Date(a.updated_at).getTime() - new Date(b.updated_at).getTime();
  }
}

function SiteIcon({ site }: { site: Site }) {
  const origin = getSiteOrigin(site.base_url);
  return (
    <span className="site-identity-icon" aria-hidden="true">
      {origin ? <img src={`${origin}/favicon.ico`} alt="" loading="lazy" /> : null}
      <span>{getSiteInitials(site.name)}</span>
    </span>
  );
}

function SiteUptimeStrip({ site }: { site: Site }) {
  const tone = getRowTone(site);
  const segments = Array.from({ length: 18 }, (_, index) => {
    if (tone === "incident") return index > 13 ? "incident" : "healthy";
    if (tone === "pending") return index > 10 ? "pending" : "muted";
    if (tone === "disabled" || tone === "muted") return "muted";
    return "healthy";
  });

  return (
    <div className="site-uptime-strip" title={`Current health: ${getHealthLabel(site)}`}>
      {segments.map((segment, index) => (
        <span key={index} className={`site-uptime-segment site-uptime-segment-${segment}`} />
      ))}
    </div>
  );
}

function MonitorCoverage({ site }: { site: Site }) {
  const configured = site.http_monitor_status !== "not_configured";
  const active = site.http_monitor_status === "active";
  return (
    <div className="site-monitor-coverage">
      <span className={`site-monitor-dot ${configured ? "is-configured" : ""} ${active ? "is-active" : ""}`} />
      <span className="site-monitor-bar"><span style={{ width: configured ? "100%" : "12%" }} /></span>
      <span>{active ? "1 active" : configured ? "configured" : "none"}</span>
    </div>
  );
}

export function SitesTable({
  canCreateSite,
  canDeleteSite,
  canUpdateSite,
  onCreateSite,
  onDeleteSite: onDeleteSiteRequest,
  onUpdateSite: onUpdateSiteRequest,
  isRefreshing,
  onOpenDashboard,
  onPageChange,
  onPageSizeChange,
  onRefresh,
  onSearchQueryChange,
  page,
  pageSize,
  searchQuery,
  sites,
  totalCount
}: SitesTableProps) {
  const [isCreateSiteModalOpen, setIsCreateSiteModalOpen] = useState(false);
  const [isCreatingSite, setIsCreatingSite] = useState(false);
  const [createSiteValidationError, setCreateSiteValidationError] = useState<string | null>(null);
  const [editSiteValidationError, setEditSiteValidationError] = useState<string | null>(null);
  const [editingSite, setEditingSite] = useState<Site | null>(null);
  const [isUpdatingSite, setIsUpdatingSite] = useState(false);
  const [isDeletingSiteId, setIsDeletingSiteId] = useState<number | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("updated_at");
  const [sortDirection, setSortDirection] = useState<SortDirection>("desc");
  const [siteDraft, setSiteDraft] = useState({
    name: "",
    base_url: ""
  });
  const [siteEditDraft, setSiteEditDraft] = useState({
    name: "",
    base_url: "",
    is_active: true
  });
  const sitesWithIncidents = sites.filter((site) => site.has_open_incident).length;
  const incidentSites = sites.filter((site) => site.has_open_incident);
  const totalPages = Math.max(1, Math.ceil(totalCount / pageSize));
  const pageStart = totalCount === 0 ? 0 : (page - 1) * pageSize + 1;
  const pageEnd = Math.min(page * pageSize, totalCount);
  const sortedSites = [...sites].sort((a, b) => {
    const result = compareSites(a, b, sortKey);
    return sortDirection === "asc" ? result : -result;
  });

  function handleSort(nextKey: SortKey) {
    if (sortKey === nextKey) {
      setSortDirection((current) => current === "asc" ? "desc" : "asc");
      return;
    }
    setSortKey(nextKey);
    setSortDirection(nextKey === "updated_at" ? "desc" : "asc");
  }

  function SortHeader({ label, column }: { label: string; column: SortKey }) {
    const active = sortKey === column;
    return (
      <button className={`sort-header${active ? " is-active" : ""}`} type="button" onClick={() => handleSort(column)}>
        <span>{label}</span>
        <span>{active ? (sortDirection === "asc" ? "▲" : "▼") : "↕"}</span>
      </button>
    );
  }

  function startCreateSite() {
    setSiteDraft({
      name: "",
      base_url: ""
    });
    setCreateSiteValidationError(null);
    setIsCreateSiteModalOpen(true);
  }

  async function handleCreateSiteSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const normalizedName = siteDraft.name.trim();
    const normalizedBaseUrl = siteDraft.base_url.trim();
    if (!normalizedName) {
      setCreateSiteValidationError("Site name is required.");
      return;
    }
    let parsedUrl: URL | null = null;
    try {
      parsedUrl = new URL(normalizedBaseUrl);
    } catch {
      parsedUrl = null;
    }
    if (!parsedUrl || !["http:", "https:"].includes(parsedUrl.protocol)) {
      setCreateSiteValidationError("Base URL must be a valid http/https URL.");
      return;
    }

    setCreateSiteValidationError(null);
    setIsCreatingSite(true);

    try {
      const created = await onCreateSite({
        name: normalizedName,
        base_url: normalizedBaseUrl
      });

      if (created) {
        setIsCreateSiteModalOpen(false);
      }
    } finally {
      setIsCreatingSite(false);
    }
  }

  function openEditSite(site: Site) {
    setEditingSite(site);
    setEditSiteValidationError(null);
    setSiteEditDraft({
      name: site.name,
      base_url: site.base_url,
      is_active: site.is_active
    });
  }

  function closeEditSite() {
    if (isUpdatingSite) return;
    setEditingSite(null);
    setEditSiteValidationError(null);
  }

  async function handleEditSiteSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!editingSite) return;
    const normalizedName = siteEditDraft.name.trim();
    const normalizedBaseUrl = siteEditDraft.base_url.trim();
    if (!normalizedName) {
      setEditSiteValidationError("Site name is required.");
      return;
    }
    let parsedUrl: URL | null = null;
    try {
      parsedUrl = new URL(normalizedBaseUrl);
    } catch {
      parsedUrl = null;
    }
    if (!parsedUrl || !["http:", "https:"].includes(parsedUrl.protocol)) {
      setEditSiteValidationError("Base URL must be a valid http/https URL.");
      return;
    }
    setEditSiteValidationError(null);
    setIsUpdatingSite(true);
    try {
      const updated = await onUpdateSiteRequest(editingSite.id, {
        name: normalizedName,
        base_url: normalizedBaseUrl,
        is_active: siteEditDraft.is_active
      });
      if (updated) {
        setEditingSite(null);
      }
    } finally {
      setIsUpdatingSite(false);
    }
  }

  async function handleToggleSiteActive(site: Site) {
    if (!canUpdateSite) return;
    setIsUpdatingSite(true);
    try {
      await onUpdateSiteRequest(site.id, {
        name: site.name,
        base_url: site.base_url,
        is_active: !site.is_active
      });
    } finally {
      setIsUpdatingSite(false);
    }
  }

  async function handleDeleteSite(site: Site) {
    if (!canDeleteSite) return;
    const confirmed = window.confirm(
      `Delete site "${site.name}"?\n\nThis removes the site and related monitoring data.`
    );
    if (!confirmed) return;
    setIsDeletingSiteId(site.id);
    try {
      await onDeleteSiteRequest(site.id);
    } finally {
      setIsDeletingSiteId(null);
    }
  }

  return (
    <section className="panel page-panel">
      <div className="panel-header">
        <div>
          <div className="panel-kicker">Inventory</div>
          <h2>Sites</h2>
          <p>All sites currently returned by the Sentinel `/v1/sites` endpoint.</p>
        </div>

        <div className="page-header-actions">
          <label className="table-toolbar-field">
            <span>Search</span>
            <input
              type="search"
              value={searchQuery}
              onChange={(event) => onSearchQueryChange(event.target.value)}
              placeholder="Search name, URL, status, or ID"
            />
          </label>
          <label className="table-toolbar-field table-toolbar-select">
            <span>Rows</span>
            <select
              value={String(pageSize)}
              onChange={(event) => onPageSizeChange(Number(event.target.value))}
            >
              <option value="10">10</option>
              <option value="25">25</option>
              <option value="50">50</option>
            </select>
          </label>
          <div className="stat-chip">
            <span>Total</span>
            <strong>{totalCount}</strong>
          </div>
          {sitesWithIncidents > 0 && (
            <div className="stat-chip stat-chip-alert">
              <span>Incidents</span>
              <strong>{sitesWithIncidents}</strong>
            </div>
          )}
          <button className="ghost-button" type="button" onClick={onRefresh}>
            {isRefreshing ? "Refreshing..." : "Refresh Sites"}
          </button>
          <button className="ghost-button" type="button" onClick={startCreateSite} disabled={!canCreateSite}>
            Add Site
          </button>
        </div>
      </div>

      {incidentSites.length > 0 && (
        <div className="site-incident-group">
          <span>Open incidents</span>
          <div>
            {incidentSites.map((site) => (
              <button key={site.id} type="button" onClick={() => onOpenDashboard(site)}>
                {site.name}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th><SortHeader label="Name" column="name" /></th>
              <th><SortHeader label="Base URL" column="base_url" /></th>
              <th><SortHeader label="Health" column="health" /></th>
              <th>Uptime</th>
              <th><SortHeader label="Monitor" column="monitor" /></th>
              <th><SortHeader label="Site Status" column="status" /></th>
              <th><SortHeader label="Updated" column="updated_at" /></th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {sortedSites.map((site) => (
              <tr key={site.id} className={`site-row site-row-${getRowTone(site)}`}>
                <td>
                  <div className="site-identity">
                    <SiteIcon site={site} />
                    <div>
                      <div className="table-primary">{site.name}</div>
                      <div className="table-secondary">Site #{site.id}</div>
                    </div>
                  </div>
                </td>
                <td className="table-url">{site.base_url}</td>
                <td>
                  {site.has_open_incident ? (
                    <span className="tag tag-failing">Incident</span>
                  ) : site.current_state === "healthy" ? (
                    <span className="tag tag-active">Healthy</span>
                  ) : site.current_state === "failing" ? (
                    <span className="tag tag-failing">Failing</span>
                  ) : site.current_state === "pending_first_check" ? (
                    <span className="tag tag-warning">Pending</span>
                  ) : (
                    <span className="tag tag-muted">—</span>
                  )}
                </td>
                <td><SiteUptimeStrip site={site} /></td>
                <td>
                  <MonitorCoverage site={site} />
                </td>
                <td>
                  <span className={`tag ${site.is_active ? "tag-active" : "tag-disabled"}`}>
                    {site.is_active ? "active" : "disabled"}
                  </span>
                </td>
                <td>{formatTimestamp(site.updated_at)}</td>
                <td className="table-actions">
                  <button className="ghost-button" type="button" onClick={() => onOpenDashboard(site)}>
                    Dashboard
                  </button>
                  <button
                    className="ghost-button"
                    type="button"
                    onClick={() => openEditSite(site)}
                    disabled={isUpdatingSite || !canUpdateSite}
                  >
                    Edit
                  </button>
                  <button
                    className="ghost-button"
                    type="button"
                    onClick={() => void handleToggleSiteActive(site)}
                    disabled={isUpdatingSite || !canUpdateSite}
                  >
                    {site.is_active ? "Deactivate" : "Activate"}
                  </button>
                  <button
                    className="ghost-button ghost-button-danger"
                    type="button"
                    onClick={() => void handleDeleteSite(site)}
                    disabled={isDeletingSiteId === site.id || !canDeleteSite}
                  >
                    {isDeletingSiteId === site.id ? "Deleting..." : "Delete"}
                  </button>
                </td>
              </tr>
            ))}

            {sites.length === 0 && (
              <tr>
                <td colSpan={8}>
                  <div className="empty-state table-empty">
                    {searchQuery.trim().length === 0
                      ? canCreateSite
                        ? "No sites yet. Click \"Add Site\" to create your first one."
                        : "No sites yet."
                      : "No sites match the current search."}
                  </div>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="table-pagination">
        <div className="table-pagination-meta">
          Showing {pageStart}-{pageEnd} of {totalCount}
        </div>

        <div className="table-pagination-actions">
          <button
            className="ghost-button"
            type="button"
            onClick={() => onPageChange(Math.max(1, page - 1))}
            disabled={page === 1}
          >
            Previous
          </button>
          <span className="table-pagination-page">
            Page {page} / {totalPages}
          </span>
          <button
            className="ghost-button"
            type="button"
            onClick={() => onPageChange(Math.min(totalPages, page + 1))}
            disabled={page === totalPages}
          >
            Next
          </button>
        </div>
      </div>

      {isCreateSiteModalOpen && canCreateSite && (
        <div className="modal-backdrop" role="presentation" onClick={() => setIsCreateSiteModalOpen(false)}>
          <div
            className="modal-panel panel"
            role="dialog"
            aria-modal="true"
            aria-label="Create site"
            onClick={(event) => event.stopPropagation()}
          >
            <form className="stacked-form" onSubmit={handleCreateSiteSubmit}>
              <div className="management-list-header">
                <strong>Create Site</strong>
                <button
                  className="ghost-button"
                  type="button"
                  onClick={() => setIsCreateSiteModalOpen(false)}
                >
                  Close
                </button>
              </div>

              <label>
                <span>Name</span>
                <input
                  required
                  value={siteDraft.name}
                  onChange={(event) =>
                    setSiteDraft((current) => ({
                      ...current,
                      name: event.target.value
                    }))
                  }
                  placeholder="Main Website"
                />
              </label>

              <label>
                <span>Base URL</span>
                <input
                  type="url"
                  required
                  value={siteDraft.base_url}
                  onChange={(event) =>
                    setSiteDraft((current) => ({
                      ...current,
                      base_url: event.target.value
                    }))
                  }
                  placeholder="https://example.com"
                />
              </label>

              {createSiteValidationError && (
                <div className="inline-alert inline-alert-danger">{createSiteValidationError}</div>
              )}


              <div className="panel-actions">
                <button className="primary-button" type="submit" disabled={isCreatingSite}>
                  {isCreatingSite ? "Creating..." : "Create Site"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {editingSite && canUpdateSite && (
        <div className="modal-backdrop" role="presentation" onClick={closeEditSite}>
          <div
            className="modal-panel panel"
            role="dialog"
            aria-modal="true"
            aria-label="Edit site"
            onClick={(event) => event.stopPropagation()}
          >
            <form className="stacked-form" onSubmit={handleEditSiteSubmit}>
              <div className="management-list-header">
                <strong>Edit Site</strong>
                <button className="ghost-button" type="button" onClick={closeEditSite}>
                  Close
                </button>
              </div>

              <label>
                <span>Name</span>
                <input
                  required
                  value={siteEditDraft.name}
                  onChange={(event) =>
                    setSiteEditDraft((current) => ({
                      ...current,
                      name: event.target.value
                    }))
                  }
                  placeholder="Main Website"
                />
              </label>

              <label>
                <span>Base URL</span>
                <input
                  type="url"
                  required
                  value={siteEditDraft.base_url}
                  onChange={(event) =>
                    setSiteEditDraft((current) => ({
                      ...current,
                      base_url: event.target.value
                    }))
                  }
                  placeholder="https://example.com"
                />
              </label>

              <label className="form-field-checkbox">
                <input
                  type="checkbox"
                  checked={siteEditDraft.is_active}
                  onChange={(event) =>
                    setSiteEditDraft((current) => ({
                      ...current,
                      is_active: event.target.checked
                    }))
                  }
                />
                <span>Site is active</span>
              </label>

              {editSiteValidationError && (
                <div className="inline-alert inline-alert-danger">{editSiteValidationError}</div>
              )}

              <div className="panel-actions">
                <button className="primary-button" type="submit" disabled={isUpdatingSite}>
                  {isUpdatingSite ? "Saving..." : "Save Changes"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </section>
  );
}
