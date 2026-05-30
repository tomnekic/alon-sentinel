import { type FormEvent, useState } from "react";
import type { ApiClientScope, ApiClientType, CreatedApiClient, ManagedApiClient } from "../api";
import { formatTimestamp } from "../utils";

type ApiClientsManagementProps = {
  canWrite: boolean;
  clients: ManagedApiClient[];
  isRefreshing: boolean;
  onRefresh: () => void;
  onCreateClient: (payload: {
    name: string;
    description?: string | null;
    client_type: ApiClientType;
    scopes: ApiClientScope[];
  }) => Promise<CreatedApiClient>;
  onUpdateClient: (
    clientId: number,
    payload: { name: string; description?: string | null; is_active: boolean }
  ) => Promise<void>;
  onDeleteClient: (clientId: number) => Promise<void>;
  onRotateSecret: (clientId: number) => Promise<CreatedApiClient>;
};

type ModalMode = "create" | "edit" | null;

type Draft = {
  id: number | null;
  name: string;
  description: string;
  client_type: ApiClientType;
  scopes: Record<ApiClientScope, boolean>;
  is_active: boolean;
};

function emptyDraft(): Draft {
  return {
    id: null,
    name: "",
    description: "",
    client_type: "internal_service",
    scopes: { "sites:read": false, "sites:write": false },
    is_active: true,
  };
}

function draftFromClient(client: ManagedApiClient): Draft {
  return {
    id: client.id,
    name: client.name,
    description: client.description ?? "",
    client_type: client.client_type,
    scopes: {
      "sites:read": client.scopes.includes("sites:read"),
      "sites:write": client.scopes.includes("sites:write"),
    },
    is_active: client.is_active,
  };
}

const SCOPE_LABELS: Record<ApiClientScope, string> = {
  "sites:read": "sites:read — view sites, monitors, checks, incidents, notifications",
  "sites:write": "sites:write — create/update/delete sites, monitors, notification channels",
};

const CLIENT_TYPE_LABELS: Record<ApiClientType, string> = {
  internal_service: "Internal Service",
  installation_client: "Installation Client",
};

export function ApiClientsManagement({
  canWrite,
  clients,
  isRefreshing,
  onRefresh,
  onCreateClient,
  onUpdateClient,
  onDeleteClient,
  onRotateSecret,
}: ApiClientsManagementProps) {
  const [modalMode, setModalMode] = useState<ModalMode>(null);
  const [draft, setDraft] = useState<Draft>(emptyDraft());
  const [formError, setFormError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const [secretReveal, setSecretReveal] = useState<CreatedApiClient | null>(null);
  const [copiedField, setCopiedField] = useState<string | null>(null);

  const [deleteTarget, setDeleteTarget] = useState<ManagedApiClient | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  const [rotateTarget, setRotateTarget] = useState<ManagedApiClient | null>(null);
  const [isRotating, setIsRotating] = useState(false);

  function openCreate() {
    setDraft(emptyDraft());
    setFormError(null);
    setModalMode("create");
  }

  function openEdit(client: ManagedApiClient) {
    setDraft(draftFromClient(client));
    setFormError(null);
    setModalMode("edit");
  }

  function closeModal() {
    setModalMode(null);
    setFormError(null);
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (draft.name.trim() === "") {
      setFormError("Name is required.");
      return;
    }
    setFormError(null);
    setIsSubmitting(true);
    try {
      const scopes = (Object.entries(draft.scopes) as [ApiClientScope, boolean][])
        .filter(([, v]) => v)
        .map(([k]) => k);

      if (modalMode === "create") {
        const created = await onCreateClient({
          name: draft.name.trim(),
          description: draft.description.trim() || null,
          client_type: draft.client_type,
          scopes,
        });
        closeModal();
        setSecretReveal(created);
      } else if (modalMode === "edit" && draft.id !== null) {
        await onUpdateClient(draft.id, {
          name: draft.name.trim(),
          description: draft.description.trim() || null,
          is_active: draft.is_active,
        });
        closeModal();
      }
    } catch (err) {
      setFormError(err instanceof Error ? err.message : "An error occurred.");
    } finally {
      setIsSubmitting(false);
    }
  }

  async function handleConfirmDelete() {
    if (!deleteTarget) return;
    setIsDeleting(true);
    try {
      await onDeleteClient(deleteTarget.id);
      setDeleteTarget(null);
    } catch (err) {
      console.error(err);
    } finally {
      setIsDeleting(false);
    }
  }

  async function handleConfirmRotate() {
    if (!rotateTarget) return;
    setIsRotating(true);
    try {
      const updated = await onRotateSecret(rotateTarget.id);
      setRotateTarget(null);
      setSecretReveal(updated);
    } catch (err) {
      console.error(err);
    } finally {
      setIsRotating(false);
    }
  }

  async function copyToClipboard(value: string, field: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedField(field);
      setTimeout(() => setCopiedField(null), 2000);
    } catch {
      // ignore
    }
  }

  return (
    <section className="page-panel">
      <article className="panel subpanel">
        <div className="panel-header">
          <div>
            <div className="panel-kicker">API Access</div>
            <h2>API Clients</h2>
            <p>Manage machine credentials for programmatic access to Sentinel.</p>
          </div>
          <div style={{ display: "flex", gap: "0.5rem" }}>
            <button
              type="button"
              className="ghost-button"
              onClick={onRefresh}
              disabled={isRefreshing}
            >
              {isRefreshing ? "Syncing..." : "Refresh"}
            </button>
            {canWrite && (
              <button type="button" className="primary-button" onClick={openCreate}>
                New Client
              </button>
            )}
          </div>
        </div>

        {clients.length === 0 ? (
          <div className="empty-state">
            {isRefreshing ? "Loading..." : "No API clients configured."}
          </div>
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Type</th>
                <th>Client ID</th>
                <th>Scopes</th>
                <th>Status</th>
                <th>Last Used</th>
                <th>Created</th>
                {canWrite && <th />}
              </tr>
            </thead>
            <tbody>
              {clients.map((client) => (
                <tr key={client.id}>
                  <td>
                    <strong>{client.name}</strong>
                    {client.description && (
                      <div style={{ fontSize: "0.8em", color: "var(--text-muted)" }}>
                        {client.description}
                      </div>
                    )}
                  </td>
                  <td>
                    <span className="tag tag-muted">{CLIENT_TYPE_LABELS[client.client_type]}</span>
                  </td>
                  <td>
                    <code style={{ fontSize: "0.85em" }}>{client.client_id}</code>
                    <div style={{ fontSize: "0.8em", color: "var(--text-muted)" }}>
                      prefix: {client.secret_prefix}…
                    </div>
                  </td>
                  <td>
                    {client.scopes.length === 0 ? (
                      <span style={{ color: "var(--text-muted)" }}>none</span>
                    ) : (
                      <div style={{ display: "flex", flexDirection: "column", gap: "0.2rem" }}>
                        {client.scopes.map((s) => (
                          <span key={s} className="tag tag-muted" style={{ fontSize: "0.8em" }}>
                            {s}
                          </span>
                        ))}
                      </div>
                    )}
                  </td>
                  <td>
                    <span className={`tag ${client.is_active ? "tag-active" : "tag-failing"}`}>
                      {client.is_active ? "Active" : "Inactive"}
                    </span>
                  </td>
                  <td>{formatTimestamp(client.last_used_at)}</td>
                  <td>{formatTimestamp(client.created_at)}</td>
                  {canWrite && (
                    <td>
                      <div style={{ display: "flex", gap: "0.5rem" }}>
                        <button
                          type="button"
                          className="ghost-button"
                          onClick={() => openEdit(client)}
                        >
                          Edit
                        </button>
                        <button
                          type="button"
                          className="ghost-button"
                          onClick={() => setRotateTarget(client)}
                        >
                          Rotate
                        </button>
                        <button
                          type="button"
                          className="ghost-button ghost-button-danger"
                          onClick={() => setDeleteTarget(client)}
                        >
                          Delete
                        </button>
                      </div>
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </article>

      {/* Create / Edit Modal */}
      {modalMode !== null && (
        <div className="modal-overlay">
          <div className="modal">
            <div className="modal-header">
              <h3>{modalMode === "create" ? "New API Client" : "Edit API Client"}</h3>
              <button type="button" className="modal-close" onClick={closeModal}>
                ×
              </button>
            </div>
            <form onSubmit={(e) => void handleSubmit(e)}>
              <div className="modal-body">
                <div className="form-field">
                  <label htmlFor="ac-name">Name</label>
                  <input
                    id="ac-name"
                    type="text"
                    value={draft.name}
                    onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
                    placeholder="e.g. Deployment Bot"
                    autoFocus
                  />
                </div>
                <div className="form-field">
                  <label htmlFor="ac-description">Description (optional)</label>
                  <input
                    id="ac-description"
                    type="text"
                    value={draft.description}
                    onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
                    placeholder="What is this client used for?"
                  />
                </div>
                {modalMode === "create" && (
                  <div className="form-field">
                    <label htmlFor="ac-type">Client Type</label>
                    <select
                      id="ac-type"
                      value={draft.client_type}
                      onChange={(e) =>
                        setDraft((d) => ({ ...d, client_type: e.target.value as ApiClientType }))
                      }
                    >
                      <option value="internal_service">Internal Service</option>
                      <option value="installation_client">Installation Client</option>
                    </select>
                  </div>
                )}
                {modalMode === "create" && (
                  <div className="form-field">
                    <label>Scopes</label>
                    {(Object.entries(SCOPE_LABELS) as [ApiClientScope, string][]).map(
                      ([scope, label]) => (
                        <label key={scope} className="checkbox-label">
                          <input
                            type="checkbox"
                            checked={draft.scopes[scope]}
                            onChange={(e) =>
                              setDraft((d) => ({
                                ...d,
                                scopes: { ...d.scopes, [scope]: e.target.checked },
                              }))
                            }
                          />
                          <code>{label}</code>
                        </label>
                      )
                    )}
                  </div>
                )}
                {modalMode === "edit" && (
                  <div className="form-field">
                    <label className="checkbox-label">
                      <input
                        type="checkbox"
                        checked={draft.is_active}
                        onChange={(e) =>
                          setDraft((d) => ({ ...d, is_active: e.target.checked }))
                        }
                      />
                      Active
                    </label>
                  </div>
                )}
                {formError && <div className="form-error">{formError}</div>}
              </div>
              <div className="modal-footer">
                <button type="button" className="ghost-button" onClick={closeModal}>
                  Cancel
                </button>
                <button type="submit" className="primary-button" disabled={isSubmitting}>
                  {isSubmitting
                    ? "Saving..."
                    : modalMode === "create"
                      ? "Create Client"
                      : "Save Changes"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Secret Reveal Modal */}
      {secretReveal !== null && (
        <div className="modal-overlay">
          <div className="modal">
            <div className="modal-header">
              <h3>Client Credentials</h3>
            </div>
            <div className="modal-body">
              <p style={{ color: "var(--text-warning)", fontWeight: 600 }}>
                Copy these credentials now — the secret will not be shown again.
              </p>
              <div className="form-field">
                <label>Client ID</label>
                <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                  <code style={{ flex: 1, padding: "0.4rem 0.6rem", background: "var(--surface-2)", borderRadius: "4px" }}>
                    {secretReveal.client_id}
                  </code>
                  <button
                    type="button"
                    className="ghost-button"
                    onClick={() => void copyToClipboard(secretReveal.client_id, "client_id")}
                  >
                    {copiedField === "client_id" ? "Copied!" : "Copy"}
                  </button>
                </div>
              </div>
              <div className="form-field">
                <label>Client Secret</label>
                <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                  <code
                    style={{
                      flex: 1,
                      padding: "0.4rem 0.6rem",
                      background: "var(--surface-2)",
                      borderRadius: "4px",
                      wordBreak: "break-all",
                    }}
                  >
                    {secretReveal.client_secret}
                  </code>
                  <button
                    type="button"
                    className="ghost-button"
                    onClick={() => void copyToClipboard(secretReveal.client_secret, "client_secret")}
                  >
                    {copiedField === "client_secret" ? "Copied!" : "Copy"}
                  </button>
                </div>
              </div>
            </div>
            <div className="modal-footer">
              <button
                type="button"
                className="primary-button"
                onClick={() => setSecretReveal(null)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation */}
      {deleteTarget !== null && (
        <div className="modal-overlay">
          <div className="modal">
            <div className="modal-header">
              <h3>Delete API Client</h3>
            </div>
            <div className="modal-body">
              <p>
                Delete <strong>{deleteTarget.name}</strong>? All associated access tokens will be
                invalidated immediately. This cannot be undone.
              </p>
            </div>
            <div className="modal-footer">
              <button
                type="button"
                className="ghost-button"
                onClick={() => setDeleteTarget(null)}
                disabled={isDeleting}
              >
                Cancel
              </button>
              <button
                type="button"
                className="primary-button primary-button-danger"
                onClick={() => void handleConfirmDelete()}
                disabled={isDeleting}
              >
                {isDeleting ? "Deleting..." : "Delete"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Rotate Secret Confirmation */}
      {rotateTarget !== null && (
        <div className="modal-overlay">
          <div className="modal">
            <div className="modal-header">
              <h3>Rotate Secret</h3>
            </div>
            <div className="modal-body">
              <p>
                Rotate the secret for <strong>{rotateTarget.name}</strong>? The current secret will
                stop working immediately. Any running services using it will need to be updated.
              </p>
            </div>
            <div className="modal-footer">
              <button
                type="button"
                className="ghost-button"
                onClick={() => setRotateTarget(null)}
                disabled={isRotating}
              >
                Cancel
              </button>
              <button
                type="button"
                className="primary-button"
                onClick={() => void handleConfirmRotate()}
                disabled={isRotating}
              >
                {isRotating ? "Rotating..." : "Rotate Secret"}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
