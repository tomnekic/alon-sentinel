import { useEffect, useMemo, useRef, useState } from "react";
import { type AuthSession } from "../api";

const SESSION_STORAGE_KEY = "alon_sentinel_admin_session";

export type AppError = { scope: string; message: string } | null;

export type WithApiFunc = <T>(
  scope: string,
  callback: (session: AuthSession) => Promise<T>
) => Promise<T | null>;

function safeJsonParse<T>(value: string | null, fallback: T): T {
  if (!value) return fallback;
  try { return JSON.parse(value) as T; } catch { return fallback; }
}

function getSessionStorage(): Storage | null {
  if (typeof window === "undefined") return null;
  try { return window.sessionStorage; } catch { return null; }
}

export function useAuth() {
  const [session, setSession] = useState<AuthSession | null>(() =>
    safeJsonParse(getSessionStorage()?.getItem(SESSION_STORAGE_KEY) ?? null, null)
  );
  const [appError, setAppError] = useState<AppError>(null);
  const refreshAbortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (session) {
      getSessionStorage()?.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
    } else {
      getSessionStorage()?.removeItem(SESSION_STORAGE_KEY);
    }
  }, [session]);

  function createRefreshSignal(): AbortSignal {
    refreshAbortRef.current?.abort();
    const controller = new AbortController();
    refreshAbortRef.current = controller;
    return controller.signal;
  }

  const withApi: WithApiFunc = async <T,>(
    scope: string,
    callback: (activeSession: AuthSession) => Promise<T>
  ): Promise<T | null> => {
    if (!session) {
      setAppError({ scope, message: "Connect to Sentinel first." });
      return null;
    }
    try {
      const result = await callback(session);
      setAppError((current) => (current?.scope === scope ? null : current));
      return result;
    } catch (error) {
      if (error instanceof Error && error.name === "AbortError") return null;
      const message = error instanceof Error ? error.message : "Unknown API error.";
      if (message.includes("Log in again")) setSession(null);
      setAppError({ scope, message });
      return null;
    }
  };

  const connected = session !== null;
  const permissionSet = useMemo(
    () => new Set(session?.permissions ?? []),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [session?.permissions]
  );

  return {
    session,
    setSession,
    appError,
    setAppError,
    connected,
    permissionSet,
    createRefreshSignal,
    withApi,
  };
}
