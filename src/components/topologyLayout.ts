import type { TopologyNode } from "../types";

export interface Position {
  x: number;
  y: number;
}

export interface RingDefinition {
  capacity: number;
  radiusX: number;
  radiusY: number;
  nodeRadius?: number;
}

export interface TopologyLayoutResult {
  positions: Map<string, Position>;
  externalRadii: Map<string, number>;
}

export const TOPOLOGY_WIDTH = 1000;
export const TOPOLOGY_HEIGHT = 620;

function ellipsePosition(index: number, count: number, radiusX: number, radiusY: number, offset = 0): Position {
  const angle = -Math.PI / 2 + offset + (index / Math.max(count, 1)) * Math.PI * 2;
  return {
    x: TOPOLOGY_WIDTH / 2 + Math.cos(angle) * radiusX,
    y: TOPOLOGY_HEIGHT / 2 + Math.sin(angle) * radiusY,
  };
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

export function layoutTopology(nodes: TopologyNode[], wideView: boolean): TopologyLayoutResult {
  const registered = nodes.filter((node) => node.registered);
  const external = nodes.filter((node) => !node.registered);
  const positions = new Map<string, Position>();
  const externalRadii = new Map<string, number>();
  if (registered.length === 1) positions.set(registered[0].id, { x: TOPOLOGY_WIDTH / 2, y: TOPOLOGY_HEIGHT / 2 });
  else placeInRings(registered, registeredRings(registered.length, wideView), positions);
  placeInRings(external, externalRings(external.length), positions, externalRadii);
  return { positions, externalRadii };
}
