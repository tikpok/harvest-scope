import { useMemo } from "react";
import type { NetworkEvent, TopologyEdge, TopologyNode } from "../types";

interface Position { x: number; y: number }

interface Props {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
  events: NetworkEvent[];
  now: number;
  wideView: boolean;
  selectedNodeId: string | null;
  onInspectNode: (nodeId: string) => void;
  onClearSelection: () => void;
}

interface RingDefinition {
  capacity: number;
  radiusX: number;
  radiusY: number;
  nodeRadius?: number;
}

interface LayoutResult {
  positions: Map<string, Position>;
  externalRadii: Map<string, number>;
}

const WIDTH = 1000;
const HEIGHT = 620;

function ellipsePosition(index: number, count: number, radiusX: number, radiusY: number, offset = 0): Position {
  const angle = -Math.PI / 2 + offset + (index / Math.max(count, 1)) * Math.PI * 2;
  return { x: WIDTH / 2 + Math.cos(angle) * radiusX, y: HEIGHT / 2 + Math.sin(angle) * radiusY };
}

function placeInRings(
  nodes: TopologyNode[],
  rings: RingDefinition[],
  positions: Map<string, Position>,
  externalRadii?: Map<string, number>,
) {
  let cursor = 0;
  rings.forEach((ring, ringIndex) => {
    const remaining = nodes.length - cursor;
    if (remaining <= 0) return;
    const count = Math.min(ring.capacity, remaining);
    nodes.slice(cursor, cursor + count).forEach((node, index) => {
      const offset = ringIndex % 2 === 0 ? 0 : Math.PI / Math.max(count, 1);
      positions.set(node.id, ellipsePosition(index, count, ring.radiusX, ring.radiusY, offset));
      if (externalRadii && ring.nodeRadius) externalRadii.set(node.id, ring.nodeRadius);
    });
    cursor += count;
  });
}

function registeredRings(count: number, wideView: boolean): RingDefinition[] {
  if (wideView) {
    if (count <= 12) return [{ capacity: 12, radiusX: 180, radiusY: 110 }];
    if (count <= 30) return [{ capacity: 12, radiusX: 145, radiusY: 90 }, { capacity: 18, radiusX: 275, radiusY: 170 }];
    return [
      { capacity: 12, radiusX: 145, radiusY: 90 },
      { capacity: 18, radiusX: 270, radiusY: 165 },
      { capacity: 32, radiusX: 395, radiusY: 245 },
    ];
  }
  if (count <= 14) return [{ capacity: 14, radiusX: 420, radiusY: 250 }];
  if (count <= 34) return [{ capacity: 14, radiusX: 230, radiusY: 140 }, { capacity: 20, radiusX: 430, radiusY: 265 }];
  return [
    { capacity: 12, radiusX: 170, radiusY: 105 },
    { capacity: 18, radiusX: 300, radiusY: 185 },
    { capacity: 36, radiusX: 450, radiusY: 280 },
  ];
}

function externalRings(count: number): RingDefinition[] {
  if (count <= 40) return [{ capacity: 40, radiusX: 700, radiusY: 430, nodeRadius: 4.5 }];
  if (count <= 85) return [
    { capacity: 36, radiusX: 560, radiusY: 345, nodeRadius: 6.5 },
    { capacity: 49, radiusX: 700, radiusY: 430, nodeRadius: 4.5 },
  ];
  return [
    { capacity: 34, radiusX: 500, radiusY: 310, nodeRadius: 7.5 },
    { capacity: 42, radiusX: 610, radiusY: 375, nodeRadius: 6 },
    { capacity: 60, radiusX: 720, radiusY: 445, nodeRadius: 4 },
  ];
}

function layout(nodes: TopologyNode[], wideView: boolean): LayoutResult {
  const registered = nodes.filter((node) => node.registered);
  const external = nodes.filter((node) => !node.registered);
  const positions = new Map<string, Position>();
  const externalRadii = new Map<string, number>();
  if (registered.length === 1) positions.set(registered[0].id, { x: WIDTH / 2, y: HEIGHT / 2 });
  else placeInRings(registered, registeredRings(registered.length, wideView), positions);
  placeInRings(external, externalRings(external.length), positions, externalRadii);
  return { positions, externalRadii };
}

function eventColor(kind: NetworkEvent["kind"]) {
  if (kind === "harvestedBlock") return "#f5d36f";
  if (kind === "finalizedBlock") return "#d58cff";
  return "#73a9ff";
}

function edgeStatus(status: string) {
  return ["active", "busy", "inactive", "failure"].includes(status) ? status : "unknown";
}

export function TopologyGraph({ nodes, edges, events, now, wideView, selectedNodeId, onInspectNode, onClearSelection }: Props) {
  const { positions, externalRadii } = useMemo(() => layout(nodes, wideView), [nodes, wideView]);
  const registeredCount = nodes.filter((node) => node.registered).length;
  const externalCount = nodes.length - registeredCount;
  const denseRegistered = registeredCount >= 30;
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
    if (event.kind === "harvestedBlock") return;
    edges
      .filter((edge) => edge.status === "active" && (edge.source === event.nodeId || edge.target === event.nodeId))
      .forEach((edge) => {
        const current = particleByEdge.get(edge.id);
        if (!current || event.observedAt > current.event.observedAt) particleByEdge.set(edge.id, { event, edge });
      });
  });
  const particles = [...particleByEdge.values()];
  const viewBox = wideView ? "-300 -190 1600 1000" : `0 0 ${WIDTH} ${HEIGHT}`;

  return (
    <svg className={`topology-graph${wideView ? " topology-graph--wide" : ""}`} viewBox={viewBox} role="img" aria-label="監視ノードのネットワークトポロジー" onClick={onClearSelection}>
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
          const radius = node.registered
            ? (denseRegistered ? 19 + Math.min(connections, 8) * .7 : 24 + Math.min(connections, 8))
            : (externalRadii.get(node.id) ?? 6) + Math.min(connections, 2) * .5;
          const shared = !node.registered && connections > 1;
          const selected = selectedNodeId === node.id;
          const showRegisteredDetails = node.registered && (!wideView || !denseRegistered || selected);
          const showLabel = showRegisteredDetails || (!node.registered && (selected || shared || externalCount <= 40));
          const meta = node.kind === "symbol"
            ? [node.websocketConnected ? "WS" : null, ...node.roles.slice(0, 3).map((role) => role.slice(0, 1))].filter(Boolean).join(" · ")
            : `NIS${node.nisStatusCode !== null ? ` ${node.nisStatusCode}` : ""}`;
          const title = [node.label, node.host && `${node.host}${node.port ? `:${node.port}` : ""}`, node.version && `v${node.version}`, `${connections} 接続`, node.peerStatus].filter(Boolean).join(" · ");
          return <g
            key={node.id}
            className={`topology-node topology-node--${node.status} ${node.registered ? "topology-node--registered" : "topology-node--external"}${denseRegistered && node.registered ? " topology-node--dense" : ""}${selectedNodeId === node.id ? " topology-node--selected" : ""}`}
            transform={`translate(${position.x} ${position.y})`}
            role="button"
            aria-label={`${node.label}の情報を表示`}
            tabIndex={0}
            onClick={(event) => { event.stopPropagation(); onInspectNode(node.id); }}
            onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") onInspectNode(node.id); }}
          >
            <title>{title}</title>
            <circle className="topology-node__hit" r={Math.max(radius + 8, 18)} />
            {activeEvent && <circle className="topology-node__pulse" r={radius} fill="none" stroke={eventColor(activeEvent.kind)}>
              <animate attributeName="r" values={`${radius};${radius + 22}`} dur="1.2s" repeatCount="3" />
              <animate attributeName="opacity" values=".9;0" dur="1.2s" repeatCount="3" />
            </circle>}
            {activeEvent?.kind === "harvestedBlock" && <circle className="topology-node__harvest" r={radius + 12}>
              <animate attributeName="r" values={`${radius + 7};${radius + 20};${radius + 10}`} dur="1.2s" repeatCount="2" />
              <animate attributeName="opacity" values=".95;.18;.7" dur="1.2s" repeatCount="2" />
            </circle>}
            {selectedNodeId === node.id && <circle className="topology-node__selection" r={radius + 7} />}
            {shared && <circle className="topology-node__shared" r={radius + 4} />}
            <circle className="topology-node__body" r={radius} />
            {node.websocketConnected && <circle className="topology-node__websocket" cx={radius * .72} cy={-radius * .72} r="4" />}
            {showRegisteredDetails && <text className="topology-node__meta" textAnchor="middle" y={-radius - 9}>{meta}</text>}
            {node.registered && <text className="topology-node__kind" textAnchor="middle" dy="4">{node.kind === "symbol" ? "S" : "N"}</text>}
            {showLabel && <text className="topology-node__label" textAnchor="middle" y={radius + 18}>{node.label.slice(0, denseRegistered ? 12 : 22)}</text>}
            {showRegisteredDetails && <text className="topology-node__height" textAnchor="middle" y={radius + 32}>高 {node.height ?? "—"}</text>}
          </g>;
        })}
      </g>
    </svg>
  );
}
