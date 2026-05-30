import {
  type AnySiteMonitor,
  type SiteMonitorInventory,
  type SiteMonitorType,
  type SiteSummary,
} from "../api";

export type DashboardTab = "overview" | "monitors" | "history" | "incidents" | "notifications" | "status-page";
export type StatusTone = "success" | "danger" | "warning" | "muted";

export type HeaderAssertionDraft = { name: string; equals: string; contains: string };
export type JsonPathAssertionDraft = { path: string; value: string };

export type ConfigureDraft = {
  type: SiteMonitorType;
  httpTargetUrl: string;
  httpInterval: string;
  httpExpectedStatusCode: string;
  httpMaxResponseTimeMs: string;
  httpBodyMustContain: string;
  httpBodyMustNotContain: string;
  httpBodyMustContainTexts: string;
  httpBodyMustNotContainTexts: string;
  httpRequiredHeaderName: string;
  httpRequiredHeaderValue: string;
  httpHeaderAssertions: HeaderAssertionDraft[];
  httpJsonPathExists: string;
  httpJsonPathEquals: JsonPathAssertionDraft[];
  httpJsonPathNotEquals: JsonPathAssertionDraft[];
  httpTimeoutSecondsOverride: string;
  httpMaxAttemptsOverride: string;
  httpRetryDelaysMsOverride: string;
  sslTargetUrl: string;
  sslInterval: string;
  sslWarningDays: string;
  sslTimeoutSecondsOverride: string;
  sslMaxAttemptsOverride: string;
  sslRetryDelaysMsOverride: string;
  heartbeatInterval: string;
  heartbeatGraceSeconds: string;
  tcpTargetHost: string;
  tcpTargetPort: string;
  tcpInterval: string;
  tcpMaxConnectTimeMs: string;
  tcpTimeoutSecondsOverride: string;
  tcpMaxAttemptsOverride: string;
  tcpRetryDelaysMsOverride: string;
  dnsHostname: string;
  dnsRecordType: string;
  dnsExpectedValue: string;
  dnsNameserver: string;
  dnsInterval: string;
  dnsTimeoutSecondsOverride: string;
  dnsMaxAttemptsOverride: string;
  dnsRetryDelaysMsOverride: string;
};

export const MONITOR_ORDER: SiteMonitorType[] = ["http", "ssl", "heartbeat", "tcp", "dns"];
export const CHECKS_PAGE_SIZE = 30;
export const INCIDENTS_PAGE_SIZE = 50;

export function formatTimestamp(value: string | null | undefined): string {
  if (!value) return "Never";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.round((seconds % 3600) / 60)}m`;
}

export function getCurrentStateTone(value: SiteSummary["current_state"]): StatusTone {
  switch (value) {
    case "healthy": return "success";
    case "failing": return "danger";
    case "pending_first_check": return "warning";
    case "disabled":
    case "not_configured": return "muted";
  }
}

export function formatStateLabel(value: string): string {
  return value.replace(/_/g, " ");
}

export function formatMonitorTypeLabel(value: SiteMonitorType): string {
  return value.toUpperCase();
}

export function getMonitorLatestStatusTone(monitor: AnySiteMonitor): StatusTone {
  if (!monitor.is_active) return "muted";
  if (monitor.last_is_success === true) return "success";
  if (monitor.last_is_success === false) return "danger";
  return "warning";
}

export function getMonitorLatestStatusLabel(monitor: AnySiteMonitor): string {
  if (!monitor.is_active) return "Paused";
  if (monitor.last_is_success === true) return "Healthy";
  if (monitor.last_is_success === false) return "Failed";
  return "Pending";
}

export function getMonitorPrimaryLabel(monitor: AnySiteMonitor): string {
  switch (monitor.monitor_type) {
    case "http":
    case "ssl": return monitor.target_url;
    case "heartbeat": return monitor.ping_path;
    case "tcp": return `${monitor.target_host}:${monitor.target_port}`;
    case "dns": return `${monitor.hostname} (${monitor.record_type})`;
  }
}

export function getMonitorConfigSummary(monitor: AnySiteMonitor): string {
  const parts: string[] = [`Every ${monitor.check_interval_seconds}s`];
  switch (monitor.monitor_type) {
    case "http": {
      parts.push(`Expected ${monitor.expected_status_code}`);
      if (monitor.max_response_time_ms) parts.push(`<=${monitor.max_response_time_ms}ms`);
      if (monitor.ssl_certificate_checks_enabled) parts.push("SSL checks");
      const assertionCount = [
        monitor.body_must_contain,
        monitor.body_must_not_contain,
        ...(monitor.body_must_contain_texts ?? []),
        ...(monitor.body_must_not_contain_texts ?? []),
        monitor.required_header_name,
        ...(monitor.header_assertions ?? []),
        ...(monitor.json_path_exists ?? []),
        ...(monitor.json_path_equals ?? []),
        ...(monitor.json_path_not_equals ?? []),
      ].filter(Boolean).length;
      if (assertionCount > 0) parts.push(`${assertionCount} assertion${assertionCount !== 1 ? "s" : ""}`);
      break;
    }
    case "ssl":
      if (monitor.ssl_expiry_warning_days) parts.push(`Warn at ${monitor.ssl_expiry_warning_days}d`);
      break;
    case "heartbeat":
      if (monitor.heartbeat_grace_seconds) parts.push(`${monitor.heartbeat_grace_seconds}s grace`);
      break;
    case "tcp":
      if (monitor.max_connect_time_ms) parts.push(`Max ${monitor.max_connect_time_ms}ms`);
      break;
    case "dns":
      parts.push(monitor.record_type);
      if (monitor.expected_value) parts.push(`Expect: ${monitor.expected_value}`);
      break;
  }
  return parts.join(" · ");
}

export function tryParseUrl(value: string): URL | null {
  try { return new URL(value); } catch { return null; }
}

function parsePositiveInt(value: string): number | null {
  if (!value.trim()) return null;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) return null;
  return parsed;
}

export function validateConfigureDraft(draft: ConfigureDraft): string[] {
  const errors: string[] = [];

  switch (draft.type) {
    case "http": {
      const target = tryParseUrl(draft.httpTargetUrl.trim());
      if (!target || !["http:", "https:"].includes(target.protocol)) errors.push("HTTP target URL must be a valid http/https URL.");
      const interval = parsePositiveInt(draft.httpInterval);
      if (interval === null || interval < 30) errors.push("HTTP interval must be an integer of at least 30 seconds.");
      const statusCode = parsePositiveInt(draft.httpExpectedStatusCode);
      if (statusCode === null || statusCode < 100 || statusCode > 599) errors.push("HTTP expected status code must be an integer between 100 and 599.");
      if (draft.httpMaxResponseTimeMs.trim() && parsePositiveInt(draft.httpMaxResponseTimeMs) === null) errors.push("HTTP max response time must be a positive integer.");
      if (draft.httpTimeoutSecondsOverride.trim() && parsePositiveInt(draft.httpTimeoutSecondsOverride) === null) errors.push("HTTP timeout override must be a positive integer.");
      if (draft.httpMaxAttemptsOverride.trim() && parsePositiveInt(draft.httpMaxAttemptsOverride) === null) errors.push("HTTP max attempts override must be a positive integer.");
      if (draft.httpRetryDelaysMsOverride.trim()) {
        const tokens = draft.httpRetryDelaysMsOverride.split(",").map((token) => token.trim()).filter(Boolean);
        if (tokens.length === 0 || tokens.some((token) => parsePositiveInt(token) === null)) errors.push("HTTP retry delays must be a comma-separated list of positive integers.");
      }
      if (draft.httpRequiredHeaderValue.trim() && !draft.httpRequiredHeaderName.trim()) errors.push("HTTP required header value requires a header name.");
      if (draft.httpHeaderAssertions.some((row) => row.name.trim() && !row.equals.trim() && !row.contains.trim())) errors.push("Each HTTP header assertion needs at least one condition (equals or contains).");
      if (draft.httpHeaderAssertions.some((row) => !row.name.trim() && (row.equals.trim() || row.contains.trim()))) errors.push("HTTP header assertion conditions require a header name.");
      if (draft.httpJsonPathEquals.some((row) => row.path.trim() && !row.value.trim())) errors.push("HTTP JSON path equals assertions require a value.");
      if (draft.httpJsonPathEquals.some((row) => !row.path.trim() && row.value.trim())) errors.push("HTTP JSON path equals assertions require a path.");
      if (draft.httpJsonPathNotEquals.some((row) => row.path.trim() && !row.value.trim())) errors.push("HTTP JSON path not-equals assertions require a value.");
      if (draft.httpJsonPathNotEquals.some((row) => !row.path.trim() && row.value.trim())) errors.push("HTTP JSON path not-equals assertions require a path.");
      break;
    }
    case "ssl": {
      const target = tryParseUrl(draft.sslTargetUrl.trim());
      if (!target || target.protocol !== "https:") errors.push("SSL target URL must be a valid https URL.");
      const interval = parsePositiveInt(draft.sslInterval);
      if (interval === null || interval < 30) errors.push("SSL interval must be an integer of at least 30 seconds.");
      const sslWarningDays = parsePositiveInt(draft.sslWarningDays);
      if (sslWarningDays === null) errors.push("SSL warning days must be a positive integer.");
      else if (sslWarningDays <= 7) errors.push("SSL warning days must be greater than 7.");
      if (draft.sslTimeoutSecondsOverride.trim() && parsePositiveInt(draft.sslTimeoutSecondsOverride) === null) errors.push("SSL timeout override must be a positive integer.");
      if (draft.sslMaxAttemptsOverride.trim() && parsePositiveInt(draft.sslMaxAttemptsOverride) === null) errors.push("SSL max attempts override must be a positive integer.");
      if (draft.sslRetryDelaysMsOverride.trim()) {
        const tokens = draft.sslRetryDelaysMsOverride.split(",").map((t) => t.trim()).filter(Boolean);
        if (tokens.length === 0 || tokens.some((t) => parsePositiveInt(t) === null)) errors.push("SSL retry delays must be a comma-separated list of positive integers.");
      }
      break;
    }
    case "heartbeat": {
      const interval = parsePositiveInt(draft.heartbeatInterval);
      if (interval === null || interval < 30) errors.push("Heartbeat interval must be an integer of at least 30 seconds.");
      if (draft.heartbeatGraceSeconds.trim()) {
        const grace = Number(draft.heartbeatGraceSeconds);
        if (!Number.isInteger(grace) || grace < 0) errors.push("Heartbeat grace window must be a non-negative integer.");
      }
      break;
    }
    case "tcp": {
      if (!draft.tcpTargetHost.trim()) errors.push("TCP target host is required.");
      const port = parsePositiveInt(draft.tcpTargetPort);
      if (port === null || port > 65535) errors.push("TCP port must be an integer between 1 and 65535.");
      const interval = parsePositiveInt(draft.tcpInterval);
      if (interval === null || interval < 30) errors.push("TCP interval must be an integer of at least 30 seconds.");
      if (draft.tcpMaxConnectTimeMs.trim() && parsePositiveInt(draft.tcpMaxConnectTimeMs) === null) errors.push("TCP max connect time must be a positive integer.");
      if (draft.tcpTimeoutSecondsOverride.trim() && parsePositiveInt(draft.tcpTimeoutSecondsOverride) === null) errors.push("TCP timeout override must be a positive integer.");
      if (draft.tcpMaxAttemptsOverride.trim() && parsePositiveInt(draft.tcpMaxAttemptsOverride) === null) errors.push("TCP max attempts override must be a positive integer.");
      if (draft.tcpRetryDelaysMsOverride.trim()) {
        const tokens = draft.tcpRetryDelaysMsOverride.split(",").map((t) => t.trim()).filter(Boolean);
        if (tokens.length === 0 || tokens.some((t) => parsePositiveInt(t) === null)) errors.push("TCP retry delays must be a comma-separated list of positive integers.");
      }
      break;
    }
    case "dns": {
      if (!draft.dnsHostname.trim()) errors.push("DNS hostname is required.");
      const validTypes = new Set(["A", "AAAA", "CNAME", "MX", "TXT", "NS"]);
      if (!validTypes.has(draft.dnsRecordType.trim().toUpperCase())) errors.push("DNS record type must be one of: A, AAAA, CNAME, MX, TXT, NS.");
      const interval = parsePositiveInt(draft.dnsInterval);
      if (interval === null || interval < 30) errors.push("DNS interval must be an integer of at least 30 seconds.");
      if (draft.dnsNameserver.trim()) {
        const parsed = draft.dnsNameserver.trim();
        const isIp = /^\d{1,3}(\.\d{1,3}){3}$/.test(parsed) || parsed.includes(":");
        if (!isIp) errors.push("DNS nameserver must be an IP address.");
      }
      if (draft.dnsTimeoutSecondsOverride.trim() && parsePositiveInt(draft.dnsTimeoutSecondsOverride) === null) errors.push("DNS timeout override must be a positive integer.");
      if (draft.dnsMaxAttemptsOverride.trim() && parsePositiveInt(draft.dnsMaxAttemptsOverride) === null) errors.push("DNS max attempts override must be a positive integer.");
      if (draft.dnsRetryDelaysMsOverride.trim()) {
        const tokens = draft.dnsRetryDelaysMsOverride.split(",").map((t) => t.trim()).filter(Boolean);
        if (tokens.length === 0 || tokens.some((t) => parsePositiveInt(t) === null)) errors.push("DNS retry delays must be a comma-separated list of positive integers.");
      }
      break;
    }
  }

  return errors;
}

export function buildDefaultDraft(summary: SiteSummary, type: SiteMonitorType, inventory: SiteMonitorInventory | null): ConfigureDraft {
  const siteUrl = tryParseUrl(summary.site.base_url);
  const httpMonitor = inventory?.http[0] ?? summary.http_monitors[0];
  const sslMonitor = inventory?.ssl[0] ?? summary.ssl_monitors[0];
  const heartbeatMonitor = inventory?.heartbeat[0] ?? summary.heartbeat_monitors[0];
  const tcpMonitor = inventory?.tcp[0];
  const dnsMonitor = inventory?.dns[0];

  return {
    type,
    httpTargetUrl: httpMonitor?.target_url ?? summary.site.base_url,
    httpInterval: String(httpMonitor?.check_interval_seconds ?? 60),
    httpExpectedStatusCode: String(httpMonitor?.expected_status_code ?? 200),
    httpMaxResponseTimeMs: httpMonitor?.max_response_time_ms ? String(httpMonitor.max_response_time_ms) : "",
    httpBodyMustContain: httpMonitor?.body_must_contain ?? "",
    httpBodyMustNotContain: httpMonitor?.body_must_not_contain ?? "",
    httpBodyMustContainTexts: (httpMonitor?.body_must_contain_texts ?? []).join("\n"),
    httpBodyMustNotContainTexts: (httpMonitor?.body_must_not_contain_texts ?? []).join("\n"),
    httpRequiredHeaderName: httpMonitor?.required_header_name ?? "",
    httpRequiredHeaderValue: httpMonitor?.required_header_value ?? "",
    httpHeaderAssertions: (httpMonitor?.header_assertions ?? []).map((a) => ({ name: a.name, equals: a.equals ?? "", contains: a.contains ?? "" })),
    httpJsonPathExists: (httpMonitor?.json_path_exists ?? []).join("\n"),
    httpJsonPathEquals: (httpMonitor?.json_path_equals ?? []).map((a) => ({ path: a.path, value: typeof a.value === "string" ? a.value : JSON.stringify(a.value) })),
    httpJsonPathNotEquals: (httpMonitor?.json_path_not_equals ?? []).map((a) => ({ path: a.path, value: typeof a.value === "string" ? a.value : JSON.stringify(a.value) })),
    httpTimeoutSecondsOverride: httpMonitor?.http_check_timeout_seconds_override ? String(httpMonitor.http_check_timeout_seconds_override) : "",
    httpMaxAttemptsOverride: httpMonitor?.http_check_max_attempts_override ? String(httpMonitor.http_check_max_attempts_override) : "",
    httpRetryDelaysMsOverride: (httpMonitor?.http_check_retry_delays_ms_override ?? []).join(", "),
    sslTargetUrl: sslMonitor?.target_url ?? (siteUrl ? `https://${siteUrl.host}${siteUrl.pathname}` : summary.site.base_url),
    sslInterval: String(sslMonitor?.check_interval_seconds ?? 300),
    sslWarningDays: String(sslMonitor?.ssl_expiry_warning_days ?? 14),
    sslTimeoutSecondsOverride: sslMonitor?.http_check_timeout_seconds_override ? String(sslMonitor.http_check_timeout_seconds_override) : "",
    sslMaxAttemptsOverride: sslMonitor?.http_check_max_attempts_override ? String(sslMonitor.http_check_max_attempts_override) : "",
    sslRetryDelaysMsOverride: (sslMonitor?.http_check_retry_delays_ms_override ?? []).join(", "),
    heartbeatInterval: String(heartbeatMonitor?.check_interval_seconds ?? 60),
    heartbeatGraceSeconds: String(heartbeatMonitor?.heartbeat_grace_seconds ?? 15),
    tcpTargetHost: tcpMonitor?.target_host ?? siteUrl?.hostname ?? "",
    tcpTargetPort: String(tcpMonitor?.target_port ?? (siteUrl?.port ? Number(siteUrl.port) : siteUrl?.protocol === "https:" ? 443 : 80)),
    tcpInterval: String(tcpMonitor?.check_interval_seconds ?? 60),
    tcpMaxConnectTimeMs: tcpMonitor?.max_connect_time_ms ? String(tcpMonitor.max_connect_time_ms) : "",
    tcpTimeoutSecondsOverride: tcpMonitor?.timeout_seconds_override ? String(tcpMonitor.timeout_seconds_override) : "",
    tcpMaxAttemptsOverride: tcpMonitor?.max_attempts_override ? String(tcpMonitor.max_attempts_override) : "",
    tcpRetryDelaysMsOverride: (tcpMonitor?.retry_delays_ms_override ?? []).join(", "),
    dnsHostname: dnsMonitor?.hostname ?? siteUrl?.hostname ?? "",
    dnsRecordType: dnsMonitor?.record_type ?? "A",
    dnsExpectedValue: dnsMonitor?.expected_value ?? "",
    dnsNameserver: dnsMonitor?.nameserver ?? "",
    dnsInterval: String(dnsMonitor?.check_interval_seconds ?? 60),
    dnsTimeoutSecondsOverride: dnsMonitor?.timeout_seconds_override ? String(dnsMonitor.timeout_seconds_override) : "",
    dnsMaxAttemptsOverride: dnsMonitor?.max_attempts_override ? String(dnsMonitor.max_attempts_override) : "",
    dnsRetryDelaysMsOverride: (dnsMonitor?.retry_delays_ms_override ?? []).join(", "),
  };
}

export function StatusBadge({ label, tone }: { label: string; tone: StatusTone }) {
  return (
    <span className={`status-badge status-badge-${tone}`}>
      <span className="status-badge-icon" aria-hidden="true">
        {tone === "success" ? (
          <svg viewBox="0 0 20 20">
            <path d="M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Zm3.72-10.78-4.6 4.6-2.84-2.84-1.06 1.06 3.9 3.9 5.66-5.66-1.06-1.06Z" fill="currentColor" />
          </svg>
        ) : tone === "danger" ? (
          <svg viewBox="0 0 20 20">
            <path d="M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Zm3.53-10.47-1.06-1.06L10 8.94 7.53 6.47 6.47 7.53 8.94 10l-2.47 2.47 1.06 1.06L10 11.06l2.47 2.47 1.06-1.06L11.06 10l2.47-2.47Z" fill="currentColor" />
          </svg>
        ) : tone === "warning" ? (
          <svg viewBox="0 0 20 20">
            <path d="M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Zm.75-12.5h-1.5v5h1.5v-5Zm0 6.5h-1.5v1.5h1.5V12Z" fill="currentColor" />
          </svg>
        ) : (
          <svg viewBox="0 0 20 20">
            <path d="M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Zm.75-11.5h-1.5v4.25l3.5 2.1.75-1.23-2.75-1.62V6.5Z" fill="currentColor" />
          </svg>
        )}
      </span>
      <span className="status-badge-label">{label}</span>
    </span>
  );
}
