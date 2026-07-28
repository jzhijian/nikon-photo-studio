import { useEffect, useRef } from "react";

export interface MenuItem {
  label: string;
  icon?: string;
  danger?: boolean;
  divider?: boolean;
  onClick: () => void;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

export default function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("mousedown", handler);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", handler);
      document.removeEventListener("keydown", esc);
    };
  }, [onClose]);

  // Keep menu within viewport
  const style: React.CSSProperties = {
    position: "fixed",
    left: Math.min(x, window.innerWidth - 200),
    top: Math.min(y, window.innerHeight - items.length * 36 - 16),
    zIndex: 100,
  };

  return (
    <div
      ref={ref}
      style={style}
      className="min-w-[180px] py-1.5 rounded-lg bg-nps-secondary border border-nps-border shadow-2xl animate-[fade-in_0.1s_ease-out]"
    >
      {items.map((item, i) =>
        item.divider ? (
          <div key={i} className="my-1 border-t border-nps-border" />
        ) : (
          <button
            key={i}
            type="button"
            onClick={() => { item.onClick(); onClose(); }}
            className={`w-full flex items-center gap-2.5 px-3.5 py-2 text-[12px] text-left transition-colors ${
              item.danger
                ? "text-nps-danger hover:bg-nps-danger/10"
                : "text-nps-text hover:bg-nps-tertiary"
            }`}
          >
            {item.icon && <span className="w-4 text-center">{item.icon}</span>}
            <span>{item.label}</span>
          </button>
        )
      )}
    </div>
  );
}