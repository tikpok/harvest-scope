import { create } from "zustand";

interface UiState {
  sidebarOpen: boolean;
  setSidebarOpen: (open: boolean) => void;
  nodeFilter: string;
  setNodeFilter: (filter: string) => void;
}

export const useUiStore = create<UiState>((set) => ({
  sidebarOpen: false,
  setSidebarOpen: (sidebarOpen) => set({ sidebarOpen }),
  nodeFilter: "",
  setNodeFilter: (nodeFilter) => set({ nodeFilter }),
}));

