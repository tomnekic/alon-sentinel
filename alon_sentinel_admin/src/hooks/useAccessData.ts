import { useState } from "react";
import {
  type ManagedAdminUser,
  type ManagedPermission,
  type ManagedRole,
  createManagedAdminUser,
  createManagedRole,
  deleteManagedAdminUser,
  deleteManagedRole,
  listManagedAdminUsers,
  listManagedPermissions,
  listManagedRoles,
  updateManagedAdminUser,
  updateManagedRole,
} from "../api";
import type { WithApiFunc } from "./useAuth";

export function useAccessData(
  withApi: WithApiFunc,
  createRefreshSignal: () => AbortSignal,
  canReadUsers: boolean,
  canReadRoles: boolean
) {
  const [managedUsers, setManagedUsers] = useState<ManagedAdminUser[]>([]);
  const [managedRoles, setManagedRoles] = useState<ManagedRole[]>([]);
  const [managedPermissions, setManagedPermissions] = useState<ManagedPermission[]>([]);
  const [isRefreshingAccess, setIsRefreshingAccess] = useState(false);
  const [lastAccessRefreshAt, setLastAccessRefreshAt] = useState<number | null>(null);

  async function refreshAccessData(options?: { skipLoadingState?: boolean }) {
    if (!options?.skipLoadingState) setIsRefreshingAccess(true);

    const signal = createRefreshSignal();
    await withApi("access", async (activeSession) => {
      let nextSession = activeSession;
      if (canReadUsers) {
        const r = await listManagedAdminUsers(nextSession, signal);
        nextSession = r.session;
        setManagedUsers(r.users);
      } else {
        setManagedUsers([]);
      }

      if (canReadRoles) {
        const rolesResult = await listManagedRoles(nextSession, signal);
        nextSession = rolesResult.session;
        setManagedRoles(rolesResult.roles);

        const permsResult = await listManagedPermissions(nextSession, signal);
        setManagedPermissions(permsResult.permissions);
      } else {
        setManagedRoles([]);
        setManagedPermissions([]);
      }

      setLastAccessRefreshAt(Date.now());
    });

    if (!options?.skipLoadingState) setIsRefreshingAccess(false);
  }

  async function handleCreateManagedUser(payload: {
    email: string;
    display_name: string;
    password: string;
    is_active?: boolean;
    role_keys: string[];
  }): Promise<number | null> {
    const result = await withApi("access", (s) => createManagedAdminUser(s, payload));
    if (!result) return null;
    await refreshAccessData();
    return result.user.id;
  }

  async function handleUpdateManagedUser(
    userId: number,
    payload: { email: string; display_name: string; password?: string; is_active: boolean; role_keys: string[] }
  ): Promise<number | null> {
    const result = await withApi("access", (s) => updateManagedAdminUser(s, userId, payload));
    if (!result) return null;
    await refreshAccessData();
    return result.user.id;
  }

  async function handleDeleteManagedUser(userId: number): Promise<boolean> {
    const result = await withApi("access", (s) => deleteManagedAdminUser(s, userId));
    if (!result) return false;
    await refreshAccessData();
    return true;
  }

  async function handleCreateManagedRole(payload: {
    key: string;
    name: string;
    description?: string;
    permission_keys: string[];
  }): Promise<number | null> {
    const result = await withApi("access", (s) => createManagedRole(s, payload));
    if (!result) return null;
    await refreshAccessData();
    return result.role.id;
  }

  async function handleUpdateManagedRole(
    roleId: number,
    payload: { name: string; description?: string; permission_keys: string[] }
  ): Promise<number | null> {
    const result = await withApi("access", (s) => updateManagedRole(s, roleId, payload));
    if (!result) return null;
    await refreshAccessData();
    return result.role.id;
  }

  async function handleDeleteManagedRole(roleId: number): Promise<boolean> {
    const result = await withApi("access", (s) => deleteManagedRole(s, roleId));
    if (!result) return false;
    await refreshAccessData();
    return true;
  }

  function reset() {
    setManagedUsers([]);
    setManagedRoles([]);
    setManagedPermissions([]);
  }

  return {
    managedUsers,
    managedRoles,
    managedPermissions,
    isRefreshingAccess,
    lastAccessRefreshAt,
    refreshAccessData,
    handleCreateManagedUser,
    handleUpdateManagedUser,
    handleDeleteManagedUser,
    handleCreateManagedRole,
    handleUpdateManagedRole,
    handleDeleteManagedRole,
    reset,
  };
}
