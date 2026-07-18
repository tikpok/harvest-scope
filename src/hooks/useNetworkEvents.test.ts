import { mergeNetworkEvents } from "./useNetworkEvents";
import type { NetworkEvent } from "../types";

function event(id: string, observedAt: number): NetworkEvent {
  return {
    id,
    nodeId: "node-a",
    kind: "block",
    height: "1",
    hash: null,
    signerPublicKey: null,
    harvesterPublicKey: null,
    harvestedNodeId: null,
    observerNodeId: null,
    observedAt,
    source: "websocket",
  };
}

test("初期読込時から1分を超えたイベントを除外する", () => {
  const now = 100_000;
  const merged = mergeNetworkEvents(
    [event("current-old", 20_000)],
    [event("incoming-old", 39_999), event("incoming-recent", 40_000)],
    now,
  );

  expect(merged.map((item) => item.id)).toEqual(["incoming-recent"]);
});

test("イベントを件数とUTF-8バイト数の両方で制限する", () => {
  const now = 100_000;
  const large = event("large", now);
  large.nodeId = "あ".repeat(50_000);
  const merged = mergeNetworkEvents([], [large, event("small", now - 1)], now);

  expect(merged.map((item) => item.id)).toEqual(["small"]);
});
