import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { api } from "../services/ipc";

export function useMonitorEvents() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const dashboardTimer = useRef<number | null>(null);
  const alertTimer = useRef<number | null>(null);

  useEffect(() => {
    const scheduleDashboardInvalidation = () => {
      if (dashboardTimer.current != null) window.clearTimeout(dashboardTimer.current);
      dashboardTimer.current = window.setTimeout(() => {
        dashboardTimer.current = null;
        void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      }, 250);
    };
    const scheduleAlertInvalidation = () => {
      if (alertTimer.current != null) window.clearTimeout(alertTimer.current);
      alertTimer.current = window.setTimeout(() => {
        alertTimer.current = null;
        void queryClient.invalidateQueries({ queryKey: ["alerts"] });
      }, 250);
    };
    const unlisteners = [
      listen<string>("node-updated", ({ payload: nodeId }) => {
        scheduleDashboardInvalidation();
        void queryClient.invalidateQueries({ queryKey: ["node", nodeId] });
      }),
      listen<string>("alert-changed", ({ payload: nodeId }) => {
        scheduleAlertInvalidation();
        scheduleDashboardInvalidation();
        void queryClient.invalidateQueries({ queryKey: ["node", nodeId] });
      }),
      listen<string>("monitor-state-changed", ({ payload: nodeId }) => {
        void queryClient.invalidateQueries({ queryKey: ["node", nodeId] });
      }),
      listen("refresh-all-state-changed", () => {
        void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
      }),
      listen("navigate-requested", () => {
        void api.takePendingNavigation().then((path) => {
          if (path) navigate(path);
        });
      }),
    ].map((unlistener) => unlistener.catch(() => () => undefined));
    void api.takePendingNavigation().then((path) => {
      if (path) navigate(path);
    });
    return () => {
      if (dashboardTimer.current != null) window.clearTimeout(dashboardTimer.current);
      if (alertTimer.current != null) window.clearTimeout(alertTimer.current);
      void Promise.all(unlisteners).then((items) => items.forEach((unlisten) => unlisten()));
    };
  }, [navigate, queryClient]);
}
