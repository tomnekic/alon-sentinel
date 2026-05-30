import { useState } from "react";
import {
  type AuthSession,
  type Site,
  type SiteMonitorInventory,
  type SiteSummary,
  getSiteMonitorInventory,
  getSiteSummary,
} from "../api";
import type { WithApiFunc } from "./useAuth";

export function useSiteDashboard(
  withApi: WithApiFunc,
  createRefreshSignal: () => AbortSignal
) {
  const [selectedSite, setSelectedSite] = useState<Site | null>(null);
  const [selectedSiteSummary, setSelectedSiteSummary] = useState<SiteSummary | null>(null);
  const [selectedSiteMonitorInventory, setSelectedSiteMonitorInventory] = useState<SiteMonitorInventory | null>(null);
  const [isRefreshingDashboard, setIsRefreshingDashboard] = useState(false);
  const [lastSiteDashboardRefreshAt, setLastSiteDashboardRefreshAt] = useState<number | null>(null);

  async function refreshSiteDashboard(siteId: number, options?: { activeSession?: AuthSession; silent?: boolean }) {
    if (!options?.silent) setIsRefreshingDashboard(true);
    const signal = options?.activeSession ? undefined : createRefreshSignal();

    try {
      const result = await (options?.activeSession
        ? Promise.all([
            getSiteSummary(options.activeSession, siteId),
            getSiteMonitorInventory(options.activeSession, siteId),
          ]).then(([summaryResult, inventoryResult]) => ({
            summary: summaryResult.summary,
            inventory: inventoryResult.inventory,
          }))
        : withApi("dashboard", async (activeSession) => {
            const [summaryResult, inventoryResult] = await Promise.all([
              getSiteSummary(activeSession, siteId, signal),
              getSiteMonitorInventory(activeSession, siteId, signal),
            ]);
            return { summary: summaryResult.summary, inventory: inventoryResult.inventory };
          }));

      if (result) {
        setSelectedSite(result.summary.site);
        setSelectedSiteSummary(result.summary);
        setSelectedSiteMonitorInventory(result.inventory);
        setLastSiteDashboardRefreshAt(Date.now());
      }
    } finally {
      setIsRefreshingDashboard(false);
    }
  }

  function reset() {
    setSelectedSite(null);
    setSelectedSiteSummary(null);
    setSelectedSiteMonitorInventory(null);
  }

  return {
    selectedSite,
    setSelectedSite,
    selectedSiteSummary,
    selectedSiteMonitorInventory,
    isRefreshingDashboard,
    lastSiteDashboardRefreshAt,
    refreshSiteDashboard,
    reset,
  };
}
