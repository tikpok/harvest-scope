import { fireEvent, render, screen } from "@testing-library/react";
import { TopologyGraph } from "./TopologyGraph";
import type { NetworkEvent, TopologyEdge, TopologyNode } from "../types";

const nodes: TopologyNode[] = [
  { id: "node-a", label: "Node A", kind: "symbol", network: "Symbol-104", status: "healthy", registered: true, host: "10.0.0.1", port: 3000, version: "1.0.3.7", roles: ["Peer", "API"], peerStatus: null, websocketConnected: true, apiStatus: "up", dbStatus: "up", nisStatusCode: null, height: "123", finalizedHeight: "120", lag: "0" },
  { id: "external:symbol:peer-b", label: "Peer B", kind: "symbol", network: "Symbol-104", status: "unknown", registered: false, host: "10.0.0.2", port: 7900, version: "1.0.3.7", roles: [], peerStatus: "active", websocketConnected: false, apiStatus: null, dbStatus: null, nisStatusCode: null, height: null, finalizedHeight: null, lag: null },
  { id: "external:symbol:peer-c", label: "Peer C", kind: "symbol", network: "Symbol-104", status: "unknown", registered: false, host: "10.0.0.3", port: 7900, version: "1.0.3.6", roles: [], peerStatus: "active", websocketConnected: false, apiStatus: null, dbStatus: null, nisStatusCode: null, height: null, finalizedHeight: null, lag: null },
];
const edges: TopologyEdge[] = [
  { id: "node-a|peer-b", source: "node-a", target: "external:symbol:peer-b", status: "active" },
  { id: "node-a|peer-c", source: "node-a", target: "external:symbol:peer-c", status: "active" },
];
const events: NetworkEvent[] = [
  { id: "event-1", nodeId: "node-a", kind: "block", height: "123", hash: "ABC", observedAt: 1_000, source: "websocket" },
  { id: "event-2", nodeId: "node-a", kind: "block", height: "124", hash: "DEF", observedAt: 1_500, source: "websocket" },
  { id: "event-3", nodeId: "external:symbol:peer-b", kind: "pollUpdate", height: "124", hash: null, observedAt: 1_200, source: "polling" },
];

test("トポロジーを描画し登録ノードを選択できる", () => {
  const onInspectNode = vi.fn();
  render(<TopologyGraph nodes={nodes} edges={edges} events={events} now={2_000} wideView={false} selectedNodeId={null} onInspectNode={onInspectNode} />);

  expect(screen.getByRole("img", { name: "監視ノードのネットワークトポロジー" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Node Aの情報を表示" }));
  expect(onInspectNode).toHaveBeenCalledWith("node-a");
  fireEvent.click(screen.getByRole("button", { name: "Peer Bの情報を表示" }));
  expect(onInspectNode).toHaveBeenCalledWith("external:symbol:peer-b");
  expect(document.querySelector("animateMotion")).toBeInTheDocument();
  expect(document.querySelectorAll(".topology-particle")).toHaveLength(2);
  expect(document.querySelectorAll(".topology-edge--status-active")).toHaveLength(2);
  expect(document.querySelector(".topology-node__websocket")).toBeInTheDocument();
});

function registeredNode(index: number): TopologyNode {
  return {
    ...nodes[0],
    id: `registered-${index}`,
    label: `Registered Node ${index + 1}`,
    host: `10.0.1.${index + 1}`,
    websocketConnected: false,
  };
}

function externalNode(index: number): TopologyNode {
  return {
    ...nodes[1],
    id: `external-${index}`,
    label: `External Peer ${index + 1}`,
    host: `10.0.2.${index + 1}`,
  };
}

test("監視ノード50台を重複しない多重リングへ配置する", () => {
  const manyNodes = Array.from({ length: 50 }, (_, index) => registeredNode(index));
  const { container } = render(<TopologyGraph nodes={manyNodes} edges={[]} events={[]} now={2_000} wideView={false} selectedNodeId={null} onInspectNode={vi.fn()} />);
  const svg = screen.getByRole("img", { name: "監視ノードのネットワークトポロジー" });
  const renderedNodes = [...container.querySelectorAll<SVGGElement>(".topology-node--registered")];
  const positions = renderedNodes.map((node) => {
    const values = node.getAttribute("transform")?.match(/[\d.]+/g)?.map(Number) ?? [];
    return { x: values[0], y: values[1] };
  });
  const distances = positions.flatMap((left, index) => positions.slice(index + 1).map((right) => Math.hypot(left.x - right.x, left.y - right.y)));

  expect(svg).toHaveAttribute("viewBox", "0 0 1000 620");
  expect(renderedNodes).toHaveLength(50);
  expect(new Set(renderedNodes.map((node) => node.getAttribute("transform"))).size).toBe(50);
  expect(Math.min(...distances)).toBeGreaterThan(45);
  expect(container.querySelectorAll(".topology-node--dense")).toHaveLength(50);
});

test("外部ピア表示では広角化し外周ほどノードを小さくする", () => {
  const manyRegistered = Array.from({ length: 50 }, (_, index) => registeredNode(index));
  const externalNodes = Array.from({ length: 90 }, (_, index) => externalNode(index));
  const { container } = render(<TopologyGraph nodes={[...manyRegistered, ...externalNodes]} edges={[]} events={[]} now={2_000} wideView selectedNodeId={null} onInspectNode={vi.fn()} />);
  const svg = screen.getByRole("img", { name: "監視ノードのネットワークトポロジー" });
  const radius = (name: string) => Number(screen.getByRole("button", { name }).querySelector(".topology-node__body")?.getAttribute("r"));

  expect(svg).toHaveAttribute("viewBox", "-300 -190 1600 1000");
  expect(radius("External Peer 1の情報を表示")).toBeGreaterThan(radius("External Peer 35の情報を表示"));
  expect(radius("External Peer 35の情報を表示")).toBeGreaterThan(radius("External Peer 77の情報を表示"));
  expect(container.querySelectorAll(".topology-node--registered .topology-node__label")).toHaveLength(0);
});
