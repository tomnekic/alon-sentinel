import { useState } from "react";
import {
  type AuthSession,
  type DashboardSiteEntry,
  type GlobalDashboardStats,
  type GlobalIncident,
  getDashboardStats,
  listGlobalIncidents,
} from "../api";
import type { WithApiFunc } from "./useAuth";

export type GlobalIncidentItem = { incident: GlobalIncident };

export function useGlobalDashboard(withApi: WithApiFunc, createRefreshSignal: () => AbortSignal) {
  const [dashboardStats, setDashboardStats] = useState<GlobalDashboardStats | null>(null);
  const [dashboardSites, setDashboardSites] = useState<DashboardSiteEntry[]>([]);
  const [dashboardOpenIncidents, setDashboardOpenIncidents] = useState<GlobalIncidentItem[]>([]);
  const [isRefreshingHomeDashboard, setIsRefreshingHomeDashboard] = useState(false);
  const [lastHomeDashboardRefreshAt, setLastHomeDashboardRefreshAt] = useState<number | null>(null);

  async function refreshGlobalDashboard(options?: { activeSession?: AuthSession }) {
    setIsRefreshingHomeDashboard(true);
    const signal = options?.activeSession ? undefined : createRefreshSignal();

    const [statsResult, incidentsResult] = await Promise.all([
      options?.activeSession
        ? getDashboardStats(options.activeSession)
        : withApi("dashboard", (s) => getDashboardStats(s, signal)),
      options?.activeSession
        ? listGlobalIncidents(options.activeSession, { status: "open", limit: 50 })
        : withApi("dashboard", (s) => listGlobalIncidents(s, { status: "open", limit: 50, signal })),
    ]);

    if (!statsResult || !incidentsResult || signal?.aborted) {
      setIsRefreshingHomeDashboard(false);
      return;
    }

    setDashboardStats(statsResult.stats);
    setDashboardSites(statsResult.sites);
    setDashboardOpenIncidents(
      incidentsResult.incidents
        .sort((a, b) => new Date(a.opened_at).getTime() - new Date(b.opened_at).getTime())
        .map((incident) => ({ incident }))
    );
    setLastHomeDashboardRefreshAt(Date.now());
    setIsRefreshingHomeDashboard(false);
  }

  return {
    dashboardStats,
    dashboardSites,
    dashboardOpenIncidents,
    isRefreshingHomeDashboard,
    lastHomeDashboardRefreshAt,
    refreshGlobalDashboard,
  };
}
