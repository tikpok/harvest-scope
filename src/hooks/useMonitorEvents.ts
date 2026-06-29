import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";

export function useMonitorEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisteners = ["node-updated", "alert-changed", "monitor-state-changed"].map((event) =>
      listen(event, () => {
        void queryClient.invalidateQueries();
      }).catch(() => () => undefined),
    );
    return () => {
      void Promise.all(unlisteners).then((items) => items.forEach((unlisten) => unlisten()));
    };
  }, [queryClient]);
}

