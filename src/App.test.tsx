import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import App from "./App";
import packageJson from "../package.json";
import { api, defaultSettings } from "./services/ipc";
import type { AlertRecord, DashboardData, DiagnosticRun, NodeSummary, TopologySnapshot } from "./types";

const emptyDashboard: DashboardData = { total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, rechecking: false, nodes: [] };

vi.mock("./services/ipc", async () => {
  const mod = await vi.importActual<typeof import("./services/ipc")>("./services/ipc");
  return {
    ...mod,
    api: {
      ...mod.api,
      dashboard: vi.fn().mockResolvedValue({ total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, rechecking: false, nodes: [] }),
      topology: vi.fn().mockResolvedValue({ nodes: [], edges: [], observedAt: 0 }),
      recentNetworkEvents: vi.fn().mockResolvedValue([]),
      recentHarvestEvents: vi.fn().mockResolvedValue([]),
      refreshAllNodes: vi.fn().mockResolvedValue(undefined),
      refreshNode: vi.fn().mockResolvedValue(undefined),
      moveNode: vi.fn().mockResolvedValue(undefined),
      deleteNode: vi.fn().mockResolvedValue(undefined),
      upsertNode: vi.fn(),
      nodeDetail: vi.fn(),
      metrics: vi.fn().mockResolvedValue([]),
      setNodeHarvestMetricsVisible: vi.fn().mockResolvedValue(undefined),
      alerts: vi.fn().mockResolvedValue([]),
      alert: vi.fn(),
      deleteResolvedAlerts: vi.fn().mockResolvedValue(0),
      nodeDiagnostics: vi.fn().mockResolvedValue([]),
      runNodeDiagnostic: vi.fn(),
      takePendingNavigation: vi.fn().mockResolvedValue(null),
      settings: vi.fn().mockResolvedValue({ ...mod.defaultSettings, onboardingCompleted: true }),
      updateSettings: vi.fn(),
      exportNodeConfig: vi.fn(),
      importNodeConfig: vi.fn(),
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
    monitorMode: "api",
    baseUrl: `http://192.168.1.${id}:3000`,
    network: "Symbol Mainnet",
    version: "1.0.0",
    roles: ["Peer", "API"],
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
    showHarvestMetrics: false,
    delegatedHarvesterCount: null,
    delegatedHarvesterDelta: null,
    totalImportance: null,
    totalBalanceMicroXym: null,
    harvestMetricError: null,
    latestHarvestAt: null,
    nodeCertificateNotAfter: null,
    nodeCertificateRemainingDays: null,
    votingKeyStartEpoch: null,
    votingKeyEndEpoch: null,
    finalizationEpoch: null,
    votingKeyRemainingEpochs: null,
    certificateMetricError: null,
    supernodeId: null,
    supernodeStatus: null,
    supernodeLastRound: null,
    supernodeLastRoundAt: null,
    supernodeChecksPassed: null,
    supernodeFailedChecks: [],
    supernodeMetricError: null,
  };
}

function topologyFixture(): TopologySnapshot {
  return {
    observedAt: 1_700_000_000_000,
    nodes: [
      {
        id: "node-a",
        label: "Node A",
        kind: "symbol",
        monitorMode: "api",
        network: "Symbol-104",
        status: "healthy",
        registered: true,
        enabled: true,
        host: "192.0.2.10",
        port: 3000,
        publicKey: "node-a-public-key",
        version: "1.0.3.7",
        roles: ["Peer", "API"],
        peerStatus: null,
        websocketConnected: true,
        apiStatus: "up",
        dbStatus: "up",
        nisStatusCode: null,
        height: "123",
        finalizedHeight: "120",
        lag: "0",
        showHarvestMetrics: true,
        delegatedHarvesterCount: 2,
        latestHarvestAt: 1_800,
      },
      {
        id: "external:symbol:peer-b",
        label: "Peer B",
        kind: "symbol",
        monitorMode: "peer",
        network: "Symbol-104",
        status: "unknown",
        registered: false,
        enabled: true,
        host: "192.0.2.10",
        port: 7900,
        publicKey: "peer-b",
        version: "1.0.3.7",
        roles: [],
        peerStatus: "active",
        websocketConnected: false,
        apiStatus: null,
        dbStatus: null,
        nisStatusCode: null,
        height: null,
        finalizedHeight: null,
        lag: null,
        showHarvestMetrics: false,
        delegatedHarvesterCount: null,
        latestHarvestAt: null,
      },
    ],
    edges: [{ id: "node-a|peer-b", source: "node-a", target: "external:symbol:peer-b", status: "active" }],
  };
}

beforeEach(() => {
  document.documentElement.removeAttribute("data-theme");
  vi.mocked(api.dashboard).mockResolvedValue(emptyDashboard);
  vi.mocked(api.topology).mockResolvedValue({ nodes: [], edges: [], observedAt: 0 });
  vi.mocked(api.recentNetworkEvents).mockResolvedValue([]);
  vi.mocked(api.recentHarvestEvents).mockResolvedValue([]);
  vi.mocked(api.moveNode).mockResolvedValue(undefined);
  vi.mocked(api.deleteNode).mockResolvedValue(undefined);
  vi.mocked(api.refreshNode).mockResolvedValue(undefined);
  vi.mocked(api.refreshAllNodes).mockResolvedValue(undefined);
  vi.mocked(api.deleteResolvedAlerts).mockResolvedValue(0);
  vi.mocked(api.upsertNode).mockReset();
  vi.mocked(api.nodeDetail).mockReset();
  vi.mocked(api.metrics).mockResolvedValue([]);
  vi.mocked(api.setNodeHarvestMetricsVisible).mockResolvedValue(undefined);
  vi.mocked(api.settings).mockResolvedValue({ ...defaultSettings, onboardingCompleted: true });
  vi.mocked(api.nodeDiagnostics).mockResolvedValue([]);
  vi.mocked(api.takePendingNavigation).mockResolvedValue(null);
  vi.mocked(api.updateSettings).mockReset();
  vi.mocked(api.updateSettings).mockImplementation(async (settings) => settings);
  vi.mocked(api.exportNodeConfig).mockReset();
  vi.mocked(api.exportNodeConfig).mockResolvedValue(null);
  vi.mocked(api.importNodeConfig).mockReset();
  vi.mocked(api.importNodeConfig).mockResolvedValue(null);
});

test("日本語ダッシュボードを表示する", async () => {
  renderApp();
  expect(await screen.findByRole("heading", { name: "Harvest Scope" })).toBeInTheDocument();
  expect(screen.getByLabelText("アプリバージョン")).toHaveTextContent(`v${packageJson.version}`);
});

test("トポロジー画面から宇宙探索ビューを開閉できる", async () => {
  vi.mocked(api.topology).mockResolvedValue(topologyFixture());
  vi.mocked(api.recentNetworkEvents).mockResolvedValue([
    { id: "event-1", nodeId: "node-a", kind: "block", height: "123", hash: "ABC", signerPublicKey: null, harvesterPublicKey: null, harvestedNodeId: null, observerNodeId: null, observedAt: Date.now(), source: "websocket" },
  ]);
  renderApp("/topology");

  const openButton = await screen.findByRole("button", { name: "宇宙探索を開く" });
  expect(openButton).toBeEnabled();
  fireEvent.click(openButton);

  expect(await screen.findByRole("dialog", { name: "トポロジー宇宙探索ビュー" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "航路" })).toHaveAttribute("aria-pressed", "false");
  expect(screen.getByRole("button", { name: "自動回転" })).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(screen.getByRole("button", { name: "航路" }));
  fireEvent.click(screen.getByRole("button", { name: "自動回転" }));
  expect(screen.getByRole("button", { name: "航路" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "自動回転" })).toHaveAttribute("aria-pressed", "true");
  const eventLog = screen.getByRole("region", { name: "観測イベントログ" });
  expect(eventLog).toBeInTheDocument();
  expect(await within(eventLog).findByText("ブロック高更新")).toBeInTheDocument();
  expect(screen.getByText(/3Dビューを初期化できません/)).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "トポロジー宇宙探索ビュー" })).not.toBeInTheDocument());

  fireEvent.click(screen.getByRole("button", { name: "宇宙探索を開く" }));
  expect(await screen.findByRole("dialog", { name: "トポロジー宇宙探索ビュー" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "宇宙探索を閉じる" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "トポロジー宇宙探索ビュー" })).not.toBeInTheDocument());
});

test("一覧からノードを上下に移動できる", async () => {
  vi.mocked(api.dashboard).mockResolvedValue({
    total: 2,
    healthy: 2,
    warning: 0,
    critical: 0,
    offline: 0,
    rechecking: false,
    nodes: [node("1", "Node A"), node("2", "Node B")],
  });
  renderApp();

  expect(await screen.findByText("Node A")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Node Aを上へ移動" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Node Bを下へ移動" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Node Bを上へ移動" }));

  await waitFor(() => expect(api.moveNode).toHaveBeenCalledWith("2", "up"));
});

test("全ノードを手動で再確認できる", async () => {
  vi.mocked(api.refreshAllNodes).mockImplementation(() => new Promise(() => undefined));
  vi.mocked(api.dashboard).mockResolvedValue({
    total: 1,
    healthy: 0,
    warning: 0,
    critical: 0,
    offline: 1,
    rechecking: false,
    nodes: [{ ...node("1", "Node A"), status: "offline" }],
  });
  renderApp();

  fireEvent.click(await screen.findByRole("button", { name: "全ノード再確認" }));

  await waitFor(() => expect(api.refreshAllNodes).toHaveBeenCalledOnce());
  expect(await screen.findByText("再確認中")).toBeInTheDocument();
});

test("NIS1詳細でSupernodeプログラムの失敗項目を表示する", async () => {
  const nisNode = {
    ...node("nem-1", "NEM Supernode"),
    kind: "nis1" as const,
    baseUrl: "http://xem35.ucanfork.duckdns.org:7890",
    network: "NEM Mainnet",
    finalizedHeight: null,
    supernodeId: 72052155,
    supernodeStatus: "active",
    supernodeLastRound: 4955,
    supernodeLastRoundAt: 1_783_592_659_998,
    supernodeChecksPassed: false,
    supernodeFailedChecks: ["bandwidth"],
  };
  vi.mocked(api.nodeDetail).mockResolvedValue({
    node: nisNode,
    publicKey: "ABC",
    roles: ["NIS"],
    restVersion: "0.6.102",
    chainScore: "123",
    apiStatus: null,
    dbStatus: null,
    nisStatusCode: 6,
    peers: [],
    websocketConnected: false,
    recentHarvests: [],
  });
  renderApp("/nodes/nem-1");

  expect(await screen.findByRole("heading", { name: "Supernodeプログラム" })).toBeInTheDocument();
  expect(screen.getByText("72052155")).toBeInTheDocument();
  expect(screen.getByText("4955")).toBeInTheDocument();
  expect(screen.getByText("失敗")).toBeInTheDocument();
  expect(screen.getByText("帯域")).toBeInTheDocument();
});

test("確認後に一覧からノードの登録を解除できる", async () => {
  vi.mocked(api.dashboard).mockResolvedValue({
    total: 1,
    healthy: 1,
    warning: 0,
    critical: 0,
    offline: 0,
    rechecking: false,
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
  fireEvent.click(screen.getByRole("button", { name: "登録して確認開始" }));

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

test("設定画面からノード設定をエクスポートする", async () => {
  vi.mocked(api.exportNodeConfig).mockResolvedValue("C:\\tmp\\harvest-scope-config.json");
  renderApp("/settings");

  fireEvent.click(await screen.findByRole("button", { name: "エクスポート" }));

  await waitFor(() => expect(api.exportNodeConfig).toHaveBeenCalledOnce());
  expect(await screen.findByText(/ノード設定を書き出しました/)).toBeInTheDocument();
});

test("設定画面からノード設定をインポートして結果を表示する", async () => {
  vi.mocked(api.importNodeConfig).mockResolvedValue({
    added: 1,
    updated: 2,
    failed: 1,
    failures: [{ baseUrl: "http://bad.test:3000", displayName: null, reason: "接続できません" }],
  });
  renderApp("/settings");

  fireEvent.click(await screen.findByRole("button", { name: "インポート" }));

  await waitFor(() => expect(api.importNodeConfig).toHaveBeenCalledOnce());
  expect(await screen.findByText("ノード設定を読み込みました: 追加 1件 / 更新 2件 / 失敗 1件")).toBeInTheDocument();
  expect(screen.getByText("接続できません")).toBeInTheDocument();
});
