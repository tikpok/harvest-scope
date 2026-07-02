import { fireEvent, render, screen } from "@testing-library/react";
import { TopologyGraph } from "./TopologyGraph";
import type { NetworkEvent, TopologyEdge, TopologyNode } from "../types";

const nodes: TopologyNode[] = [
  { id: "node-a", label: "Node A", kind: "symbol", network: "Symbol-104", status: "healthy", registered: true, host: "192.0.2.10", port: 3000, version: "1.0.3.7", roles: ["Peer", "API"], peerStatus: null, websocketConnected: true, apiStatus: "up", dbStatus: "up", nisStatusCode: null, height: "123", finalizedHeight: "120", lag: "0" },
  { id: "external:symbol:peer-b", label: "Peer B", kind: "symbol", network: "Symbol-104", status: "unknown", registered: false, host: "192.0.2.10", port: 7900, version: "1.0.3.7", roles: [], peerStatus: "active", websocketConnected: false, apiStatus: null, dbStatus: null, nisStatusCode: null, height: null, finalizedHeight: null, lag: null },
  { id: "external:symbol:peer-c", label: "Peer C", kind: "symbol", network: "Symbol-104", status: "unknown", registered: false, host: "192.0.2.10", port: 7900, version: "1.0.3.6", roles: [], peerStatus: "active", websocketConnected: false, apiStatus: null, dbStatus: null, nisStatusCode: null, height: null, finalizedHeight: null, lag: null },
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
  render(<TopologyGraph nodes={nodes} edges={edges} events={events} now={2_000} selectedNodeId={null} onInspectNode={onInspectNode} />);

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
