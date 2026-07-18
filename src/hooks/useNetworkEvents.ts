import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../services/ipc";
import type { HarvestEvent, NetworkEvent } from "../types";

const MAX_EVENTS = 80;
const MAX_EVENT_BYTES = 128 * 1024;
const RETENTION_MS = 60_000;
const encoder = new TextEncoder();

function eventBytes(event: NetworkEvent) {
  return encoder.encode(JSON.stringify(event)).byteLength;
}

function harvestToNetworkEvent(event: HarvestEvent): NetworkEvent {
  return {
    id: `harvest-${event.id}`,
    nodeId: event.nodeId,
    kind: "harvestedBlock",
    height: event.height,
    hash: event.hash,
    signerPublicKey: event.harvesterPublicKey,
    harvesterPublicKey: event.harvesterPublicKey,
    harvestedNodeId: event.nodeId,
    observerNodeId: event.observerNodeId,
    observedAt: event.observedAt,
    source: "websocket",
  };
}

export function mergeNetworkEvents(current: NetworkEvent[], incoming: NetworkEvent[], now = Date.now()) {
  const cutoff = now - RETENTION_MS;
  const byId = new Map(
    current
      .filter((event) => event.observedAt >= cutoff)
      .map((event) => [event.id, event]),
  );
  incoming
    .filter((event) => event.observedAt >= cutoff)
    .forEach((event) => byId.set(event.id, event));
  const result: NetworkEvent[] = [];
  let totalBytes = 0;
  for (const event of [...byId.values()].sort((left, right) => right.observedAt - left.observedAt)) {
    const size = eventBytes(event);
    if (size > MAX_EVENT_BYTES || totalBytes + size > MAX_EVENT_BYTES) continue;
    result.push(event);
    totalBytes += size;
    if (result.length >= MAX_EVENTS) break;
  }
  return result;
}

export function useNetworkEvents(paused: boolean) {
  const [events, setEvents] = useState<NetworkEvent[]>([]);

  useEffect(() => {
    void Promise.all([api.recentNetworkEvents(MAX_EVENTS), api.recentHarvestEvents(MAX_EVENTS)]).then(([network, harvest]) => {
      setEvents((current) => mergeNetworkEvents(current, [...network, ...harvest.map(harvestToNetworkEvent)]));
    });
  }, []);

  useEffect(() => {
    const unlistener = listen<NetworkEvent>("network-event", ({ payload }) => {
      if (!paused) setEvents((current) => mergeNetworkEvents(current, [payload]));
    }).catch(() => () => undefined);
    return () => { void unlistener.then((unlisten) => unlisten()); };
  }, [paused]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setEvents((current) => mergeNetworkEvents(current, []));
    }, 5_000);
    return () => window.clearInterval(timer);
  }, []);

  return events;
}
