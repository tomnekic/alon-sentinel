// @vitest-environment jsdom
import { renderHook, act } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useAuth } from "./useAuth";
import { type AuthSession } from "../api";

const SESSION_KEY = "alon_sentinel_admin_session";

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

beforeEach(() => {
  sessionStorage.clear();
});

describe("useAuth — withApi", () => {
  it("returns null and sets an error when no session is active", async () => {
    const { result } = renderHook(() => useAuth());

    let returnValue: unknown;
    await act(async () => {
      returnValue = await result.current.withApi("test-scope", async () => "data");
    });

    expect(returnValue).toBeNull();
    expect(result.current.appError).toEqual({
      scope: "test-scope",
      message: "Connect to Sentinel first.",
    });
  });

  it("returns the callback value and clears a matching scope error on success", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
      result.current.setAppError({ scope: "my-scope", message: "previous error" });
    });

    let returnValue: unknown;
    await act(async () => {
      returnValue = await result.current.withApi("my-scope", async () => "ok");
    });

    expect(returnValue).toBe("ok");
    expect(result.current.appError).toBeNull();
  });

  it("preserves an error from a different scope when another scope succeeds", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
      result.current.setAppError({ scope: "other-scope", message: "unrelated error" });
    });

    await act(async () => {
      await result.current.withApi("my-scope", async () => "ok");
    });

    expect(result.current.appError).toEqual({
      scope: "other-scope",
      message: "unrelated error",
    });
  });

  it("clears the session when the thrown error message includes 'Log in again'", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
    });
    expect(result.current.session).not.toBeNull();

    await act(async () => {
      await result.current.withApi("test", async () => {
        throw new Error("Admin session has expired. Log in again.");
      });
    });

    expect(result.current.session).toBeNull();
  });

  it("swallows AbortError silently — returns null without setting an app error", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
    });

    let returnValue: unknown;
    await act(async () => {
      returnValue = await result.current.withApi("test", async () => {
        const err = new Error("aborted");
        err.name = "AbortError";
        throw err;
      });
    });

    expect(returnValue).toBeNull();
    expect(result.current.appError).toBeNull();
  });
});

describe("useAuth — session persistence", () => {
  it("restores a valid session from sessionStorage on mount", () => {
    sessionStorage.setItem(SESSION_KEY, JSON.stringify(activeSession()));

    const { result } = renderHook(() => useAuth());

    expect(result.current.session).toMatchObject({ user: { email: "admin@example.com" } });
    expect(result.current.connected).toBe(true);
  });

  it("starts with no session when sessionStorage contains malformed JSON", () => {
    sessionStorage.setItem(SESSION_KEY, "{not valid json");

    const { result } = renderHook(() => useAuth());

    expect(result.current.session).toBeNull();
  });

  it("writes the session to sessionStorage when set", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
    });

    expect(sessionStorage.getItem(SESSION_KEY)).not.toBeNull();
  });

  it("removes the sessionStorage entry on logout", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession());
    });
    expect(sessionStorage.getItem(SESSION_KEY)).not.toBeNull();

    await act(async () => {
      result.current.setSession(null);
    });
    expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
  });
});

describe("useAuth — permissionSet", () => {
  it("exposes session permissions as a Set for O(1) lookup", async () => {
    const { result } = renderHook(() => useAuth());

    await act(async () => {
      result.current.setSession(activeSession({ permissions: ["sites.read", "users.manage"] }));
    });

    expect(result.current.permissionSet.has("sites.read")).toBe(true);
    expect(result.current.permissionSet.has("users.manage")).toBe(true);
    expect(result.current.permissionSet.has("sites.delete")).toBe(false);
  });

  it("returns an empty Set when not connected", () => {
    const { result } = renderHook(() => useAuth());
    expect(result.current.permissionSet.size).toBe(0);
  });
});
