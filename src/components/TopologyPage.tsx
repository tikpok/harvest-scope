import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { ChevronRight, Pause, Play, Radio, RefreshCw } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useNetworkEvents } from "../hooks/useNetworkEvents";
import { api } from "../services/ipc";
import type { NetworkEventKind, NodeKind, TopologyNode } from "../types";
import { TopologyGraph } from "./TopologyGraph";

const labels: Record<NetworkEventKind, string> = {
  block: "ブロック高更新",
  finalizedBlock: "ファイナライズ高更新",
  pollUpdate: "ブロック高更新",
  harvestedBlock: "ハーベスト検知",
};

const peerStatusLabels: Record<string, string> = {
  active: "接続中",
  busy: "ビジー",
  inactive: "非アクティブ",
  failure: "接続失敗",
};

function NodeInspector({ node, connections, onOpen }: { node: TopologyNode; connections: number; onOpen: () => void }) {
  const endpoint = node.host ? `${node.host}${node.port ? `:${node.port}` : ""}` : "—";
  return <section className="topology-inspector" aria-label="選択したノードの情報">
    <div className="topology-inspector__heading">
      <div><span className={`kind kind--${node.kind}`}>{node.kind === "symbol" ? "Symbol" : "NIS1"}</span><h3>{node.label}</h3></div>
      <span className={`topology-peer-state topology-peer-state--${node.peerStatus ?? node.status}`}>{node.registered ? "監視中" : peerStatusLabels[node.peerStatus ?? ""] ?? "外部ピア"}</span>
    </div>
    <dl>
      <div><dt>エンドポイント</dt><dd className="mono">{endpoint}</dd></div>
      <div><dt>接続</dt><dd>{connections} 本{!node.registered && connections > 1 ? " · 共有ピア" : ""}</dd></div>
      <div><dt>バージョン</dt><dd>{node.version ?? "—"}</dd></div>
      {node.registered && <div><dt>ブロック高</dt><dd>{node.height ?? "—"}{node.kind === "symbol" && node.finalizedHeight ? ` / F ${node.finalizedHeight}` : ""}</dd></div>}
      {node.kind === "symbol" && node.registered && <div><dt>ライブ接続</dt><dd className={node.websocketConnected ? "text-good" : "muted"}>{node.websocketConnected ? "WebSocket接続中" : "RESTフォールバック"}</dd></div>}
      {node.kind === "symbol" && node.registered && <div><dt>API / DB</dt><dd>{[node.apiStatus, node.dbStatus].filter(Boolean).join(" / ") || "—"}</dd></div>}
      {node.kind === "symbol" && node.registered && <div><dt>委任者</dt><dd>{node.showHarvestMetrics ? node.delegatedHarvesterCount ?? "—" : "非表示"}</dd></div>}
      {node.kind === "symbol" && node.registered && <div><dt>最新ハーベスト</dt><dd>{node.latestHarvestAt ? new Date(node.latestHarvestAt).toLocaleTimeString("ja-JP") : "—"}</dd></div>}
      {node.kind === "nis1" && node.registered && <div><dt>NIS状態コード</dt><dd>{node.nisStatusCode ?? "—"}</dd></div>}
    </dl>
    {node.roles.length > 0 && <div className="topology-role-list">{node.roles.map((role) => <span key={role}>{role}</span>)}</div>}
    {node.registered && <button className="link-button topology-inspector__link" onClick={onOpen}>ノード詳細を表示 <ChevronRight size={14} /></button>}
  </section>;
}

export function TopologyPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [paused, setPaused] = useState(false);
  const [showExternal, setShowExternal] = useState(false);
  const [kind, setKind] = useState<NodeKind | "all">("symbol");
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const events = useNetworkEvents(paused);
  const { data = { nodes: [], edges: [], observedAt: 0 }, isLoading, error, refetch } = useQuery({
    queryKey: ["topology"],
    queryFn: api.topology,
    refetchInterval: 60_000,
  });

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const unlistener = listen("topology-updated", () => {
      void queryClient.invalidateQueries({ queryKey: ["topology"] });
    }).catch(() => () => undefined);
    return () => { void unlistener.then((unlisten) => unlisten()); };
  }, [queryClient]);

  const graph = useMemo(() => {
    const registered = data.nodes.filter((node) => node.registered && (kind === "all" || node.kind === kind));
    const registeredIds = new Set(registered.map((node) => node.id));
    const external = showExternal
      ? data.nodes.filter((node) => !node.registered && (kind === "all" || node.kind === kind)).slice(0, 120)
      : [];
    const nodes = [...registered, ...external];
    const ids = new Set(nodes.map((node) => node.id));
    const edges = data.edges.filter((edge) => ids.has(edge.source) && ids.has(edge.target)
      && (registeredIds.has(edge.source) || registeredIds.has(edge.target)));
    return { nodes, edges };
  }, [data, kind, showExternal]);

  useEffect(() => {
    if (selectedNodeId && !graph.nodes.some((node) => node.id === selectedNodeId)) setSelectedNodeId(null);
  }, [graph.nodes, selectedNodeId]);

  const summary = useMemo(() => {
    const edgeCounts = { active: 0, busy: 0, inactive: 0, failure: 0 };
    const degrees = new Map<string, number>();
    graph.edges.forEach((edge) => {
      if (edge.status in edgeCounts) edgeCounts[edge.status as keyof typeof edgeCounts] += 1;
      degrees.set(edge.source, (degrees.get(edge.source) ?? 0) + 1);
      degrees.set(edge.target, (degrees.get(edge.target) ?? 0) + 1);
    });
    const shared = graph.nodes.filter((node) => !node.registered && (degrees.get(node.id) ?? 0) > 1).length;
    const versions = new Map<string, number>();
    graph.nodes.forEach((node) => {
      if (node.version) versions.set(node.version, (versions.get(node.version) ?? 0) + 1);
    });
    return {
      edgeCounts,
      degrees,
      shared,
      versions: [...versions.entries()].sort((left, right) => right[1] - left[1]).slice(0, 5),
    };
  }, [graph]);

  const visibleEvents = events.filter((event) => graph.nodes.some((node) => node.id === event.nodeId));
  const registeredCount = graph.nodes.filter((node) => node.registered).length;
  const externalCount = graph.nodes.length - registeredCount;
  const selectedNode = graph.nodes.find((node) => node.id === selectedNodeId) ?? null;

  return <>
    <div className="page-heading topology-heading">
      <div><h2>ネットワークトポロジー</h2><p>ピア関係と、各ノードで観測したブロック高更新をリアルタイム表示します。</p></div>
      <div className="actions">
        <select value={kind} onChange={(event) => setKind(event.target.value as NodeKind | "all")} aria-label="ノード種別">
          <option value="symbol">Symbol</option><option value="nis1">NEM</option><option value="all">Symbol / NEM</option>
        </select>
        <label className="topology-toggle"><input type="checkbox" checked={showExternal} onChange={(event) => setShowExternal(event.target.checked)} />外部ピア</label>
        <button className="button" onClick={() => setPaused((value) => !value)}>{paused ? <Play size={16} /> : <Pause size={16} />}{paused ? "再開" : "一時停止"}</button>
        <button className="icon-button" title="再取得" aria-label="トポロジーを再取得" onClick={() => void refetch()}><RefreshCw size={16} /></button>
      </div>
    </div>
    <section className="topology-stats" aria-label="トポロジー集計">
      <span><strong>{registeredCount}</strong> 監視</span><span><strong>{externalCount}</strong> 外部</span><span className="topology-stat--active"><strong>{summary.edgeCounts.active}</strong> active</span><span className="topology-stat--busy"><strong>{summary.edgeCounts.busy}</strong> busy</span><span className="topology-stat--failure"><strong>{summary.edgeCounts.failure}</strong> failure</span><span><strong>{summary.shared}</strong> 共有ピア</span><span className={paused ? "topology-live topology-live--paused" : "topology-live"}><Radio size={14} />{paused ? "表示停止中" : "ライブ"}</span>
    </section>
    {error && <div className="notice notice--error">トポロジーを取得できません: {String(error)}</div>}
    <div className="topology-layout">
      <section className="panel topology-canvas-panel">
        <div className="topology-legend"><span><i className="legend-node legend-node--registered" />監視ノード</span><span><i className="legend-node legend-node--shared" />共有ピア</span><span><i className="legend-line legend-line--active" />active</span><span><i className="legend-line legend-line--busy" />busy</span><span><i className="legend-line legend-line--failure" />failure</span><span><i className="legend-flow" />推定伝播</span></div>
        {isLoading ? <div className="loading">トポロジーを読み込み中…</div> : graph.nodes.length === 0 ? <div className="empty-state"><h3>表示できるノードがありません</h3><p>ノードを登録し、最初のピア情報が取得されるまでお待ちください。</p></div> : <TopologyGraph nodes={graph.nodes} edges={graph.edges} events={paused ? [] : visibleEvents} now={now} wideView={showExternal} selectedNodeId={selectedNodeId} onInspectNode={setSelectedNodeId} onClearSelection={() => setSelectedNodeId(null)} />}
      </section>
      <aside className={`panel topology-events${selectedNode ? " topology-events--inspecting" : ""}`}>
        {selectedNode ? (
          <NodeInspector node={selectedNode} connections={summary.degrees.get(selectedNode.id) ?? 0} onOpen={() => navigate(`/nodes/${selectedNode.id}`)} />
        ) : <>
          <section className="topology-network-summary">
            <h3>ネットワーク構成</h3>
            <div className="topology-version-list">{summary.versions.length ? summary.versions.map(([version, count]) => <span key={version}>v{version}<strong>{count}</strong></span>) : <small>バージョン情報を収集中…</small>}</div>
          </section>
          <div className="panel__header"><div><h2>観測イベント</h2><p>直近1分・最大80件</p></div></div>
          <div className="topology-event-list">
            {visibleEvents.length === 0 ? <p className="topology-event-empty">イベントを待っています…</p> : visibleEvents.slice(0, 80).map((event) => {
              const targetNodeId = event.harvestedNodeId ?? event.nodeId;
              const node = data.nodes.find((item) => item.id === targetNodeId);
              const detail = event.kind === "harvestedBlock"
                ? `${event.harvesterPublicKey ? `${event.harvesterPublicKey.slice(0, 6)}…${event.harvesterPublicKey.slice(-6)} · ` : ""}${event.observerNodeId ? `観測 ${data.nodes.find((item) => item.id === event.observerNodeId)?.label ?? event.observerNodeId}` : "WebSocket"}`
                : event.source === "websocket" ? "WebSocket" : "ポーリング";
              return <article
                key={event.id}
                className={`topology-event topology-event--${event.kind}`}
                role="button"
                tabIndex={0}
                onClick={() => setSelectedNodeId(targetNodeId)}
                onKeyDown={(keyboardEvent) => { if (keyboardEvent.key === "Enter" || keyboardEvent.key === " ") setSelectedNodeId(targetNodeId); }}
              >
                <i /><div><strong>{labels[event.kind]}</strong><span>{node?.label ?? targetNodeId}</span><small>{event.height ? `高さ ${event.height} · ` : ""}{detail} · {new Date(event.observedAt).toLocaleTimeString("ja-JP")}</small></div>
              </article>;
            })}
          </div>
          <p className="topology-disclaimer">粒子は観測順と現在のピア関係から描く推定表現です。実際の送信元やパケット経路を示すものではありません。</p>
        </>}
      </aside>
    </div>
  </>;
}
