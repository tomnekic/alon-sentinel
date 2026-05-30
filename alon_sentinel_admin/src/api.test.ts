import { afterEach, describe, expect, it, vi } from "vitest";
import {
  authorizedRequest,
  configureHttpMonitor,
  getSiteChecks,
  issueAdminToken,
  type AuthSession,
} from "./api";

const user = {
  id: 1,
  email: "admin@example.com",
  display_name: "Admin",
  is_active: true,
  last_login_at: null,
  created_at: "2026-05-13T00:00:00Z",
  updated_at: "2026-05-13T00:00:00Z",
};

function activeSession(overrides: Partial<AuthSession> = {}): AuthSession {
  return {
    expiresAt: "2999-01-01T00:00:00Z",
    expiresIn: 3600,
    roles: ["admin"],
    permissions: ["sites.read", "sites.update"],
    user,
    ...overrides,
  };
}

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json", ...(init.headers ?? {}) },
    ...init,
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("issueAdminToken", () => {
  it("trims the email before login and normalizes the session response", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse({
        expires_at: "2026-05-13T19:00:00Z",
        expires_in: 900,
        roles: ["operator"],
        permissions: ["sites.read"],
        user,
      })
    );

    await expect(issueAdminToken(" admin@example.com ", "secret")).resolves.toMatchObject({
      expiresAt: "2026-05-13T19:00:00Z",
      expiresIn: 900,
      roles: ["operator"],
      permissions: ["sites.read"],
      user,
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/v1/admin/auth/login",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ email: "admin@example.com", password: "secret" }),
      })
    );
  });

  it("surfaces API error payloads", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse({ error: "Invalid credentials" }, { status: 401 })
    );

    await expect(issueAdminToken("admin@example.com", "bad")).rejects.toThrow("Invalid credentials");
  });
});

describe("authorized API helpers", () => {
  it("rejects expired sessions before making a request", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch");

    await expect(
      authorizedRequest(activeSession({ expiresAt: "2000-01-01T00:00:00Z" }), "/v1/sites")
    ).rejects.toThrow("Admin session has expired. Log in again.");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("sends monitor configuration to the expected endpoint as JSON", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse({ id: 55, monitor_type: "http" })
    );

    await configureHttpMonitor(activeSession(), 42, {
      target_url: "https://example.com/health",
      check_interval_seconds: 60,
      expected_status_code: 200,
      is_active: true,
      body_must_contain_texts: ["ok"],
      header_assertions: [{ name: "x-service", equals: "sentinel", contains: null }],
      json_path_equals: [{ path: "$.status", value: "ok" }],
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/v1/sites/42/monitoring/http",
      expect.objectContaining({
        method: "PUT",
        headers: expect.objectContaining({
          Accept: "application/json",
          "Content-Type": "application/json",
        }),
        body: JSON.stringify({
          target_url: "https://example.com/health",
          check_interval_seconds: 60,
          expected_status_code: 200,
          is_active: true,
          body_must_contain_texts: ["ok"],
          header_assertions: [{ name: "x-service", equals: "sentinel", contains: null }],
          json_path_equals: [{ path: "$.status", value: "ok" }],
        }),
      })
    );
  });

  it("preserves cursor pagination metadata from list responses", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse(
        [{ id: 1, is_success: true }],
        { headers: { "x-next-cursor": "next-page" } }
      )
    );

    const result = await getSiteChecks(activeSession(), 42, {
      filter: "failure",
      cursor: "current-page",
      limit: 10,
    });

    expect(globalThis.fetch).toHaveBeenCalledWith(
      "/v1/sites/42/checks?outcome=failure&cursor=current-page&limit=10",
      expect.any(Object)
    );
    expect(result.nextCursor).toBe("next-page");
    expect(result.checks).toEqual([{ id: 1, is_success: true }]);
  });
});
