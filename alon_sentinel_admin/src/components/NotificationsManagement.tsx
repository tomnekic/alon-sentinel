import { type FormEvent, useState } from "react";
import type { NotificationChannel, NotificationChannelSetup, NotificationChannelType } from "../api";

type NotificationsManagementProps = {
  canWrite: boolean;
  channels: NotificationChannel[];
  isRefreshing: boolean;
  onRefresh: () => void;
  onCreateChannel: (payload: NotificationChannelSetup) => Promise<number | null>;
  onUpdateChannel: (channelId: number, payload: NotificationChannelSetup) => Promise<number | null>;
  onDeleteChannel: (channelId: number) => Promise<boolean>;
};

type ChannelModalMode = "create" | "edit" | null;

type ChannelDraft = {
  id: number | null;
  channel_type: NotificationChannelType;
  name: string;
  destination: string;
  webhook_secret: string;
  notify_on_failure: boolean;
  notify_on_recovery: boolean;
  is_active: boolean;
};

function emptyDraft(): ChannelDraft {
  return {
    id: null,
    channel_type: "webhook",
    name: "",
    destination: "",
    webhook_secret: "",
    notify_on_failure: true,
    notify_on_recovery: true,
    is_active: true,
  };
}

function draftFromChannel(channel: NotificationChannel): ChannelDraft {
  return {
    id: channel.id,
    channel_type: channel.channel_type,
    name: channel.name,
    destination: channel.destination,
    webhook_secret: "",
    notify_on_failure: channel.notify_on_failure,
    notify_on_recovery: channel.notify_on_recovery,
    is_active: channel.is_active,
  };
}

function formatTimestamp(value: string | null): string {
  if (!value) return "Never";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

export function NotificationsManagement({
  canWrite,
  channels,
  isRefreshing,
  onRefresh,
  onCreateChannel,
  onUpdateChannel,
  onDeleteChannel,
}: NotificationsManagementProps) {
  const [modalMode, setModalMode] = useState<ChannelModalMode>(null);
  const [draft, setDraft] = useState<ChannelDraft>(emptyDraft);
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<{ id: number; name: string } | null>(null);
  const [formError, setFormError] = useState<string | null>(null);

  function openCreate() {
    setDraft(emptyDraft());
    setFormError(null);
    setModalMode("create");
  }

  function openEdit(channel: NotificationChannel) {
    setDraft(draftFromChannel(channel));
    setFormError(null);
    setModalMode("edit");
  }

  function closeModal() {
    setModalMode(null);
    setDraft(emptyDraft());
    setFormError(null);
  }

  function openDelete(channel: NotificationChannel) {
    setDeleteTarget({ id: channel.id, name: channel.name });
  }

  function validateDraft(): string | null {
    if (!draft.name.trim()) return "Name is required.";
    if (!draft.destination.trim()) return "Destination is required.";
    if (!draft.notify_on_failure && !draft.notify_on_recovery)
      return "At least one notification event (failure or recovery) must be enabled.";
    if (draft.channel_type === "webhook" && modalMode === "create" && !draft.webhook_secret.trim())
      return "Webhook secret is required for new webhook channels.";
    return null;
  }

  function channelDestinationLabel(type: NotificationChannelType): string {
    if (type === "email") return "Email Address";
    if (type === "slack") return "Slack Webhook URL";
    if (type === "discord") return "Discord Webhook URL";
    return "Webhook URL";
  }

  function channelDestinationPlaceholder(type: NotificationChannelType): string {
    if (type === "email") return "oncall@example.com";
    if (type === "slack") return "https://hooks.slack.com/services/...";
    if (type === "discord") return "https://discord.com/api/webhooks/...";
    return "https://hooks.example.com/services/...";
  }

  function channelTypeLabel(type: NotificationChannelType): string {
    if (type === "slack") return "Slack";
    if (type === "discord") return "Discord";
    if (type === "email") return "Email";
    return "Webhook";
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const error = validateDraft();
    if (error) { setFormError(error); return; }
    setFormError(null);
    setIsSaving(true);

    try {
      const payload: NotificationChannelSetup = {
        channel_type: draft.channel_type,
        name: draft.name.trim(),
        destination: draft.destination.trim(),
        notify_on_failure: draft.notify_on_failure,
        notify_on_recovery: draft.notify_on_recovery,
        is_active: draft.is_active,
        webhook_secret: draft.channel_type === "webhook" && draft.webhook_secret.trim()
          ? draft.webhook_secret.trim()
          : null,
      };

      const resultId =
        modalMode === "create"
          ? await onCreateChannel(payload)
          : draft.id !== null
            ? await onUpdateChannel(draft.id, payload)
            : null;

      if (resultId !== null) closeModal();
    } finally {
      setIsSaving(false);
    }
  }

  async function handleDeleteConfirm() {
    if (!deleteTarget) return;
    setIsDeleting(true);
    try {
      const deleted = await onDeleteChannel(deleteTarget.id);
      if (deleted) setDeleteTarget(null);
    } finally {
      setIsDeleting(false);
    }
  }

  return (
    <section className="panel page-panel">
      <div className="panel-header">
        <div>
          <div className="panel-kicker">Notifications</div>
          <h2>Notification Channels</h2>
          <p>Configure webhook, Slack, Discord, and email channels for alert delivery.</p>
        </div>
        <div className="page-header-actions">
          <button className="ghost-button" type="button" onClick={onRefresh}>
            {isRefreshing ? "Refreshing..." : "Refresh"}
          </button>
          {canWrite && (
            <button className="primary-button" type="button" onClick={openCreate}>
              Add Channel
            </button>
          )}
        </div>
      </div>

      <div className="panel">
        <div className="table-wrap management-table">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Type</th>
                <th>Destination</th>
                <th>Events</th>
                <th>Status</th>
                <th>Created</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              {channels.length === 0 && (
                <tr>
                  <td colSpan={7}>
                    <div className="empty-state">
                      No notification channels configured. Add a channel to receive alerts.
                    </div>
                  </td>
                </tr>
              )}
              {channels.map((channel) => (
                <tr key={channel.id}>
                  <td>
                    <div className="table-primary">{channel.name}</div>
                    <div className="table-secondary">#{channel.id}</div>
                  </td>
                  <td>
                    <span className={`tag ${channel.channel_type === "email" ? "tag-disabled" : "tag-active"}`}>
                      {channelTypeLabel(channel.channel_type)}
                    </span>
                  </td>
                  <td>
                    <div className="table-primary">{channel.destination}</div>
                    {channel.channel_type === "webhook" && (
                      <div className="table-secondary">
                        {channel.has_webhook_secret ? "Secret configured" : "No secret"}
                      </div>
                    )}
                  </td>
                  <td>
                    <div style={{ display: "flex", gap: "4px", flexWrap: "wrap" }}>
                      {channel.notify_on_failure && <span className="tag tag-danger">failure</span>}
                      {channel.notify_on_recovery && <span className="tag tag-active">recovery</span>}
                    </div>
                  </td>
                  <td>
                    <span className={`tag ${channel.is_active ? "tag-active" : "tag-disabled"}`}>
                      {channel.is_active ? "active" : "disabled"}
                    </span>
                  </td>
                  <td>{formatTimestamp(channel.created_at)}</td>
                  <td className="table-actions">
                    <button
                      className="ghost-button"
                      type="button"
                      onClick={() => openEdit(channel)}
                      disabled={!canWrite}
                    >
                      Edit
                    </button>
                    <button
                      className="ghost-button danger"
                      type="button"
                      onClick={() => openDelete(channel)}
                      disabled={!canWrite}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {modalMode && canWrite && (
        <div className="modal-backdrop" role="presentation" onClick={closeModal}>
          <div
            className="modal-panel panel"
            role="dialog"
            aria-modal="true"
            aria-label={modalMode === "create" ? "Add notification channel" : "Edit notification channel"}
            onClick={(e) => e.stopPropagation()}
          >
            <form className="stacked-form" onSubmit={handleSubmit}>
              <div className="management-list-header">
                <strong>{modalMode === "create" ? "Add Channel" : "Edit Channel"}</strong>
                <button className="ghost-button" type="button" onClick={closeModal}>
                  Close
                </button>
              </div>

              {formError && <div className="error-banner workspace-banner">{formError}</div>}

              <label>
                <span>Channel Type</span>
                <select
                  value={draft.channel_type}
                  disabled={modalMode === "edit"}
                  onChange={(e) =>
                    setDraft((c) => ({
                      ...c,
                      channel_type: e.target.value as NotificationChannelType,
                      webhook_secret: "",
                    }))
                  }
                >
                  <option value="webhook">Webhook</option>
                  <option value="slack">Slack</option>
                  <option value="discord">Discord</option>
                  <option value="email">Email</option>
                </select>
              </label>

              <label>
                <span>Name</span>
                <input
                  value={draft.name}
                  placeholder={draft.channel_type === "email" ? "e.g. On-Call Email" : draft.channel_type === "slack" ? "e.g. #ops-alerts" : draft.channel_type === "discord" ? "e.g. #incidents" : "e.g. Ops Webhook"}
                  onChange={(e) => setDraft((c) => ({ ...c, name: e.target.value }))}
                />
              </label>

              <label>
                <span>{channelDestinationLabel(draft.channel_type)}</span>
                <input
                  type={draft.channel_type === "email" ? "email" : "url"}
                  value={draft.destination}
                  placeholder={channelDestinationPlaceholder(draft.channel_type)}
                  onChange={(e) => setDraft((c) => ({ ...c, destination: e.target.value }))}
                />
              </label>

              {draft.channel_type === "webhook" && (
                <label>
                  <span>
                    Webhook Secret
                    {modalMode === "edit" ? " (leave blank to keep existing)" : ""}
                  </span>
                  <input
                    type="password"
                    value={draft.webhook_secret}
                    placeholder={modalMode === "edit" ? "Enter new secret to replace" : "Required"}
                    onChange={(e) => setDraft((c) => ({ ...c, webhook_secret: e.target.value }))}
                  />
                </label>
              )}

              <div className="two-column-grid">
                <label className="toggle-field">
                  <input
                    type="checkbox"
                    checked={draft.notify_on_failure}
                    onChange={(e) => setDraft((c) => ({ ...c, notify_on_failure: e.target.checked }))}
                  />
                  <span className="toggle-switch" aria-hidden="true" />
                  <span className="toggle-copy">Notify on failure</span>
                </label>

                <label className="toggle-field">
                  <input
                    type="checkbox"
                    checked={draft.notify_on_recovery}
                    onChange={(e) => setDraft((c) => ({ ...c, notify_on_recovery: e.target.checked }))}
                  />
                  <span className="toggle-switch" aria-hidden="true" />
                  <span className="toggle-copy">Notify on recovery</span>
                </label>
              </div>

              <label className="toggle-field">
                <input
                  type="checkbox"
                  checked={draft.is_active}
                  onChange={(e) => setDraft((c) => ({ ...c, is_active: e.target.checked }))}
                />
                <span className="toggle-switch" aria-hidden="true" />
                <span className="toggle-copy">Active</span>
              </label>

              <div className="panel-actions">
                <button className="primary-button" type="submit" disabled={isSaving}>
                  {isSaving
                    ? "Saving..."
                    : modalMode === "create"
                      ? "Create Channel"
                      : "Save Channel"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {deleteTarget && (
        <div className="modal-backdrop" role="presentation" onClick={() => setDeleteTarget(null)}>
          <div
            className="modal-panel panel"
            role="dialog"
            aria-modal="true"
            aria-label="Delete notification channel"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="stacked-form">
              <div className="management-list-header">
                <strong>Delete Channel</strong>
                <button className="ghost-button" type="button" onClick={() => setDeleteTarget(null)}>
                  Close
                </button>
              </div>
              <p>
                Delete channel <strong>{deleteTarget.name}</strong>? This will remove the channel from all
                sites and stop future alert deliveries.
              </p>
              <div className="panel-actions">
                <button className="ghost-button" type="button" onClick={() => setDeleteTarget(null)}>
                  Cancel
                </button>
                <button
                  className="primary-button danger-button"
                  type="button"
                  onClick={() => void handleDeleteConfirm()}
                  disabled={isDeleting}
                >
                  {isDeleting ? "Deleting..." : "Delete"}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
