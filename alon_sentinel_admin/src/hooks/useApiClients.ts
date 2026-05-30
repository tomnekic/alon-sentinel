import { useState } from "react";
import {
  type ApiClientScope,
  type ApiClientType,
  type AuthSession,
  type CreatedApiClient,
  type ManagedApiClient,
  createManagedApiClient,
  deleteManagedApiClient,
  listManagedApiClients,
  rotateManagedApiClientSecret,
  updateManagedApiClient,
} from "../api";
import type { WithApiFunc } from "./useAuth";

export function useApiClients(
  withApi: WithApiFunc,
  createRefreshSignal: () => AbortSignal,
  canRead: boolean
) {
  const [clients, setClients] = useState<ManagedApiClient[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastRefreshAt, setLastRefreshAt] = useState<number | null>(null);

  async function refreshClients(options?: { activeSession?: AuthSession }) {
    if (!canRead) return;
    setIsRefreshing(true);
    const signal = options?.activeSession ? undefined : createRefreshSignal();

    const result = await (options?.activeSession
      ? listManagedApiClients(options.activeSession, signal)
      : withApi("api_clients", (s) => listManagedApiClients(s, signal)));

    if (result) {
      setClients(result.clients);
      setLastRefreshAt(Date.now());
    }
    setIsRefreshing(false);
  }

  async function handleCreateClient(payload: {
    name: string;
    description?: string | null;
    client_type: ApiClientType;
    scopes: ApiClientScope[];
  }): Promise<CreatedApiClient> {
    const result = await withApi("api_clients", (s) =>
      createManagedApiClient(s, payload)
    );
    if (!result) throw new Error("Session expired");
    setClients((prev) => [result.client, ...prev]);
    return result.client;
  }

  async function handleUpdateClient(
    clientId: number,
    payload: { name: string; description?: string | null; is_active: boolean }
  ): Promise<void> {
    const result = await withApi("api_clients", (s) =>
      updateManagedApiClient(s, clientId, payload)
    );
    if (!result) throw new Error("Session expired");
    setClients((prev) => prev.map((c) => (c.id === clientId ? result.client : c)));
  }

  async function handleDeleteClient(clientId: number): Promise<void> {
    await withApi("api_clients", (s) => deleteManagedApiClient(s, clientId));
    setClients((prev) => prev.filter((c) => c.id !== clientId));
  }

  async function handleRotateSecret(clientId: number): Promise<CreatedApiClient> {
    const result = await withApi("api_clients", (s) =>
      rotateManagedApiClientSecret(s, clientId)
    );
    if (!result) throw new Error("Session expired");
    setClients((prev) => prev.map((c) => (c.id === clientId ? result.client : c)));
    return result.client;
  }

  function reset() {
    setClients([]);
    setLastRefreshAt(null);
  }

  return {
    clients,
    isRefreshing,
    lastRefreshAt,
    refreshClients,
    handleCreateClient,
    handleUpdateClient,
    handleDeleteClient,
    handleRotateSecret,
    reset,
  };
}
