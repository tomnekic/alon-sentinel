import { useState } from "react";
import {
  type AuthSession,
  type Site,
  authorizedRequest,
  createSite,
  deleteSite,
  updateSite,
} from "../api";
import { buildQuery } from "../utils";
import type { WithApiFunc } from "./useAuth";

export function useSites(withApi: WithApiFunc, createRefreshSignal: () => AbortSignal) {
  const [sites, setSites] = useState<Site[]>([]);
  const [siteSearchQuery, setSiteSearchQuery] = useState("");
  const [sitePage, setSitePage] = useState(1);
  const [sitePageSize, setSitePageSize] = useState(10);
  const [siteTotalCount, setSiteTotalCount] = useState(0);
  const [isRefreshingSites, setIsRefreshingSites] = useState(false);
  const [lastSitesRefreshAt, setLastSitesRefreshAt] = useState<number | null>(null);

  async function refreshSites(options?: {
    activeSession?: AuthSession;
    page?: number;
    pageSize?: number;
    searchQuery?: string;
    skipLoadingState?: boolean;
  }) {
    const nextPage = options?.page ?? sitePage;
    const nextPageSize = options?.pageSize ?? sitePageSize;
    const nextSearchQuery = options?.searchQuery ?? siteSearchQuery;

    if (!options?.skipLoadingState) setIsRefreshingSites(true);

    const signal = options?.activeSession ? undefined : createRefreshSignal();
    const runner = options?.activeSession
      ? async <T,>(cb: (s: AuthSession) => Promise<T>) => cb(options.activeSession as AuthSession)
      : async <T,>(cb: (s: AuthSession) => Promise<T>) => withApi("sites", cb);

    await runner(async (activeSession) => {
      const result = await authorizedRequest<Site[]>(
        activeSession,
        `/v1/sites${buildQuery({ q: nextSearchQuery || null, page: nextPage, limit: nextPageSize })}`,
        { signal }
      );
      setSites(result.response.items);
      setSiteTotalCount(result.response.totalCount ?? result.response.items.length);
      setSitePage(result.response.page ?? nextPage);
      setSitePageSize(result.response.pageSize ?? nextPageSize);
      setLastSitesRefreshAt(Date.now());
    });

    if (!options?.skipLoadingState) setIsRefreshingSites(false);
  }

  async function handleCreateSite(payload: { name: string; base_url: string }): Promise<boolean> {
    const normalizedName = payload.name.trim();
    const normalizedBaseUrl = payload.base_url.trim();
    if (!normalizedName || !normalizedBaseUrl) return false;

    const result = await withApi("sites", (s) =>
      createSite(s, { name: normalizedName, base_url: normalizedBaseUrl })
    );
    if (!result) return false;
    await refreshSites();
    return true;
  }

  async function handleUpdateSite(
    siteId: number,
    payload: { name: string; base_url: string; is_active: boolean }
  ): Promise<Site | null> {
    const normalizedName = payload.name.trim();
    const normalizedBaseUrl = payload.base_url.trim();
    if (!normalizedName || !normalizedBaseUrl) return null;

    const result = await withApi("sites", (s) =>
      updateSite(s, siteId, { name: normalizedName, base_url: normalizedBaseUrl, is_active: payload.is_active })
    );
    if (!result) return null;
    await refreshSites();
    return result.site;
  }

  async function handleDeleteSite(siteId: number): Promise<boolean> {
    const result = await withApi("sites", (s) => deleteSite(s, siteId));
    if (!result) return false;
    await refreshSites();
    return true;
  }

  function handleSiteSearchChange(value: string) {
    setSiteSearchQuery(value);
    setSitePage(1);
    void refreshSites({ searchQuery: value, page: 1 });
  }

  function handleSitePageChange(page: number) {
    setSitePage(page);
    void refreshSites({ page });
  }

  function handleSitePageSizeChange(pageSize: number) {
    setSitePageSize(pageSize);
    setSitePage(1);
    void refreshSites({ pageSize, page: 1 });
  }

  function reset() {
    setSites([]);
    setSiteSearchQuery("");
    setSitePage(1);
    setSitePageSize(10);
    setSiteTotalCount(0);
  }

  return {
    sites,
    siteSearchQuery,
    sitePage,
    sitePageSize,
    siteTotalCount,
    isRefreshingSites,
    lastSitesRefreshAt,
    refreshSites,
    handleCreateSite,
    handleUpdateSite,
    handleDeleteSite,
    handleSiteSearchChange,
    handleSitePageChange,
    handleSitePageSizeChange,
    reset,
  };
}
