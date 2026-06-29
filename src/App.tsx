import { FormEvent, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  Bell,
  ChevronRight,
  CircleGauge,
  Download,
  Menu,
  Network,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings as SettingsIcon,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { NavLink, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { api, defaultSettings } from "./services/ipc";
import { useMonitorEvents } from "./hooks/useMonitorEvents";
import { useUiStore } from "./store/ui";
import { StatusBadge } from "./components/StatusBadge";
import { MetricChart } from "./components/MetricChart";
import type { AppSettings, NodeStatus, ScanCandidate } from "./types";

function formatTime(value: number | null) {
  return value ? new Intl.DateTimeFormat("ja-JP", { dateStyle: "short", timeStyle: "medium" }).format(value) : "—";
}

function formatLatency(value: number | null) {
  return value == null ? "—" : `${value} ms`;
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

function Layout({ children }: { children: React.ReactNode }) {
  const { sidebarOpen, setSidebarOpen } = useUiStore();
  const nav = [
    { to: "/", label: "ダッシュボード", icon: CircleGauge },
    { to: "/discovery", label: "ノード検出", icon: Network },
    { to: "/alerts", label: "アラート", icon: Bell },
    { to: "/settings", label: "設定", icon: SettingsIcon },
  ];
  return (
    <div className="app-shell">
      <aside className={`sidebar ${sidebarOpen ? "sidebar--open" : ""}`}>
        <div className="brand">
          <div className="brand__mark"><Activity size={22} /></div>
          <div><strong>Node Watcher</strong><span>NEM / Symbol</span></div>
          <button className="icon-button sidebar__close" onClick={() => setSidebarOpen(false)} aria-label="メニューを閉じる"><X /></button>
        </div>
        <nav>
          {nav.map(({ to, label, icon: Icon }) => (
            <NavLink key={to} to={to} end={to === "/"} onClick={() => setSidebarOpen(false)}>
              <Icon size={19} /><span>{label}</span>
            </NavLink>
          ))}
        </nav>
        <div className="sidebar__footer"><ShieldCheck size={17} /> 読み取り専用監視</div>
      </aside>
      {sidebarOpen && <button className="scrim" onClick={() => setSidebarOpen(false)} aria-label="メニューを閉じる" />}
      <main className="main">
        <header className="topbar">
          <button className="icon-button mobile-menu" onClick={() => setSidebarOpen(true)} aria-label="メニューを開く"><Menu /></button>
          <div><h1>ノード監視</h1><p>ローカルネットワークの状態をひと目で確認</p></div>
          <div className="live-indicator"><span />監視エンジン稼働中</div>
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
  return { total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, nodes: [] };
}

function DashboardPage() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { nodeFilter, setNodeFilter } = useUiStore();
  const [statusFilter, setStatusFilter] = useState<NodeStatus | "all">("all");
  const { data = emptyDashboard(), isLoading, error } = useQuery({ queryKey: ["dashboard"], queryFn: api.dashboard, refetchInterval: 10_000 });
  const refresh = useMutation({ mutationFn: api.refreshNode, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["dashboard"] }) });
  const pause = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => enabled ? api.pauseNode(id) : api.resumeNode(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
  });
  const nodes = useMemo(() => data.nodes.filter((node) => {
    const text = `${node.displayName} ${node.baseUrl} ${node.network ?? ""}`.toLowerCase();
    return text.includes(nodeFilter.toLowerCase()) && (statusFilter === "all" || node.status === statusFilter);
  }), [data.nodes, nodeFilter, statusFilter]);

  return (
    <>
      <section className="stats-grid">
        {statMeta.map((stat) => <article key={stat.key} className={`stat-card stat-card--${stat.tone}`}><span>{stat.label}</span><strong>{data[stat.key]}</strong></article>)}
      </section>
      <section className="panel">
        <div className="panel__header">
          <div><h2>ノード一覧</h2><p>高さ、同期遅延、接続状態を自動更新します</p></div>
          <div className="actions">
            <label className="search-field"><Search size={17} /><input value={nodeFilter} onChange={(event) => setNodeFilter(event.target.value)} placeholder="名前・URL・ネットワーク" /></label>
            <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as NodeStatus | "all")} aria-label="状態で絞り込み">
              <option value="all">すべての状態</option><option value="healthy">正常</option><option value="warning">警告</option><option value="critical">重大</option><option value="offline">オフライン</option><option value="unknown">未確認</option>
            </select>
            <button className="button button--primary" onClick={() => navigate("/discovery")}><Plus size={17} />ノード追加</button>
          </div>
        </div>
        {error && <div className="notice notice--error">監視サービスに接続できません: {String(error)}</div>}
        {isLoading ? <div className="loading">監視データを読み込み中…</div> : nodes.length === 0 ? (
          <EmptyState title="監視ノードがありません">「ノード追加」からURLを登録するか、LAN内をスキャンしてください。</EmptyState>
        ) : (
          <div className="table-wrap"><table>
            <thead><tr><th>ノード</th><th>種別</th><th>状態</th><th>チェーン高</th><th>最終化高</th><th>遅延</th><th>ピア</th><th>REST</th><th>最終確認</th><th aria-label="操作" /></tr></thead>
            <tbody>{nodes.map((node) => <tr key={node.id} className="clickable" onClick={() => navigate(`/nodes/${node.id}`)}>
              <td><strong>{node.displayName}</strong><small>{node.baseUrl}</small></td>
              <td><span className={`kind kind--${node.kind}`}>{node.kind === "symbol" ? "Symbol" : "NIS1"}</span></td>
              <td><StatusBadge status={node.status} /></td><td className="mono">{node.height ?? "—"}</td><td className="mono">{node.finalizedHeight ?? "—"}</td><td>{node.lag ?? "—"}</td><td>{node.peerCount ?? "—"}</td><td>{formatLatency(node.latencyMs)}</td><td>{formatTime(node.lastSeenAt)}</td>
              <td><div className="row-actions">
                <button className="icon-button" title="今すぐ更新" onClick={(event) => { event.stopPropagation(); refresh.mutate(node.id); }}><RefreshCw size={16} /></button>
                <button className="icon-button" title={node.enabled ? "監視を停止" : "監視を再開"} onClick={(event) => { event.stopPropagation(); pause.mutate({ id: node.id, enabled: node.enabled }); }}>{node.enabled ? <Pause size={16} /> : <Play size={16} />}</button>
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
  const now = Date.now();
  const { data, isLoading, error } = useQuery({ queryKey: ["node", nodeId], queryFn: () => api.nodeDetail(nodeId), refetchInterval: 10_000 });
  const { data: metrics = [] } = useQuery({ queryKey: ["metrics", nodeId], queryFn: () => api.metrics(nodeId, now - 24 * 60 * 60 * 1000, now), refetchInterval: 30_000 });
  const remove = useMutation({ mutationFn: () => api.deleteNode(nodeId), onSuccess: () => navigate("/") });
  if (isLoading) return <div className="loading">ノード詳細を読み込み中…</div>;
  if (error || !data) return <div className="notice notice--error">ノード情報を取得できません。</div>;
  return <>
    <div className="page-heading"><div><button className="link-button" onClick={() => navigate("/")}>← ダッシュボード</button><h2>{data.node.displayName}</h2><p>{data.node.baseUrl}</p></div><div className="actions"><StatusBadge status={data.node.status} /><button className="button button--danger" onClick={() => remove.mutate()}><Trash2 size={16} />削除</button></div></div>
    <section className="detail-grid">
      <article className="panel detail-card"><h3>チェーン</h3><dl><div><dt>高さ</dt><dd>{data.node.height ?? "—"}</dd></div><div><dt>最終化高</dt><dd>{data.node.finalizedHeight ?? "対象外"}</dd></div><div><dt>同期遅延</dt><dd>{data.node.lag ?? "—"}</dd></div><div><dt>チェーンスコア</dt><dd className="mono text-truncate">{data.chainScore ?? "—"}</dd></div></dl></article>
      <article className="panel detail-card"><h3>ノード</h3><dl><div><dt>種別</dt><dd>{data.node.kind === "symbol" ? "Symbol" : "NIS1"}</dd></div><div><dt>ネットワーク</dt><dd>{data.node.network ?? "—"}</dd></div><div><dt>バージョン</dt><dd>{data.node.version ?? "—"}</dd></div><div><dt>REST</dt><dd>{data.restVersion ?? "—"}</dd></div></dl></article>
      <article className="panel detail-card"><h3>接続</h3><dl><div><dt>REST応答</dt><dd>{formatLatency(data.node.latencyMs)}</dd></div><div><dt>ピア</dt><dd>{data.node.peerCount ?? "—"}</dd></div><div><dt>API / DB</dt><dd>{[data.apiStatus, data.dbStatus].filter(Boolean).join(" / ") || "—"}</dd></div><div><dt>WebSocket</dt><dd>{data.node.kind === "symbol" ? (data.websocketConnected ? "接続中" : "RESTへフォールバック") : "対象外"}</dd></div></dl></article>
    </section>
    <section className="panel"><div className="panel__header"><div><h2>24時間メトリクス</h2><p>REST応答、ピア数、同期遅延</p></div><button className="button" onClick={() => api.exportCsv("metrics", nodeId)}><Download size={16} />CSV</button></div>{metrics.length ? <MetricChart data={metrics} /> : <EmptyState title="履歴を収集中">最初の監視サンプルが保存されるとグラフが表示されます。</EmptyState>}</section>
    <section className="panel"><div className="panel__header"><div><h2>現在のピア</h2><p>{data.peers.length}件</p></div></div>{data.peers.length ? <div className="table-wrap"><table><thead><tr><th>名前</th><th>ホスト</th><th>ポート</th><th>状態</th><th>バージョン</th></tr></thead><tbody>{data.peers.map((peer) => <tr key={peer.key}><td>{peer.name ?? "—"}</td><td>{peer.host ?? "—"}</td><td>{peer.port ?? "—"}</td><td>{peer.status}</td><td>{peer.version ?? "—"}</td></tr>)}</tbody></table></div> : <EmptyState title="ピア情報がありません">次回のピアポーリング後に表示されます。</EmptyState>}</section>
  </>;
}

function DiscoveryPage() {
  const queryClient = useQueryClient();
  const { data: interfaces = [] } = useQuery({ queryKey: ["interfaces"], queryFn: api.interfaces });
  const [cidr, setCidr] = useState("192.0.2.10/24");
  const [ports, setPorts] = useState("3000,3001,7890");
  const [manualUrl, setManualUrl] = useState("");
  const [manualName, setManualName] = useState("");
  const scan = useMutation({ mutationFn: api.scan });
  const add = useMutation({ mutationFn: api.upsertNode, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["dashboard"] }) });
  function submitScan(event: FormEvent) { event.preventDefault(); scan.mutate({ cidr, ports: ports.split(",").map(Number).filter((port) => Number.isInteger(port)) }); }
  function addManual(event: FormEvent) { event.preventDefault(); add.mutate({ baseUrl: manualUrl, displayName: manualName || undefined }); setManualUrl(""); setManualName(""); }
  function addCandidate(candidate: ScanCandidate) { add.mutate({ baseUrl: candidate.baseUrl, displayName: candidate.name ?? undefined, kind: candidate.kind }); }
  return <>
    <div className="page-heading"><div><h2>ノード検出</h2><p>選択したCIDRだけを明示的にスキャンします。候補は確認後に登録されます。</p></div></div>
    <section className="split-grid">
      <form className="panel form-card" onSubmit={submitScan}><h3>LANをスキャン</h3><label>ネットワーク候補<select value={cidr} onChange={(event) => setCidr(event.target.value)}><option value={cidr}>{cidr}</option>{interfaces.filter((item) => item.cidr !== cidr).map((item) => <option key={`${item.name}-${item.cidr}`} value={item.cidr}>{item.name} — {item.cidr}</option>)}</select></label><label>CIDR<input value={cidr} onChange={(event) => setCidr(event.target.value)} required /></label><label>ポート（カンマ区切り）<input value={ports} onChange={(event) => setPorts(event.target.value)} required /></label><div className="actions"><button className="button button--primary" disabled={scan.isPending}><Search size={16} />{scan.isPending ? "スキャン中…" : "スキャン開始"}</button>{scan.isPending && <button type="button" className="button" onClick={() => api.cancelScan()}>キャンセル</button>}</div><p className="form-hint">最大1,024アドレス、既定ポート3000 / 3001 / 7890</p></form>
      <form className="panel form-card" onSubmit={addManual}><h3>URLを手動登録</h3><label>表示名<input value={manualName} onChange={(event) => setManualName(event.target.value)} placeholder="例: Symbol Main API" /></label><label>REST URL<input type="url" value={manualUrl} onChange={(event) => setManualUrl(event.target.value)} placeholder="http://192.0.2.10:3000" required /></label><button className="button button--primary" disabled={add.isPending}><Plus size={16} />接続確認して登録</button><p className="form-hint">HTTP/HTTPSのみ。秘密鍵やログイン情報は入力しないでください。</p></form>
    </section>
    {(scan.error || add.error) && <div className="notice notice--error">{String(scan.error ?? add.error)}</div>}
    <section className="panel"><div className="panel__header"><div><h2>検出候補</h2><p>{scan.data?.length ?? 0}件のノードを検出</p></div></div>{scan.data?.length ? <div className="candidate-grid">{scan.data.map((candidate) => <article className="candidate" key={candidate.baseUrl}><div><span className={`kind kind--${candidate.kind}`}>{candidate.kind === "symbol" ? "Symbol" : "NIS1"}</span><h3>{candidate.name || candidate.baseUrl}</h3><p>{candidate.baseUrl}</p><small>{candidate.network ?? "ネットワーク未確認"} · {candidate.latencyMs} ms</small></div><button className="button" onClick={() => addCandidate(candidate)}><Plus size={16} />登録</button></article>)}</div> : <EmptyState title="まだスキャンしていません">CIDRとポートを確認して、スキャンを開始してください。</EmptyState>}</section>
  </>;
}

function AlertsPage() {
  const queryClient = useQueryClient();
  const { data = [], isLoading } = useQuery({ queryKey: ["alerts"], queryFn: api.alerts, refetchInterval: 10_000 });
  const ack = useMutation({ mutationFn: api.acknowledgeAlert, onSuccess: () => queryClient.invalidateQueries({ queryKey: ["alerts"] }) });
  return <section className="panel"><div className="panel__header"><div><h2>アラート</h2><p>継続中の障害と復旧履歴</p></div><button className="button" onClick={() => api.exportCsv("alerts")}><Download size={16} />CSV</button></div>{isLoading ? <div className="loading">読み込み中…</div> : data.length ? <div className="alert-list">{data.map((alert) => <article className={`alert-item alert-item--${alert.severity}`} key={alert.id}><div className="alert-item__icon"><Bell size={18} /></div><div><div className="alert-item__heading"><strong>{alert.nodeName}</strong><span>{alert.severity === "critical" ? "重大" : "警告"}</span><span className="muted">{alert.state === "resolved" ? "復旧" : alert.state === "acknowledged" ? "確認済み" : "未確認"}</span></div><p>{alert.message}</p><small>{formatTime(alert.updatedAt)} · 発生{alert.occurrenceCount}回</small></div>{alert.state === "open" && <button className="button" onClick={() => ack.mutate(alert.id)}>確認</button>}</article>)}</div> : <EmptyState title="アラートはありません">異常を検出するとWindows通知とここに表示されます。</EmptyState>}</section>;
}

function SettingsPage() {
  const queryClient = useQueryClient();
  const { data = defaultSettings, isLoading } = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const value = draft ?? data;
  const save = useMutation({ mutationFn: api.updateSettings, onSuccess: (settings) => { setDraft(settings); void queryClient.invalidateQueries({ queryKey: ["settings"] }); } });
  const update = <K extends keyof AppSettings>(key: K, next: AppSettings[K]) => setDraft({ ...value, [key]: next });
  if (isLoading) return <div className="loading">設定を読み込み中…</div>;
  return <section className="panel settings-panel"><div className="panel__header"><div><h2>設定</h2><p>監視間隔、通知、履歴保持を調整</p></div><button className="button button--primary" onClick={() => save.mutate(value)} disabled={save.isPending}>保存</button></div>
    <div className="settings-section"><h3>監視</h3><div className="settings-grid"><label>ヘルス間隔（秒）<input type="number" min="5" max="300" value={value.healthIntervalSeconds} onChange={(event) => update("healthIntervalSeconds", Number(event.target.value))} /></label><label>チェーン間隔（秒）<input type="number" min="10" max="600" value={value.chainIntervalSeconds} onChange={(event) => update("chainIntervalSeconds", Number(event.target.value))} /></label><label>ピア間隔（秒）<input type="number" min="30" max="1800" value={value.peerIntervalSeconds} onChange={(event) => update("peerIntervalSeconds", Number(event.target.value))} /></label><label>REST警告（ms）<input type="number" min="250" max="30000" value={value.latencyWarningMs} onChange={(event) => update("latencyWarningMs", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>同期アラート</h3><div className="settings-grid"><label>警告ブロック差<input type="number" min="1" value={value.lagWarningBlocks} onChange={(event) => update("lagWarningBlocks", Number(event.target.value))} /></label><label>重大ブロック差<input type="number" min="2" value={value.lagCriticalBlocks} onChange={(event) => update("lagCriticalBlocks", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>データ保持</h3><div className="settings-grid"><label>生データ（日）<input type="number" min="1" max="90" value={value.rawRetentionDays} onChange={(event) => update("rawRetentionDays", Number(event.target.value))} /></label><label>集約データ（日）<input type="number" min="30" max="1825" value={value.aggregateRetentionDays} onChange={(event) => update("aggregateRetentionDays", Number(event.target.value))} /></label></div></div>
    <div className="settings-section"><h3>Windows</h3><div className="toggle-list"><label><span><strong>Windows通知</strong><small>障害発生・重大化・復旧を通知します</small></span><input type="checkbox" checked={value.notificationEnabled} onChange={(event) => update("notificationEnabled", event.target.checked)} /></label><label><span><strong>バックグラウンド監視</strong><small>ウィンドウを閉じてもトレイで監視を続けます</small></span><input type="checkbox" checked={value.backgroundMonitoring} onChange={(event) => update("backgroundMonitoring", event.target.checked)} /></label><label><span><strong>Windowsログオン時に起動</strong><small>既定では無効です</small></span><input type="checkbox" checked={value.autostartEnabled} onChange={(event) => update("autostartEnabled", event.target.checked)} /></label></div></div>
    {save.isSuccess && <div className="notice notice--success">設定を保存しました。</div>}
  </section>;
}

export default function App() {
  useMonitorEvents();
  return <Layout><Routes><Route path="/" element={<DashboardPage />} /><Route path="/nodes/:nodeId" element={<NodeDetailPage />} /><Route path="/discovery" element={<DiscoveryPage />} /><Route path="/alerts" element={<AlertsPage />} /><Route path="/settings" element={<SettingsPage />} /></Routes></Layout>;
}

