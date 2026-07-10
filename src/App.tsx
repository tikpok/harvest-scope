import { FormEvent, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Bell,
  CheckCircle2,
  ChevronRight,
  Cherry,
  CircleAlert,
  CircleDashed,
  CircleGauge,
  Download,
  FileInput,
  FileOutput,
  Menu,
  Moon,
  Network,
  Pause,
  Pickaxe,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings as SettingsIcon,
  Share2,
  ShieldCheck,
  Stethoscope,
  Sun,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { NavLink, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { api, defaultSettings } from "./services/ipc";
import { useMonitorEvents } from "./hooks/useMonitorEvents";
import { useUiStore } from "./store/ui";
import { StatusBadge } from "./components/StatusBadge";
import { MetricChart } from "./components/MetricChart";
import { TopologyPage } from "./components/TopologyPage";
import packageJson from "../package.json";
import type { AppSettings, DashboardData, DiagnosticRun, HarvestEvent, NodeDetail, NodeStatus, NodeSummary, ScanCandidate, TopologySnapshot } from "./types";

function formatTime(value: number | null) {
  return value ? new Intl.DateTimeFormat("ja-JP", { dateStyle: "short", timeStyle: "medium" }).format(value) : "—";
}

function formatLatency(value: number | null) {
  return value == null ? "—" : `${value} ms`;
}

function isPeerMonitor(node: { monitorMode: string }) {
  return node.monitorMode === "peer";
}

function hasRole(node: { roles?: string[] }, role: string) {
  return node.roles?.some((value) => value.toLowerCase() === role.toLowerCase()) ?? false;
}

function nodeKindLabel(node: { kind: string; monitorMode: string; roles?: string[] }) {
  if (node.kind === "nis1") return "NIS1";
  if (node.monitorMode === "peer") return hasRole(node, "Voting") ? "Symbol Voting" : "Symbol Peer";
  return "Symbol API";
}

function formatNetwork(value: string | null) {
  if (!value) return "—";
  const normalized = value.trim().toLowerCase();
  if (["-104", "104", "symbol--104", "symbol-104", "symbol mainnet", "symbol main net"].includes(normalized)) return "SYMBOL Mainnet";
  if (["152", "symbol-152", "symbol testnet", "symbol test net"].includes(normalized)) return "SYMBOL Testnet";
  return value;
}

function formatXym(value: string | null) {
  if (!value) return "—";
  try {
    const whole = BigInt(value) / 1_000_000n;
    return `${whole.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",")} XYM`;
  } catch {
    const micro = Number(value);
    if (!Number.isFinite(micro)) return "—";
    return `${new Intl.NumberFormat("ja-JP", { maximumFractionDigits: 0 }).format(Math.trunc(micro / 1_000_000))} XYM`;
  }
}

function formatDelta(value: number | null) {
  if (value == null) return "—";
  if (value > 0) return `+${value}`;
  return String(value);
}

function formatDays(value: number | null) {
  if (value == null) return "—";
  if (value < 0) return "期限切れ";
  return `${value} 日`;
}

function formatEpochs(value: number | null) {
  if (value == null) return "—";
  if (value === 0) return "終了";
  return `${value} epoch`;
}

const supernodeCheckLabels: Record<string, string> = {
  balance: "残高",
  bandwidth: "帯域",
  chainHeight: "チェーン高",
  chainPart: "チェーン一致",
  computingPower: "計算性能",
  ping: "Ping",
  responsiveness: "応答性",
  version: "バージョン",
};

function supernodeLabel(passed: boolean | null, status: string | null) {
  if (passed === true) return "SN OK";
  if (passed === false) return "SN NG";
  if (status === "not_found") return "SN —";
  return "SN —";
}

function supernodeClass(passed: boolean | null, status: string | null) {
  if (passed === true) return "supernode-chip--ok";
  if (passed === false) return "supernode-chip--ng";
  if (status === "not_found") return "supernode-chip--muted";
  return "supernode-chip--muted";
}

function formatFailedChecks(values: string[]) {
  return values.length ? values.map((value) => supernodeCheckLabels[value] ?? value).join(" / ") : "—";
}

function shortKey(value: string | null | undefined) {
  return value ? `${value.slice(0, 6)}…${value.slice(-6)}` : "—";
}

function EmptyState({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="empty-state">
      <Network size={38} aria-hidden="true" />
      <h3>{title}</h3>
      <p>{children}</p>
    </div>
  );
}

const diagnosticStatusLabel = {
  passed: "正常",
  warning: "注意あり",
  failed: "異常あり",
};

function DiagnosticResult({ run }: { run: DiagnosticRun | undefined }) {
  if (!run) return <EmptyState title="診断履歴はありません">「今すぐ診断」でネットワークとノードAPIをまとめて確認できます。</EmptyState>;
  return <div className="diagnostic-result">
    <div className={`diagnostic-summary diagnostic-summary--${run.status}`}>
      <div>{run.status === "passed" ? <CheckCircle2 /> : run.status === "warning" ? <CircleAlert /> : <XCircle />}</div>
      <div><strong>{diagnosticStatusLabel[run.status]}</strong><p>{run.summary}</p><small>{formatTime(run.completedAt)}</small></div>
    </div>
    <div className="diagnostic-checks">{run.checks.map((item) => <article className={`diagnostic-check diagnostic-check--${item.status}`} key={item.key}>
      <div className="diagnostic-check__icon">{item.status === "pass" ? <CheckCircle2 size={17} /> : item.status === "skipped" ? <CircleDashed size={17} /> : item.status === "warning" ? <CircleAlert size={17} /> : <XCircle size={17} />}</div>
      <div><strong>{item.label}</strong><p>{item.message}</p>{item.recommendation && <small>{item.recommendation}</small>}</div>
      <span>{item.durationMs} ms</span>
    </article>)}</div>
  </div>;
}

function DiagnosticPanel({
  latest,
  history,
  isPending,
  error,
  onRun,
}: {
  latest: DiagnosticRun | undefined;
  history: DiagnosticRun[];
  isPending: boolean;
  error: Error | null;
  onRun: () => void;
}) {
  return <section className="panel">
    <div className="panel__header"><div><h2>ワンクリック診断</h2><p>通常監視を変更せず、接続・API・同期状態を再検査します</p></div><button className="button button--primary" onClick={onRun} disabled={isPending}><Stethoscope size={16} />{isPending ? "診断中…" : "今すぐ診断"}</button></div>
    {error && <div className="notice notice--error">診断を実行できません: {String(error)}</div>}
    <DiagnosticResult run={latest} />
    {history.length > 1 && <details className="diagnostic-history"><summary>過去の診断 {history.length - 1}件</summary><ul>{history.slice(1).map((run) => <li key={run.id}><span className={`diagnostic-history__status diagnostic-history__status--${run.status}`}>{diagnosticStatusLabel[run.status]}</span><span>{formatTime(run.completedAt)}</span><span>{run.summary}</span></li>)}</ul></details>}
  </section>;
}

function HarvestEventList({ events }: { events: HarvestEvent[] }) {
  if (events.length === 0) {
    return <p className="harvest-empty">まだハーベスト検知はありません。</p>;
  }
  return <div className="harvest-event-list">{events.slice(0, 1).map((event) => <article className="harvest-event" key={event.id}>
    <Pickaxe size={15} />
    <div><strong>高さ {event.height ?? "—"}</strong><span className="mono">{shortKey(event.harvesterPublicKey)}</span><small>{formatTime(event.observedAt)}</small></div>
  </article>)}</div>;
}

function Layout({ children }: { children: React.ReactNode }) {
  const { sidebarOpen, setSidebarOpen } = useUiStore();
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
  return (
    <div className="app-shell">
      <aside className={`sidebar ${sidebarOpen ? "sidebar--open" : ""}`}>
        <div className="brand">
          <div className="brand__mark"><Activity size={22} /></div>
          <div><strong>Harvest Scope</strong><span>NEM / Symbol</span></div>
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
          <div><h1>Harvest Scope</h1><p>Symbol / NEM ノードの状態をひと目で確認</p></div>
          <div className="live-indicator"><span />監視エンジン稼働中</div>
          <button className="icon-button theme-toggle" aria-label={dark ? "ライトテーマに切り替え" : "ダークテーマに切り替え"} title={dark ? "ライトテーマ" : "ダークテーマ"} disabled={changeTheme.isPending} onClick={() => changeTheme.mutate(dark ? "light" : "dark")}>{dark ? <Sun size={17} /> : <Moon size={17} />}</button>
        </header>
        <div className="page">{children}</div>
      </main>
    </div>
  );
}

const statMeta: { key: keyof Pick<ReturnType<typeof emptyDashboard>, "total" | "healthy" | "warning" | "critical" | "offline">; label: string; tone: string }[] = [
  { key: "total", label: "監視ノード", tone: "blue" },
  { key: "healthy", label: "正常", tone: "green" },
  { key: "warning", label: "警告", tone: "amber" },
  { key: "critical", label: "重大", tone: "red" },
  { key: "offline", label: "オフライン", tone: "muted" },
];

function emptyDashboard() {
  return { total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, rechecking: false, nodes: [] };
}

function dashboardFromNodes(nodes: NodeSummary[], rechecking = false): DashboardData {
  return {
    total: nodes.length,
    healthy: nodes.filter((node) => node.status === "healthy").length,
    warning: nodes.filter((node) => node.status === "warning").length,
    critical: nodes.filter((node) => node.status === "critical").length,
    offline: nodes.filter((node) => node.status === "offline").length,
    rechecking,
    nodes,
  };
}

function updateDashboard(
  queryClient: ReturnType<typeof useQueryClient>,
  updater: (nodes: NodeSummary[]) => NodeSummary[],
) {
  queryClient.setQueryData<DashboardData>(["dashboard"], (current) => {
    if (!current) return current;
    return dashboardFromNodes(updater(current.nodes), current.rechecking);
  });
}

function restoreDashboard(
  queryClient: ReturnType<typeof useQueryClient>,
  context: { previous?: DashboardData } | undefined,
) {
  if (context?.previous) queryClient.setQueryData(["dashboard"], context.previous);
}

function DashboardPage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { nodeFilter, setNodeFilter } = useUiStore();
  const [statusFilter, setStatusFilter] = useState<NodeStatus | "all">("all");
  const { data = emptyDashboard(), isLoading, error } = useQuery({ queryKey: ["dashboard"], queryFn: api.dashboard, refetchInterval: 10_000 });
  const refreshNode = useMutation({ mutationFn: api.refreshNode, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["dashboard"] }) });
  const refreshAll = useMutation({
    mutationFn: api.refreshAllNodes,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
  });
  const isRechecking = refreshAll.isPending || data.rechecking;
  const pause = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => enabled ? api.pauseNode(id) : api.resumeNode(id),
    onMutate: async ({ id, enabled }) => {
      await queryClient.cancelQueries({ queryKey: ["dashboard"] });
      const previous = queryClient.getQueryData<DashboardData>(["dashboard"]);
      updateDashboard(queryClient, (current) => current.map((node) => node.id === id ? { ...node, enabled: !enabled, status: "unknown" } : node));
      return { previous };
    },
    onError: (_error, _variables, context) => restoreDashboard(queryClient, context),
  });
  const reorder = useMutation({
    mutationFn: ({ id, direction }: { id: string; direction: "up" | "down" }) => api.moveNode(id, direction),
    onMutate: async ({ id, direction }) => {
      await queryClient.cancelQueries({ queryKey: ["dashboard"] });
      const previous = queryClient.getQueryData<DashboardData>(["dashboard"]);
      updateDashboard(queryClient, (current) => {
        const next = [...current];
        const index = next.findIndex((node) => node.id === id);
        const target = direction === "up" ? index - 1 : index + 1;
        if (index < 0 || target < 0 || target >= next.length) return current;
        [next[index], next[target]] = [next[target], next[index]];
        return next;
      });
      return { previous };
    },
    onError: (_error, _variables, context) => restoreDashboard(queryClient, context),
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.deleteNode(id),
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: ["dashboard"] });
      const previous = queryClient.getQueryData<DashboardData>(["dashboard"]);
      updateDashboard(queryClient, (current) => current.filter((node) => node.id !== id));
      return { previous };
    },
    onError: (_error, _variables, context) => restoreDashboard(queryClient, context),
  });
  const nodes = useMemo(() => data.nodes.filter((node) => {
    const text = `${node.displayName} ${node.baseUrl} ${node.network ?? ""}`.toLowerCase();
    return text.includes(nodeFilter.toLowerCase()) && (statusFilter === "all" || node.status === statusFilter);
  }), [data.nodes, nodeFilter, statusFilter]);
  const filterActive = nodeFilter.trim() !== "" || statusFilter !== "all";
  const reorderTitle = filterActive ? "絞り込みを解除すると並び替えできます" : undefined;

  function unregisterNode(nodeId: string, displayName: string) {
    const confirmed = window.confirm(`「${displayName}」の登録を解除しますか？\n保存済みの監視履歴とアラートも削除されます。`);
    if (confirmed) remove.mutate(nodeId);
  }

  return (
    <>
      <section className="stats-grid">
        {statMeta.map((stat) => <article key={stat.key} className={`stat-card stat-card--${stat.tone}`}><span>{stat.label}</span><strong>{isRechecking && stat.key !== "total" ? "—" : data[stat.key]}</strong></article>)}
      </section>
      <section className="panel">
        <div className="panel__header">
          <div><h2>ノード一覧</h2><p>高さ、同期遅延、接続状態を自動更新します</p></div>
          <div className="actions">
            <button className="button" aria-label="全ノード再確認" disabled={isRechecking} onClick={() => refreshAll.mutate()}><RefreshCw size={17} />{isRechecking ? "再確認中…" : "全ノード再確認"}</button>
            <label className="search-field"><Search size={17} /><input value={nodeFilter} onChange={(event) => setNodeFilter(event.target.value)} placeholder="名前・URL・ネットワーク" /></label>
            <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as NodeStatus | "all")} aria-label="状態で絞り込み">
              <option value="all">すべての状態</option><option value="healthy">正常</option><option value="warning">警告</option><option value="critical">重大</option><option value="offline">オフライン</option><option value="unknown">未確認</option>
            </select>
            <button className="button button--primary" onClick={() => navigate("/discovery")}><Plus size={17} />ノード追加</button>
          </div>
        </div>
        {error && <div className="notice notice--error">監視サービスに接続できません: {String(error)}</div>}
        {(reorder.error || remove.error || refreshAll.error) && <div className="notice notice--error">操作に失敗しました: {String(reorder.error ?? remove.error ?? refreshAll.error)}</div>}
        {isRechecking && <div className="notice notice--info">全ノードの接続状態を再確認しています。完了すると自動的に表示を更新します。</div>}
        {isLoading ? <div className="loading">監視データを読み込み中…</div> : nodes.length === 0 ? (
          <EmptyState title="監視ノードがありません">「ノード追加」からURLを登録するか、LAN内をスキャンしてください。</EmptyState>
        ) : (
          <div className="table-wrap"><table className="node-table">
            <thead><tr><th>ノード</th><th>種別</th><th>状態</th><th>チェーン高</th><th>ファイナライズ高</th><th>遅延</th><th>ピア</th><th>REST</th><th>最終確認</th><th aria-label="操作" /></tr></thead>
            <tbody>{nodes.map((node, index) => <tr key={node.id} className="clickable" onClick={() => navigate(`/nodes/${node.id}`)}>
              <td className="node-table__node" title={`${node.displayName}\n${node.baseUrl}`}><strong>{node.displayName}</strong><small>{node.baseUrl}</small>{node.showHarvestMetrics && node.kind === "symbol" && <div className="harvest-chips"><span><Cherry size={12} />{node.delegatedHarvesterCount ?? "—"}</span><span className={node.delegatedHarvesterDelta && node.delegatedHarvesterDelta !== 0 ? (node.delegatedHarvesterDelta > 0 ? "text-good" : "text-warn") : undefined}>{formatDelta(node.delegatedHarvesterDelta)}</span><span>{formatXym(node.totalBalanceMicroXym)}</span></div>}{node.kind === "nis1" && <div className="supernode-chips"><span className={`supernode-chip ${supernodeClass(node.supernodeChecksPassed, node.supernodeStatus)}`}>{supernodeLabel(node.supernodeChecksPassed, node.supernodeStatus)}</span>{node.supernodeLastRound != null && <span>#{node.supernodeLastRound}</span>}</div>}</td>
              <td><span className={`kind kind--${node.kind}`}>{nodeKindLabel(node)}</span></td>
              <td><StatusBadge status={isRechecking ? "unknown" : node.status} label={isRechecking ? "再確認中" : undefined} /></td><td className="mono">{node.height ?? "—"}</td><td className="mono">{node.finalizedHeight ?? "—"}</td><td>{node.lag ?? "—"}</td><td>{node.peerCount ?? "—"}</td><td>{formatLatency(node.latencyMs)}</td><td>{formatTime(node.lastSeenAt)}</td>
              <td><div className="row-actions">
                <button className="icon-button" aria-label={`${node.displayName}を上へ移動`} title={reorderTitle ?? "上へ移動"} disabled={filterActive || index === 0 || reorder.isPending} onClick={(event) => { event.stopPropagation(); reorder.mutate({ id: node.id, direction: "up" }); }}><ArrowUp size={16} /></button>
                <button className="icon-button" aria-label={`${node.displayName}を下へ移動`} title={reorderTitle ?? "下へ移動"} disabled={filterActive || index === nodes.length - 1 || reorder.isPending} onClick={(event) => { event.stopPropagation(); reorder.mutate({ id: node.id, direction: "down" }); }}><ArrowDown size={16} /></button>
                <button className="icon-button" title="今すぐ更新" onClick={(event) => { event.stopPropagation(); refreshNode.mutate(node.id); }}><RefreshCw size={16} /></button>
                <button className="icon-button" title={node.enabled ? "監視を停止" : "監視を再開"} onClick={(event) => { event.stopPropagation(); pause.mutate({ id: node.id, enabled: node.enabled }); }}>{node.enabled ? <Pause size={16} /> : <Play size={16} />}</button>
                <button className="icon-button icon-button--danger" aria-label={`${node.displayName}の登録を解除`} title="登録解除" disabled={remove.isPending} onClick={(event) => { event.stopPropagation(); unregisterNode(node.id, node.displayName); }}><Trash2 size={16} /></button>
                <ChevronRight size={17} />
              </div></td>
            </tr>)}</tbody>
          </table></div>
        )}
      </section>
    </>
  );
}

function NodeDetailPage() {
  const { nodeId = "" } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data, isLoading, error } = useQuery({ queryKey: ["node", nodeId], queryFn: () => api.nodeDetail(nodeId), refetchInterval: 10_000 });
  const { data: metrics = [] } = useQuery({ queryKey: ["metrics", nodeId], queryFn: () => api.metrics(nodeId, 0, Date.now()), refetchInterval: 30_000 });
  const { data: diagnostics = [], refetch: refetchDiagnostics } = useQuery({ queryKey: ["diagnostics", nodeId], queryFn: () => api.nodeDiagnostics(nodeId) });
  const diagnose = useMutation({ mutationFn: () => api.runNodeDiagnostic(nodeId), onSuccess: () => void refetchDiagnostics() });
  const remove = useMutation({ mutationFn: () => api.deleteNode(nodeId), onSuccess: () => navigate("/") });
  const toggleHarvest = useMutation({
    mutationFn: (visible: boolean) => api.setNodeHarvestMetricsVisible(nodeId, visible),
    onMutate: async (visible) => {
      await queryClient.cancelQueries({ queryKey: ["node", nodeId] });
      const previousDetail = queryClient.getQueryData<NodeDetail>(["node", nodeId]);
      const previousDashboard = queryClient.getQueryData<DashboardData>(["dashboard"]);
      const previousTopology = queryClient.getQueryData<TopologySnapshot>(["topology"]);
      queryClient.setQueryData<NodeDetail>(["node", nodeId], (current) => current ? {
        ...current,
        node: { ...current.node, showHarvestMetrics: visible },
      } : current);
      queryClient.setQueryData<DashboardData>(["dashboard"], (current) => current ? {
        ...current,
        nodes: current.nodes.map((node) => node.id === nodeId ? { ...node, showHarvestMetrics: visible } : node),
      } : current);
      queryClient.setQueryData<TopologySnapshot>(["topology"], (current) => current ? {
        ...current,
        nodes: current.nodes.map((node) => node.id === nodeId ? { ...node, showHarvestMetrics: visible } : node),
      } : current);
      return { previousDetail, previousDashboard, previousTopology };
    },
    onError: (_error, _visible, context) => {
      if (context?.previousDetail) queryClient.setQueryData(["node", nodeId], context.previousDetail);
      if (context?.previousDashboard) queryClient.setQueryData(["dashboard"], context.previousDashboard);
      if (context?.previousTopology) queryClient.setQueryData(["topology"], context.previousTopology);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: ["node", nodeId] });
      void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      void queryClient.invalidateQueries({ queryKey: ["topology"] });
    },
  });
  if (isLoading) return <div className="loading">ノード詳細を読み込み中…</div>;
  if (error || !data) return <div className="notice notice--error">ノード情報を取得できません。</div>;
  return <>
    <div className="page-heading"><div><button className="link-button" onClick={() => navigate("/")}>← ダッシュボード</button><h2>{data.node.displayName}</h2><p>{data.node.baseUrl}</p></div><div className="actions"><StatusBadge status={data.node.status} /><button className="button button--danger" onClick={() => remove.mutate()}><Trash2 size={16} />削除</button></div></div>
    <section className="detail-grid">
      <article className="panel detail-card"><h3>チェーン</h3><dl><div><dt>高さ</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.node.height ?? "—"}</dd></div><div><dt>ファイナライズ高</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.node.finalizedHeight ?? "対象外"}</dd></div><div><dt>同期遅延</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.node.lag ?? "—"}</dd></div><div><dt>チェーンスコア</dt><dd className="mono text-truncate">{isPeerMonitor(data.node) ? "対象外" : data.chainScore ?? "—"}</dd></div></dl></article>
      <article className="panel detail-card"><h3>ノード</h3><dl><div><dt>種別</dt><dd>{nodeKindLabel({ ...data.node, roles: data.roles })}</dd></div><div><dt>ネットワーク</dt><dd>{formatNetwork(data.node.network)}</dd></div><div><dt>バージョン</dt><dd>{data.node.version ?? "—"}</dd></div><div><dt>REST</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.restVersion ?? "—"}</dd></div></dl></article>
      <article className="panel detail-card"><h3>接続</h3><dl><div><dt>{isPeerMonitor(data.node) ? "P2P応答" : "REST応答"}</dt><dd>{formatLatency(data.node.latencyMs)}</dd></div><div><dt>ピア</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.node.peerCount ?? "—"}</dd></div><div><dt>API / DB</dt><dd>{isPeerMonitor(data.node) ? "対象外" : [data.apiStatus, data.dbStatus].filter(Boolean).join(" / ") || "—"}</dd></div><div><dt>WebSocket</dt><dd>{isPeerMonitor(data.node) ? "対象外" : data.node.kind === "symbol" ? (data.websocketConnected ? "接続中" : "RESTへフォールバック") : "対象外"}</dd></div></dl></article>
      <article className="panel detail-card certificate-card"><h3>証明書</h3>{data.node.kind !== "symbol" ? <p className="harvest-empty">Symbolノードのみ対象です。</p> : <><dl><div><dt>ノード証明書</dt><dd>{isPeerMonitor(data.node) ? "対象外" : formatTime(data.node.nodeCertificateNotAfter)}</dd></div><div><dt>残り日数</dt><dd className={data.node.nodeCertificateRemainingDays != null && data.node.nodeCertificateRemainingDays <= 7 ? "text-warn" : undefined}>{isPeerMonitor(data.node) ? "対象外" : formatDays(data.node.nodeCertificateRemainingDays)}</dd></div><div><dt>投票キー範囲</dt><dd>{hasRole(data, "Voting") || !isPeerMonitor(data.node) ? (data.node.votingKeyStartEpoch != null || data.node.votingKeyEndEpoch != null ? `${data.node.votingKeyStartEpoch ?? "—"} - ${data.node.votingKeyEndEpoch ?? "—"}` : "—") : "対象外"}</dd></div><div><dt>残りepoch</dt><dd className={data.node.votingKeyRemainingEpochs != null && data.node.votingKeyRemainingEpochs <= 7 ? "text-warn" : undefined}>{hasRole(data, "Voting") || !isPeerMonitor(data.node) ? formatEpochs(data.node.votingKeyRemainingEpochs) : "対象外"}</dd></div><div><dt>現在epoch</dt><dd>{hasRole(data, "Voting") || !isPeerMonitor(data.node) ? data.node.finalizationEpoch ?? "—" : "対象外"}</dd></div></dl>{data.node.certificateMetricError && <p className="harvest-error">取得エラー: {data.node.certificateMetricError}</p>}</>}</article>
      <article className="panel detail-card harvest-card"><div className="detail-card__heading"><h3>委任ハーベスト</h3>{data.node.kind === "symbol" && <label className="mini-toggle"><input type="checkbox" checked={data.node.showHarvestMetrics} disabled={toggleHarvest.isPending} onChange={(event) => toggleHarvest.mutate(event.target.checked)} />表示</label>}</div>{data.node.kind !== "symbol" ? <p className="harvest-empty">Symbolノードのみ対象です。</p> : data.node.showHarvestMetrics ? <><dl><div><dt>委任者数</dt><dd>{data.node.delegatedHarvesterCount ?? "—"}</dd></div><div><dt>24時間増減</dt><dd>{formatDelta(data.node.delegatedHarvesterDelta)}</dd></div><div><dt>合計XYM</dt><dd>{formatXym(data.node.totalBalanceMicroXym)}</dd></div><div><dt>最新検知</dt><dd>{formatTime(data.node.latestHarvestAt)}</dd></div></dl>{data.node.harvestMetricError && <p className="harvest-error">取得エラー: {data.node.harvestMetricError}</p>}<HarvestEventList events={data.recentHarvests} /></> : <p className="harvest-empty">表示をONにすると委任ハーベスト情報を表示します。</p>}</article>
      <article className="panel detail-card supernode-card"><h3>Supernodeプログラム</h3>{data.node.kind !== "nis1" ? <p className="harvest-empty">NIS1ノードのみ対象です。</p> : data.node.supernodeStatus === "not_found" ? <p className="harvest-empty">Supernode対象リストに見つかりません。</p> : <><dl><div><dt>対象ID</dt><dd>{data.node.supernodeId ?? "—"}</dd></div><div><dt>状態</dt><dd>{data.node.supernodeStatus ?? "—"}</dd></div><div><dt>最新ラウンド</dt><dd>{data.node.supernodeLastRound ?? "—"}</dd></div><div><dt>最終チェック</dt><dd>{formatTime(data.node.supernodeLastRoundAt)}</dd></div><div><dt>総合合否</dt><dd className={data.node.supernodeChecksPassed === false ? "text-warn" : data.node.supernodeChecksPassed === true ? "text-good" : undefined}>{data.node.supernodeChecksPassed == null ? "—" : data.node.supernodeChecksPassed ? "合格" : "失敗"}</dd></div><div><dt>失敗項目</dt><dd>{formatFailedChecks(data.node.supernodeFailedChecks)}</dd></div></dl>{data.node.supernodeMetricError && <p className="harvest-error">取得エラー: {data.node.supernodeMetricError}</p>}</>}</article>
    </section>
    <DiagnosticPanel latest={diagnose.data ?? diagnostics[0]} history={diagnostics} isPending={diagnose.isPending} error={diagnose.error} onRun={() => diagnose.mutate()} />
    <section className="panel"><div className="panel__header"><div><h2>監視期間メトリクス</h2><p>{isPeerMonitor(data.node) ? "P2P応答" : "REST応答、ピア数、同期遅延、委任ハーベスト、Supernode合否"}</p></div><button className="button" onClick={() => api.exportCsv("metrics", nodeId)}><Download size={16} />CSV</button></div>{metrics.length ? <MetricChart data={metrics} showHarvestMetrics={!isPeerMonitor(data.node) && data.node.showHarvestMetrics} showSupernodeMetrics={data.node.kind === "nis1"} latencyLabel={isPeerMonitor(data.node) ? "P2P応答" : "REST応答"} /> : <EmptyState title="履歴を収集中">最初の監視サンプルが保存されるとグラフが表示されます。</EmptyState>}</section>
    <section className="panel"><div className="panel__header"><div><h2>現在のピア</h2><p>{isPeerMonitor(data.node) ? "対象外" : `${data.peers.length}件`}</p></div></div>{isPeerMonitor(data.node) ? <EmptyState title="ピア一覧は対象外です">APIなしPeerノード自身からpeer一覧は取得できません。トポロジーでは他の監視APIノードが報告した接続として表示されます。</EmptyState> : data.peers.length ? <div className="table-wrap"><table><thead><tr><th>名前</th><th>ホスト</th><th>ポート</th><th>状態</th><th>バージョン</th></tr></thead><tbody>{data.peers.map((peer) => <tr key={peer.key}><td>{peer.name ?? "—"}</td><td>{peer.host ?? "—"}</td><td>{peer.port ?? "—"}</td><td>{peer.status}</td><td>{peer.version ?? "—"}</td></tr>)}</tbody></table></div> : <EmptyState title="ピア情報がありません">次回のピアポーリング後に表示されます。</EmptyState>}</section>
  </>;
}

function DiscoveryPage() {
  const queryClient = useQueryClient();
  const { data: interfaces = [] } = useQuery({ queryKey: ["interfaces"], queryFn: api.interfaces });
  const [cidr, setCidr] = useState("");
  const [ports, setPorts] = useState("3000,3001,7890");
  const [manualMode, setManualMode] = useState<"api" | "peer" | "peer-voting">("api");
  const [manualUrl, setManualUrl] = useState("");
  const [peerPublicKey, setPeerPublicKey] = useState("");
  const [peerNetwork, setPeerNetwork] = useState("Symbol Mainnet");
  const scan = useMutation({ mutationFn: api.scan });
  const add = useMutation({
    mutationFn: api.upsertNode,
    onSuccess: (registered) => {
      queryClient.setQueryData<DashboardData>(["dashboard"], (current) => {
        if (!current) return current;
        const nodes = current.nodes.some((node) => node.id === registered.id)
          ? current.nodes.map((node) => node.id === registered.id ? registered : node)
          : [...current.nodes, registered];
        return dashboardFromNodes(nodes, current.rechecking);
      });
    },
  });
  useEffect(() => {
    if (!cidr && interfaces.length > 0) setCidr(interfaces[0].cidr);
  }, [cidr, interfaces]);
  function submitScan(event: FormEvent) { event.preventDefault(); scan.mutate({ cidr, ports: ports.split(",").map(Number).filter((port) => Number.isInteger(port)) }); }
  function addManual(event: FormEvent) {
    event.preventDefault();
    const input = manualMode === "peer"
      ? {
          baseUrl: manualUrl,
          kind: "symbol" as const,
          monitorMode: "peer" as const,
          network: peerNetwork.trim() || undefined,
          publicKey: undefined,
          roles: ["Peer"],
        }
      : manualMode === "peer-voting"
      ? {
          baseUrl: manualUrl,
          kind: "symbol" as const,
          monitorMode: "peer" as const,
          network: peerNetwork.trim() || undefined,
          publicKey: peerPublicKey.trim() || undefined,
          roles: ["Peer", "Voting"],
        }
      : { baseUrl: manualUrl };
    add.mutate(input, { onSuccess: () => { setManualUrl(""); setPeerPublicKey(""); } });
  }
  function addCandidate(candidate: ScanCandidate) { add.mutate({ baseUrl: candidate.baseUrl, displayName: candidate.name ?? undefined, kind: candidate.kind }); }
  return <>
    <div className="page-heading"><div><h2>ノード検出</h2><p>選択したCIDRだけを明示的にスキャンします。候補は確認後に登録されます。</p></div></div>
    <section className="split-grid">
      <form className="panel form-card" onSubmit={submitScan}><h3>LANをスキャン</h3><label>ネットワーク候補<select value={cidr} onChange={(event) => setCidr(event.target.value)}>{!cidr && <option value="">ネットワークを選択</option>}{cidr && !interfaces.some((item) => item.cidr === cidr) && <option value={cidr}>{cidr}</option>}{interfaces.map((item) => <option key={`${item.name}-${item.cidr}`} value={item.cidr}>{item.name} — {item.cidr}</option>)}</select></label><label>CIDR<input value={cidr} onChange={(event) => setCidr(event.target.value)} required /></label><label>ポート（カンマ区切り）<input value={ports} onChange={(event) => setPorts(event.target.value)} required /></label><div className="actions"><button className="button button--primary" disabled={scan.isPending}><Search size={16} />{scan.isPending ? "スキャン中…" : "スキャン開始"}</button>{scan.isPending && <button type="button" className="button" onClick={() => api.cancelScan()}>キャンセル</button>}</div><p className="form-hint">最大1,024アドレス、既定ポート3000 / 3001 / 7890</p></form>
      <form className="panel form-card" onSubmit={addManual}><h3>ノードを手動登録</h3><label>監視モード<select value={manualMode} onChange={(event) => { setManualMode(event.target.value as "api" | "peer" | "peer-voting"); if (add.isSuccess || add.isError) add.reset(); }}><option value="api">REST APIノード</option><option value="peer">Peerノード</option><option value="peer-voting">Peer + Votingノード</option></select></label><label>{manualMode === "api" ? "REST URL" : "Peerエンドポイント"}<input type={manualMode === "api" ? "url" : "text"} value={manualUrl} onChange={(event) => { setManualUrl(event.target.value); if (add.isSuccess || add.isError) add.reset(); }} placeholder={manualMode === "api" ? "http://192.0.2.10:3000" : "192.0.2.10:7900"} required /></label>{manualMode !== "api" && <label>ネットワーク<input value={peerNetwork} onChange={(event) => setPeerNetwork(event.target.value)} placeholder="Symbol Mainnet" required={manualMode === "peer-voting"} /></label>}{manualMode === "peer-voting" && <label>公開鍵<input value={peerPublicKey} onChange={(event) => setPeerPublicKey(event.target.value)} placeholder="必須" required /></label>}<button className="button button--primary" disabled={add.isPending}><Plus size={16} />{add.isPending ? "登録中…" : "登録して確認開始"}</button><p className="form-hint">{manualMode === "api" ? "登録後すぐ一覧に追加し、表示名と接続状態はバックグラウンドで確認します。HTTP/HTTPSのみ指定できます。" : manualMode === "peer-voting" ? "登録後すぐ一覧に追加し、P2P疎通とVoting期限をバックグラウンドで確認します。" : "登録後すぐ一覧に追加し、P2Pポートの疎通をバックグラウンドで確認します。"}</p></form>
    </section>
    {(scan.error || add.error) && <div className="notice notice--error">{String(scan.error ?? add.error)}</div>}
    {add.isSuccess && <div className="notice notice--success">「{add.data.displayName}」を登録しました。<small>{add.data.baseUrl}</small></div>}
    <section className="panel"><div className="panel__header"><div><h2>検出候補</h2><p>{scan.data?.length ?? 0}件のノードを検出</p></div></div>{scan.data?.length ? <div className="candidate-grid">{scan.data.map((candidate) => <article className="candidate" key={candidate.baseUrl}><div><span className={`kind kind--${candidate.kind}`}>{candidate.kind === "symbol" ? "Symbol" : "NIS1"}</span><h3>{candidate.name || candidate.baseUrl}</h3><p>{candidate.baseUrl}</p><small>{candidate.network ?? "ネットワーク未確認"} · {candidate.latencyMs} ms</small></div><button className="button" onClick={() => addCandidate(candidate)}><Plus size={16} />登録</button></article>)}</div> : <EmptyState title="まだスキャンしていません">CIDRとポートを確認して、スキャンを開始してください。</EmptyState>}</section>
  </>;
}

function AlertsPage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data = [], isLoading } = useQuery({ queryKey: ["alerts"], queryFn: api.alerts, refetchInterval: 10_000 });
  const ack = useMutation({ mutationFn: api.acknowledgeAlert, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["alerts"] }) });
  return <section className="panel"><div className="panel__header"><div><h2>アラート</h2><p>継続中の障害と復旧履歴</p></div><button className="button" onClick={() => api.exportCsv("alerts")}><Download size={16} />CSV</button></div>{isLoading ? <div className="loading">読み込み中…</div> : data.length ? <div className="alert-list">{data.map((alert) => <article className={`alert-item alert-item--${alert.severity}`} key={alert.id} onClick={() => navigate(`/alerts/${alert.id}`)}><div className="alert-item__icon"><Bell size={18} /></div><div><div className="alert-item__heading"><strong>{alert.nodeName}</strong><span>{alert.severity === "critical" ? "重大" : "警告"}</span><span className="muted">{alert.state === "resolved" ? "復旧" : alert.state === "acknowledged" ? "確認済み" : "未確認"}</span></div><p>{alert.message}</p><small>{formatTime(alert.updatedAt)} · 発生{alert.occurrenceCount}回</small></div><div className="actions">{alert.state === "open" && <button className="button" onClick={(event) => { event.stopPropagation(); ack.mutate(alert.id); }}>確認</button>}<ChevronRight size={17} /></div></article>)}</div> : <EmptyState title="アラートはありません">異常を検出するとWindows通知とここに表示されます。</EmptyState>}</section>;
}

function AlertDetailPage() {
  const { alertId = "" } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: alert, isLoading, error } = useQuery({ queryKey: ["alert", alertId], queryFn: () => api.alert(alertId) });
  const { data: diagnostics = [], refetch: refetchDiagnostics } = useQuery({ queryKey: ["diagnostics", alert?.nodeId], queryFn: () => api.nodeDiagnostics(alert!.nodeId), enabled: Boolean(alert) });
  const diagnose = useMutation({ mutationFn: () => api.runNodeDiagnostic(alert!.nodeId, alertId), onSuccess: () => void refetchDiagnostics() });
  const acknowledge = useMutation({ mutationFn: () => api.acknowledgeAlert(alertId), onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ["alert", alertId] }); await queryClient.invalidateQueries({ queryKey: ["alerts"] }); } });
  if (isLoading) return <div className="loading">障害情報を読み込み中…</div>;
  if (error || !alert) return <div className="notice notice--error">障害情報を取得できません。</div>;
  const related = diagnostics.filter((run) => run.alertId === alert.id);
  const latest = diagnose.data ?? related[0] ?? diagnostics[0];
  return <>
    <div className="page-heading"><div><button className="link-button" onClick={() => navigate("/alerts")}>← アラート</button><h2>{alert.nodeName}</h2><p>{alert.message}</p></div><div className="actions"><span className={`status status--${alert.severity === "critical" ? "critical" : "warning"}`}>{alert.severity === "critical" ? "重大" : "警告"}</span>{alert.state === "open" && <button className="button" onClick={() => acknowledge.mutate()}>確認済みにする</button>}</div></div>
    <section className="panel alert-detail"><dl><div><dt>状態</dt><dd>{alert.state === "resolved" ? "復旧" : alert.state === "acknowledged" ? "確認済み" : "未確認"}</dd></div><div><dt>種別</dt><dd>{alert.alertType}</dd></div><div><dt>発生</dt><dd>{formatTime(alert.raisedAt)}</dd></div><div><dt>更新</dt><dd>{formatTime(alert.updatedAt)}</dd></div><div><dt>復旧</dt><dd>{formatTime(alert.resolvedAt)}</dd></div><div><dt>発生回数</dt><dd>{alert.occurrenceCount}</dd></div></dl><button className="link-button" onClick={() => navigate(`/nodes/${alert.nodeId}`)}>ノード詳細を表示 →</button></section>
    <DiagnosticPanel latest={latest} history={related.length ? related : diagnostics} isPending={diagnose.isPending} error={diagnose.error} onRun={() => diagnose.mutate()} />
  </>;
}

function Onboarding() {
  const queryClient = useQueryClient();
  const { data } = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const save = useMutation({ mutationFn: (autostartEnabled: boolean) => api.updateSettings({ ...(data ?? defaultSettings), backgroundMonitoring: true, autostartEnabled, onboardingCompleted: true }), onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }) });
  if (!data || data.onboardingCompleted) return null;
  return <div className="modal-backdrop"><section className="onboarding" role="dialog" aria-modal="true" aria-labelledby="onboarding-title"><ShieldCheck size={34} /><h2 id="onboarding-title">バックグラウンド監視を開始します</h2><p>ウィンドウを閉じてもトレイで監視を続け、異常時にWindows通知でお知らせします。</p><div className="actions"><button className="button button--primary" disabled={save.isPending} onClick={() => save.mutate(true)}>ログオン時に起動する</button><button className="button" disabled={save.isPending} onClick={() => save.mutate(false)}>今はしない</button></div>{save.error && <div className="notice notice--error">設定を保存できません: {String(save.error)}</div>}</section></div>;
}

function SettingsPage() {
  const queryClient = useQueryClient();
  const { data = defaultSettings, isLoading } = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [nodeConfigMessage, setNodeConfigMessage] = useState<string | null>(null);
  const value = draft ?? data;
  const save = useMutation({ mutationFn: api.updateSettings, onSuccess: (settings) => { setDraft(settings); void queryClient.invalidateQueries({ queryKey: ["settings"] }); } });
  const exportNodes = useMutation({
    mutationFn: api.exportNodeConfig,
    onSuccess: (path) => setNodeConfigMessage(path ? `ノード設定を書き出しました: ${path}` : null),
  });
  const importNodes = useMutation({
    mutationFn: api.importNodeConfig,
    onSuccess: async (result) => {
      if (!result) {
        setNodeConfigMessage(null);
        return;
      }
      setNodeConfigMessage(`ノード設定を読み込みました: 追加 ${result.added}件 / 更新 ${result.updated}件 / 失敗 ${result.failed}件`);
      await queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      await queryClient.invalidateQueries({ queryKey: ["topology"] });
    },
  });
  const update = <K extends keyof AppSettings>(key: K, next: AppSettings[K]) => setDraft({ ...value, [key]: next });
  if (isLoading) return <div className="loading">設定を読み込み中…</div>;
  return <section className="panel settings-panel"><div className="panel__header"><div><h2>設定</h2><p>監視間隔、通知、履歴保持を調整</p></div><button className="button button--primary" onClick={() => save.mutate(value)} disabled={save.isPending}>保存</button></div>
    <div className="settings-section"><h3>監視</h3><div className="settings-grid"><label>ヘルス間隔（秒）<input type="number" min="5" max="300" value={value.healthIntervalSeconds} onChange={(event) => update("healthIntervalSeconds", Number(event.target.value))} /></label><label>チェーン間隔（秒）<input type="number" min="10" max="600" value={value.chainIntervalSeconds} onChange={(event) => update("chainIntervalSeconds", Number(event.target.value))} /></label><label>ピア間隔（秒）<input type="number" min="30" max="1800" value={value.peerIntervalSeconds} onChange={(event) => update("peerIntervalSeconds", Number(event.target.value))} /></label><label>REST警告（ms）<input type="number" min="250" max="30000" value={value.latencyWarningMs} onChange={(event) => update("latencyWarningMs", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>同期アラート</h3><div className="settings-grid"><label>警告ブロック差<input type="number" min="1" value={value.lagWarningBlocks} onChange={(event) => update("lagWarningBlocks", Number(event.target.value))} /></label><label>重大ブロック差<input type="number" min="2" value={value.lagCriticalBlocks} onChange={(event) => update("lagCriticalBlocks", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>データ保持</h3><div className="settings-grid"><label>生データ（日）<input type="number" min="1" max="90" value={value.rawRetentionDays} onChange={(event) => update("rawRetentionDays", Number(event.target.value))} /></label><label>集約データ（日）<input type="number" min="30" max="1825" value={value.aggregateRetentionDays} onChange={(event) => update("aggregateRetentionDays", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>表示</h3><div className="settings-grid"><label>テーマ<select value={value.theme} onChange={(event) => update("theme", event.target.value as AppSettings["theme"])}><option value="system">Windowsの設定に合わせる</option><option value="light">ライト</option><option value="dark">ダーク</option></select></label></div></div>
    <div className="settings-section"><h3>ノード設定</h3><div className="settings-actions"><button className="button" disabled={exportNodes.isPending || importNodes.isPending} onClick={() => exportNodes.mutate()}><FileOutput size={16} />エクスポート</button><button className="button" disabled={exportNodes.isPending || importNodes.isPending} onClick={() => importNodes.mutate()}><FileInput size={16} />インポート</button></div><p className="form-hint">監視対象ノードのURL、表示名、有効状態、お気に入り、ハーベスト表示設定だけをJSONで移行します。</p>{nodeConfigMessage && <div className="notice notice--success">{nodeConfigMessage}</div>}{(exportNodes.error || importNodes.error) && <div className="notice notice--error">ノード設定の操作に失敗しました: {String(exportNodes.error ?? importNodes.error)}</div>}{importNodes.data?.failures.length ? <ul className="import-failure-list">{importNodes.data.failures.slice(0, 5).map((failure, index) => <li key={`${failure.baseUrl ?? failure.displayName ?? "failure"}-${index}`}><strong>{failure.displayName || failure.baseUrl || "不明なノード"}</strong><span>{failure.reason}</span></li>)}</ul> : null}</div>
    <div className="settings-section"><h3>Windows</h3><div className="toggle-list"><label><span><strong>Windows通知</strong><small>障害発生・重大化・復旧を通知します</small></span><input type="checkbox" checked={value.notificationEnabled} onChange={(event) => update("notificationEnabled", event.target.checked)} /></label><label><span><strong>バックグラウンド監視</strong><small>ウィンドウを閉じてもトレイで監視を続けます</small></span><input type="checkbox" checked={value.backgroundMonitoring} onChange={(event) => update("backgroundMonitoring", event.target.checked)} /></label><label><span><strong>Windowsログオン時に起動</strong><small>既定では無効です</small></span><input type="checkbox" checked={value.autostartEnabled} onChange={(event) => update("autostartEnabled", event.target.checked)} /></label></div></div>
    {save.isSuccess && <div className="notice notice--success">設定を保存しました。</div>}
  </section>;
}

export default function App() {
  useMonitorEvents();
  return <><Layout><Routes><Route path="/" element={<DashboardPage />} /><Route path="/topology" element={<TopologyPage />} /><Route path="/nodes/:nodeId" element={<NodeDetailPage />} /><Route path="/discovery" element={<DiscoveryPage />} /><Route path="/alerts" element={<AlertsPage />} /><Route path="/alerts/:alertId" element={<AlertDetailPage />} /><Route path="/settings" element={<SettingsPage />} /></Routes></Layout><Onboarding /></>;
}
