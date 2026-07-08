import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { api } from "../services/ipc";

export function useMonitorEvents() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  useEffect(() => {
    const unlisteners = [
      listen<string>("node-updated", ({ payload: nodeId }) => {
        void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
        void queryClient.invalidateQueries({ queryKey: ["node", nodeId] });
      }),
      listen<string>("alert-changed", ({ payload: nodeId }) => {
        void queryClient.invalidateQueries({ queryKey: ["alerts"] });
        void queryClient.invalidateQueries({ queryKey: ["dashboard"] });
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
      void Promise.all(unlisteners).then((items) => items.forEach((unlisten) => unlisten()));
    };
  }, [navigate, queryClient]);
}
