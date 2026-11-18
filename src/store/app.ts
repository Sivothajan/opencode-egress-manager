import { create } from "zustand";

export interface Toast {
  id: number;
  message: string;
}

interface AppStore {
  toasts: Toast[];
  pushToast: (message: string) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 1;

export const useAppStore = create<AppStore>((set) => ({
  toasts: [],
  pushToast: (message) => {
    const id = toastSeq++;
    set((state) => ({ toasts: [...state.toasts.slice(-3), { id, message }] }));
    setTimeout(() => {
      set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }));
    }, 4200);
  },
  dismissToast: (id) => set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),
}));
