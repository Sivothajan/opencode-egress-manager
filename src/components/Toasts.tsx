import { X } from "lucide-react";
import { useAppStore } from "@/store/app";

export function Toasts() {
  const toasts = useAppStore((s) => s.toasts);
  const dismiss = useAppStore((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role="status"
          className="pointer-events-auto flex items-start justify-between gap-2 rounded-lg border px-3 py-2 text-sm text-foreground shadow-lg"
          style={{
            background: "var(--popover)",
            borderColor: "var(--border)",
          }}
        >
          <span>{toast.message}</span>
          <button
            aria-label="Dismiss"
            onClick={() => dismiss(toast.id)}
            className="text-muted-foreground hover:text-foreground"
          >
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
