import type { NodeStatus } from "../types";

const labels: Record<NodeStatus, string> = {
  unknown: "未確認",
  healthy: "正常",
  warning: "警告",
  critical: "重大",
  offline: "オフライン",
};

export function StatusBadge({ status }: { status: NodeStatus }) {
  return <span className={`status status--${status}`}>{labels[status]}</span>;
}

