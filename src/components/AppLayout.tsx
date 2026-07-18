import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Activity, Bell, CircleGauge, Menu, Moon, Network, Settings as SettingsIcon, Share2, ShieldCheck, Sun, X } from "lucide-react";
import { NavLink, useLocation } from "react-router-dom";
import { api, defaultSettings } from "../services/ipc";
import { useUiStore } from "../store/ui";
import packageJson from "../../package.json";

const routeTitles = [
  { match: (path: string) => path === "/", title: "ダッシュボード", description: "監視ノードの状態と直近の変化を確認" },
  { match: (path: string) => path.startsWith("/topology"), title: "ネットワーク", description: "ピア関係と伝播イベントを確認" },
  { match: (path: string) => path.startsWith("/discovery"), title: "ノード検出", description: "LANスキャンと手動登録で監視対象を追加" },
  { match: (path: string) => path === "/alerts", title: "アラート", description: "継続中の障害と復旧履歴を確認" },
  { match: (path: string) => path.startsWith("/alerts/"), title: "アラート詳細", description: "発生状況と関連診断を確認" },
  { match: (path: string) => path.startsWith("/nodes/"), title: "ノード詳細", description: "個別ノードのメトリクスと接続情報を確認" },
  { match: (path: string) => path.startsWith("/settings"), title: "設定", description: "監視間隔、通知、履歴保持を調整" },
];

export function Layout({ children }: { children: React.ReactNode }) {
  const { sidebarOpen, setSidebarOpen } = useUiStore();
  const location = useLocation();
  const queryClient = useQueryClient();
  const { data: appSettings } = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const theme = appSettings?.theme ?? "system";
  const systemDark = typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches;
  const dark = theme === "dark" || (theme === "system" && systemDark);
  const changeTheme = useMutation({
    mutationFn: (nextTheme: "light" | "dark") => api.updateSettings({ ...(appSettings ?? defaultSettings), theme: nextTheme }),
    onSuccess: (settings) => queryClient.setQueryData(["settings"], settings),
  });
  useEffect(() => {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.dataset.theme = theme;
  }, [theme]);
  const nav = [
    { to: "/", label: "ダッシュボード", icon: CircleGauge },
    { to: "/topology", label: "ネットワーク", icon: Share2 },
    { to: "/discovery", label: "ノード検出", icon: Network },
    { to: "/alerts", label: "アラート", icon: Bell },
    { to: "/settings", label: "設定", icon: SettingsIcon },
  ];
  const routeTitle = routeTitles.find((item) => item.match(location.pathname)) ?? {
    title: "Harvest Scope",
    description: "Symbol / NEM ノードの状態をひと目で確認",
  };
  return (
    <div className="app-shell">
      <aside className={`sidebar ${sidebarOpen ? "sidebar--open" : ""}`}>
        <div className="brand">
          <div className="brand__mark"><Activity size={22} /></div>
          <div><strong role="heading" aria-level={2}>Harvest Scope</strong><span>NEM / Symbol</span></div>
          <button className="icon-button sidebar__close" onClick={() => setSidebarOpen(false)} aria-label="メニューを閉じる"><X /></button>
        </div>
        <nav>
          {nav.map(({ to, label, icon: Icon }) => (
            <NavLink key={to} to={to} end={to === "/"} onClick={() => setSidebarOpen(false)}>
              <Icon size={19} /><span>{label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="sidebar__footer">
          <span><ShieldCheck size={17} /> 読み取り専用監視</span>
          <small aria-label="アプリバージョン">v{packageJson.version}</small>
        </div>
      </aside>
      {sidebarOpen && <button className="scrim" onClick={() => setSidebarOpen(false)} aria-label="メニューを閉じる" />}
      <main className="main">
        <header className="topbar">
          <button className="icon-button mobile-menu" onClick={() => setSidebarOpen(true)} aria-label="メニューを開く"><Menu /></button>
          <div className="topbar__title"><h1>{routeTitle.title}</h1><p>{routeTitle.description}</p></div>
          <div className="live-indicator"><span />監視エンジン稼働中</div>
          {changeTheme.error && <div className="theme-error" role="alert">テーマを変更できません: {String(changeTheme.error)}</div>}
          <button className="icon-button theme-toggle" aria-label={dark ? "ライトテーマに切り替え" : "ダークテーマに切り替え"} title={dark ? "ライトテーマ" : "ダークテーマ"} disabled={changeTheme.isPending} onClick={() => changeTheme.mutate(dark ? "light" : "dark")}>{dark ? <Sun size={17} /> : <Moon size={17} />}</button>
        </header>
        <div className="page">{children}</div>
      </main>
    </div>
  );
}
