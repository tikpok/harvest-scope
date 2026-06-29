import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import App from "./App";
import { api } from "./services/ipc";
import type { DashboardData, NodeSummary } from "./types";

const emptyDashboard: DashboardData = { total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, nodes: [] };

vi.mock("./services/ipc", async () => {
  const mod = await vi.importActual<typeof import("./services/ipc")>("./services/ipc");
  return {
    ...mod,
    api: {
      ...mod.api,
      dashboard: vi.fn().mockResolvedValue({ total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, nodes: [] }),
      moveNode: vi.fn().mockResolvedValue(undefined),
      deleteNode: vi.fn().mockResolvedValue(undefined),
      alerts: vi.fn().mockResolvedValue([]),
      settings: vi.fn().mockResolvedValue(mod.defaultSettings),
      interfaces: vi.fn().mockResolvedValue([]),
    },
  };
});

function renderApp() {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>
      <MemoryRouter>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function node(id: string, displayName: string): NodeSummary {
  return {
    id,
    displayName,
    kind: "symbol",
    baseUrl: `http://192.168.1.${id}:3000`,
    network: "Symbol Mainnet",
    version: "1.0.0",
    enabled: true,
    favorite: false,
    status: "healthy",
    height: "100",
    finalizedHeight: "90",
    lag: "0",
    peerCount: 5,
    latencyMs: 20,
    lastSeenAt: 1_700_000_000_000,
    lastError: null,
  };
}

beforeEach(() => {
  vi.mocked(api.dashboard).mockResolvedValue(emptyDashboard);
  vi.mocked(api.moveNode).mockResolvedValue(undefined);
  vi.mocked(api.deleteNode).mockResolvedValue(undefined);
});

test("日本語ダッシュボードを表示する", async () => {
  renderApp();
  expect(await screen.findByText("ノード監視")).toBeInTheDocument();
});

test("一覧からノードを上下に移動できる", async () => {
  vi.mocked(api.dashboard).mockResolvedValue({
    total: 2,
    healthy: 2,
    warning: 0,
    critical: 0,
    offline: 0,
    nodes: [node("1", "Node A"), node("2", "Node B")],
  });
  renderApp();

  expect(await screen.findByText("Node A")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Node Aを上へ移動" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Node Bを下へ移動" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Node Bを上へ移動" }));

  await waitFor(() => expect(api.moveNode).toHaveBeenCalledWith("2", "up"));
});

test("確認後に一覧からノードの登録を解除できる", async () => {
  vi.mocked(api.dashboard).mockResolvedValue({
    total: 1,
    healthy: 1,
    warning: 0,
    critical: 0,
    offline: 0,
    nodes: [node("1", "Node A")],
  });
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  renderApp();

  fireEvent.click(await screen.findByRole("button", { name: "Node Aの登録を解除" }));

  expect(confirm).toHaveBeenCalledWith("「Node A」の登録を解除しますか？\n保存済みの監視履歴とアラートも削除されます。");
  await waitFor(() => expect(api.deleteNode).toHaveBeenCalledWith("1"));
  confirm.mockRestore();
});
