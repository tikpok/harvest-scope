import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../services/ipc";
import type { NetworkEvent } from "../types";

const MAX_EVENTS = 80;
const RETENTION_MS = 60_000;

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
  return [...byId.values()]
    .sort((left, right) => right.observedAt - left.observedAt)
    .slice(0, MAX_EVENTS);
}

export function useNetworkEvents(paused: boolean) {
  const [events, setEvents] = useState<NetworkEvent[]>([]);

  useEffect(() => {
    void api.recentNetworkEvents(MAX_EVENTS).then((items) => {
      setEvents((current) => mergeNetworkEvents(current, items));
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
