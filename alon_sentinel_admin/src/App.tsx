import { useEffect, useRef, useState } from "react";
import { Navigate, Route, Routes, useLocation, useMatch, useNavigate } from "react-router-dom";
import {
  type AuthSession,
  getAdminSession,
  issueAdminToken,
  revokeAdminToken,
} from "./api";
import { AccessManagement } from "./components/AccessManagement";
import { ApiClientsManagement } from "./components/ApiClientsManagement";
import { AppSidebar, type AppView } from "./components/AppSidebar";
import { DashboardPlaceholder } from "./components/DashboardPlaceholder";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { LoginScreen } from "./components/LoginScreen";
import { NotificationsManagement } from "./components/NotificationsManagement";
import { SiteDashboard } from "./components/SiteDashboard";
import { SitesTable } from "./components/SitesTable";
import { StatusPage } from "./components/StatusPage";
import { useAccessData } from "./hooks/useAccessData";
import { useApiClients } from "./hooks/useApiClients";
import { useAuth } from "./hooks/useAuth";
import { useGlobalDashboard } from "./hooks/useGlobalDashboard";
import { useNotifications } from "./hooks/useNotifications";
import { useSiteDashboard } from "./hooks/useSiteDashboard";
import { useSites } from "./hooks/useSites";
import { formatRefreshTimestamp, formatTimestamp } from "./utils";

const AUTO_REFRESH_INTERVAL_MS = 15000;

type BreadcrumbItem = { label: string; onClick?: () => void };

export default function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const siteDashboardMatch = useMatch("/sites/:siteId");
  const siteDashboardRouteId = siteDashboardMatch
    ? Number.parseInt(siteDashboardMatch.params.siteId ?? "", 10)
    : null;

  // --- auth primitives ---
  const auth = useAuth();
  const { session, connected, permissionSet } = auth;

  // --- permissions ---
  const canReadSites = permissionSet.has("sites.read");
  const canCreateSites = permissionSet.has("sites.create");
  const canUpdateSites = permissionSet.has("sites.update");
  const canDeleteSites = permissionSet.has("sites.delete");
  const canReadUsers = permissionSet.has("users.read");
  const canWriteUsers = permissionSet.has("users.write");
  const canReadRoles = permissionSet.has("roles.read");
  const canWriteRoles = permissionSet.has("roles.write");
  const canAccessView = canReadUsers || canReadRoles;
  const canReadNotificationChannels = permissionSet.has("notification_channels.read");
  const canWriteNotificationChannels =
    permissionSet.has("notification_channels.create") ||
    permissionSet.has("notification_channels.update") ||
    permissionSet.has("notification_channels.delete");
  const canReadApiClients = permissionSet.has("api_clients.read");
  const canWriteApiClients = permissionSet.has("api_clients.write");
  const canWriteMonitors =
    permissionSet.has("site_monitors.create") ||
    permissionSet.has("site_monitors.update") ||
    permissionSet.has("site_monitors.delete");
  const canWriteSiteNotificationOverrides =
    permissionSet.has("site_notification_channel_overrides.create") ||
    permissionSet.has("site_notification_channel_overrides.update") ||
    permissionSet.has("site_notification_channel_overrides.delete");

  // --- domain hooks ---
  const sites = useSites(auth.withApi, auth.createRefreshSignal);
  const accessData = useAccessData(auth.withApi, auth.createRefreshSignal, canReadUsers, canReadRoles);
  const siteDashboard = useSiteDashboard(auth.withApi, auth.createRefreshSignal);
  const globalDashboard = useGlobalDashboard(auth.withApi, auth.createRefreshSignal);
  const notifications = useNotifications(auth.withApi, auth.createRefreshSignal, canReadNotificationChannels);
  const apiClients = useApiClients(auth.withApi, auth.createRefreshSignal, canReadApiClients);

  // --- login form (local to the login screen) ---
  const [isConnecting, setIsConnecting] = useState(false);
  const [loginEmail, setLoginEmail] = useState("");
  const [loginPassword, setLoginPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);

  const activeView: AppView = location.pathname.startsWith("/access")
    ? "access"
    : location.pathname.startsWith("/notifications")
      ? "notifications"
      : location.pathname.startsWith("/api-clients")
        ? "api-clients"
        : location.pathname.startsWith("/dashboard")
          ? "dashboard"
          : "sites";

  const canSubmitLogin = loginEmail.trim().length > 0 && loginPassword.length > 0;
  const sessionExpiresLabel = formatTimestamp(session?.expiresAt);
  const isSiteDashboard =
    activeView === "sites" &&
    Number.isInteger(siteDashboardRouteId) &&
    siteDashboardRouteId !== null &&
    siteDashboard.selectedSiteSummary !== null;

  // --- session lifecycle ---

  function clearSessionState() {
    auth.setSession(null);
    sites.reset();
    accessData.reset();
    siteDashboard.reset();
    notifications.reset();
    apiClients.reset();
    setLoginPassword("");
    setShowPassword(false);
    navigate("/dashboard", { replace: true });
  }

  async function bootstrapAdmin(forcedSession?: AuthSession) {
    const activeSession = forcedSession ?? session;
    if (!activeSession) return;

    try {
      const adminSession = await getAdminSession();
      const normalizedSession: AuthSession = {
        ...activeSession,
        roles: adminSession.roles,
        permissions: adminSession.permissions,
        user: adminSession.user,
      };
      auth.setSession(normalizedSession);
      await sites.refreshSites({ activeSession: normalizedSession, skipLoadingState: true });
      auth.setAppError((current) => (current?.scope === "connection" ? null : current));
    } catch (error) {
      const message = error instanceof Error ? error.message : "Failed to load admin session.";
      if (message.includes("Log in again")) clearSessionState();
      auth.setAppError({ scope: "connection", message });
    }
  }

  async function connect() {
    const trimmedEmail = loginEmail.trim();
    if (!trimmedEmail || !loginPassword) {
      auth.setAppError({ scope: "connection", message: "Enter the admin email and password." });
      return;
    }

    setIsConnecting(true);
    auth.setAppError(null);
    try {
      const nextSession = await issueAdminToken(trimmedEmail, loginPassword);
      auth.setSession(nextSession);
      setLoginEmail(trimmedEmail);
      setLoginPassword("");
      setShowPassword(false);
      await bootstrapAdmin(nextSession);
      navigate("/dashboard", { replace: true });
    } catch (error) {
      auth.setAppError({
        scope: "connection",
        message: error instanceof Error ? error.message : "Failed to connect.",
      });
    } finally {
      setIsConnecting(false);
    }
  }

  async function disconnect() {
    try {
      if (session) await revokeAdminToken();
    } finally {
      clearSessionState();
    }
  }

  // --- cross-hook site operations (add navigation/selectedSite sync on top of domain handlers) ---

  async function handleCreateSite(payload: { name: string; base_url: string }) {
    if (!payload.name.trim() || !payload.base_url.trim()) {
      auth.setAppError({ scope: "sites", message: "Site name and base URL are required." });
      return false;
    }
    return sites.handleCreateSite(payload);
  }

  async function handleUpdateSite(
    siteId: number,
    payload: { name: string; base_url: string; is_active: boolean }
  ) {
    if (!payload.name.trim() || !payload.base_url.trim()) {
      auth.setAppError({ scope: "sites", message: "Site name and base URL are required." });
      return false;
    }
    const updatedSite = await sites.handleUpdateSite(siteId, payload);
    if (updatedSite && siteDashboard.selectedSite?.id === siteId) {
      siteDashboard.setSelectedSite(updatedSite);
    }
    return Boolean(updatedSite);
  }

  async function handleDeleteSite(siteId: number) {
    const deleted = await sites.handleDeleteSite(siteId);
    if (deleted && siteDashboard.selectedSite?.id === siteId) {
      siteDashboard.reset();
      navigate("/sites", { replace: true });
    }
    return deleted;
  }

  // --- navigation ---

  function handleOpenSiteDashboard(site: import("./api").Site) {
    siteDashboard.reset();
    siteDashboard.setSelectedSite(site);
    navigate(`/sites/${site.id}`);
  }

  function handleNavigate(view: AppView) {
    if (view === "dashboard") navigate("/dashboard");
    else if (view === "access") navigate("/access");
    else if (view === "notifications") navigate("/notifications");
    else if (view === "api-clients") navigate("/api-clients");
    else navigate("/sites");
    siteDashboard.reset();
  }

  // --- auto-refresh ---

  const refreshForCurrentViewRef = useRef<(opts?: { silent?: boolean }) => Promise<void>>(async () => {});

  async function refreshForCurrentView(opts?: { silent?: boolean }) {
    if (activeView === "access") {
      if (!canAccessView || accessData.isRefreshingAccess) return;
      await accessData.refreshAccessData();
      return;
    }
    if (activeView === "notifications") {
      if (!canReadNotificationChannels || notifications.isRefreshing) return;
      await notifications.refreshChannels();
      return;
    }
    if (activeView === "api-clients") {
      if (!canReadApiClients || apiClients.isRefreshing) return;
      await apiClients.refreshClients();
      return;
    }
    if (activeView === "dashboard") {
      if (!canReadSites || globalDashboard.isRefreshingHomeDashboard) return;
      await globalDashboard.refreshGlobalDashboard();
      return;
    }
    if (activeView === "sites" && isSiteDashboard && siteDashboard.selectedSite) {
      if (siteDashboard.isRefreshingDashboard) return;
      await siteDashboard.refreshSiteDashboard(siteDashboard.selectedSite.id, { silent: opts?.silent });
      return;
    }
    if (sites.isRefreshingSites) return;
    await sites.refreshSites();
  }

  useEffect(() => {
    refreshForCurrentViewRef.current = refreshForCurrentView;
  });

  useEffect(() => {
    if (!connected) return;
    void bootstrapAdmin();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!connected) return;
    void refreshForCurrentViewRef.current();
    const interval = window.setInterval(() => {
      if (!document.hidden) void refreshForCurrentViewRef.current({ silent: true });
    }, AUTO_REFRESH_INTERVAL_MS);
    function onVisibilityChange() {
      if (!document.hidden) void refreshForCurrentViewRef.current({ silent: true });
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [activeView, canAccessView, canReadApiClients, canReadNotificationChannels, canReadSites, connected, isSiteDashboard, siteDashboard.selectedSite?.id]);

  useEffect(() => {
    if (!connected || !siteDashboardRouteId || Number.isNaN(siteDashboardRouteId)) return;
    if (siteDashboard.selectedSiteSummary?.site.id === siteDashboardRouteId) return;

    const knownSite = sites.sites.find((s) => s.id === siteDashboardRouteId);
    if (knownSite) siteDashboard.setSelectedSite(knownSite);

    void siteDashboard.refreshSiteDashboard(siteDashboardRouteId);
  }, [connected, siteDashboard.selectedSiteSummary?.site.id, siteDashboardRouteId, sites.sites]);

  // --- derived layout ---

  const breadcrumbs: BreadcrumbItem[] = isSiteDashboard
    ? [
        { label: "Sentinel", onClick: () => handleNavigate("dashboard") },
        { label: "Sites", onClick: () => handleNavigate("sites") },
        { label: siteDashboard.selectedSite?.name ?? "Site" },
        { label: "Dashboard" },
      ]
    : activeView === "dashboard"
      ? [{ label: "Sentinel" }, { label: "Dashboard" }]
      : activeView === "notifications"
        ? [{ label: "Sentinel", onClick: () => handleNavigate("dashboard") }, { label: "Notifications" }]
        : activeView === "api-clients"
          ? [{ label: "Sentinel", onClick: () => handleNavigate("dashboard") }, { label: "API Clients" }]
          : activeView === "access"
            ? [{ label: "Sentinel", onClick: () => handleNavigate("dashboard") }, { label: "Access" }]
            : [{ label: "Sentinel", onClick: () => handleNavigate("dashboard") }, { label: "Sites" }];

  const isRefreshingCurrent = activeView === "access"
    ? accessData.isRefreshingAccess
    : activeView === "notifications"
      ? notifications.isRefreshing
      : activeView === "api-clients"
        ? apiClients.isRefreshing
        : activeView === "dashboard"
          ? globalDashboard.isRefreshingHomeDashboard
          : isSiteDashboard
            ? siteDashboard.isRefreshingDashboard
            : sites.isRefreshingSites;

  const lastSyncLabel = activeView === "access"
    ? formatRefreshTimestamp(accessData.lastAccessRefreshAt)
    : activeView === "notifications"
      ? formatRefreshTimestamp(notifications.lastRefreshAt)
      : activeView === "api-clients"
        ? formatRefreshTimestamp(apiClients.lastRefreshAt)
        : activeView === "dashboard"
          ? formatRefreshTimestamp(globalDashboard.lastHomeDashboardRefreshAt)
          : isSiteDashboard
            ? formatRefreshTimestamp(siteDashboard.lastSiteDashboardRefreshAt)
            : formatRefreshTimestamp(sites.lastSitesRefreshAt);

  const environmentLabel = import.meta.env.MODE === "production" ? "Production" : "Local";
  const regionLabel = Intl.DateTimeFormat().resolvedOptions().timeZone || "Local";
  const globalStats = globalDashboard.dashboardStats;
  const siteSummary = siteDashboard.selectedSiteSummary;
  const siteInventory = siteDashboard.selectedSiteMonitorInventory;
  const siteMonitors = siteInventory
    ? [...siteInventory.http, ...siteInventory.ssl, ...siteInventory.heartbeat, ...siteInventory.tcp, ...siteInventory.dns]
    : [];
  const siteActiveMonitors = siteMonitors.filter((monitor) => monitor.is_active).length;
  const globalHealthPercent = globalStats && globalStats.sites_with_monitor > 0
    ? Math.round((globalStats.sites_up / globalStats.sites_with_monitor) * 100)
    : null;
  const siteSuccessRate = siteSummary?.recent_checks.success_rate ?? null;
  const headerMetrics = isSiteDashboard
    ? [
        { label: "Environment", value: environmentLabel, tone: "neutral" },
        { label: "Region", value: regionLabel, tone: "muted" },
        { label: "Uptime", value: siteSuccessRate !== null ? `${siteSuccessRate.toFixed(1)}%` : "Pending", tone: siteSuccessRate !== null && siteSuccessRate >= 99 ? "success" : siteSuccessRate !== null && siteSuccessRate < 95 ? "danger" : "neutral" },
        { label: "Monitors", value: `${siteActiveMonitors}/${siteMonitors.length || 0} active`, tone: siteActiveMonitors > 0 ? "success" : "muted" },
        { label: "Incidents", value: siteSummary?.incident_open ? "Active" : "Clear", tone: siteSummary?.incident_open ? "danger" : "success" },
        { label: "Auto-refresh", value: `${AUTO_REFRESH_INTERVAL_MS / 1000}s · ${isRefreshingCurrent ? "syncing" : lastSyncLabel}`, tone: isRefreshingCurrent ? "neutral" : "muted" },
      ]
    : [
        { label: "Environment", value: environmentLabel, tone: "neutral" },
        { label: "Region", value: regionLabel, tone: "muted" },
        { label: "Fleet health", value: globalHealthPercent !== null ? `${globalHealthPercent}%` : "Pending", tone: globalHealthPercent !== null && globalHealthPercent >= 99 ? "success" : globalHealthPercent !== null && globalHealthPercent < 95 ? "danger" : "neutral" },
        { label: "Monitors", value: globalStats ? `${globalStats.monitors_active} active` : "Loading", tone: globalStats && globalStats.monitors_active > 0 ? "success" : "muted" },
        { label: "Incidents", value: `${globalStats?.open_incidents ?? 0} open`, tone: (globalStats?.open_incidents ?? 0) > 0 ? "danger" : "success" },
        { label: "Auto-refresh", value: `${AUTO_REFRESH_INTERVAL_MS / 1000}s · ${isRefreshingCurrent ? "syncing" : lastSyncLabel}`, tone: isRefreshingCurrent ? "neutral" : "muted" },
      ];

  function handleRefreshCurrent() {
    if (activeView === "access") { void accessData.refreshAccessData(); return; }
    if (activeView === "notifications") { void notifications.refreshChannels(); return; }
    if (activeView === "api-clients") { void apiClients.refreshClients(); return; }
    if (activeView === "dashboard") { void globalDashboard.refreshGlobalDashboard(); return; }
    if (isSiteDashboard && siteDashboard.selectedSite) {
      void siteDashboard.refreshSiteDashboard(siteDashboard.selectedSite.id);
      return;
    }
    void sites.refreshSites();
  }

  // --- render ---

  const statusSlugMatch = useMatch("/status/:slug");
  if (statusSlugMatch) {
    return <StatusPage slug={statusSlugMatch.params.slug!} />;
  }

  return (
    <div className="app-shell">
      <div className="app-bg app-bg-primary" />
      <div className="app-bg app-bg-secondary" />

      {!connected ? (
        <LoginScreen
          appError={auth.appError}
          canSubmitLogin={canSubmitLogin}
          isConnecting={isConnecting}
          loginEmail={loginEmail}
          loginPassword={loginPassword}
          onConnect={() => void connect()}
          onLoginEmailChange={setLoginEmail}
          onLoginPasswordChange={setLoginPassword}
          showPassword={showPassword}
          onTogglePassword={() => setShowPassword((c) => !c)}
        />
      ) : (
        <div className="workspace-layout">
          <AppSidebar
            activeView={activeView}
            canAccessView={canAccessView}
            canNotificationsView={canReadNotificationChannels}
            canApiClientsView={canReadApiClients}
            onNavigate={handleNavigate}
          />

          <section className="workspace-content">
            <header className="workspace-topbar panel">
              <div>
                <div className="workspace-breadcrumbs" aria-label="Breadcrumb">
                  {breadcrumbs.map((crumb, index) => (
                    <span key={crumb.label} className="workspace-breadcrumb-item">
                      {index > 0 && <span className="workspace-breadcrumb-separator">/</span>}
                      {crumb.onClick ? (
                        <button className="workspace-breadcrumb-button" type="button" onClick={crumb.onClick}>
                          {crumb.label}
                        </button>
                      ) : (
                        <span>{crumb.label}</span>
                      )}
                    </span>
                  ))}
                </div>
                <h1>
                  {isSiteDashboard
                    ? `${siteDashboard.selectedSite?.name ?? "Site"} Dashboard`
                    : activeView === "dashboard"
                      ? "Dashboard"
                      : activeView === "notifications"
                        ? "Notifications"
                        : activeView === "access"
                          ? "Access"
                          : "Sites"}
                </h1>
                {isSiteDashboard && <p>Monitor summary and recent runtime state for the selected site.</p>}
                {activeView === "dashboard" && !isSiteDashboard && (
                  <p>Health overview and active incidents across all monitored sites.</p>
                )}
                {activeView === "notifications" && (
                  <p>Webhook and email channels for monitor failure and recovery alerts.</p>
                )}
                {activeView === "access" && canAccessView && (
                  <p>Users, roles, and permissions for Sentinel administration.</p>
                )}
                {activeView === "access" && !canAccessView && (
                  <p>Your account does not have permissions to access this section.</p>
                )}
              </div>

              <div className="workspace-header-ops">
                <div className="workspace-metrics" aria-label="Operational context">
                  {headerMetrics.map((metric) => (
                    <div key={metric.label} className={`workspace-metric workspace-metric-${metric.tone}`}>
                      <span>{metric.label}</span>
                      <strong>{metric.value}</strong>
                    </div>
                  ))}
                </div>
                <div className="workspace-actions">
                  <div className="workspace-user">
                    <span>{session?.user.display_name || session?.user.email}</span>
                    <strong>{session?.user.email}</strong>
                  </div>
                  <div className="status-pill is-live">Signed in</div>
                  <button className="ghost-button" onClick={handleRefreshCurrent}>
                    {isRefreshingCurrent ? "Refreshing..." : "Refresh"}
                  </button>
                  <button className="ghost-button" onClick={() => void disconnect()}>
                    Disconnect
                  </button>
                </div>
              </div>
            </header>

            <div className="workspace-banner panel">
              {`Auto-refresh every 15s • Last sync ${lastSyncLabel}`}
            </div>

            {auth.appError && auth.appError.scope !== "connection" && (
              <div className="error-banner workspace-banner">{auth.appError.message}</div>
            )}

            <ErrorBoundary>
              <Routes>
              <Route path="/" element={<Navigate to="/dashboard" replace />} />
              <Route
                path="/dashboard"
                element={
                  canReadSites ? (
                    <DashboardPlaceholder
                      isRefreshing={globalDashboard.isRefreshingHomeDashboard}
                      stats={globalDashboard.dashboardStats}
                      sites={globalDashboard.dashboardSites}
                      onOpenSite={(siteId) => navigate(`/sites/${siteId}`)}
                      openIncidents={globalDashboard.dashboardOpenIncidents}
                    />
                  ) : (
                    <div className="panel workspace-banner">
                      You do not have permission to view dashboard widgets.
                    </div>
                  )
                }
              />
              <Route
                path="/notifications"
                element={
                  canReadNotificationChannels ? (
                    <NotificationsManagement
                      canWrite={canWriteNotificationChannels}
                      channels={notifications.channels}
                      isRefreshing={notifications.isRefreshing}
                      onRefresh={() => void notifications.refreshChannels()}
                      onCreateChannel={notifications.handleCreateChannel}
                      onUpdateChannel={notifications.handleUpdateChannel}
                      onDeleteChannel={notifications.handleDeleteChannel}
                    />
                  ) : (
                    <div className="panel workspace-banner">
                      You do not have permission to view notification channels.
                    </div>
                  )
                }
              />
              <Route
                path="/api-clients"
                element={
                  canReadApiClients ? (
                    <ApiClientsManagement
                      canWrite={canWriteApiClients}
                      clients={apiClients.clients}
                      isRefreshing={apiClients.isRefreshing}
                      onRefresh={() => void apiClients.refreshClients()}
                      onCreateClient={apiClients.handleCreateClient}
                      onUpdateClient={apiClients.handleUpdateClient}
                      onDeleteClient={apiClients.handleDeleteClient}
                      onRotateSecret={apiClients.handleRotateSecret}
                    />
                  ) : (
                    <div className="panel workspace-banner">
                      You do not have permission to view API clients.
                    </div>
                  )
                }
              />
              <Route
                path="/access"
                element={
                  canAccessView ? (
                    <AccessManagement
                      canReadRoles={canReadRoles}
                      canReadUsers={canReadUsers}
                      canWriteRoles={canWriteRoles}
                      canWriteUsers={canWriteUsers}
                      isRefreshing={accessData.isRefreshingAccess}
                      onRefresh={() => void accessData.refreshAccessData()}
                      onCreateRole={accessData.handleCreateManagedRole}
                      onCreateUser={accessData.handleCreateManagedUser}
                      onDeleteRole={accessData.handleDeleteManagedRole}
                      onDeleteUser={accessData.handleDeleteManagedUser}
                      onUpdateRole={accessData.handleUpdateManagedRole}
                      onUpdateUser={accessData.handleUpdateManagedUser}
                      permissions={accessData.managedPermissions}
                      roles={accessData.managedRoles}
                      users={accessData.managedUsers}
                    />
                  ) : (
                    <div className="panel workspace-banner">
                      You do not have permission to view access management.
                    </div>
                  )
                }
              />
              <Route
                path="/sites"
                element={
                  canReadSites ? (
                    <SitesTable
                      canCreateSite={canCreateSites}
                      canDeleteSite={canDeleteSites}
                      canUpdateSite={canUpdateSites}
                      onCreateSite={handleCreateSite}
                      onDeleteSite={handleDeleteSite}
                      isRefreshing={sites.isRefreshingSites}
                      onOpenDashboard={handleOpenSiteDashboard}
                      onPageChange={sites.handleSitePageChange}
                      onPageSizeChange={sites.handleSitePageSizeChange}
                      sites={sites.sites}
                      onSearchQueryChange={sites.handleSiteSearchChange}
                      page={sites.sitePage}
                      pageSize={sites.sitePageSize}
                      onRefresh={() => void sites.refreshSites()}
                      searchQuery={sites.siteSearchQuery}
                      totalCount={sites.siteTotalCount}
                      onUpdateSite={handleUpdateSite}
                    />
                  ) : (
                    <div className="panel workspace-banner">
                      You do not have permission to view sites.
                    </div>
                  )
                }
              />
              <Route
                path="/sites/:siteId"
                element={
                  isSiteDashboard ? (
                    <SiteDashboard
                      summary={siteDashboard.selectedSiteSummary!}
                      monitorInventory={siteDashboard.selectedSiteMonitorInventory}
                      session={session}
                      canWriteMonitors={canWriteMonitors}
                      canWriteSiteNotificationOverrides={canWriteSiteNotificationOverrides}
                      canUpdateSite={canUpdateSites}
                      onRefresh={() =>
                        siteDashboardRouteId
                          ? siteDashboard.refreshSiteDashboard(siteDashboardRouteId)
                          : Promise.resolve()
                      }
                    />
                  ) : (
                    <div className="panel workspace-banner">Loading site dashboard...</div>
                  )
                }
              />
              <Route path="/status/:slug" element={null} />
              <Route path="*" element={<Navigate to="/dashboard" replace />} />
              </Routes>
            </ErrorBoundary>
          </section>
        </div>
      )}

      <footer className="footer-note">
        {connected && <span>{`Signed in as ${session?.user.email} until ${sessionExpiresLabel}`}</span>}
      </footer>
    </div>
  );
}
