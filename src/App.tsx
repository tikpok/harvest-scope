import { Route, Routes } from "react-router-dom";
import { useMonitorEvents } from "./hooks/useMonitorEvents";
import { Layout } from "./components/AppLayout";
import { TopologyPage } from "./components/TopologyPage";
import { AlertDetailPage, AlertsPage, DashboardPage, DiscoveryPage, NodeDetailPage, Onboarding, SettingsPage } from "./pages/AppPages";

export default function App() {
  useMonitorEvents();
  return <><Layout><Routes><Route path="/" element={<DashboardPage />} /><Route path="/topology" element={<TopologyPage />} /><Route path="/nodes/:nodeId" element={<NodeDetailPage />} /><Route path="/discovery" element={<DiscoveryPage />} /><Route path="/alerts" element={<AlertsPage />} /><Route path="/alerts/:alertId" element={<AlertDetailPage />} /><Route path="/settings" element={<SettingsPage />} /></Routes></Layout><Onboarding /></>;
}
