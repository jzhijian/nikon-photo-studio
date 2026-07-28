import { useToastStore, ToastKind } from "../stores/toastStore";

const ICONS: Record<ToastKind, string> = {
  success: "✅",
  info: "ℹ️",
  warn: "⚠️",
  error: "⛔",
};

const COLORS: Record<ToastKind, string> = {
  success: "border-nps-success/40",
  info: "border-nps-accent/40",
  warn: "border-nps-accent/40",
  error: "border-nps-danger/40",
};

export default function ToastHost() {
  const toasts = useToastStore((s) => s.toasts);
  const remove = useToastStore((s) => s.remove);

  return (
    <div className="fixed bottom-20 right-4 z-50 flex flex-col gap-2 pointer-events-none">
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => remove(t.id)}
          className={`pointer-events-auto flex items-center gap-2 px-3.5 py-2.5 rounded-lg border ${COLORS[t.kind]} bg-nps-card shadow-lg text-[13px] cursor-pointer animate-[toast-in_0.2s_ease-out] max-w-[300px]`}
          style={{ boxShadow: "0 8px 24px var(--shadow)" }}
        >
          <span>{ICONS[t.kind]}</span>
          <span className="text-nps-text">{t.message}</span>
        </div>
      ))}
    </div>
  );
}