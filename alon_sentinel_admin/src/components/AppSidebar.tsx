import type { JSX } from "react";
import { BrandLogo } from "./BrandLogo";

export type AppView = "dashboard" | "sites" | "notifications" | "access" | "api-clients";

type AppSidebarProps = {
  activeView: AppView;
  canAccessView: boolean;
  canNotificationsView: boolean;
  canApiClientsView: boolean;
  onNavigate: (view: AppView) => void;
};

function DashboardIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M4 5.5C4 4.67 4.67 4 5.5 4h4C10.33 4 11 4.67 11 5.5v4c0 .83-.67 1.5-1.5 1.5h-4C4.67 11 4 10.33 4 9.5v-4Zm9 0c0-.83.67-1.5 1.5-1.5h4c.83 0 1.5.67 1.5 1.5v7c0 .83-.67 1.5-1.5 1.5h-4c-.83 0-1.5-.67-1.5-1.5v-7Zm-9 9c0-.83.67-1.5 1.5-1.5h4c.83 0 1.5.67 1.5 1.5v4c0 .83-.67 1.5-1.5 1.5h-4C4.67 20 4 19.33 4 18.5v-4Zm9 3c0-.83.67-1.5 1.5-1.5h4c.83 0 1.5.67 1.5 1.5v1c0 .83-.67 1.5-1.5 1.5h-4c-.83 0-1.5-.67-1.5-1.5v-1Z"
        fill="currentColor"
      />
    </svg>
  );
}

function SitesIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M5.5 5h13c.83 0 1.5.67 1.5 1.5v11c0 .83-.67 1.5-1.5 1.5h-13C4.67 19 4 18.33 4 17.5v-11C4 5.67 4.67 5 5.5 5Zm.5 3v2h12V8H6Zm0 4v5h3v-5H6Zm5 0v5h7v-5h-7Z"
        fill="currentColor"
      />
    </svg>
  );
}

function NotificationsIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M12 22a2 2 0 0 0 2-2h-4a2 2 0 0 0 2 2Zm6-6V11c0-3.07-1.64-5.64-4.5-6.32V4a1.5 1.5 0 0 0-3 0v.68C7.63 5.36 6 7.92 6 11v5l-2 2v1h16v-1l-2-2Z"
        fill="currentColor"
      />
    </svg>
  );
}

function ApiClientsIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M9.4 16.6 4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4Zm5.2 0 4.6-4.6-4.6-4.6L16 6l6 6-6 6-1.4-1.4Z"
        fill="currentColor"
      />
    </svg>
  );
}

function AccessIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M12 3.75 5.25 6.5v5.19c0 4.1 2.62 7.93 6.75 8.56 4.13-.63 6.75-4.46 6.75-8.56V6.5L12 3.75Zm0 4.1a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8Zm0 8.48c-1.61 0-3.03-.65-4-1.67.05-1.33 2.67-2.06 4-2.06 1.32 0 3.94.73 4 2.06-.97 1.02-2.39 1.67-4 1.67Z"
        fill="currentColor"
      />
    </svg>
  );
}

const navItems: Array<{ key: AppView; label: string; icon: () => JSX.Element; guard?: "access" | "notifications" | "api-clients" }> = [
  {
    key: "dashboard",
    label: "Dashboard",
    icon: DashboardIcon,
  },
  {
    key: "sites",
    label: "Sites",
    icon: SitesIcon,
  },
  {
    key: "notifications",
    label: "Notifications",
    icon: NotificationsIcon,
    guard: "notifications",
  },
  {
    key: "api-clients",
    label: "API Clients",
    icon: ApiClientsIcon,
    guard: "api-clients",
  },
  {
    key: "access",
    label: "Access",
    icon: AccessIcon,
    guard: "access",
  },
];

export function AppSidebar({ activeView, canAccessView, canNotificationsView, canApiClientsView, onNavigate }: AppSidebarProps) {
  const visibleNavItems = navItems.filter((item) => {
    if (item.guard === "access") return canAccessView;
    if (item.guard === "notifications") return canNotificationsView;
    if (item.guard === "api-clients") return canApiClientsView;
    return true;
  });
  return (
    <aside className="sidebar panel">
      <div className="sidebar-header">
        <div className="sidebar-brand sidebar-brand-wide">
          <BrandLogo size="wide" />
        </div>
        <div className="sidebar-brand sidebar-brand-compact">
          <BrandLogo size="sm" />
        </div>
      </div>

      <nav className="sidebar-nav">
        {visibleNavItems.map((item) => (
          <button
            key={item.key}
            className={`sidebar-link ${activeView === item.key ? "is-active" : ""}`}
            type="button"
            onClick={() => onNavigate(item.key)}
          >
            <span className="sidebar-link-icon">
              <item.icon />
            </span>
            <span className="sidebar-link-label">{item.label}</span>
          </button>
        ))}
      </nav>
    </aside>
  );
}
