import { useState } from "react";
import {
  type NotificationChannel,
  type NotificationChannelSetup,
  createNotificationChannel,
  deleteNotificationChannel,
  listNotificationChannels,
  updateNotificationChannel,
} from "../api";
import type { WithApiFunc } from "./useAuth";

export function useNotifications(
  withApi: WithApiFunc,
  createRefreshSignal: () => AbortSignal,
  canRead: boolean
) {
  const [channels, setChannels] = useState<NotificationChannel[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastRefreshAt, setLastRefreshAt] = useState<number | null>(null);

  async function refreshChannels(options?: { skipLoadingState?: boolean }) {
    if (!canRead) {
      setChannels([]);
      return;
    }
    if (!options?.skipLoadingState) setIsRefreshing(true);
    const signal = createRefreshSignal();
    await withApi("notifications", async (activeSession) => {
      const r = await listNotificationChannels(activeSession, signal);
      setChannels(r.channels);
      setLastRefreshAt(Date.now());
    });
    if (!options?.skipLoadingState) setIsRefreshing(false);
  }

  async function handleCreateChannel(payload: NotificationChannelSetup): Promise<number | null> {
    const result = await withApi("notifications", (s) => createNotificationChannel(s, payload));
    if (!result) return null;
    await refreshChannels();
    return result.channel.id;
  }

  async function handleUpdateChannel(
    channelId: number,
    payload: NotificationChannelSetup
  ): Promise<number | null> {
    const result = await withApi("notifications", (s) =>
      updateNotificationChannel(s, channelId, payload)
    );
    if (!result) return null;
    await refreshChannels();
    return result.channel.id;
  }

  async function handleDeleteChannel(channelId: number): Promise<boolean> {
    const result = await withApi("notifications", (s) => deleteNotificationChannel(s, channelId));
    if (!result) return false;
    await refreshChannels();
    return true;
  }

  function reset() {
    setChannels([]);
  }

  return {
    channels,
    isRefreshing,
    lastRefreshAt,
    refreshChannels,
    handleCreateChannel,
    handleUpdateChannel,
    handleDeleteChannel,
    reset,
  };
}
