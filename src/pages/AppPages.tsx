import type { ReactNode } from "react";
import { Route, Routes } from "react-router-dom";

interface AppPagesProps {
  dashboard: ReactNode;
  topology: ReactNode;
  nodeDetail: ReactNode;
  discovery: ReactNode;
  alerts: ReactNode;
  alertDetail: ReactNode;
  settings: ReactNode;
}

export function AppPages({
  dashboard,
  topology,
  nodeDetail,
  discovery,
  alerts,
  alertDetail,
  settings,
}: AppPagesProps) {
  return (
    <Routes>
      <Route path="/" element={dashboard} />
      <Route path="/topology" element={topology} />
      <Route path="/nodes/:nodeId" element={nodeDetail} />
      <Route path="/discovery" element={discovery} />
      <Route path="/alerts" element={alerts} />
      <Route path="/alerts/:alertId" element={alertDetail} />
      <Route path="/settings" element={settings} />
    </Routes>
  );
}
