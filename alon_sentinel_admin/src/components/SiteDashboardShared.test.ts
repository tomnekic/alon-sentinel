import { describe, expect, it } from "vitest";
import { validateConfigureDraft, type ConfigureDraft } from "./SiteDashboardShared";

function draft(overrides: Partial<ConfigureDraft> = {}): ConfigureDraft {
  return {
    type: "http",
    httpTargetUrl: "https://example.com/health",
    httpInterval: "60",
    httpExpectedStatusCode: "200",
    httpMaxResponseTimeMs: "",
    httpBodyMustContain: "",
    httpBodyMustNotContain: "",
    httpBodyMustContainTexts: "",
    httpBodyMustNotContainTexts: "",
    httpRequiredHeaderName: "",
    httpRequiredHeaderValue: "",
    httpHeaderAssertions: [],
    httpJsonPathExists: "",
    httpJsonPathEquals: [],
    httpJsonPathNotEquals: [],
    httpTimeoutSecondsOverride: "",
    httpMaxAttemptsOverride: "",
    httpRetryDelaysMsOverride: "",
    sslTargetUrl: "https://example.com",
    sslInterval: "300",
    sslWarningDays: "14",
    sslTimeoutSecondsOverride: "",
    sslMaxAttemptsOverride: "",
    sslRetryDelaysMsOverride: "",
    heartbeatInterval: "60",
    heartbeatGraceSeconds: "15",
    tcpTargetHost: "example.com",
    tcpTargetPort: "443",
    tcpInterval: "60",
    tcpMaxConnectTimeMs: "",
    tcpTimeoutSecondsOverride: "",
    tcpMaxAttemptsOverride: "",
    tcpRetryDelaysMsOverride: "",
    dnsHostname: "example.com",
    dnsRecordType: "A",
    dnsExpectedValue: "",
    dnsNameserver: "",
    dnsInterval: "60",
    dnsTimeoutSecondsOverride: "",
    dnsMaxAttemptsOverride: "",
    dnsRetryDelaysMsOverride: "",
    ...overrides,
  };
}

describe("validateConfigureDraft", () => {
  it("accepts a complete HTTP monitor draft with assertions and retry overrides", () => {
    expect(validateConfigureDraft(draft({
      httpMaxResponseTimeMs: "750",
      httpRequiredHeaderName: "x-service",
      httpRequiredHeaderValue: "sentinel",
      httpHeaderAssertions: [{ name: "cache-control", equals: "", contains: "no-cache" }],
      httpJsonPathEquals: [{ path: "$.status", value: "\"ok\"" }],
      httpJsonPathNotEquals: [{ path: "$.degraded", value: "true" }],
      httpTimeoutSecondsOverride: "5",
      httpMaxAttemptsOverride: "3",
      httpRetryDelaysMsOverride: "100, 250, 500",
    }))).toEqual([]);
  });

  it("rejects malformed HTTP monitor fields before submit", () => {
    expect(validateConfigureDraft(draft({
      httpTargetUrl: "ftp://example.com",
      httpInterval: "29",
      httpExpectedStatusCode: "99",
      httpMaxResponseTimeMs: "0",
      httpRequiredHeaderName: "",
      httpRequiredHeaderValue: "sentinel",
      httpHeaderAssertions: [{ name: "x-service", equals: "", contains: "" }],
      httpJsonPathEquals: [{ path: "$.status", value: "" }],
      httpRetryDelaysMsOverride: "100, nope",
    }))).toEqual([
      "HTTP target URL must be a valid http/https URL.",
      "HTTP interval must be an integer of at least 30 seconds.",
      "HTTP expected status code must be an integer between 100 and 599.",
      "HTTP max response time must be a positive integer.",
      "HTTP retry delays must be a comma-separated list of positive integers.",
      "HTTP required header value requires a header name.",
      "Each HTTP header assertion needs at least one condition (equals or contains).",
      "HTTP JSON path equals assertions require a value.",
    ]);
  });

  it("enforces SSL monitor https targets and positive numeric fields", () => {
    expect(validateConfigureDraft(draft({
      type: "ssl",
      sslTargetUrl: "http://example.com",
      sslInterval: "10",
      sslWarningDays: "soon",
    }))).toEqual([
      "SSL target URL must be a valid https URL.",
      "SSL interval must be an integer of at least 30 seconds.",
      "SSL warning days must be a positive integer.",
    ]);

    expect(validateConfigureDraft(draft({
      type: "ssl",
      sslWarningDays: "7",
    }))).toEqual([
      "SSL warning days must be greater than 7.",
    ]);

    expect(validateConfigureDraft(draft({
      type: "ssl",
      sslWarningDays: "8",
    }))).toEqual([]);

    expect(validateConfigureDraft(draft({
      type: "ssl",
      sslTimeoutSecondsOverride: "0",
      sslMaxAttemptsOverride: "bad",
      sslRetryDelaysMsOverride: "100, nope",
    }))).toEqual([
      "SSL timeout override must be a positive integer.",
      "SSL max attempts override must be a positive integer.",
      "SSL retry delays must be a comma-separated list of positive integers.",
    ]);
  });

  it("enforces heartbeat interval and allows zero grace seconds", () => {
    expect(validateConfigureDraft(draft({
      type: "heartbeat",
      heartbeatInterval: "30",
      heartbeatGraceSeconds: "0",
    }))).toEqual([]);

    expect(validateConfigureDraft(draft({
      type: "heartbeat",
      heartbeatInterval: "5",
      heartbeatGraceSeconds: "-1",
    }))).toEqual([
      "Heartbeat interval must be an integer of at least 30 seconds.",
      "Heartbeat grace window must be a non-negative integer.",
    ]);
  });

  it("enforces TCP host, port, interval, max connect time, and advanced overrides", () => {
    expect(validateConfigureDraft(draft({
      type: "tcp",
      tcpTargetHost: "",
      tcpTargetPort: "70000",
      tcpInterval: "0",
      tcpMaxConnectTimeMs: "slow",
    }))).toEqual([
      "TCP target host is required.",
      "TCP port must be an integer between 1 and 65535.",
      "TCP interval must be an integer of at least 30 seconds.",
      "TCP max connect time must be a positive integer.",
    ]);

    expect(validateConfigureDraft(draft({
      type: "tcp",
      tcpTimeoutSecondsOverride: "0",
      tcpMaxAttemptsOverride: "bad",
      tcpRetryDelaysMsOverride: "100, nope",
    }))).toEqual([
      "TCP timeout override must be a positive integer.",
      "TCP max attempts override must be a positive integer.",
      "TCP retry delays must be a comma-separated list of positive integers.",
    ]);
  });

  it("enforces DNS hostname, record type, interval, nameserver shape, and advanced overrides", () => {
    expect(validateConfigureDraft(draft({
      type: "dns",
      dnsHostname: "",
      dnsRecordType: "SOA",
      dnsInterval: "15",
      dnsNameserver: "resolver.example.com",
    }))).toEqual([
      "DNS hostname is required.",
      "DNS record type must be one of: A, AAAA, CNAME, MX, TXT, NS.",
      "DNS interval must be an integer of at least 30 seconds.",
      "DNS nameserver must be an IP address.",
    ]);

    expect(validateConfigureDraft(draft({
      type: "dns",
      dnsTimeoutSecondsOverride: "0",
      dnsMaxAttemptsOverride: "bad",
      dnsRetryDelaysMsOverride: "100, nope",
    }))).toEqual([
      "DNS timeout override must be a positive integer.",
      "DNS max attempts override must be a positive integer.",
      "DNS retry delays must be a comma-separated list of positive integers.",
    ]);
  });
});
