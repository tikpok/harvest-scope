import { useMemo } from "react";
import type { NetworkEvent, TopologyEdge, TopologyNode } from "../types";

interface Position { x: number; y: number }

interface Props {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
  events: NetworkEvent[];
  now: number;
  selectedNodeId: string | null;
  onInspectNode: (nodeId: string) => void;
}

const WIDTH = 1000;
const HEIGHT = 620;

function ellipsePosition(index: number, count: number, radiusX: number, radiusY: number): Position {
  const angle = -Math.PI / 2 + (index / Math.max(count, 1)) * Math.PI * 2;
  return { x: WIDTH / 2 + Math.cos(angle) * radiusX, y: HEIGHT / 2 + Math.sin(angle) * radiusY };
}

function layout(nodes: TopologyNode[]) {
  const registered = nodes.filter((node) => node.registered);
  const external = nodes.filter((node) => !node.registered);
  const positions = new Map<string, Position>();
  registered.forEach((node, index) => {
    positions.set(node.id, registered.length === 1
      ? { x: WIDTH / 2, y: HEIGHT / 2 }
      : ellipsePosition(index, registered.length, 165, 105));
  });
  const perRing = 40;
  external.forEach((node, index) => {
    const ring = Math.floor(index / perRing);
    const first = ring * perRing;
    const count = Math.min(perRing, external.length - first);
    positions.set(node.id, ellipsePosition(index - first, count, 450 - ring * 75, 275 - ring * 48));
  });
  return positions;
}

function eventColor(kind: NetworkEvent["kind"]) {
  if (kind === "finalizedBlock") return "#d58cff";
  return "#73a9ff";
}

function edgeStatus(status: string) {
  return ["active", "busy", "inactive", "failure"].includes(status) ? status : "unknown";
}

export function TopologyGraph({ nodes, edges, events, now, selectedNodeId, onInspectNode }: Props) {
  const positions = useMemo(() => layout(nodes), [nodes]);
  const degree = useMemo(() => {
    const values = new Map<string, number>();
    edges.forEach((edge) => {
      values.set(edge.source, (values.get(edge.source) ?? 0) + 1);
      values.set(edge.target, (values.get(edge.target) ?? 0) + 1);
    });
    return values;
  }, [edges]);
  const activeEvents = events.filter((event) => now - event.observedAt < 4_500);
  const activeByNode = new Map<string, NetworkEvent>();
  activeEvents.forEach((event) => {
    const current = activeByNode.get(event.nodeId);
    if (!current || event.observedAt > current.observedAt) activeByNode.set(event.nodeId, event);
  });
  const particleByEdge = new Map<string, { event: NetworkEvent; edge: TopologyEdge }>();
  activeByNode.forEach((event) => {
    edges
      .filter((edge) => edge.status === "active" && (edge.source === event.nodeId || edge.target === event.nodeId))
      .forEach((edge) => {
        const current = particleByEdge.get(edge.id);
        if (!current || event.observedAt > current.event.observedAt) particleByEdge.set(edge.id, { event, edge });
      });
  });
  const particles = [...particleByEdge.values()];

  return (
    <svg className="topology-graph" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label="監視ノードのネットワークトポロジー">
      <defs>
        <filter id="node-glow"><feGaussianBlur stdDeviation="5" result="blur" /><feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge></filter>
        <filter id="particle-glow"><feGaussianBlur stdDeviation="2" result="blur" /><feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge></filter>
      </defs>
      <g className="topology-edges">
        {edges.map((edge) => {
          const source = positions.get(edge.source);
          const target = positions.get(edge.target);
          if (!source || !target) return null;
          const flowing = edge.status === "active" && (activeByNode.has(edge.source) || activeByNode.has(edge.target));
          return <line key={edge.id} x1={source.x} y1={source.y} x2={target.x} y2={target.y} className={`topology-edge topology-edge--status-${edgeStatus(edge.status)}${flowing ? " topology-edge--flowing" : ""}`} />;
        })}
      </g>
      <g className="topology-particles" aria-hidden="true">
        {particles.map(({ event, edge }) => {
          const sourceId = edge.source === event.nodeId ? edge.target : edge.source;
          const source = positions.get(sourceId);
          const target = positions.get(event.nodeId);
          if (!source || !target) return null;
          return <circle className="topology-particle" key={`${event.id}-${edge.id}`} r="4" fill={eventColor(event.kind)} filter="url(#particle-glow)">
            <animateMotion dur="1.4s" repeatCount="2" path={`M ${source.x} ${source.y} L ${target.x} ${target.y}`} />
          </circle>;
        })}
      </g>
      <g className="topology-nodes">
        {nodes.map((node) => {
          const position = positions.get(node.id);
          if (!position) return null;
          const activeEvent = activeByNode.get(node.id);
          const connections = degree.get(node.id) ?? 0;
          const radius = node.registered ? 24 + Math.min(connections, 8) : 7 + Math.min(connections, 4) * 1.5;
          const shared = !node.registered && connections > 1;
          const meta = node.kind === "symbol"
            ? [node.websocketConnected ? "WS" : null, ...node.roles.slice(0, 3).map((role) => role.slice(0, 1))].filter(Boolean).join(" · ")
            : `NIS${node.nisStatusCode !== null ? ` ${node.nisStatusCode}` : ""}`;
          const title = [node.label, node.host && `${node.host}${node.port ? `:${node.port}` : ""}`, node.version && `v${node.version}`, `${connections} 接続`, node.peerStatus].filter(Boolean).join(" · ");
          return <g
            key={node.id}
            className={`topology-node topology-node--${node.status} ${node.registered ? "topology-node--registered" : "topology-node--external"}${selectedNodeId === node.id ? " topology-node--selected" : ""}`}
            transform={`translate(${position.x} ${position.y})`}
            role="button"
            aria-label={`${node.label}の情報を表示`}
            tabIndex={0}
            onClick={() => onInspectNode(node.id)}
            onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onInspectNode(node.id); }}
          >
            <title>{title}</title>
            <circle className="topology-node__hit" r={Math.max(radius + 8, 18)} />
            {activeEvent && <circle className="topology-node__pulse" r={radius} fill="none" stroke={eventColor(activeEvent.kind)}>
              <animate attributeName="r" values={`${radius};${radius + 22}`} dur="1.2s" repeatCount="3" />
              <animate attributeName="opacity" values=".9;0" dur="1.2s" repeatCount="3" />
            </circle>}
            {selectedNodeId === node.id && <circle className="topology-node__selection" r={radius + 7} />}
            {shared && <circle className="topology-node__shared" r={radius + 4} />}
            <circle className="topology-node__body" r={radius} />
            {node.websocketConnected && <circle className="topology-node__websocket" cx={radius * .72} cy={-radius * .72} r="4" />}
            {node.registered && <text className="topology-node__meta" textAnchor="middle" y={-radius - 9}>{meta}</text>}
            {node.registered && <text className="topology-node__kind" textAnchor="middle" dy="4">{node.kind === "symbol" ? "S" : "N"}</text>}
            <text className="topology-node__label" textAnchor="middle" y={radius + 18}>{node.label.slice(0, 22)}</text>
            {node.registered && <text className="topology-node__height" textAnchor="middle" y={radius + 32}>高 {node.height ?? "—"}</text>}
          </g>;
        })}
      </g>
    </svg>
  );
}
