import { useEffect } from "react";

interface Props {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({
  title,
  message,
  confirmLabel = "确认",
  cancelLabel = "取消",
  danger,
  onConfirm,
  onCancel,
}: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onConfirm();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onConfirm, onCancel]);

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/60 animate-[fade-in_0.12s_ease-out]"
      onClick={onCancel}
    >
      <div
        className="w-[360px] rounded-xl bg-nps-secondary border border-nps-border shadow-2xl p-6 animate-[scale-in_0.15s_ease-out]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 mb-3">
          {danger && (
            <div className="w-9 h-9 rounded-full bg-nps-danger/15 flex items-center justify-center text-lg shrink-0">
              ⚠️
            </div>
          )}
          <h3 className="text-[15px] font-bold text-nps-text">{title}</h3>
        </div>
        <p className="text-[13px] text-nps-text-dim leading-relaxed mb-6">{message}</p>
        <div className="flex justify-end gap-2.5">
          <button
            type="button"
            onClick={onCancel}
            className="px-4 py-2 rounded-lg border border-nps-border bg-nps-tertiary text-[13px] text-nps-text hover:bg-nps-hover active:scale-[0.97] transition-all"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className={`px-4 py-2 rounded-lg text-[13px] font-semibold active:scale-[0.97] transition-all ${
              danger
                ? "bg-nps-danger text-white hover:opacity-90"
                : "bg-nps-accent text-nps-primary hover:bg-nps-accent-hover"
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}