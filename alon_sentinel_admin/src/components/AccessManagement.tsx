import { type ChangeEvent, type DragEvent, type FormEvent, useEffect, useState } from "react";
import type { ManagedAdminUser, ManagedPermission, ManagedRole } from "../api";

type AccessManagementProps = {
  canReadRoles: boolean;
  canReadUsers: boolean;
  canWriteRoles: boolean;
  canWriteUsers: boolean;
  isRefreshing: boolean;
  onRefresh: () => void;
  onCreateRole: (payload: {
    key: string;
    name: string;
    description?: string;
    permission_keys: string[];
  }) => Promise<number | null>;
  onCreateUser: (payload: {
    email: string;
    display_name: string;
    password: string;
    is_active?: boolean;
    role_keys: string[];
  }) => Promise<number | null>;
  onDeleteRole: (roleId: number) => Promise<boolean>;
  onDeleteUser: (userId: number) => Promise<boolean>;
  onUpdateRole: (
    roleId: number,
    payload: {
      name: string;
      description?: string;
      permission_keys: string[];
    }
  ) => Promise<number | null>;
  onUpdateUser: (
    userId: number,
    payload: {
      email: string;
      display_name: string;
      password?: string;
      is_active: boolean;
      role_keys: string[];
    }
  ) => Promise<number | null>;
  permissions: ManagedPermission[];
  roles: ManagedRole[];
  users: ManagedAdminUser[];
};

type AccessTab = "users" | "roles" | "permissions";
type RoleModalMode = "create" | "edit" | null;
type PermissionDropTarget = "available" | "assigned" | null;
type DeleteDialogState =
  | {
      type: "user";
      id: number;
      name: string;
    }
  | {
      type: "role";
      id: number;
      name: string;
    };

function formatTimestamp(value: string | null): string {
  if (!value) {
    return "Never";
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function sortStrings(values: string[]) {
  return [...values].sort((left, right) => left.localeCompare(right));
}

function selectedValues(event: ChangeEvent<HTMLSelectElement>) {
  return Array.from(event.target.selectedOptions, (option) => option.value);
}

function createEmptyRoleDraft() {
  return {
    id: null as number | null,
    key: "",
    name: "",
    description: "",
    permission_keys: [] as string[]
  };
}

function createRoleDraft(role: ManagedRole) {
  return {
    id: role.id,
    key: role.key,
    name: role.name,
    description: role.description ?? "",
    permission_keys: sortStrings(role.permissions)
  };
}

function sortPermissions(values: ManagedPermission[]) {
  return [...values].sort((left, right) => left.name.localeCompare(right.name));
}

export function AccessManagement({
  canReadRoles,
  canReadUsers,
  canWriteRoles,
  canWriteUsers,
  isRefreshing,
  onRefresh,
  onCreateRole,
  onCreateUser,
  onDeleteRole,
  onDeleteUser,
  onUpdateRole,
  onUpdateUser,
  permissions,
  roles,
  users
}: AccessManagementProps) {
  const [activeTab, setActiveTab] = useState<AccessTab>("users");
  const [isCreateUserModalOpen, setIsCreateUserModalOpen] = useState(false);
  const [isEditUserModalOpen, setIsEditUserModalOpen] = useState(false);
  const [editingUserId, setEditingUserId] = useState<number | null>(null);
  const [roleModalMode, setRoleModalMode] = useState<RoleModalMode>(null);
  const [editingRoleId, setEditingRoleId] = useState<number | null>(null);
  const [draggingPermissionKey, setDraggingPermissionKey] = useState<string | null>(null);
  const [permissionDropTarget, setPermissionDropTarget] = useState<PermissionDropTarget>(null);
  const [deleteDialog, setDeleteDialog] = useState<DeleteDialogState | null>(null);
  const [isSavingUser, setIsSavingUser] = useState(false);
  const [isCreatingUser, setIsCreatingUser] = useState(false);
  const [isSavingRole, setIsSavingRole] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [userDraft, setUserDraft] = useState({
    email: "",
    display_name: "",
    password: "",
    is_active: true,
    role_keys: [] as string[]
  });
  const [createUserDraft, setCreateUserDraft] = useState({
    email: "",
    display_name: "",
    password: "",
    is_active: true,
    role_keys: [] as string[]
  });
  const [roleDraft, setRoleDraft] = useState(createEmptyRoleDraft);

  const assignedPermissions = sortPermissions(
    permissions.filter((permission) => roleDraft.permission_keys.includes(permission.key))
  );
  const availablePermissions = sortPermissions(
    permissions.filter((permission) => !roleDraft.permission_keys.includes(permission.key))
  );
  const availableTabs: AccessTab[] = [
    ...(canReadUsers ? ["users" as const] : []),
    ...(canReadRoles ? ["roles" as const, "permissions" as const] : [])
  ];

  useEffect(() => {
    if (!availableTabs.includes(activeTab) && availableTabs.length > 0) {
      setActiveTab(availableTabs[0]);
    }
  }, [activeTab, availableTabs]);

  function startNewUser() {
    if (!canWriteUsers) return;
    setCreateUserDraft({
      email: "",
      display_name: "",
      password: "",
      is_active: true,
      role_keys: []
    });
    setIsCreateUserModalOpen(true);
  }

  function startEditUser(user: ManagedAdminUser) {
    if (!canWriteUsers) return;
    setUserDraft({
      email: user.email,
      display_name: user.display_name,
      password: "",
      is_active: user.is_active,
      role_keys: sortStrings(user.roles)
    });
    setEditingUserId(user.id);
    setIsEditUserModalOpen(true);
  }

  function closeRoleModal() {
    setRoleModalMode(null);
    setEditingRoleId(null);
    setDraggingPermissionKey(null);
    setPermissionDropTarget(null);
    setRoleDraft(createEmptyRoleDraft());
  }

  function startNewRole() {
    if (!canWriteRoles) return;
    setRoleDraft(createEmptyRoleDraft());
    setEditingRoleId(null);
    setRoleModalMode("create");
  }

  function startEditRole(role: ManagedRole) {
    if (!canWriteRoles) return;
    setRoleDraft(createRoleDraft(role));
    setEditingRoleId(role.id);
    setRoleModalMode("edit");
  }

  function addRolePermission(permissionKey: string) {
    setRoleDraft((current) => {
      if (current.permission_keys.includes(permissionKey)) {
        return current;
      }

      return {
        ...current,
        permission_keys: sortStrings([...current.permission_keys, permissionKey])
      };
    });
  }

  function removeRolePermission(permissionKey: string) {
    setRoleDraft((current) => ({
      ...current,
      permission_keys: current.permission_keys.filter((value) => value !== permissionKey)
    }));
  }

  function startPermissionDrag(event: DragEvent<HTMLButtonElement>, permissionKey: string) {
    setDraggingPermissionKey(permissionKey);
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", permissionKey);
  }

  function finishPermissionDrag() {
    setDraggingPermissionKey(null);
    setPermissionDropTarget(null);
  }

  function handlePermissionDragOver(
    event: DragEvent<HTMLDivElement>,
    target: Exclude<PermissionDropTarget, null>
  ) {
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setPermissionDropTarget(target);
  }

  function handlePermissionDrop(
    event: DragEvent<HTMLDivElement>,
    target: Exclude<PermissionDropTarget, null>
  ) {
    event.preventDefault();

    const permissionKey = event.dataTransfer.getData("text/plain") || draggingPermissionKey;
    if (!permissionKey) {
      finishPermissionDrag();
      return;
    }

    if (target === "assigned") {
      addRolePermission(permissionKey);
    } else {
      removeRolePermission(permissionKey);
    }

    finishPermissionDrag();
  }

  async function handleUserSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIsSavingUser(true);

    try {
      if (editingUserId === null) {
        return;
      }

      const nextId = await onUpdateUser(editingUserId, {
        email: userDraft.email,
        display_name: userDraft.display_name,
        password: userDraft.password || undefined,
        is_active: userDraft.is_active,
        role_keys: userDraft.role_keys
      });

      if (nextId !== null) {
        setEditingUserId(nextId);
        setIsEditUserModalOpen(false);
      }
    } finally {
      setIsSavingUser(false);
    }
  }

  async function handleCreateUserSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIsCreatingUser(true);

    try {
      const nextId = await onCreateUser({
        email: createUserDraft.email,
        display_name: createUserDraft.display_name,
        password: createUserDraft.password,
        is_active: createUserDraft.is_active,
        role_keys: createUserDraft.role_keys
      });

      if (nextId !== null) {
        setIsCreateUserModalOpen(false);
      }
    } finally {
      setIsCreatingUser(false);
    }
  }

  async function handleRoleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIsSavingRole(true);

    try {
      const nextId =
        roleModalMode === "create"
          ? await onCreateRole({
              key: roleDraft.key,
              name: roleDraft.name,
              description: roleDraft.description || undefined,
              permission_keys: roleDraft.permission_keys
            })
          : editingRoleId === null
            ? null
            : await onUpdateRole(editingRoleId, {
                name: roleDraft.name,
                description: roleDraft.description || undefined,
                permission_keys: roleDraft.permission_keys
              });

      if (nextId !== null) {
        closeRoleModal();
      }
    } finally {
      setIsSavingRole(false);
    }
  }

  function openDeleteUser(user: ManagedAdminUser) {
    setDeleteDialog({
      type: "user",
      id: user.id,
      name: user.display_name
    });
  }

  function openDeleteRole(role: ManagedRole) {
    setDeleteDialog({
      type: "role",
      id: role.id,
      name: role.name
    });
  }

  async function handleDeleteConfirm() {
    if (!deleteDialog) {
      return;
    }

    setIsDeleting(true);

    try {
      const deleted =
        deleteDialog.type === "user"
          ? await onDeleteUser(deleteDialog.id)
          : await onDeleteRole(deleteDialog.id);

      if (!deleted) {
        return;
      }

      if (deleteDialog.type === "user" && editingUserId === deleteDialog.id) {
        setEditingUserId(null);
        setIsEditUserModalOpen(false);
        setUserDraft({
          email: "",
          display_name: "",
          password: "",
          is_active: true,
          role_keys: []
        });
      }

      if (deleteDialog.type === "role" && editingRoleId === deleteDialog.id) {
        closeRoleModal();
      }

      setDeleteDialog(null);
    } finally {
      setIsDeleting(false);
    }
  }

  return (
    <section className="panel page-panel">
      <div className="panel-header">
        <div>
          <div className="panel-kicker">Access</div>
          <h2>User Management</h2>
          <p>Manage Sentinel users, roles, and permission assignments.</p>
        </div>

        <div className="page-header-actions">
          <button className="ghost-button" type="button" onClick={onRefresh}>
            {isRefreshing ? "Refreshing..." : "Refresh Access"}
          </button>
        </div>
      </div>

      <div className="tab-row">
        {availableTabs.map((tab) => (
          <button
            key={tab}
            className={`tab-button ${activeTab === tab ? "is-active" : ""}`}
            type="button"
            onClick={() => setActiveTab(tab)}
          >
            {tab}
          </button>
        ))}
      </div>

      {availableTabs.length === 0 && (
        <div className="empty-state">Your account does not have access permissions to view this section.</div>
      )}

      {activeTab === "users" && (
        <div className="management-stack">
          <div className="panel">
            <div className="management-list-header">
              <strong>Users</strong>
              <button className="ghost-button" type="button" onClick={startNewUser} disabled={!canWriteUsers}>
                Add User
              </button>
            </div>

            <div className="table-wrap management-table">
              <table>
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Email</th>
                    <th>Roles</th>
                    <th>Status</th>
                    <th>Last Login</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {users.map((user) => (
                    <tr key={user.id}>
                      <td>
                        <div className="table-primary">{user.display_name}</div>
                        <div className="table-secondary">User #{user.id}</div>
                      </td>
                      <td>{user.email}</td>
                      <td>{user.roles.join(", ") || "No roles"}</td>
                      <td>
                        <span className={`tag ${user.is_active ? "tag-active" : "tag-disabled"}`}>
                          {user.is_active ? "active" : "disabled"}
                        </span>
                      </td>
                      <td>{formatTimestamp(user.last_login_at)}</td>
                      <td className="table-actions">
                        <button
                          className="ghost-button"
                          type="button"
                          onClick={() => startEditUser(user)}
                          disabled={!canWriteUsers}
                        >
                          Edit
                        </button>
                        <button
                          className="ghost-button danger"
                          type="button"
                          onClick={() => openDeleteUser(user)}
                          disabled={!canWriteUsers}
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

          {isCreateUserModalOpen && canWriteUsers && (
            <div className="modal-backdrop" role="presentation" onClick={() => setIsCreateUserModalOpen(false)}>
              <div
                className="modal-panel panel"
                role="dialog"
                aria-modal="true"
                aria-label="Create user"
                onClick={(event) => event.stopPropagation()}
              >
                <form className="stacked-form" onSubmit={handleCreateUserSubmit}>
                  <div className="management-list-header">
                    <strong>Create User</strong>
                    <button
                      className="ghost-button"
                      type="button"
                      onClick={() => setIsCreateUserModalOpen(false)}
                    >
                      Close
                    </button>
                  </div>

                  <label>
                    <span>Display Name</span>
                    <input
                      value={createUserDraft.display_name}
                      onChange={(event) =>
                        setCreateUserDraft((current) => ({
                          ...current,
                          display_name: event.target.value
                        }))
                      }
                    />
                  </label>

                  <label>
                    <span>Email</span>
                    <input
                      type="email"
                      value={createUserDraft.email}
                      onChange={(event) =>
                        setCreateUserDraft((current) => ({
                          ...current,
                          email: event.target.value
                        }))
                      }
                    />
                  </label>

                  <label>
                    <span>Password</span>
                    <input
                      type="password"
                      value={createUserDraft.password}
                      onChange={(event) =>
                        setCreateUserDraft((current) => ({
                          ...current,
                          password: event.target.value
                        }))
                      }
                    />
                  </label>

                  <label className="toggle-field">
                    <input
                      type="checkbox"
                      checked={createUserDraft.is_active}
                      onChange={(event) =>
                        setCreateUserDraft((current) => ({
                          ...current,
                          is_active: event.target.checked
                        }))
                      }
                    />
                    <span className="toggle-switch" aria-hidden="true" />
                    <span className="toggle-copy">Active</span>
                  </label>

                  <label>
                    <span>Roles</span>
                    <select
                      multiple
                      className="management-multiselect"
                      value={createUserDraft.role_keys}
                      onChange={(event) =>
                        setCreateUserDraft((current) => ({
                          ...current,
                          role_keys: sortStrings(selectedValues(event))
                        }))
                      }
                    >
                      {roles.map((role) => (
                        <option key={role.id} value={role.key}>
                          {role.name} ({role.key})
                        </option>
                      ))}
                    </select>
                  </label>

                  <div className="panel-actions">
                    <button className="primary-button" type="submit" disabled={isCreatingUser}>
                      {isCreatingUser ? "Creating..." : "Create User"}
                    </button>
                  </div>
                </form>
              </div>
            </div>
          )}

          {isEditUserModalOpen && canWriteUsers && (
            <div className="modal-backdrop" role="presentation" onClick={() => setIsEditUserModalOpen(false)}>
              <div
                className="modal-panel panel"
                role="dialog"
                aria-modal="true"
                aria-label="Edit user"
                onClick={(event) => event.stopPropagation()}
              >
                <form className="stacked-form" onSubmit={handleUserSubmit}>
                  <div className="management-list-header">
                    <strong>Edit User</strong>
                    <button
                      className="ghost-button"
                      type="button"
                      onClick={() => setIsEditUserModalOpen(false)}
                    >
                      Close
                    </button>
                  </div>

                  <label>
                    <span>Display Name</span>
                    <input
                      value={userDraft.display_name}
                      onChange={(event) =>
                        setUserDraft((current) => ({ ...current, display_name: event.target.value }))
                      }
                    />
                  </label>

                  <label>
                    <span>Email</span>
                    <input
                      type="email"
                      value={userDraft.email}
                      onChange={(event) =>
                        setUserDraft((current) => ({ ...current, email: event.target.value }))
                      }
                    />
                  </label>

                  <label>
                    <span>Password (optional)</span>
                    <input
                      type="password"
                      value={userDraft.password}
                      onChange={(event) =>
                        setUserDraft((current) => ({ ...current, password: event.target.value }))
                      }
                    />
                  </label>

                  <label className="toggle-field">
                    <input
                      type="checkbox"
                      checked={userDraft.is_active}
                      onChange={(event) =>
                        setUserDraft((current) => ({ ...current, is_active: event.target.checked }))
                      }
                    />
                    <span className="toggle-switch" aria-hidden="true" />
                    <span className="toggle-copy">Active</span>
                  </label>

                  <label>
                    <span>Roles</span>
                    <select
                      multiple
                      className="management-multiselect"
                      value={userDraft.role_keys}
                      onChange={(event) =>
                        setUserDraft((current) => ({
                          ...current,
                          role_keys: sortStrings(selectedValues(event))
                        }))
                      }
                    >
                      {roles.map((role) => (
                        <option key={role.id} value={role.key}>
                          {role.name} ({role.key})
                        </option>
                      ))}
                    </select>
                  </label>

                  <div className="panel-actions">
                    <button className="primary-button" type="submit" disabled={isSavingUser}>
                      {isSavingUser ? "Saving..." : "Save User"}
                    </button>
                  </div>
                </form>
              </div>
            </div>
          )}
        </div>
      )}

      {activeTab === "roles" && (
        <div className="management-stack">
          <div className="panel">
            <div className="management-list-header">
              <strong>Roles</strong>
              <button className="ghost-button" type="button" onClick={startNewRole} disabled={!canWriteRoles}>
                Add Role
              </button>
            </div>

            <div className="table-wrap management-table">
              <table>
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Key</th>
                    <th>Permissions</th>
                    <th>Type</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {roles.map((role) => (
                    <tr key={role.id}>
                      <td>
                        <div className="table-primary">{role.name}</div>
                        <div className="table-secondary">{role.description ?? "No description"}</div>
                      </td>
                      <td>{role.key}</td>
                      <td>{role.permissions.length}</td>
                      <td>
                        <span className={`tag ${role.is_system ? "tag-active" : "tag-disabled"}`}>
                          {role.is_system ? "system" : "custom"}
                        </span>
                      </td>
                      <td className="table-actions">
                        <button
                          className="ghost-button"
                          type="button"
                          onClick={() => startEditRole(role)}
                          disabled={!canWriteRoles}
                        >
                          Edit
                        </button>
                        <button
                          className="ghost-button danger"
                          type="button"
                          onClick={() => openDeleteRole(role)}
                          disabled={role.is_system || !canWriteRoles}
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

          {roleModalMode && canWriteRoles && (
            <div className="modal-backdrop" role="presentation" onClick={closeRoleModal}>
              <div
                className="modal-panel modal-panel-wide panel"
                role="dialog"
                aria-modal="true"
                aria-label={roleModalMode === "create" ? "Create role" : "Edit role"}
                onClick={(event) => event.stopPropagation()}
              >
                <form className="stacked-form" onSubmit={handleRoleSubmit}>
                  <div className="management-list-header">
                    <strong>{roleModalMode === "create" ? "Create Role" : "Edit Role"}</strong>
                    <button className="ghost-button" type="button" onClick={closeRoleModal}>
                      Close
                    </button>
                  </div>

                  <div className="two-column-grid">
                    <label>
                      <span>Key</span>
                      <input
                        value={roleDraft.key}
                        disabled={roleModalMode === "edit"}
                        onChange={(event) =>
                          setRoleDraft((current) => ({ ...current, key: event.target.value }))
                        }
                      />
                    </label>

                    <label>
                      <span>Name</span>
                      <input
                        value={roleDraft.name}
                        onChange={(event) =>
                          setRoleDraft((current) => ({ ...current, name: event.target.value }))
                        }
                      />
                    </label>
                  </div>

                  <label>
                    <span>Description</span>
                    <textarea
                      rows={3}
                      value={roleDraft.description}
                      onChange={(event) =>
                        setRoleDraft((current) => ({ ...current, description: event.target.value }))
                      }
                    />
                  </label>

                  <div className="permission-transfer">
                    <div
                      className={`permission-pane ${permissionDropTarget === "available" ? "is-drop-target" : ""}`}
                      onDragOver={(event) => handlePermissionDragOver(event, "available")}
                      onDragLeave={() => setPermissionDropTarget(null)}
                      onDrop={(event) => handlePermissionDrop(event, "available")}
                    >
                      <div className="permission-pane-header">
                        <div>
                          <strong>Available Permissions</strong>
                          <p>Drag into the role to grant access.</p>
                        </div>
                        <span className="tag tag-disabled">{availablePermissions.length}</span>
                      </div>

                      <div className="permission-list">
                        {availablePermissions.length === 0 ? (
                          <div className="permission-empty">All permissions are assigned.</div>
                        ) : (
                          availablePermissions.map((permission) => (
                            <button
                              key={permission.id}
                              className="permission-card"
                              type="button"
                              draggable
                              onClick={() => addRolePermission(permission.key)}
                              onDragStart={(event) => startPermissionDrag(event, permission.key)}
                              onDragEnd={finishPermissionDrag}
                            >
                              <div className="permission-card-copy">
                                <strong>{permission.name}</strong>
                                <span>{permission.key}</span>
                                {permission.description ? <small>{permission.description}</small> : null}
                              </div>
                              <span className="permission-card-action">Add</span>
                            </button>
                          ))
                        )}
                      </div>
                    </div>

                    <div
                      className={`permission-pane ${permissionDropTarget === "assigned" ? "is-drop-target" : ""}`}
                      onDragOver={(event) => handlePermissionDragOver(event, "assigned")}
                      onDragLeave={() => setPermissionDropTarget(null)}
                      onDrop={(event) => handlePermissionDrop(event, "assigned")}
                    >
                      <div className="permission-pane-header">
                        <div>
                          <strong>Role Permissions</strong>
                          <p>Drag out of the role to remove access.</p>
                        </div>
                        <span className="tag tag-active">{assignedPermissions.length}</span>
                      </div>

                      <div className="permission-list">
                        {assignedPermissions.length === 0 ? (
                          <div className="permission-empty">Drop permissions here.</div>
                        ) : (
                          assignedPermissions.map((permission) => (
                            <button
                              key={permission.id}
                              className="permission-card is-assigned"
                              type="button"
                              draggable
                              onClick={() => removeRolePermission(permission.key)}
                              onDragStart={(event) => startPermissionDrag(event, permission.key)}
                              onDragEnd={finishPermissionDrag}
                            >
                              <div className="permission-card-copy">
                                <strong>{permission.name}</strong>
                                <span>{permission.key}</span>
                                {permission.description ? <small>{permission.description}</small> : null}
                              </div>
                              <span className="permission-card-action">Remove</span>
                            </button>
                          ))
                        )}
                      </div>
                    </div>
                  </div>

                  <div className="panel-actions">
                    <button className="primary-button" type="submit" disabled={isSavingRole}>
                      {isSavingRole
                        ? "Saving..."
                        : roleModalMode === "create"
                          ? "Create Role"
                          : "Save Role"}
                    </button>
                  </div>
                </form>
              </div>
            </div>
          )}
        </div>
      )}

      {activeTab === "permissions" && (
        <div className="table-wrap management-table">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Key</th>
                <th>Assigned Roles</th>
              </tr>
            </thead>
            <tbody>
              {permissions.map((permission) => (
                <tr key={permission.id}>
                  <td>
                    <div className="table-primary">{permission.name}</div>
                    <div className="table-secondary">{permission.description ?? "No description"}</div>
                  </td>
                  <td>{permission.key}</td>
                  <td>{permission.roles.join(", ") || "Unassigned"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {deleteDialog && (
        <div className="modal-backdrop" role="presentation" onClick={() => setDeleteDialog(null)}>
          <div
            className="modal-panel panel"
            role="dialog"
            aria-modal="true"
            aria-label={`Delete ${deleteDialog.type}`}
            onClick={(event) => event.stopPropagation()}
          >
            <div className="stacked-form">
              <div className="management-list-header">
                <strong>{`Delete ${deleteDialog.type === "user" ? "User" : "Role"}`}</strong>
                <button className="ghost-button" type="button" onClick={() => setDeleteDialog(null)}>
                  Close
                </button>
              </div>

              <p>
                {`Delete ${deleteDialog.type} `}
                <strong>{deleteDialog.name}</strong>?
              </p>

              <div className="panel-actions">
                <button className="ghost-button" type="button" onClick={() => setDeleteDialog(null)}>
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
