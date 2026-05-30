export type AdminUser = {
  id: number;
  email: string;
  display_name: string;
  is_active: boolean;
  last_login_at: string | null;
  created_at: string;
  updated_at: string;
};

export type AuthSession = {
  expiresAt: string;
  expiresIn: number;
  roles: string[];
  permissions: string[];
  user: AdminUser;
};

export type ManagedAdminUser = AdminUser & {
  roles: string[];
  permissions: string[];
};

export type ManagedRole = {
  id: number;
  key: string;
  name: string;
  description: string | null;
  is_system: boolean;
  created_at: string;
  updated_at: string;
  permissions: string[];
};

export type ManagedPermission = {
  id: number;
  key: string;
  name: string;
  description: string | null;
  created_at: string;
  roles: string[];
};

export type ApiClientType = "internal_service" | "installation_client";
export type ApiClientScope = "sites:read" | "sites:write";

export type ManagedApiClient = {
  id: number;
  name: string;
  description: string | null;
  client_type: ApiClientType;
  client_id: string;
  secret_prefix: string;
  scopes: ApiClientScope[];
  is_active: boolean;
  last_used_at: string | null;
  created_by_user_id: string | null;
  created_at: string;
  updated_at: string;
};

export type CreatedApiClient = ManagedApiClient & {
  client_secret: string;
};

export type Site = {
  id: number;
  name: string;
  base_url: string;
  is_active: boolean;
  has_http_monitor: boolean;
  http_monitor_status: "active" | "disabled" | "not_configured";
  has_open_incident: boolean;
  current_state: "healthy" | "failing" | "pending_first_check" | "disabled" | "not_configured";
  created_at: string;
  updated_at: string;
};

export type SiteMonitorType = "http" | "ssl" | "heartbeat" | "tcp" | "dns";

export type HttpHeaderAssertion = {
  name: string;
  equals: string | null;
  contains: string | null;
};

export type JsonPathValueAssertion = {
  path: string;
  value: unknown;
};

export type HttpMonitor = {
  id: number;
  site_id: number;
  monitor_type: "http";
  target_url: string;
  check_interval_seconds: number;
  expected_status_code: number;
  is_active: boolean;
  body_must_contain: string | null;
  body_must_not_contain: string | null;
  body_must_contain_texts: string[] | null;
  body_must_not_contain_texts: string[] | null;
  max_response_time_ms: number | null;
  required_header_name: string | null;
  required_header_value: string | null;
  header_assertions: HttpHeaderAssertion[] | null;
  json_path_exists: string[] | null;
  json_path_equals: JsonPathValueAssertion[] | null;
  json_path_not_equals: JsonPathValueAssertion[] | null;
  ssl_certificate_checks_enabled: boolean;
  ssl_expiry_warning_days: number | null;
  last_certificate_expires_at: string | null;
  last_certificate_days_remaining: number | null;
  last_certificate_issuer: string | null;
  last_certificate_subject: string | null;
  last_certificate_domain: string | null;
  http_check_timeout_seconds_override: number | null;
  http_check_max_attempts_override: number | null;
  http_check_retry_delays_ms_override: number[] | null;
  last_checked_at: string | null;
  last_successful_check_at: string | null;
  last_is_success: boolean | null;
  last_status_code: number | null;
  last_response_time_ms: number | null;
  last_failure_reason: string | null;
  last_error_message: string | null;
  created_at: string;
  updated_at: string;
};

export type SslMonitor = {
  id: number;
  site_id: number;
  monitor_type: "ssl";
  target_url: string;
  check_interval_seconds: number;
  ssl_expiry_warning_days: number | null;
  http_check_timeout_seconds_override: number | null;
  http_check_max_attempts_override: number | null;
  http_check_retry_delays_ms_override: number[] | null;
  is_active: boolean;
  last_checked_at: string | null;
  last_successful_check_at: string | null;
  last_is_success: boolean | null;
  last_failure_reason: string | null;
  last_error_message: string | null;
  last_certificate_expires_at: string | null;
  last_certificate_days_remaining: number | null;
  last_certificate_issuer: string | null;
  last_certificate_subject: string | null;
  last_certificate_domain: string | null;
  created_at: string;
  updated_at: string;
};

export type HeartbeatMonitor = {
  id: number;
  site_id: number;
  monitor_type: "heartbeat";
  ping_path: string;
  check_interval_seconds: number;
  heartbeat_grace_seconds: number | null;
  is_active: boolean;
  last_heartbeat_received_at: string | null;
  last_checked_at: string | null;
  last_successful_check_at: string | null;
  last_is_success: boolean | null;
  last_failure_reason: string | null;
  last_error_message: string | null;
  created_at: string;
  updated_at: string;
};

export type TcpMonitor = {
  id: number;
  site_id: number;
  monitor_type: "tcp";
  target_host: string;
  target_port: number;
  check_interval_seconds: number;
  max_connect_time_ms: number | null;
  timeout_seconds_override: number | null;
  max_attempts_override: number | null;
  retry_delays_ms_override: number[] | null;
  is_active: boolean;
  last_checked_at: string | null;
  last_successful_check_at: string | null;
  last_is_success: boolean | null;
  last_response_time_ms: number | null;
  last_failure_reason: string | null;
  last_error_message: string | null;
  created_at: string;
  updated_at: string;
};

export type DnsMonitor = {
  id: number;
  site_id: number;
  monitor_type: "dns";
  hostname: string;
  record_type: string;
  expected_value: string | null;
  nameserver: string | null;
  check_interval_seconds: number;
  timeout_seconds_override: number | null;
  max_attempts_override: number | null;
  retry_delays_ms_override: number[] | null;
  is_active: boolean;
  last_checked_at: string | null;
  last_successful_check_at: string | null;
  last_is_success: boolean | null;
  last_response_time_ms: number | null;
  last_failure_reason: string | null;
  last_error_message: string | null;
  created_at: string;
  updated_at: string;
};

export type AnySiteMonitor =
  | HttpMonitor
  | SslMonitor
  | HeartbeatMonitor
  | TcpMonitor
  | DnsMonitor;

export type SiteMonitorCheck = {
  id: number;
  site_monitor_id: number;
  checked_at: string;
  monitor_type: SiteMonitorType;
  url_checked: string;
  expected_status_code: number | null;
  is_success: boolean;
  status_code: number | null;
  response_time_ms: number | null;
  failure_reason: string | null;
  error_message: string | null;
};

export type SiteIncident = {
  id: number;
  monitor_id: number;
  monitor_type: SiteMonitorType;
  target_url: string;
  status: "open" | "resolved";
  opened_at: string;
  resolved_at: string | null;
  started_check_id: number;
  resolved_check_id: number | null;
  failure_count: number;
  last_status_code: number | null;
  last_error_message: string | null;
  resolved_status_code: number | null;
  acknowledged_at: string | null;
  acknowledged_by: number | null;
  downtime_seconds: number | null;
  resolved_reason: "recovered" | "monitoring_disabled" | "site_deactivated" | null;
};

export type GlobalDashboardStats = {
  sites_total: number;
  sites_active: number;
  sites_up: number;
  sites_down: number;
  sites_paused: number;
  sites_unchecked: number;
  sites_with_monitor: number;
  sites_without_monitor: number;
  open_incidents: number;
  monitors_active: number;
};

export type DashboardSiteEntry = {
  id: number;
  name: string;
  base_url: string;
  is_active: boolean;
  status: "up" | "down" | "paused" | "unchecked" | "inactive";
  has_open_incident: boolean;
  last_checked_at: string | null;
  last_response_time_ms: number | null;
  monitors_active: number;
  monitors_total: number;
};

export type SiteUptime = {
  window: "7d" | "30d";
  window_days: number;
  uptime_percent: number | null;
  total_checks: number;
  successful_checks: number;
  failed_checks: number;
};

export type GlobalIncident = SiteIncident & {
  site_id: number;
  site_name: string;
  site_base_url: string;
};

export type NotificationChannelType = "email" | "webhook" | "slack" | "discord";
export type NotificationEventType = "failure" | "recovery";
export type NotificationDeliveryStatus = "pending" | "delivered" | "failed";

export type NotificationChannel = {
  id: number;
  channel_type: NotificationChannelType;
  name: string;
  destination: string;
  has_webhook_secret: boolean;
  notify_on_failure: boolean;
  notify_on_recovery: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
};

export type NotificationChannelSetup = {
  channel_type: NotificationChannelType;
  name: string;
  destination: string;
  webhook_secret?: string | null;
  notify_on_failure: boolean;
  notify_on_recovery: boolean;
  is_active: boolean;
};

export type SiteNotificationChannel = {
  id: number;
  channel_type: NotificationChannelType;
  name: string;
  destination: string;
  default_notify_on_failure: boolean;
  default_notify_on_recovery: boolean;
  default_is_active: boolean;
  effective_notify_on_failure: boolean;
  effective_notify_on_recovery: boolean;
  effective_is_active: boolean;
  override_id: number | null;
  override_notify_on_failure: boolean | null;
  override_notify_on_recovery: boolean | null;
  override_is_active: boolean | null;
};

export type SiteNotificationDelivery = {
  id: number;
  notification_channel_id: number;
  site_monitor_id: number;
  site_monitor_check_id: number;
  event_type: NotificationEventType;
  payload: Record<string, unknown>;
  status: NotificationDeliveryStatus;
  attempts: number;
  next_attempt_at: string | null;
  claimed_at: string | null;
  lease_until: string | null;
  claimed_by: string | null;
  delivered_at: string | null;
  last_error: string | null;
  created_at: string;
  updated_at: string;
  channel_type: NotificationChannelType;
  channel_name: string;
  destination: string;
};

export type SiteSummary = {
  site: Site;
  http_monitors: HttpMonitor[];
  ssl_monitors: SslMonitor[];
  heartbeat_monitors: HeartbeatMonitor[];
  current_state:
    | "healthy"
    | "failing"
    | "pending_first_check"
    | "disabled"
    | "not_configured";
  incident_open: boolean;
  recent_checks: {
    window_size: number;
    total_checks: number;
    successful_checks: number;
    failed_checks: number;
    success_rate: number | null;
  };
  latest_check: SiteMonitorCheck | null;
  latest_failure: SiteMonitorCheck | null;
};

export type PaginatedResponse<T> = {
  items: T;
  nextCursor: string | null;
  totalCount: number | null;
  page: number | null;
  pageSize: number | null;
};

type FetchOptions = RequestInit;

const FETCH_TIMEOUT_MS = 15_000;

function withTimeout(signal?: AbortSignal): AbortSignal {
  const timeout = AbortSignal.timeout(FETCH_TIMEOUT_MS);
  return signal ? AbortSignal.any([signal, timeout]) : timeout;
}

function isTokenExpired(session: AuthSession | null): boolean {
  if (!session) return true;
  const expiresAt = new Date(session.expiresAt).getTime();
  return Number.isNaN(expiresAt) || expiresAt <= Date.now();
}

async function parseError(response: Response): Promise<Error> {
  try {
    const payload = (await response.json()) as { error?: string };
    return new Error(payload.error || `Request failed with ${response.status}`);
  } catch {
    return new Error(`Request failed with ${response.status}`);
  }
}

async function authenticatedFetch<T>(
  path: string,
  options: FetchOptions = {}
): Promise<PaginatedResponse<T>> {
  const response = await fetch(path, {
    ...options,
    signal: withTimeout(options.signal ?? undefined),
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
      ...(options.headers ?? {}),
    },
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  const totalCountHeader = response.headers.get("x-total-count");
  const pageHeader = response.headers.get("x-page");
  const pageSizeHeader = response.headers.get("x-page-size");

  return {
    items: (await response.json()) as T,
    nextCursor: response.headers.get("x-next-cursor"),
    totalCount: totalCountHeader === null ? null : Number(totalCountHeader),
    page: pageHeader === null ? null : Number(pageHeader),
    pageSize: pageSizeHeader === null ? null : Number(pageSizeHeader),
  };
}

export async function issueAdminToken(email: string, password: string): Promise<AuthSession> {
  const response = await fetch("/v1/admin/auth/login", {
    method: "POST",
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
    },
    body: JSON.stringify({ email: email.trim(), password }),
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  const payload = (await response.json()) as {
    expires_at: string;
    expires_in: number;
    roles: string[];
    permissions: string[];
    user: AdminUser;
  };

  return {
    expiresAt: payload.expires_at,
    expiresIn: payload.expires_in,
    roles: payload.roles,
    permissions: payload.permissions,
    user: payload.user,
  };
}

type AdminSessionResponse = {
  roles: string[];
  permissions: string[];
  user: AdminUser;
};

export async function getAdminSession(): Promise<AdminSessionResponse> {
  const response = await fetch("/v1/admin/auth/me", {
    method: "GET",
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    headers: { Accept: "application/json" },
  });

  if (!response.ok) {
    throw await parseError(response);
  }

  return (await response.json()) as AdminSessionResponse;
}

export async function revokeAdminToken(): Promise<void> {
  await fetch("/v1/admin/auth/logout", {
    method: "POST",
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    headers: { Accept: "application/json" },
  });
}

export async function authorizedRequest<T>(
  session: AuthSession | null,
  path: string,
  options: FetchOptions = {}
): Promise<{ session: AuthSession; response: PaginatedResponse<T> }> {
  if (!session || isTokenExpired(session)) {
    throw new Error("Admin session has expired. Log in again.");
  }

  try {
    const response = await authenticatedFetch<T>(path, options);
    return { session, response };
  } catch (error) {
    if (error instanceof Error && error.message.includes("401")) {
      throw new Error("Admin session is no longer valid. Log in again.");
    }
    throw error;
  }
}

export async function listManagedAdminUsers(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; users: ManagedAdminUser[] }> {
  const result = await authorizedRequest<ManagedAdminUser[]>(session, "/v1/admin/users", { signal });
  return { session: result.session, users: result.response.items };
}

export async function createManagedAdminUser(
  session: AuthSession | null,
  payload: {
    email: string;
    display_name: string;
    password: string;
    is_active?: boolean;
    role_keys: string[];
  }
): Promise<{ session: AuthSession; user: ManagedAdminUser }> {
  const result = await authorizedRequest<ManagedAdminUser>(session, "/v1/admin/users", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  return { session: result.session, user: result.response.items };
}

export async function updateManagedAdminUser(
  session: AuthSession | null,
  userId: number,
  payload: {
    email: string;
    display_name: string;
    password?: string;
    is_active: boolean;
    role_keys: string[];
  }
): Promise<{ session: AuthSession; user: ManagedAdminUser }> {
  const result = await authorizedRequest<ManagedAdminUser>(
    session,
    `/v1/admin/users/${userId}`,
    { method: "PATCH", body: JSON.stringify(payload) }
  );
  return { session: result.session, user: result.response.items };
}

export async function deleteManagedAdminUser(
  session: AuthSession | null,
  userId: number
): Promise<{ session: AuthSession }> {
  const result = await authorizedRequest<{ deleted: boolean }>(
    session,
    `/v1/admin/users/${userId}`,
    { method: "DELETE" }
  );
  return { session: result.session };
}

export async function listManagedRoles(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; roles: ManagedRole[] }> {
  const result = await authorizedRequest<ManagedRole[]>(session, "/v1/admin/roles", { signal });
  return { session: result.session, roles: result.response.items };
}

export async function createManagedRole(
  session: AuthSession | null,
  payload: {
    key: string;
    name: string;
    description?: string;
    permission_keys: string[];
  }
): Promise<{ session: AuthSession; role: ManagedRole }> {
  const result = await authorizedRequest<ManagedRole>(session, "/v1/admin/roles", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  return { session: result.session, role: result.response.items };
}

export async function updateManagedRole(
  session: AuthSession | null,
  roleId: number,
  payload: {
    name: string;
    description?: string;
    permission_keys: string[];
  }
): Promise<{ session: AuthSession; role: ManagedRole }> {
  const result = await authorizedRequest<ManagedRole>(
    session,
    `/v1/admin/roles/${roleId}`,
    { method: "PATCH", body: JSON.stringify(payload) }
  );
  return { session: result.session, role: result.response.items };
}

export async function deleteManagedRole(
  session: AuthSession | null,
  roleId: number
): Promise<{ session: AuthSession }> {
  const result = await authorizedRequest<{ deleted: boolean }>(
    session,
    `/v1/admin/roles/${roleId}`,
    { method: "DELETE" }
  );
  return { session: result.session };
}

export async function getSiteSummary(
  session: AuthSession | null,
  siteId: number,
  signal?: AbortSignal
): Promise<{ session: AuthSession; summary: SiteSummary }> {
  const result = await authorizedRequest<SiteSummary>(session, `/v1/sites/${siteId}/summary`, { signal });
  return { session: result.session, summary: result.response.items };
}

export type SiteMonitorInventory = {
  http: HttpMonitor[];
  ssl: SslMonitor[];
  heartbeat: HeartbeatMonitor[];
  tcp: TcpMonitor[];
  dns: DnsMonitor[];
};

export type HttpMonitorSetup = {
  target_url: string;
  check_interval_seconds: number;
  expected_status_code: number;
  is_active: boolean;
  ssl_certificate_checks_enabled?: boolean | null;
  ssl_expiry_warning_days?: number | null;
  max_response_time_ms?: number | null;
  body_must_contain?: string | null;
  body_must_not_contain?: string | null;
  body_must_contain_texts?: string[] | null;
  body_must_not_contain_texts?: string[] | null;
  required_header_name?: string | null;
  required_header_value?: string | null;
  header_assertions?: HttpHeaderAssertion[] | null;
  json_path_exists?: string[] | null;
  json_path_equals?: JsonPathValueAssertion[] | null;
  json_path_not_equals?: JsonPathValueAssertion[] | null;
  http_check_timeout_seconds_override?: number | null;
  http_check_max_attempts_override?: number | null;
  http_check_retry_delays_ms_override?: number[] | null;
};

export type SslMonitorSetup = {
  target_url: string;
  check_interval_seconds: number;
  ssl_expiry_warning_days?: number | null;
  http_check_timeout_seconds_override?: number | null;
  http_check_max_attempts_override?: number | null;
  http_check_retry_delays_ms_override?: number[] | null;
  is_active: boolean;
};

export type HeartbeatMonitorSetup = {
  check_interval_seconds: number;
  heartbeat_grace_seconds?: number | null;
  is_active: boolean;
};

export type TcpMonitorSetup = {
  target_host: string;
  target_port: number;
  check_interval_seconds: number;
  max_connect_time_ms?: number | null;
  timeout_seconds_override?: number | null;
  max_attempts_override?: number | null;
  retry_delays_ms_override?: number[] | null;
  is_active: boolean;
};

export type DnsMonitorSetup = {
  hostname: string;
  record_type: string;
  expected_value?: string | null;
  nameserver?: string | null;
  check_interval_seconds: number;
  timeout_seconds_override?: number | null;
  max_attempts_override?: number | null;
  retry_delays_ms_override?: number[] | null;
  is_active: boolean;
};

type HttpMonitoringResponse = { site: Site; http_monitors: HttpMonitor[] };
type SslMonitoringResponse = { site: Site; ssl_monitors: SslMonitor[] };
type HeartbeatMonitoringResponse = { site: Site; heartbeat_monitors: HeartbeatMonitor[] };
type TcpMonitoringResponse = { site: Site; tcp_monitors: TcpMonitor[] };
type DnsMonitoringResponse = { site: Site; dns_monitors: DnsMonitor[] };

export async function getSiteMonitorInventory(
  session: AuthSession | null,
  siteId: number,
  signal?: AbortSignal
): Promise<{ session: AuthSession; inventory: SiteMonitorInventory }> {
  if (!session || isTokenExpired(session)) {
    throw new Error("Admin session has expired. Log in again.");
  }

  const [http, ssl, heartbeat, tcp, dns] = await Promise.all([
    authenticatedFetch<HttpMonitoringResponse>(`/v1/sites/${siteId}/monitoring/http`, { signal }),
    authenticatedFetch<SslMonitoringResponse>(`/v1/sites/${siteId}/monitoring/ssl`, { signal }),
    authenticatedFetch<HeartbeatMonitoringResponse>(`/v1/sites/${siteId}/monitoring/heartbeat`, { signal }),
    authenticatedFetch<TcpMonitoringResponse>(`/v1/sites/${siteId}/monitoring/tcp`, { signal }),
    authenticatedFetch<DnsMonitoringResponse>(`/v1/sites/${siteId}/monitoring/dns`, { signal }),
  ]);

  return {
    session,
    inventory: {
      http: http.items.http_monitors,
      ssl: ssl.items.ssl_monitors,
      heartbeat: heartbeat.items.heartbeat_monitors,
      tcp: tcp.items.tcp_monitors,
      dns: dns.items.dns_monitors,
    },
  };
}

export async function configureHttpMonitor(
  session: AuthSession | null,
  siteId: number,
  payload: HttpMonitorSetup
): Promise<{ session: AuthSession; monitor: HttpMonitor }> {
  const result = await authorizedRequest<HttpMonitor>(session, `/v1/sites/${siteId}/monitoring/http`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  return { session: result.session, monitor: result.response.items };
}

export async function configureSslMonitor(
  session: AuthSession | null,
  siteId: number,
  payload: SslMonitorSetup
): Promise<{ session: AuthSession; monitor: SslMonitor }> {
  const result = await authorizedRequest<SslMonitor>(session, `/v1/sites/${siteId}/monitoring/ssl`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  return { session: result.session, monitor: result.response.items };
}

export async function configureHeartbeatMonitor(
  session: AuthSession | null,
  siteId: number,
  payload: HeartbeatMonitorSetup
): Promise<{ session: AuthSession; monitor: HeartbeatMonitor }> {
  const result = await authorizedRequest<HeartbeatMonitor>(
    session,
    `/v1/sites/${siteId}/monitoring/heartbeat`,
    { method: "PUT", body: JSON.stringify(payload) }
  );
  return { session: result.session, monitor: result.response.items };
}

export async function configureTcpMonitor(
  session: AuthSession | null,
  siteId: number,
  payload: TcpMonitorSetup
): Promise<{ session: AuthSession; monitor: TcpMonitor }> {
  const result = await authorizedRequest<TcpMonitor>(session, `/v1/sites/${siteId}/monitoring/tcp`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  return { session: result.session, monitor: result.response.items };
}

export async function configureDnsMonitor(
  session: AuthSession | null,
  siteId: number,
  payload: DnsMonitorSetup
): Promise<{ session: AuthSession; monitor: DnsMonitor }> {
  const result = await authorizedRequest<DnsMonitor>(session, `/v1/sites/${siteId}/monitoring/dns`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  return { session: result.session, monitor: result.response.items };
}

export async function disableSiteMonitorType(
  session: AuthSession | null,
  siteId: number,
  monitorType: SiteMonitorType
): Promise<{ session: AuthSession }> {
  await authorizedRequest<{ disabled: boolean }>(session, `/v1/sites/${siteId}/monitoring/${monitorType}`, {
    method: "DELETE",
  });
  return { session: session as AuthSession };
}

export async function resumeSiteMonitor(
  session: AuthSession | null,
  siteId: number,
  monitorType: SiteMonitorType,
  monitorId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<unknown>(
    session,
    `/v1/sites/${siteId}/monitoring/${monitorType}/${monitorId}/resume`,
    { method: "POST" }
  );
  return { session: session as AuthSession };
}

export async function createSite(
  session: AuthSession | null,
  payload: { name: string; base_url: string }
): Promise<{ session: AuthSession; site: Site }> {
  const result = await authorizedRequest<Site>(session, "/v1/sites", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  return { session: result.session, site: result.response.items };
}

export async function updateSite(
  session: AuthSession | null,
  siteId: number,
  payload: { name: string; base_url: string; is_active: boolean }
): Promise<{ session: AuthSession; site: Site }> {
  const result = await authorizedRequest<Site>(session, `/v1/sites/${siteId}`, {
    method: "PATCH",
    body: JSON.stringify(payload),
  });
  return { session: result.session, site: result.response.items };
}

export async function deleteSite(
  session: AuthSession | null,
  siteId: number
): Promise<{ session: AuthSession }> {
  const result = await authorizedRequest<{ deleted: boolean }>(session, `/v1/sites/${siteId}`, {
    method: "DELETE",
  });
  return { session: result.session };
}

export async function listManagedPermissions(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; permissions: ManagedPermission[] }> {
  const result = await authorizedRequest<ManagedPermission[]>(session, "/v1/admin/permissions", { signal });
  return { session: result.session, permissions: result.response.items };
}

export async function pauseSiteMonitor(
  session: AuthSession | null,
  siteId: number,
  monitorType: SiteMonitorType,
  monitorId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<unknown>(
    session,
    `/v1/sites/${siteId}/monitoring/${monitorType}/${monitorId}/pause`,
    { method: "POST" }
  );
  return { session: session as AuthSession };
}

export async function deleteSiteMonitor(
  session: AuthSession | null,
  siteId: number,
  monitorType: SiteMonitorType,
  monitorId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<unknown>(
    session,
    `/v1/sites/${siteId}/monitoring/${monitorType}/${monitorId}`,
    { method: "DELETE" }
  );
  return { session: session as AuthSession };
}

export async function getSiteChecks(
  session: AuthSession | null,
  siteId: number,
  options?: { filter?: "success" | "failure"; cursor?: string; limit?: number }
): Promise<{ session: AuthSession; checks: SiteMonitorCheck[]; nextCursor: string | null }> {
  const params = new URLSearchParams();
  if (options?.filter) params.set("outcome", options.filter);
  if (options?.cursor) params.set("cursor", options.cursor);
  if (options?.limit) params.set("limit", String(options.limit));
  const query = params.toString() ? `?${params.toString()}` : "";
  const result = await authorizedRequest<SiteMonitorCheck[]>(
    session,
    `/v1/sites/${siteId}/checks${query}`
  );
  return { session: result.session, checks: result.response.items, nextCursor: result.response.nextCursor };
}

export async function getSiteIncidents(
  session: AuthSession | null,
  siteId: number,
  options?: { status?: "open" | "resolved"; cursor?: string; limit?: number; signal?: AbortSignal }
): Promise<{ session: AuthSession; incidents: SiteIncident[]; nextCursor: string | null }> {
  const params = new URLSearchParams();
  if (options?.status) params.set("status", options.status);
  if (options?.cursor) params.set("cursor", options.cursor);
  if (options?.limit) params.set("limit", String(options.limit));
  const query = params.toString() ? `?${params.toString()}` : "";
  const result = await authorizedRequest<SiteIncident[]>(
    session,
    `/v1/sites/${siteId}/incidents${query}`,
    { signal: options?.signal }
  );
  return { session: result.session, incidents: result.response.items, nextCursor: result.response.nextCursor };
}

export async function getSiteUptime(
  session: AuthSession | null,
  siteId: number,
  options?: { window?: "7d" | "30d"; signal?: AbortSignal }
): Promise<{ session: AuthSession; uptime: SiteUptime }> {
  const params = new URLSearchParams();
  if (options?.window) params.set("window", options.window);
  const query = params.size > 0 ? `?${params.toString()}` : "";
  const result = await authorizedRequest<SiteUptime>(
    session,
    `/v1/sites/${siteId}/uptime${query}`,
    { signal: options?.signal }
  );
  return { session: result.session, uptime: result.response.items };
}

export type DailyUptimeBucket = {
  date: string;
  total_checks: number;
  successful_checks: number;
  uptime_percent: number | null;
};

export type SiteUptimeDaily = {
  days: number;
  buckets: DailyUptimeBucket[];
};

export async function getSiteUptimeDaily(
  session: AuthSession | null,
  siteId: number,
  options?: { days?: number; signal?: AbortSignal }
): Promise<{ session: AuthSession; data: SiteUptimeDaily }> {
  const params = new URLSearchParams();
  if (options?.days) params.set("days", String(options.days));
  const query = params.size > 0 ? `?${params.toString()}` : "";
  const result = await authorizedRequest<SiteUptimeDaily>(
    session,
    `/v1/sites/${siteId}/uptime/daily${query}`,
    { signal: options?.signal }
  );
  return { session: result.session, data: result.response.items };
}

export async function acknowledgeIncident(
  session: AuthSession | null,
  siteId: number,
  incidentId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<unknown>(
    session,
    `/v1/admin/sites/${siteId}/incidents/${incidentId}/acknowledge`,
    { method: "POST" }
  );
  return { session: session as AuthSession };
}

export async function listNotificationChannels(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; channels: NotificationChannel[] }> {
  const result = await authorizedRequest<NotificationChannel[]>(
    session,
    "/v1/notifications/channels",
    { signal }
  );
  return { session: result.session, channels: result.response.items };
}

export async function createNotificationChannel(
  session: AuthSession | null,
  payload: NotificationChannelSetup
): Promise<{ session: AuthSession; channel: NotificationChannel }> {
  const result = await authorizedRequest<NotificationChannel>(
    session,
    "/v1/notifications/channels",
    { method: "POST", body: JSON.stringify(payload) }
  );
  return { session: result.session, channel: result.response.items };
}

export async function updateNotificationChannel(
  session: AuthSession | null,
  channelId: number,
  payload: NotificationChannelSetup
): Promise<{ session: AuthSession; channel: NotificationChannel }> {
  const result = await authorizedRequest<NotificationChannel>(
    session,
    `/v1/notifications/channels/${channelId}`,
    { method: "PATCH", body: JSON.stringify(payload) }
  );
  return { session: result.session, channel: result.response.items };
}

export async function deleteNotificationChannel(
  session: AuthSession | null,
  channelId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<{ deleted: boolean }>(
    session,
    `/v1/notifications/channels/${channelId}`,
    { method: "DELETE" }
  );
  return { session: session as AuthSession };
}

export async function listSiteNotificationChannels(
  session: AuthSession | null,
  siteId: number,
  signal?: AbortSignal
): Promise<{ session: AuthSession; channels: SiteNotificationChannel[] }> {
  const result = await authorizedRequest<SiteNotificationChannel[]>(
    session,
    `/v1/sites/${siteId}/notifications/channels`,
    { signal }
  );
  return { session: result.session, channels: result.response.items };
}

export async function upsertSiteNotificationChannelOverride(
  session: AuthSession | null,
  siteId: number,
  channelId: number,
  payload: { notify_on_failure?: boolean | null; notify_on_recovery?: boolean | null; is_active?: boolean | null }
): Promise<{ session: AuthSession; channel: SiteNotificationChannel }> {
  const result = await authorizedRequest<SiteNotificationChannel>(
    session,
    `/v1/sites/${siteId}/notifications/channels/${channelId}`,
    { method: "PATCH", body: JSON.stringify(payload) }
  );
  return { session: result.session, channel: result.response.items };
}

export async function deleteSiteNotificationChannelOverride(
  session: AuthSession | null,
  siteId: number,
  channelId: number
): Promise<{ session: AuthSession }> {
  await authorizedRequest<{ deleted: boolean }>(
    session,
    `/v1/sites/${siteId}/notifications/channels/${channelId}`,
    { method: "DELETE" }
  );
  return { session: session as AuthSession };
}

export async function listSiteNotificationDeliveries(
  session: AuthSession | null,
  siteId: number,
  options?: { status?: NotificationDeliveryStatus; event_type?: NotificationEventType; cursor?: string; limit?: number; signal?: AbortSignal }
): Promise<{ session: AuthSession; deliveries: SiteNotificationDelivery[]; nextCursor: string | null }> {
  const params = new URLSearchParams();
  if (options?.status) params.set("status", options.status);
  if (options?.event_type) params.set("event_type", options.event_type);
  if (options?.cursor) params.set("cursor", options.cursor);
  if (options?.limit) params.set("limit", String(options.limit));
  const query = params.toString() ? `?${params.toString()}` : "";
  const result = await authorizedRequest<SiteNotificationDelivery[]>(
    session,
    `/v1/sites/${siteId}/notifications/deliveries${query}`,
    { signal: options?.signal }
  );
  return { session: result.session, deliveries: result.response.items, nextCursor: result.response.nextCursor };
}

export async function getDashboardStats(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; stats: GlobalDashboardStats; sites: DashboardSiteEntry[] }> {
  const result = await authorizedRequest<{ summary: GlobalDashboardStats; sites: DashboardSiteEntry[] }>(
    session, "/v1/dashboard", { signal }
  );
  return {
    session: result.session,
    stats: result.response.items.summary,
    sites: result.response.items.sites,
  };
}

export async function listGlobalIncidents(
  session: AuthSession | null,
  options?: { status?: "open" | "resolved"; cursor?: string; limit?: number; signal?: AbortSignal }
): Promise<{ session: AuthSession; incidents: GlobalIncident[]; nextCursor: string | null }> {
  const params = new URLSearchParams();
  if (options?.status) params.set("status", options.status);
  if (options?.cursor) params.set("cursor", options.cursor);
  if (options?.limit !== undefined) params.set("limit", String(options.limit));
  const query = params.size > 0 ? `?${params.toString()}` : "";
  const result = await authorizedRequest<GlobalIncident[]>(
    session,
    `/v1/incidents${query}`,
    { signal: options?.signal }
  );
  return { session: result.session, incidents: result.response.items, nextCursor: result.response.nextCursor };
}

export async function listManagedApiClients(
  session: AuthSession | null,
  signal?: AbortSignal
): Promise<{ session: AuthSession; clients: ManagedApiClient[] }> {
  const result = await authorizedRequest<ManagedApiClient[]>(session, "/v1/admin/api-clients", { signal });
  return { session: result.session, clients: result.response.items };
}

export async function createManagedApiClient(
  session: AuthSession | null,
  payload: { name: string; description?: string | null; client_type: ApiClientType; scopes: ApiClientScope[] }
): Promise<{ session: AuthSession; client: CreatedApiClient }> {
  const result = await authorizedRequest<CreatedApiClient>(session, "/v1/admin/api-clients", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  return { session: result.session, client: result.response.items };
}

export async function updateManagedApiClient(
  session: AuthSession | null,
  clientId: number,
  payload: { name: string; description?: string | null; is_active: boolean }
): Promise<{ session: AuthSession; client: ManagedApiClient }> {
  const result = await authorizedRequest<ManagedApiClient>(session, `/v1/admin/api-clients/${clientId}`, {
    method: "PATCH",
    body: JSON.stringify(payload),
  });
  return { session: result.session, client: result.response.items };
}

export async function deleteManagedApiClient(
  session: AuthSession | null,
  clientId: number
): Promise<{ session: AuthSession }> {
  const result = await authorizedRequest<null>(session, `/v1/admin/api-clients/${clientId}`, {
    method: "DELETE",
  });
  return { session: result.session };
}

export async function rotateManagedApiClientSecret(
  session: AuthSession | null,
  clientId: number
): Promise<{ session: AuthSession; client: CreatedApiClient }> {
  const result = await authorizedRequest<CreatedApiClient>(
    session,
    `/v1/admin/api-clients/${clientId}/rotate-secret`,
    { method: "POST" }
  );
  return { session: result.session, client: result.response.items };
}

// --- status page ---

export type StatusPageConfig = {
  site_id: number;
  is_enabled: boolean;
  slug: string;
  page_title: string | null;
  show_monitor_details: boolean;
  show_uptime_percentages: boolean;
};

export type StatusPageUpsertPayload = {
  is_enabled: boolean;
  slug: string;
  page_title: string | null;
  show_monitor_details: boolean;
  show_uptime_percentages: boolean;
};

export type PublicUptimeDayBucket = {
  date: string;
  total: number;
  success: number;
};

export type PublicMonitorStatus = {
  label: string;
  monitor_type: string;
  status: string;
  response_time_ms: number | null;
  last_checked_at: string | null;
  uptime_7d: number | null;
  uptime_30d: number | null;
  uptime_history: PublicUptimeDayBucket[];
};

export type PublicOpenIncident = {
  opened_at: string;
  monitor_label: string;
  monitor_type: string;
};

export type PublicResolvedIncident = {
  opened_at: string;
  resolved_at: string;
  monitor_label: string;
  monitor_type: string;
  downtime_seconds: number | null;
  failure_reason: string | null;
};

export type PublicStatusPage = {
  slug: string;
  page_title: string;
  overall_status: string;
  show_monitor_details: boolean;
  show_uptime_percentages: boolean;
  monitors: PublicMonitorStatus[];
  uptime_7d: number | null;
  uptime_30d: number | null;
  open_incidents: PublicOpenIncident[];
  incident_history: PublicResolvedIncident[];
  last_updated: string;
};

export async function getStatusPageConfig(
  session: AuthSession | null,
  siteId: number,
  signal?: AbortSignal
): Promise<{ session: AuthSession; config: StatusPageConfig }> {
  const result = await authorizedRequest<StatusPageConfig>(
    session,
    `/v1/sites/${siteId}/status-page`,
    { signal }
  );
  return { session: result.session, config: result.response.items };
}

export async function upsertStatusPageConfig(
  session: AuthSession | null,
  siteId: number,
  payload: StatusPageUpsertPayload
): Promise<{ session: AuthSession; config: StatusPageConfig }> {
  const result = await authorizedRequest<StatusPageConfig>(
    session,
    `/v1/sites/${siteId}/status-page`,
    { method: "PUT", body: JSON.stringify(payload) }
  );
  return { session: result.session, config: result.response.items };
}

export async function fetchPublicStatusPage(
  slug: string,
  signal?: AbortSignal
): Promise<PublicStatusPage | null> {
  const res = await fetch(`/v1/public/status/${encodeURIComponent(slug)}`, { signal });
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`Failed to load status page: ${res.status}`);
  return res.json() as Promise<PublicStatusPage>;
}
