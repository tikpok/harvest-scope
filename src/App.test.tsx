import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import App from "./App";
import packageJson from "../package.json";
import { api, defaultSettings } from "./services/ipc";
import type { AlertRecord, DashboardData, DiagnosticRun, NodeSummary } from "./types";

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
      upsertNode: vi.fn(),
      alerts: vi.fn().mockResolvedValue([]),
      alert: vi.fn(),
      nodeDiagnostics: vi.fn().mockResolvedValue([]),
      runNodeDiagnostic: vi.fn(),
      takePendingNavigation: vi.fn().mockResolvedValue(null),
      settings: vi.fn().mockResolvedValue({ ...mod.defaultSettings, onboardingCompleted: true }),
      updateSettings: vi.fn(),
      interfaces: vi.fn().mockResolvedValue([]),
    },
  };
});

function renderApp(path = "/") {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })}>
      <MemoryRouter initialEntries={[path]}>
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
  document.documentElement.removeAttribute("data-theme");
  vi.mocked(api.dashboard).mockResolvedValue(emptyDashboard);
  vi.mocked(api.moveNode).mockResolvedValue(undefined);
  vi.mocked(api.deleteNode).mockResolvedValue(undefined);
  vi.mocked(api.upsertNode).mockReset();
  vi.mocked(api.settings).mockResolvedValue({ ...defaultSettings, onboardingCompleted: true });
  vi.mocked(api.nodeDiagnostics).mockResolvedValue([]);
  vi.mocked(api.takePendingNavigation).mockResolvedValue(null);
  vi.mocked(api.updateSettings).mockReset();
  vi.mocked(api.updateSettings).mockImplementation(async (settings) => settings);
});

test("日本語ダッシュボードを表示する", async () => {
  renderApp();
  expect(await screen.findByText("ノード監視")).toBeInTheDocument();
  expect(screen.getByLabelText("アプリバージョン")).toHaveTextContent(`v${packageJson.version}`);
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

test("URLだけで登録し成功結果を表示する", async () => {
  const registered = {
    ...node("3", "Symbol API Node"),
    baseUrl: "https://example.test:3001",
  };
  vi.mocked(api.upsertNode).mockResolvedValue(registered);
  renderApp("/discovery");

  expect(screen.queryByLabelText("表示名")).not.toBeInTheDocument();
  const url = await screen.findByLabelText("REST URL");
  fireEvent.change(url, { target: { value: registered.baseUrl } });
  fireEvent.click(screen.getByRole("button", { name: "接続確認して登録" }));

  await waitFor(() => expect(api.upsertNode).toHaveBeenCalled());
  expect(vi.mocked(api.upsertNode).mock.calls[0][0]).toEqual({ baseUrl: registered.baseUrl });
  expect(await screen.findByText("「Symbol API Node」を登録しました。")).toBeInTheDocument();
  expect(screen.getByText(registered.baseUrl)).toBeInTheDocument();
  expect(url).toHaveValue("");
});

const alert: AlertRecord = {
  id: "alert-1",
  nodeId: "node-1",
  nodeName: "Problem Node",
  alertType: "offline",
  severity: "critical",
  state: "open",
  message: "ノードへ接続できません。",
  raisedAt: 1_700_000_000_000,
  updatedAt: 1_700_000_010_000,
  acknowledgedAt: null,
  resolvedAt: null,
  occurrenceCount: 1,
};

const diagnostic: DiagnosticRun = {
  id: "run-1",
  nodeId: "node-1",
  alertId: "alert-1",
  startedAt: 1_700_000_020_000,
  completedAt: 1_700_000_021_000,
  status: "failed",
  summary: "1件の異常を検出しました。",
  checks: [{ key: "tcp", label: "RESTポート", status: "fail", message: "接続できません。", recommendation: "ファイアウォールを確認してください。", durationMs: 1000 }],
};

test("障害詳細からワンクリック診断を実行する", async () => {
  vi.mocked(api.alert).mockResolvedValue(alert);
  vi.mocked(api.runNodeDiagnostic).mockResolvedValue(diagnostic);
  renderApp("/alerts/alert-1");

  expect(await screen.findByText("Problem Node")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "今すぐ診断" }));

  await waitFor(() => expect(api.runNodeDiagnostic).toHaveBeenCalledWith("node-1", "alert-1"));
  expect(await screen.findByText("1件の異常を検出しました。")).toBeInTheDocument();
  expect(screen.getByText("ファイアウォールを確認してください。")).toBeInTheDocument();
});

test("保留された通知遷移から障害詳細を開く", async () => {
  vi.mocked(api.takePendingNavigation).mockResolvedValueOnce("/alerts/alert-1").mockResolvedValue(null);
  vi.mocked(api.alert).mockResolvedValue(alert);
  renderApp();

  expect(await screen.findByText("Problem Node")).toBeInTheDocument();
  expect(screen.getByText("ノードへ接続できません。")).toBeInTheDocument();
});

test("初回案内で自動起動を選択せず常駐監視を有効化する", async () => {
  const firstRun = { ...defaultSettings, backgroundMonitoring: true, onboardingCompleted: false };
  const completed = { ...firstRun, onboardingCompleted: true };
  vi.mocked(api.settings).mockResolvedValueOnce(firstRun).mockResolvedValue(completed);
  vi.mocked(api.updateSettings).mockResolvedValue(completed);
  renderApp();

  expect(await screen.findByRole("dialog")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "今はしない" }));
  await waitFor(() => expect(api.updateSettings).toHaveBeenCalledWith(expect.objectContaining({
    backgroundMonitoring: true,
    autostartEnabled: false,
    onboardingCompleted: true,
  })));
});

test("上部バーからライトテーマへ切り替える", async () => {
  vi.mocked(api.settings).mockResolvedValue({ ...defaultSettings, theme: "dark", onboardingCompleted: true });
  renderApp();

  const toggle = await screen.findByRole("button", { name: "ライトテーマに切り替え" });
  fireEvent.click(toggle);
  await waitFor(() => expect(api.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ theme: "light" })));
});
