import { create } from "zustand";

export type ToastKind = "success" | "info" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastState {
  toasts: Toast[];
  push: (message: string, kind?: ToastKind) => void;
  remove: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (message, kind = "info") => {
    const id = nextId++;
    let inserted = false;
    set((s) => {
      // Connection/network polling can report the same state more than once;
      // keep one visible notification instead of stacking identical cards.
      if (s.toasts.some((toast) => toast.message === message && toast.kind === kind)) return s;
      inserted = true;
      return { toasts: [...s.toasts, { id, kind, message }] };
    });
    if (!inserted) return;
    // auto-dismiss after 2.6s
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    }, 2600);
  },
  remove: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export const toast = (message: string, kind?: ToastKind) =>
  useToastStore.getState().push(message, kind);
