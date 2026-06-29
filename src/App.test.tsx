import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import App from "./App";

vi.mock("./services/ipc", async () => {
  const mod = await vi.importActual<typeof import("./services/ipc")>("./services/ipc");
  return {
    ...mod,
    api: {
      ...mod.api,
      dashboard: vi.fn().mockResolvedValue({ total: 0, healthy: 0, warning: 0, critical: 0, offline: 0, nodes: [] }),
      alerts: vi.fn().mockResolvedValue([]),
      settings: vi.fn().mockResolvedValue(mod.defaultSettings),
      interfaces: vi.fn().mockResolvedValue([]),
    },
  };
});

test("日本語ダッシュボードを表示する", async () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <MemoryRouter>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  expect(await screen.findByText("ノード監視")).toBeInTheDocument();
});

