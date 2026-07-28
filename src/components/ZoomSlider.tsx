import { useLibraryStore } from "../stores/libraryStore";
import { useTranslation } from "react-i18next";

/** Compact thumbnail-size zoom control. Max shows ~2 columns. */
export default function ZoomSlider() {
  const { t } = useTranslation();
  const thumbSize = useLibraryStore((s) => s.thumbSize);
  const setThumbSize = useLibraryStore((s) => s.setThumbSize);
  return (
    <div className="flex items-center gap-2 text-nps-text-dim">
      <button
        type="button"
        onClick={() => setThumbSize(Math.max(96, thumbSize - 32))}
        className="w-6 h-6 rounded border border-nps-border bg-nps-tertiary hover:bg-nps-hover text-sm leading-none active:scale-95 transition-all"
        title={t("library.zoomOut")}
      >
        −
      </button>
      <input
        type="range"
        min={96}
        max={520}
        step={16}
        value={thumbSize}
        onChange={(e) => setThumbSize(Number(e.target.value))}
        className="w-28 accent-[var(--accent)] cursor-pointer"
        title={t("library.zoom")}
      />
      <button
        type="button"
        onClick={() => setThumbSize(Math.min(520, thumbSize + 32))}
        className="w-6 h-6 rounded border border-nps-border bg-nps-tertiary hover:bg-nps-hover text-sm leading-none active:scale-95 transition-all"
        title={t("library.zoomIn")}
      >
        ＋
      </button>
    </div>
  );
}