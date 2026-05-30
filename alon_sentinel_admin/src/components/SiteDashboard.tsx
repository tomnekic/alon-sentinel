import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  type AuthSession,
  type SiteMonitorInventory,
  type SiteMonitorType,
  type SiteNotificationChannel,
  type SiteNotificationDelivery,
  type SiteSummary,
  type SiteUptime,
  configureDnsMonitor,
  configureHeartbeatMonitor,
  configureHttpMonitor,
  configureSslMonitor,
  configureTcpMonitor,
  deleteSiteMonitor,
  deleteSiteNotificationChannelOverride,
  getSiteUptime,
  listSiteNotificationChannels,
  listSiteNotificationDeliveries,
  type StatusPageConfig,
  getStatusPageConfig,
  pauseSiteMonitor,
  resumeSiteMonitor,
  upsertSiteNotificationChannelOverride,
  upsertStatusPageConfig,
} from "../api";
import { HistoryTab } from "./HistoryTab";
import { IncidentsTab } from "./IncidentsTab";
import { MonitorsTab } from "./MonitorsTab";
import { OverviewTab } from "./OverviewTab";
import {
  buildDefaultDraft,
  type ConfigureDraft,
  type DashboardTab,
  formatMonitorTypeLabel,
  getMonitorPrimaryLabel,
  formatTimestamp,
  StatusBadge,
  validateConfigureDraft,
} from "./SiteDashboardShared";

type SiteDashboardProps = {
  summary: SiteSummary;
  monitorInventory: SiteMonitorInventory | null;
  session: AuthSession | null;
  canWriteMonitors: boolean;
  canWriteSiteNotificationOverrides: boolean;
  canUpdateSite: boolean;
  onRefresh: () => Promise<void>;
};

export function SiteDashboard({
  summary,
  monitorInventory,
  session,
  canWriteMonitors,
  canWriteSiteNotificationOverrides,
  canUpdateSite,
  onRefresh
}: SiteDashboardProps) {
  const { incident_open, site } = summary;
  const inventory = monitorInventory;

  const [activeTab, setActiveTab] = useState<DashboardTab>("overview");

  // Monitor mutation
  const [draft, setDraft] = useState<ConfigureDraft | null>(null);
  const [isMutating, setIsMutating] = useState(false);
  const [monitorActionError, setMonitorActionError] = useState<string | null>(null);

  // Notifications tab
  const [siteChannels, setSiteChannels] = useState<SiteNotificationChannel[]>([]);
  const [deliveries, setDeliveries] = useState<SiteNotificationDelivery[]>([]);
  const [deliveriesNextCursor, setDeliveriesNextCursor] = useState<string | null>(null);
  const [isLoadingNotifications, setIsLoadingNotifications] = useState(false);
  const [notificationsError, setNotificationsError] = useState<string | null>(null);
  const notificationsLoadedSiteRef = useRef<number | null>(null);
  const [togglingChannelId, setTogglingChannelId] = useState<number | null>(null);

  // Uptime
  const [uptime7d, setUptime7d] = useState<SiteUptime | null>(null);
  const [uptime30d, setUptime30d] = useState<SiteUptime | null>(null);

  // Status page tab
  const [statusPageConfig, setStatusPageConfig] = useState<StatusPageConfig | null>(null);
  const [isLoadingStatusPage, setIsLoadingStatusPage] = useState(false);
  const [statusPageError, setStatusPageError] = useState<string | null>(null);
  const [statusPageSaveError, setStatusPageSaveError] = useState<string | null>(null);
  const [statusPageSaveSuccess, setStatusPageSaveSuccess] = useState(false);
  const [isSavingStatusPage, setIsSavingStatusPage] = useState(false);
  const statusPageLoadedSiteRef = useRef<number | null>(null);
  // draft form fields
  const [spEnabled, setSpEnabled] = useState(false);
  const [spSlug, setSpSlug] = useState("");
  const [spTitle, setSpTitle] = useState("");
  const [spShowMonitors, setSpShowMonitors] = useState(true);
  const [spShowUptime, setSpShowUptime] = useState(true);

  useEffect(() => {
    setActiveTab("overview");
    setSiteChannels([]);
    setDeliveries([]);
    setDeliveriesNextCursor(null);
    setNotificationsError(null);
    notificationsLoadedSiteRef.current = null;
    setMonitorActionError(null);
    setUptime7d(null);
    setUptime30d(null);
    setStatusPageConfig(null);
    setStatusPageError(null);
    setStatusPageSaveError(null);
    setStatusPageSaveSuccess(false);
    statusPageLoadedSiteRef.current = null;
  }, [site.id]);

  async function ensureSession(): Promise<AuthSession> {
    if (!session) throw new Error("Admin session has expired. Log in again.");
    return session;
  }

  useEffect(() => {
    if (!session) return;
    void Promise.all([
      getSiteUptime(session, site.id, { window: "7d" }),
      getSiteUptime(session, site.id, { window: "30d" }),
    ]).then(([r7d, r30d]) => {
      setUptime7d(r7d.uptime);
      setUptime30d(r30d.uptime);
    }).catch(() => {});
  }, [site.id, session]);

  // --- Data loading ---

  async function loadSiteNotifications(reset: boolean) {
    if (isLoadingNotifications) return;
    setIsLoadingNotifications(true);
    setNotificationsError(null);
    try {
      const activeSession = await ensureSession();
      const [channelsResult, deliveriesResult] = await Promise.all([
        listSiteNotificationChannels(activeSession, site.id),
        listSiteNotificationDeliveries(activeSession, site.id, {
          cursor: reset ? undefined : (deliveriesNextCursor ?? undefined),
          limit: 50,
        }),
      ]);
      setSiteChannels(channelsResult.channels);
      setDeliveries(reset ? deliveriesResult.deliveries : (prev) => [...prev, ...deliveriesResult.deliveries]);
      setDeliveriesNextCursor(deliveriesResult.nextCursor);
      notificationsLoadedSiteRef.current = site.id;
    } catch (e) {
      setNotificationsError(e instanceof Error ? e.message : "Failed to load notification data.");
    } finally {
      setIsLoadingNotifications(false);
    }
  }

  async function handleToggleSiteChannelOverride(channel: SiteNotificationChannel, field: "is_active" | "notify_on_failure" | "notify_on_recovery") {
    setTogglingChannelId(channel.id);
    try {
      const activeSession = await ensureSession();
      const currentValue = field === "is_active"
        ? channel.effective_is_active
        : field === "notify_on_failure"
          ? channel.effective_notify_on_failure
          : channel.effective_notify_on_recovery;
      await upsertSiteNotificationChannelOverride(activeSession, site.id, channel.id, {
        [field]: !currentValue,
      });
      await loadSiteNotifications(true);
    } catch (e) {
      setNotificationsError(e instanceof Error ? e.message : "Failed to update channel override.");
    } finally {
      setTogglingChannelId(null);
    }
  }

  async function handleResetSiteChannelOverride(channelId: number) {
    setTogglingChannelId(channelId);
    try {
      const activeSession = await ensureSession();
      await deleteSiteNotificationChannelOverride(activeSession, site.id, channelId);
      await loadSiteNotifications(true);
    } catch (e) {
      setNotificationsError(e instanceof Error ? e.message : "Failed to reset channel override.");
    } finally {
      setTogglingChannelId(null);
    }
  }

  async function loadStatusPageConfig() {
    if (isLoadingStatusPage) return;
    setIsLoadingStatusPage(true);
    setStatusPageError(null);
    try {
      const activeSession = await ensureSession();
      const { config } = await getStatusPageConfig(activeSession, site.id);
      setStatusPageConfig(config);
      setSpEnabled(config.is_enabled);
      setSpSlug(config.slug);
      setSpTitle(config.page_title ?? "");
      setSpShowMonitors(config.show_monitor_details);
      setSpShowUptime(config.show_uptime_percentages);
      statusPageLoadedSiteRef.current = site.id;
    } catch (e) {
      setStatusPageError(e instanceof Error ? e.message : "Failed to load status page config.");
    } finally {
      setIsLoadingStatusPage(false);
    }
  }

  async function handleSaveStatusPage() {
    setIsSavingStatusPage(true);
    setStatusPageSaveError(null);
    setStatusPageSaveSuccess(false);
    try {
      const activeSession = await ensureSession();
      const { config } = await upsertStatusPageConfig(activeSession, site.id, {
        is_enabled: spEnabled,
        slug: spSlug.trim(),
        page_title: spTitle.trim() || null,
        show_monitor_details: spShowMonitors,
        show_uptime_percentages: spShowUptime,
      });
      setStatusPageConfig(config);
      setSpEnabled(config.is_enabled);
      setSpSlug(config.slug);
      setSpTitle(config.page_title ?? "");
      setSpShowMonitors(config.show_monitor_details);
      setSpShowUptime(config.show_uptime_percentages);
      setStatusPageSaveSuccess(true);
    } catch (e) {
      setStatusPageSaveError(e instanceof Error ? e.message : "Failed to save status page config.");
    } finally {
      setIsSavingStatusPage(false);
    }
  }

  function handleTabChange(tab: DashboardTab) {
    setActiveTab(tab);
    if (tab === "notifications" && notificationsLoadedSiteRef.current !== site.id) {
      void loadSiteNotifications(true);
    }
    if (tab === "status-page" && statusPageLoadedSiteRef.current !== site.id) {
      void loadStatusPageConfig();
    }
  }

  // --- Monitor actions ---

  function openConfigureMonitor(type: SiteMonitorType) {
    setMonitorActionError(null);
    setDraft(buildDefaultDraft(summary, type, inventory));
  }

  function closeConfigureMonitor() {
    if (isMutating) return;
    setDraft(null);
    setMonitorActionError(null);
  }

  async function handlePauseMonitor(type: SiteMonitorType, monitorId: number) {
    setIsMutating(true);
    setMonitorActionError(null);
    try {
      const s = await ensureSession();
      await pauseSiteMonitor(s, site.id, type, monitorId);
      await onRefresh();
    } catch (e) {
      setMonitorActionError(e instanceof Error ? e.message : "Failed to pause monitor.");
    } finally {
      setIsMutating(false);
    }
  }

  async function handleResumeMonitor(type: SiteMonitorType, monitorId: number) {
    setIsMutating(true);
    setMonitorActionError(null);
    try {
      const s = await ensureSession();
      await resumeSiteMonitor(s, site.id, type, monitorId);
      await onRefresh();
    } catch (e) {
      setMonitorActionError(e instanceof Error ? e.message : "Failed to resume monitor.");
    } finally {
      setIsMutating(false);
    }
  }

  async function handleDeleteMonitor(type: SiteMonitorType, monitorId: number) {
    const monitor = (inventory?.[type] ?? []).find((m) => m.id === monitorId);
    const label = monitor ? getMonitorPrimaryLabel(monitor) : formatMonitorTypeLabel(type);
    const confirmed = window.confirm(
      `Delete ${formatMonitorTypeLabel(type)} monitor "${label}"?\n\nThis removes the monitor and all its check history and cannot be undone.`
    );
    if (!confirmed) return;

    setIsMutating(true);
    setMonitorActionError(null);
    try {
      const s = await ensureSession();
      await deleteSiteMonitor(s, site.id, type, monitorId);
      await onRefresh();
    } catch (e) {
      setMonitorActionError(e instanceof Error ? e.message : "Failed to delete monitor.");
    } finally {
      setIsMutating(false);
    }
  }

  async function handleSubmitConfigureMonitor(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft) return;
    setIsMutating(true);
    setMonitorActionError(null);
    try {
      const s = await ensureSession();
      const parseNums = (v: string) => {
        const nums = v.split(",").map((s) => s.trim()).filter(Boolean).map(Number).filter((n) => !Number.isNaN(n) && n > 0);
        return nums.length > 0 ? nums : null;
      };
      switch (draft.type) {
        case "http": {
          const parseLines = (v: string) => {
            const lines = v.split("\n").map((l) => l.trim()).filter(Boolean);
            return lines.length > 0 ? lines : null;
          };
          const parseJsonVal = (v: string): unknown => {
            const t = v.trim();
            try { return JSON.parse(t); } catch { return t; }
          };
          await configureHttpMonitor(s, site.id, {
            target_url: draft.httpTargetUrl.trim(),
            check_interval_seconds: Number(draft.httpInterval),
            expected_status_code: Number(draft.httpExpectedStatusCode),
            is_active: true,
            max_response_time_ms: draft.httpMaxResponseTimeMs.trim()
              ? Number(draft.httpMaxResponseTimeMs) : null,
            body_must_contain: draft.httpBodyMustContain.trim() || null,
            body_must_not_contain: draft.httpBodyMustNotContain.trim() || null,
            body_must_contain_texts: parseLines(draft.httpBodyMustContainTexts),
            body_must_not_contain_texts: parseLines(draft.httpBodyMustNotContainTexts),
            required_header_name: draft.httpRequiredHeaderName.trim() || null,
            required_header_value: draft.httpRequiredHeaderValue.trim() || null,
            header_assertions: draft.httpHeaderAssertions.filter((a) => a.name.trim()).length > 0
              ? draft.httpHeaderAssertions
                  .filter((a) => a.name.trim())
                  .map((a) => ({
                    name: a.name.trim(),
                    equals: a.equals.trim() || null,
                    contains: a.contains.trim() || null
                  }))
              : null,
            json_path_exists: parseLines(draft.httpJsonPathExists),
            json_path_equals: draft.httpJsonPathEquals.filter((a) => a.path.trim()).length > 0
              ? draft.httpJsonPathEquals
                  .filter((a) => a.path.trim())
                  .map((a) => ({ path: a.path.trim(), value: parseJsonVal(a.value) }))
              : null,
            json_path_not_equals: draft.httpJsonPathNotEquals.filter((a) => a.path.trim()).length > 0
              ? draft.httpJsonPathNotEquals
                  .filter((a) => a.path.trim())
                  .map((a) => ({ path: a.path.trim(), value: parseJsonVal(a.value) }))
              : null,
            http_check_timeout_seconds_override: draft.httpTimeoutSecondsOverride.trim()
              ? Number(draft.httpTimeoutSecondsOverride) : null,
            http_check_max_attempts_override: draft.httpMaxAttemptsOverride.trim()
              ? Number(draft.httpMaxAttemptsOverride) : null,
            http_check_retry_delays_ms_override: parseNums(draft.httpRetryDelaysMsOverride)
          });
          break;
        }
        case "ssl":
          await configureSslMonitor(s, site.id, {
            target_url: draft.sslTargetUrl.trim(),
            check_interval_seconds: Number(draft.sslInterval),
            ssl_expiry_warning_days: draft.sslWarningDays.trim() ? Number(draft.sslWarningDays) : undefined,
            http_check_timeout_seconds_override: draft.sslTimeoutSecondsOverride.trim() ? Number(draft.sslTimeoutSecondsOverride) : null,
            http_check_max_attempts_override: draft.sslMaxAttemptsOverride.trim() ? Number(draft.sslMaxAttemptsOverride) : null,
            http_check_retry_delays_ms_override: parseNums(draft.sslRetryDelaysMsOverride),
            is_active: true
          });
          break;
        case "heartbeat":
          await configureHeartbeatMonitor(s, site.id, {
            check_interval_seconds: Number(draft.heartbeatInterval),
            heartbeat_grace_seconds: draft.heartbeatGraceSeconds.trim()
              ? Number(draft.heartbeatGraceSeconds)
              : undefined,
            is_active: true
          });
          break;
        case "tcp":
          await configureTcpMonitor(s, site.id, {
            target_host: draft.tcpTargetHost.trim(),
            target_port: Number(draft.tcpTargetPort),
            check_interval_seconds: Number(draft.tcpInterval),
            max_connect_time_ms: draft.tcpMaxConnectTimeMs.trim() ? Number(draft.tcpMaxConnectTimeMs) : undefined,
            timeout_seconds_override: draft.tcpTimeoutSecondsOverride.trim() ? Number(draft.tcpTimeoutSecondsOverride) : null,
            max_attempts_override: draft.tcpMaxAttemptsOverride.trim() ? Number(draft.tcpMaxAttemptsOverride) : null,
            retry_delays_ms_override: parseNums(draft.tcpRetryDelaysMsOverride),
            is_active: true
          });
          break;
        case "dns":
          await configureDnsMonitor(s, site.id, {
            hostname: draft.dnsHostname.trim(),
            record_type: draft.dnsRecordType.trim().toUpperCase(),
            expected_value: draft.dnsExpectedValue.trim() || undefined,
            nameserver: draft.dnsNameserver.trim() || undefined,
            check_interval_seconds: Number(draft.dnsInterval),
            timeout_seconds_override: draft.dnsTimeoutSecondsOverride.trim() ? Number(draft.dnsTimeoutSecondsOverride) : null,
            max_attempts_override: draft.dnsMaxAttemptsOverride.trim() ? Number(draft.dnsMaxAttemptsOverride) : null,
            retry_delays_ms_override: parseNums(draft.dnsRetryDelaysMsOverride),
            is_active: true
          });
          break;
      }
      setDraft(null);
      await onRefresh();
    } catch (e) {
      setMonitorActionError(e instanceof Error ? e.message : "Failed to configure monitor.");
    } finally {
      setIsMutating(false);
    }
  }

  // Tab bodies live in OverviewTab, MonitorsTab, HistoryTab, and IncidentsTab.

  // --- Render: Configure fields (modal) ---

  function renderConfigureFields() {
    if (!draft) return null;
    switch (draft.type) {
      case "http":
        return (
          <div className="http-monitor-form">
            {/* ── Basic ── */}
            <div className="form-section">
              <span className="form-section-title">Basic</span>
              <div className="site-dashboard-form-grid">
                <label className="form-field-wide">
                  <span>Target URL</span>
                  <input
                    value={draft.httpTargetUrl}
                    onChange={(e) => setDraft({ ...draft, httpTargetUrl: e.target.value })}
                    placeholder="https://example.com/health"
                  />
                </label>
                <label>
                  <span>Interval (seconds)</span>
                  <input
                    type="number"
                    min={30}
                    value={draft.httpInterval}
                    onChange={(e) => setDraft({ ...draft, httpInterval: e.target.value })}
                  />
                </label>
                <label>
                  <span>Expected status code</span>
                  <input
                    type="number"
                    min={100}
                    max={599}
                    value={draft.httpExpectedStatusCode}
                    onChange={(e) => setDraft({ ...draft, httpExpectedStatusCode: e.target.value })}
                  />
                </label>
                <label>
                  <span>Max response time (ms)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.httpMaxResponseTimeMs}
                    onChange={(e) => setDraft({ ...draft, httpMaxResponseTimeMs: e.target.value })}
                    placeholder="Optional"
                  />
                </label>
              </div>
            </div>

            {/* ── Body Assertions ── */}
            <div className="form-section">
              <span className="form-section-title">Body Assertions</span>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Body must contain</span>
                  <input
                    value={draft.httpBodyMustContain}
                    onChange={(e) => setDraft({ ...draft, httpBodyMustContain: e.target.value })}
                    placeholder="Optional — single text match"
                  />
                </label>
                <label>
                  <span>Body must NOT contain</span>
                  <input
                    value={draft.httpBodyMustNotContain}
                    onChange={(e) => setDraft({ ...draft, httpBodyMustNotContain: e.target.value })}
                    placeholder="Optional — single text match"
                  />
                </label>
                <label className="form-field-wide">
                  <span>Body must contain ALL (one per line)</span>
                  <textarea
                    rows={3}
                    value={draft.httpBodyMustContainTexts}
                    onChange={(e) => setDraft({ ...draft, httpBodyMustContainTexts: e.target.value })}
                    placeholder={"success\nok\n..."}
                  />
                </label>
                <label className="form-field-wide">
                  <span>Body must NOT contain ANY (one per line)</span>
                  <textarea
                    rows={3}
                    value={draft.httpBodyMustNotContainTexts}
                    onChange={(e) => setDraft({ ...draft, httpBodyMustNotContainTexts: e.target.value })}
                    placeholder={"error\nfail\n..."}
                  />
                </label>
              </div>
            </div>

            {/* ── Header Assertions ── */}
            <div className="form-section">
              <span className="form-section-title">Header Assertions</span>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Required header name</span>
                  <input
                    value={draft.httpRequiredHeaderName}
                    onChange={(e) => setDraft({ ...draft, httpRequiredHeaderName: e.target.value })}
                    placeholder="Content-Type"
                  />
                </label>
                <label>
                  <span>Required header value</span>
                  <input
                    value={draft.httpRequiredHeaderValue}
                    onChange={(e) => setDraft({ ...draft, httpRequiredHeaderValue: e.target.value })}
                    placeholder="application/json"
                  />
                </label>
              </div>
              {draft.httpHeaderAssertions.length > 0 && (
                <div className="assertion-list">
                  <div className="assertion-list-header">
                    <span>Header</span>
                    <span>Equals</span>
                    <span>Contains</span>
                    <span />
                  </div>
                  {draft.httpHeaderAssertions.map((row, idx) => (
                    <div key={idx} className="assertion-list-row">
                      <input
                        placeholder="X-Custom-Header"
                        value={row.name}
                        onChange={(e) => {
                          const next = [...draft.httpHeaderAssertions];
                          next[idx] = { ...row, name: e.target.value };
                          setDraft({ ...draft, httpHeaderAssertions: next });
                        }}
                      />
                      <input
                        placeholder="exact value"
                        value={row.equals}
                        onChange={(e) => {
                          const next = [...draft.httpHeaderAssertions];
                          next[idx] = { ...row, equals: e.target.value };
                          setDraft({ ...draft, httpHeaderAssertions: next });
                        }}
                      />
                      <input
                        placeholder="partial value"
                        value={row.contains}
                        onChange={(e) => {
                          const next = [...draft.httpHeaderAssertions];
                          next[idx] = { ...row, contains: e.target.value };
                          setDraft({ ...draft, httpHeaderAssertions: next });
                        }}
                      />
                      <button
                        type="button"
                        className="assertion-remove-btn"
                        onClick={() => {
                          const next = draft.httpHeaderAssertions.filter((_, i) => i !== idx);
                          setDraft({ ...draft, httpHeaderAssertions: next });
                        }}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <button
                type="button"
                className="assertion-add-btn"
                onClick={() =>
                  setDraft({
                    ...draft,
                    httpHeaderAssertions: [
                      ...draft.httpHeaderAssertions,
                      { name: "", equals: "", contains: "" }
                    ]
                  })
                }
              >
                + Add header assertion
              </button>
            </div>

            {/* ── JSON Assertions ── */}
            <div className="form-section">
              <span className="form-section-title">JSON Assertions</span>
              <div className="site-dashboard-form-grid">
                <label className="form-field-wide">
                  <span>JSON paths must exist (one per line)</span>
                  <textarea
                    rows={3}
                    value={draft.httpJsonPathExists}
                    onChange={(e) => setDraft({ ...draft, httpJsonPathExists: e.target.value })}
                    placeholder={"$.status\n$.data.id\n..."}
                  />
                </label>
              </div>
              {draft.httpJsonPathEquals.length > 0 && (
                <div className="assertion-list" style={{ marginTop: "0.75rem" }}>
                  <div className="assertion-list-header assertion-list-header-two">
                    <span>Path must equal</span>
                    <span>Value (string or JSON)</span>
                    <span />
                  </div>
                  {draft.httpJsonPathEquals.map((row, idx) => (
                    <div key={idx} className="assertion-list-row assertion-list-row-two">
                      <input
                        placeholder="$.status"
                        value={row.path}
                        onChange={(e) => {
                          const next = [...draft.httpJsonPathEquals];
                          next[idx] = { ...row, path: e.target.value };
                          setDraft({ ...draft, httpJsonPathEquals: next });
                        }}
                      />
                      <input
                        placeholder='"ok" or 200 or true'
                        value={row.value}
                        onChange={(e) => {
                          const next = [...draft.httpJsonPathEquals];
                          next[idx] = { ...row, value: e.target.value };
                          setDraft({ ...draft, httpJsonPathEquals: next });
                        }}
                      />
                      <button
                        type="button"
                        className="assertion-remove-btn"
                        onClick={() => {
                          const next = draft.httpJsonPathEquals.filter((_, i) => i !== idx);
                          setDraft({ ...draft, httpJsonPathEquals: next });
                        }}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <button
                type="button"
                className="assertion-add-btn"
                onClick={() =>
                  setDraft({
                    ...draft,
                    httpJsonPathEquals: [...draft.httpJsonPathEquals, { path: "", value: "" }]
                  })
                }
              >
                + Add path equals assertion
              </button>
              {draft.httpJsonPathNotEquals.length > 0 && (
                <div className="assertion-list" style={{ marginTop: "0.75rem" }}>
                  <div className="assertion-list-header assertion-list-header-two">
                    <span>Path must NOT equal</span>
                    <span>Value (string or JSON)</span>
                    <span />
                  </div>
                  {draft.httpJsonPathNotEquals.map((row, idx) => (
                    <div key={idx} className="assertion-list-row assertion-list-row-two">
                      <input
                        placeholder="$.status"
                        value={row.path}
                        onChange={(e) => {
                          const next = [...draft.httpJsonPathNotEquals];
                          next[idx] = { ...row, path: e.target.value };
                          setDraft({ ...draft, httpJsonPathNotEquals: next });
                        }}
                      />
                      <input
                        placeholder='"error" or 500'
                        value={row.value}
                        onChange={(e) => {
                          const next = [...draft.httpJsonPathNotEquals];
                          next[idx] = { ...row, value: e.target.value };
                          setDraft({ ...draft, httpJsonPathNotEquals: next });
                        }}
                      />
                      <button
                        type="button"
                        className="assertion-remove-btn"
                        onClick={() => {
                          const next = draft.httpJsonPathNotEquals.filter((_, i) => i !== idx);
                          setDraft({ ...draft, httpJsonPathNotEquals: next });
                        }}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <button
                type="button"
                className="assertion-add-btn"
                onClick={() =>
                  setDraft({
                    ...draft,
                    httpJsonPathNotEquals: [...draft.httpJsonPathNotEquals, { path: "", value: "" }]
                  })
                }
              >
                + Add path not-equals assertion
              </button>
            </div>

            {/* ── Advanced ── */}
            <details className="form-section">
              <summary className="form-section-title">Advanced</summary>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Timeout (seconds)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.httpTimeoutSecondsOverride}
                    onChange={(e) => setDraft({ ...draft, httpTimeoutSecondsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label>
                  <span>Max attempts</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.httpMaxAttemptsOverride}
                    onChange={(e) => setDraft({ ...draft, httpMaxAttemptsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label className="form-field-wide">
                  <span>Retry delays ms (comma-separated)</span>
                  <input
                    value={draft.httpRetryDelaysMsOverride}
                    onChange={(e) => setDraft({ ...draft, httpRetryDelaysMsOverride: e.target.value })}
                    placeholder="1000, 2000, 5000"
                  />
                </label>
              </div>
            </details>
          </div>
        );
      case "ssl":
        return (
          <div className="http-monitor-form">
            <div className="form-section">
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Target URL</span>
                  <input
                    value={draft.sslTargetUrl}
                    onChange={(e) => setDraft({ ...draft, sslTargetUrl: e.target.value })}
                    placeholder="https://example.com"
                  />
                </label>
                <label>
                  <span>Interval (seconds)</span>
                  <input
                    type="number"
                    min={30}
                    value={draft.sslInterval}
                    onChange={(e) => setDraft({ ...draft, sslInterval: e.target.value })}
                  />
                </label>
                <label>
                  <span>Warning days</span>
                  <input
                    type="number"
                    min={8}
                    value={draft.sslWarningDays}
                    onChange={(e) => setDraft({ ...draft, sslWarningDays: e.target.value })}
                  />
                </label>
              </div>
            </div>
            <details className="form-section">
              <summary className="form-section-title">Advanced</summary>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Timeout (seconds)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.sslTimeoutSecondsOverride}
                    onChange={(e) => setDraft({ ...draft, sslTimeoutSecondsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label>
                  <span>Max attempts</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.sslMaxAttemptsOverride}
                    onChange={(e) => setDraft({ ...draft, sslMaxAttemptsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label className="form-field-wide">
                  <span>Retry delays ms (comma-separated)</span>
                  <input
                    value={draft.sslRetryDelaysMsOverride}
                    onChange={(e) => setDraft({ ...draft, sslRetryDelaysMsOverride: e.target.value })}
                    placeholder="1000, 2000, 5000"
                  />
                </label>
              </div>
            </details>
          </div>
        );
      case "heartbeat":
        return (
          <div className="site-dashboard-form-grid">
            <label>
              <span>Interval (seconds)</span>
              <input
                type="number"
                min={30}
                value={draft.heartbeatInterval}
                onChange={(e) => setDraft({ ...draft, heartbeatInterval: e.target.value })}
              />
            </label>
            <label>
              <span>Grace window (seconds)</span>
              <input
                type="number"
                min={0}
                value={draft.heartbeatGraceSeconds}
                onChange={(e) => setDraft({ ...draft, heartbeatGraceSeconds: e.target.value })}
              />
            </label>
          </div>
        );
      case "tcp":
        return (
          <div className="http-monitor-form">
            <div className="form-section">
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Target host</span>
                  <input
                    value={draft.tcpTargetHost}
                    onChange={(e) => setDraft({ ...draft, tcpTargetHost: e.target.value })}
                    placeholder="example.com"
                  />
                </label>
                <label>
                  <span>Port</span>
                  <input
                    type="number"
                    min={1}
                    max={65535}
                    value={draft.tcpTargetPort}
                    onChange={(e) => setDraft({ ...draft, tcpTargetPort: e.target.value })}
                  />
                </label>
                <label>
                  <span>Interval (seconds)</span>
                  <input
                    type="number"
                    min={30}
                    value={draft.tcpInterval}
                    onChange={(e) => setDraft({ ...draft, tcpInterval: e.target.value })}
                  />
                </label>
                <label>
                  <span>Max connect time (ms)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.tcpMaxConnectTimeMs}
                    onChange={(e) => setDraft({ ...draft, tcpMaxConnectTimeMs: e.target.value })}
                    placeholder="Optional"
                  />
                </label>
              </div>
            </div>
            <details className="form-section">
              <summary className="form-section-title">Advanced</summary>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Timeout (seconds)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.tcpTimeoutSecondsOverride}
                    onChange={(e) => setDraft({ ...draft, tcpTimeoutSecondsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label>
                  <span>Max attempts</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.tcpMaxAttemptsOverride}
                    onChange={(e) => setDraft({ ...draft, tcpMaxAttemptsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label className="form-field-wide">
                  <span>Retry delays ms (comma-separated)</span>
                  <input
                    value={draft.tcpRetryDelaysMsOverride}
                    onChange={(e) => setDraft({ ...draft, tcpRetryDelaysMsOverride: e.target.value })}
                    placeholder="1000, 2000, 5000"
                  />
                </label>
              </div>
            </details>
          </div>
        );
      case "dns":
        return (
          <div className="http-monitor-form">
            <div className="form-section">
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Hostname</span>
                  <input
                    value={draft.dnsHostname}
                    onChange={(e) => setDraft({ ...draft, dnsHostname: e.target.value })}
                    placeholder="example.com"
                  />
                </label>
                <label>
                  <span>Record type</span>
                  <select
                    value={draft.dnsRecordType}
                    onChange={(e) => setDraft({ ...draft, dnsRecordType: e.target.value })}
                  >
                    {["A", "AAAA", "CNAME", "MX", "TXT", "NS"].map((v) => (
                      <option key={v} value={v}>{v}</option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>Expected value</span>
                  <input
                    value={draft.dnsExpectedValue}
                    onChange={(e) => setDraft({ ...draft, dnsExpectedValue: e.target.value })}
                    placeholder="Optional"
                  />
                </label>
                <label>
                  <span>Nameserver</span>
                  <input
                    value={draft.dnsNameserver}
                    onChange={(e) => setDraft({ ...draft, dnsNameserver: e.target.value })}
                    placeholder="8.8.8.8"
                  />
                </label>
                <label>
                  <span>Interval (seconds)</span>
                  <input
                    type="number"
                    min={30}
                    value={draft.dnsInterval}
                    onChange={(e) => setDraft({ ...draft, dnsInterval: e.target.value })}
                  />
                </label>
              </div>
            </div>
            <details className="form-section">
              <summary className="form-section-title">Advanced</summary>
              <div className="site-dashboard-form-grid">
                <label>
                  <span>Timeout (seconds)</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.dnsTimeoutSecondsOverride}
                    onChange={(e) => setDraft({ ...draft, dnsTimeoutSecondsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label>
                  <span>Max attempts</span>
                  <input
                    type="number"
                    min={1}
                    value={draft.dnsMaxAttemptsOverride}
                    onChange={(e) => setDraft({ ...draft, dnsMaxAttemptsOverride: e.target.value })}
                    placeholder="Default"
                  />
                </label>
                <label className="form-field-wide">
                  <span>Retry delays ms (comma-separated)</span>
                  <input
                    value={draft.dnsRetryDelaysMsOverride}
                    onChange={(e) => setDraft({ ...draft, dnsRetryDelaysMsOverride: e.target.value })}
                    placeholder="1000, 2000, 5000"
                  />
                </label>
              </div>
            </details>
          </div>
        );
    }
  }

  // --- Notifications tab render ---

  function renderNotificationsTab() {
    if (isLoadingNotifications && siteChannels.length === 0) {
      return <div className="panel workspace-banner">Loading notifications...</div>;
    }
    if (notificationsError) {
      return <div className="error-banner workspace-banner">{notificationsError}</div>;
    }

    return (
      <div className="management-stack">
        <div className="panel">
          <div className="management-list-header">
            <div>
              <strong>Alert Channels</strong>
              <p style={{ margin: "2px 0 0", fontSize: "0.8rem", color: "var(--color-text-muted)" }}>
                Per-site overrides for global notification channels. Toggle to customise delivery for this site.
              </p>
            </div>
          </div>
          {siteChannels.length === 0 ? (
            <div className="empty-state">
              No notification channels configured. Add global channels in the Notifications section.
            </div>
          ) : (
            <div className="table-wrap management-table">
              <table>
                <thead>
                  <tr>
                    <th>Channel</th>
                    <th>Type</th>
                    <th>Failure</th>
                    <th>Recovery</th>
                    <th>Active</th>
                    <th>Override</th>
                  </tr>
                </thead>
                <tbody>
                  {siteChannels.map((ch) => (
                    <tr key={ch.id}>
                      <td>
                        <div className="table-primary">{ch.name}</div>
                        <div className="table-secondary">{ch.destination}</div>
                      </td>
                      <td>
                        <span className={`tag ${ch.channel_type === "webhook" ? "tag-active" : "tag-disabled"}`}>
                          {ch.channel_type}
                        </span>
                      </td>
                      <td>
                        {canWriteSiteNotificationOverrides ? (
                          <button
                            className={`tag ${ch.effective_notify_on_failure ? "tag-active" : "tag-disabled"}`}
                            type="button"
                            disabled={togglingChannelId === ch.id}
                            onClick={() => void handleToggleSiteChannelOverride(ch, "notify_on_failure")}
                            style={{ cursor: "pointer", border: "none", background: "none" }}
                          >
                            {ch.effective_notify_on_failure ? "on" : "off"}
                          </button>
                        ) : (
                          <span className={`tag ${ch.effective_notify_on_failure ? "tag-active" : "tag-disabled"}`}>
                            {ch.effective_notify_on_failure ? "on" : "off"}
                          </span>
                        )}
                      </td>
                      <td>
                        {canWriteSiteNotificationOverrides ? (
                          <button
                            className={`tag ${ch.effective_notify_on_recovery ? "tag-active" : "tag-disabled"}`}
                            type="button"
                            disabled={togglingChannelId === ch.id}
                            onClick={() => void handleToggleSiteChannelOverride(ch, "notify_on_recovery")}
                            style={{ cursor: "pointer", border: "none", background: "none" }}
                          >
                            {ch.effective_notify_on_recovery ? "on" : "off"}
                          </button>
                        ) : (
                          <span className={`tag ${ch.effective_notify_on_recovery ? "tag-active" : "tag-disabled"}`}>
                            {ch.effective_notify_on_recovery ? "on" : "off"}
                          </span>
                        )}
                      </td>
                      <td>
                        {canWriteSiteNotificationOverrides ? (
                          <button
                            className={`tag ${ch.effective_is_active ? "tag-active" : "tag-disabled"}`}
                            type="button"
                            disabled={togglingChannelId === ch.id}
                            onClick={() => void handleToggleSiteChannelOverride(ch, "is_active")}
                            style={{ cursor: "pointer", border: "none", background: "none" }}
                          >
                            {ch.effective_is_active ? "active" : "disabled"}
                          </button>
                        ) : (
                          <span className={`tag ${ch.effective_is_active ? "tag-active" : "tag-disabled"}`}>
                            {ch.effective_is_active ? "active" : "disabled"}
                          </span>
                        )}
                      </td>
                      <td className="table-actions">
                        {ch.override_id !== null ? (
                          canWriteSiteNotificationOverrides ? (
                            <button
                              className="ghost-button"
                              type="button"
                              disabled={togglingChannelId === ch.id}
                              onClick={() => void handleResetSiteChannelOverride(ch.id)}
                            >
                              Reset to default
                            </button>
                          ) : (
                            <span className="table-secondary">overridden</span>
                          )
                        ) : (
                          <span className="table-secondary">default</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div className="panel">
          <div className="management-list-header">
            <strong>Delivery History</strong>
            {deliveriesNextCursor && (
              <button
                className="ghost-button"
                type="button"
                disabled={isLoadingNotifications}
                onClick={() => void loadSiteNotifications(false)}
              >
                {isLoadingNotifications ? "Loading..." : "Load more"}
              </button>
            )}
          </div>
          {deliveries.length === 0 ? (
            <div className="empty-state">No notification deliveries recorded for this site yet.</div>
          ) : (
            <div className="table-wrap management-table">
              <table>
                <thead>
                  <tr>
                    <th>Time</th>
                    <th>Channel</th>
                    <th>Event</th>
                    <th>Status</th>
                    <th>Attempts</th>
                    <th>Error</th>
                  </tr>
                </thead>
                <tbody>
                  {deliveries.map((d) => (
                    <tr key={d.id}>
                      <td>{formatTimestamp(d.created_at)}</td>
                      <td>
                        <div className="table-primary">{d.channel_name}</div>
                        <div className="table-secondary">{d.channel_type}</div>
                      </td>
                      <td>
                        <span className={`tag ${d.event_type === "failure" ? "tag-danger" : "tag-active"}`}>
                          {d.event_type}
                        </span>
                      </td>
                      <td>
                        <span
                          className={`tag ${
                            d.status === "delivered"
                              ? "tag-active"
                              : d.status === "failed"
                                ? "tag-danger"
                                : "tag-disabled"
                          }`}
                        >
                          {d.status}
                        </span>
                      </td>
                      <td>{d.attempts}</td>
                      <td>
                        {d.last_error ? (
                          <span className="table-secondary" title={d.last_error}>
                            {d.last_error.length > 60 ? `${d.last_error.slice(0, 60)}…` : d.last_error}
                          </span>
                        ) : (
                          <span className="table-secondary">—</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    );
  }

  function renderStatusPageTab() {
    const slugifiedName = site.name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");

    if (isLoadingStatusPage) {
      return <div className="panel workspace-banner">Loading status page config…</div>;
    }
    if (statusPageError) {
      return <div className="error-banner workspace-banner">{statusPageError}</div>;
    }

    const effectiveSlug = spSlug || (statusPageConfig === null ? slugifiedName : "");
    const publicUrl = `${window.location.origin}${window.location.pathname}#/status/${effectiveSlug}`;

    return (
      <div className="site-dashboard-tab-content">
        <article className="panel subpanel">
          <div className="panel-header">
            <div>
              <div className="panel-kicker">Public Status Page</div>
              <h2>Status Page Settings</h2>
            </div>
          </div>

          <div className="form-section">
            <span className="form-section-title">Visibility</span>
            <div className="site-dashboard-form-grid">
              <label className="form-field-checkbox">
                <input
                  type="checkbox"
                  checked={spEnabled}
                  onChange={(e) => {
                    setSpEnabled(e.target.checked);
                    setStatusPageSaveSuccess(false);
                  }}
                />
                <span>Enable public status page</span>
              </label>
            </div>
          </div>

          <div className="form-section">
            <span className="form-section-title">URL &amp; Identity</span>
            <div className="site-dashboard-form-grid">
              <label className="form-field-wide">
                <span>URL slug</span>
                <input
                  type="text"
                  value={effectiveSlug}
                  placeholder={slugifiedName}
                  onChange={(e) => {
                    setSpSlug(e.target.value);
                    setStatusPageSaveSuccess(false);
                  }}
                />
                <small style={{ color: "var(--muted)", marginTop: "0.25rem", display: "block" }}>
                  Lowercase letters, digits, and hyphens only. 3–60 characters.
                </small>
              </label>
              <label className="form-field-wide">
                <span>Page title <small style={{ color: "var(--muted)" }}>(optional, defaults to site name)</small></span>
                <input
                  type="text"
                  value={spTitle}
                  placeholder={site.name}
                  onChange={(e) => {
                    setSpTitle(e.target.value);
                    setStatusPageSaveSuccess(false);
                  }}
                />
              </label>
            </div>
          </div>

          <div className="form-section">
            <span className="form-section-title">Content</span>
            <div className="site-dashboard-form-grid">
              <label className="form-field-checkbox">
                <input
                  type="checkbox"
                  checked={spShowMonitors}
                  onChange={(e) => {
                    setSpShowMonitors(e.target.checked);
                    setStatusPageSaveSuccess(false);
                  }}
                />
                <span>Show individual monitor statuses</span>
              </label>
              <label className="form-field-checkbox">
                <input
                  type="checkbox"
                  checked={spShowUptime}
                  onChange={(e) => {
                    setSpShowUptime(e.target.checked);
                    setStatusPageSaveSuccess(false);
                  }}
                />
                <span>Show uptime percentages</span>
              </label>
            </div>
          </div>

          <div className="form-section" style={{ display: "flex", gap: "0.75rem", alignItems: "center", flexWrap: "wrap" }}>
            {canUpdateSite && (
              <button
                type="button"
                className="primary-button"
                onClick={() => void handleSaveStatusPage()}
                disabled={isSavingStatusPage}
              >
                {isSavingStatusPage ? "Saving…" : "Save"}
              </button>
            )}
            {statusPageSaveSuccess && spEnabled && (
              <a
                href={publicUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="ghost-button"
              >
                View status page →
              </a>
            )}
          </div>

          {statusPageSaveError && (
            <div className="inline-alert inline-alert-danger" style={{ marginTop: "0.5rem" }}>
              {statusPageSaveError}
            </div>
          )}
          {statusPageSaveSuccess && (
            <div className="inline-alert" style={{ marginTop: "0.5rem", color: "var(--accent)" }}>
              Saved. {spEnabled ? <>Public URL: <a href={publicUrl} target="_blank" rel="noopener noreferrer">{publicUrl}</a></> : "Status page is disabled."}
            </div>
          )}
        </article>
      </div>
    );
  }

  // --- Main render ---

  const tabs: { id: DashboardTab; label: string }[] = [
    { id: "overview", label: "Overview" },
    { id: "monitors", label: "Monitors" },
    { id: "history", label: "History" },
    { id: "incidents", label: "Incidents" },
    { id: "notifications", label: "Notifications" },
    { id: "status-page", label: "Status Page" },
  ];
  const draftValidationErrors = draft ? validateConfigureDraft(draft) : [];

  return (
    <section className="page-panel site-dashboard">
      <nav className="site-dashboard-tabs" aria-label="Dashboard sections">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`site-dashboard-tab${activeTab === tab.id ? " active" : ""}`}
            onClick={() => handleTabChange(tab.id)}
            type="button"
            aria-current={activeTab === tab.id ? "page" : undefined}
          >
            {tab.label}
            {tab.id === "incidents" && incident_open && (
              <span className="tab-alert-dot" aria-hidden="true" />
            )}
          </button>
        ))}
      </nav>

      {activeTab === "overview" && (
        <OverviewTab
          summary={summary}
          inventory={inventory}
          uptime7d={uptime7d}
          uptime30d={uptime30d}
          isMutating={isMutating}
          session={session}
          canWrite={canWriteMonitors}
          onConfigureMonitor={openConfigureMonitor}
          onTabChange={handleTabChange}
        />
      )}
      {activeTab === "monitors" && (
        <MonitorsTab
          inventory={inventory}
          isMutating={isMutating}
          monitorActionError={monitorActionError}
          session={session}
          siteId={site.id}
          canWrite={canWriteMonitors}
          onConfigureMonitor={openConfigureMonitor}
          onPauseMonitor={(type, monitorId) => void handlePauseMonitor(type, monitorId)}
          onResumeMonitor={(type, monitorId) => void handleResumeMonitor(type, monitorId)}
          onDeleteMonitor={(type, monitorId) => void handleDeleteMonitor(type, monitorId)}
        />
      )}
      {activeTab === "history" && (
        <HistoryTab
          siteId={site.id}
          session={session}
        />
      )}
      {activeTab === "incidents" && (
        <IncidentsTab
          siteId={site.id}
          session={session}
        />
      )}
      {activeTab === "notifications" && renderNotificationsTab()}
      {activeTab === "status-page" && renderStatusPageTab()}

      {draft && (
        <div className="modal-backdrop" role="presentation" onClick={closeConfigureMonitor}>
          <article
            className="modal-panel modal-panel-wide panel"
            role="dialog"
            aria-modal="true"
            aria-label={`Configure ${draft.type.toUpperCase()} monitor`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="panel-header">
              <div>
                <div className="panel-kicker">Configure Monitor</div>
                <h2>{draft.type.toUpperCase()} Monitor</h2>
                <p>Fill in the fields below. The monitor will be active immediately after saving.</p>
              </div>
            </div>
            <form
              className="site-dashboard-config-form"
              onSubmit={(e) => void handleSubmitConfigureMonitor(e)}
            >
              {renderConfigureFields()}
              {draftValidationErrors.length > 0 && (
                <div className="inline-alert inline-alert-danger">
                  {draftValidationErrors[0]}
                </div>
              )}
              {monitorActionError && (
                <div className="inline-alert inline-alert-danger">{monitorActionError}</div>
              )}
              <div className="inline-actions">
                <button
                  className="primary-button"
                  disabled={isMutating || draftValidationErrors.length > 0}
                  type="submit"
                >
                  {isMutating ? "Saving…" : "Save Monitor"}
                </button>
                <button
                  className="ghost-button"
                  disabled={isMutating}
                  onClick={closeConfigureMonitor}
                  type="button"
                >
                  Cancel
                </button>
              </div>
            </form>
          </article>
        </div>
      )}
    </section>
  );
}
