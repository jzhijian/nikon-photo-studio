import { useEffect, useState, useCallback } from "react";
import { useLibraryStore, selectFiltered } from "../../stores/libraryStore";
import { usePresetStore, PRESET_CATEGORIES } from "../../stores/presetStore";
import { toast } from "../../stores/toastStore";
import PhotoCard from "../../components/PhotoCard";
import { prefetchThumbs } from "../../components/Thumbnail";
import DevelopViewer from "../../components/DevelopViewer";
import ZoomSlider from "../../components/ZoomSlider";
import ContextMenu, { MenuItem } from "../../components/ContextMenu";
import ConfirmDialog from "../../components/ConfirmDialog";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import type { Photo } from "../../stores/libraryStore";

export default function LibraryView() {
  const { t, i18n } = useTranslation();
  const photos = useLibraryStore((s) => s.photos);
  const folder = useLibraryStore((s) => s.folder);
  const filter = useLibraryStore((s) => s.filter);
  const ratingOperator = useLibraryStore((s) => s.ratingOperator);
  const ratingValue = useLibraryStore((s) => s.ratingValue);
  const importing = useLibraryStore((s) => s.importing);
  const importFromDialog = useLibraryStore((s) => s.importFromDialog);
  const thumbSize = useLibraryStore((s) => s.thumbSize);
  const selectMany = useLibraryStore((s) => s.selectMany);
  const deselectAll = useLibraryStore((s) => s.deselectAll);
  const removeMany = useLibraryStore((s) => s.removeMany);
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const setRating = useLibraryStore((s) => s.setRating);
  const select = useLibraryStore((s) => s.select);

  const presets = usePresetStore((s) => s.presets);
  const presetCategory = usePresetStore((s) => s.category);
  const setPresetCategory = usePresetStore((s) => s.setCategory);
  const applyPreset = usePresetStore((s) => s.apply);
  const setAdjust = useLibraryStore((s) => s.setAdjust);
  const adjust = useLibraryStore((s) => s.adjust);

  const list = selectFiltered({ photos, folder, filter, ratingOperator, ratingValue });
  const orderedPaths = list.map((p) => p.filePath);
  const selectedCount = photos.filter((p) => p.selected).length;

  // Warm grid thumbnails whenever the visible set changes (import / folder /
  // filter / rating). Backend renders them in parallel; the JS cache + inflight
  // dedup mean cards that mount afterwards show instantly instead of each firing
  // its own decode. Fire-and-forget; idempotent.
  const listSignature = orderedPaths.join("|");
  useEffect(() => {
    if (orderedPaths.length > 0) void prefetchThumbs(orderedPaths, 360);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listSignature]);

  // In-place develop mode (LR-style: double-click shows image centered in current view)
  const developPath = useLibraryStore((s) => s.developPath);
  const setDevelopPath = useLibraryStore((s) => s.setDevelopPath);
  // Context menu state
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; photo: Photo } | null>(null);
  // Confirm dialog state
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);

  const developPhoto = developPath ? photos.find((p) => p.filePath === developPath) ?? null : null;
  // A photo opened from Organize may not match the library's currently active
  // folder/format/rating filters. Still show it, and use the complete library
  // for previous/next navigation instead of producing an invalid 0 / N index.
  const developList = developPhoto && !list.some((p) => p.filePath === developPhoto.filePath)
    ? photos
    : list;
  const developIdx = developPhoto ? developList.findIndex((p) => p.filePath === developPhoto.filePath) : -1;

  const doImport = async () => {
    const n = await importFromDialog();
    if (n > 0) toast(t("library.importedMsg", { count: n }), "success");
    else if (n === 0) toast(t("library.noNewPhotos"), "info");
  };

  const selectAll = () => {
    selectMany(list.map((p) => p.filePath));
  };

  const requestDelete = useCallback((paths: string[]) => {
    if (paths.length === 0) return;
    setConfirmDelete(paths);
  }, []);

  const confirmDeleteAction = async () => {
    if (!confirmDelete) return;
    const count = confirmDelete.length;
    // Try to move files to recycle bin via backend
    for (const p of confirmDelete) {
      try {
        await invoke("move_to_trash", { path: p });
      } catch {
        // If backend delete fails, just remove from library
      }
    }
    removeMany(confirmDelete);
    setConfirmDelete(null);
    if (developPath && confirmDelete.includes(developPath)) {
      setDevelopPath(null);
    }
    toast(t("organize.deletedMsg", { count }), "success");
  };

  // Navigate in develop mode
  const developNext = useCallback(() => {
    if (developIdx < developList.length - 1) {
      const n = developList[developIdx + 1];
      setDevelopPath(n.filePath);
      select(n.filePath);
    }
  }, [developIdx, developList, select]);

  const developPrev = useCallback(() => {
    if (developIdx > 0) {
      const n = developList[developIdx - 1];
      setDevelopPath(n.filePath);
      select(n.filePath);
    }
  }, [developIdx, developList, select]);

  // ---- Keyboard shortcuts ----
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Skip if typing in input
      if ((e.target as HTMLElement)?.tagName === "INPUT" || (e.target as HTMLElement)?.tagName === "TEXTAREA") return;

      // ESC: close develop mode
      if (e.key === "Escape" && developPath) {
        e.preventDefault();
        setDevelopPath(null);
        return;
      }

      // Arrow keys in develop mode
      if (developPath) {
        if (e.key === "ArrowRight") { developNext(); return; }
        if (e.key === "ArrowLeft") { developPrev(); return; }
      }

      // Ctrl+A: select all
      if ((e.ctrlKey || e.metaKey) && e.key === "a") {
        e.preventDefault();
        selectAll();
        return;
      }

      // Delete key: delete selected
      if (e.key === "Delete" || e.key === "Backspace") {
        const sel = photos.filter((p) => p.selected).map((p) => p.filePath);
        if (sel.length > 0) {
          e.preventDefault();
          requestDelete(sel);
        }
        return;
      }

      // 1-5: rate selected/develop photo
      const rateTarget = developPath || selectedPath;
      if (e.key >= "1" && e.key <= "5" && rateTarget) {
        setRating(rateTarget, parseInt(e.key));
        return;
      }
      // 0: clear rating
      if (e.key === "0" && rateTarget) {
        setRating(rateTarget, 0);
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [photos, selectedPath, developPath, list, setRating, requestDelete, developNext, developPrev]);

  // ---- Context menu handler ----
  const handleContextMenu = (e: React.MouseEvent, photo: Photo) => {
    setCtxMenu({ x: e.clientX, y: e.clientY, photo });
  };

  const buildMenuItems = (): MenuItem[] => {
    if (!ctxMenu) return [];
    const photo = ctxMenu.photo;
    const isMultiSelected = photo.selected && selectedCount > 1;
    const targets = isMultiSelected
      ? photos.filter((p) => p.selected).map((p) => p.filePath)
      : [photo.filePath];

    const items: MenuItem[] = [
      {
        label: isMultiSelected ? `打开 (${selectedCount} 张已选)` : "打开大图",
        icon: "🔍",
        onClick: () => {
          select(photo.filePath);
          setDevelopPath(photo.filePath);
        },
      },
      { label: "---", icon: "", onClick: () => {} },
      {
        label: "⭐ 1 星",
        icon: "",
        onClick: () => setRating(photo.filePath, 1),
      },
      {
        label: "⭐⭐ 2 星",
        icon: "",
        onClick: () => setRating(photo.filePath, 2),
      },
      {
        label: "⭐⭐⭐ 3 星",
        icon: "",
        onClick: () => setRating(photo.filePath, 3),
      },
      {
        label: "⭐⭐⭐⭐ 4 星",
        icon: "",
        onClick: () => setRating(photo.filePath, 4),
      },
      {
        label: "⭐⭐⭐⭐⭐ 5 星",
        icon: "",
        onClick: () => setRating(photo.filePath, 5),
      },
      { label: "---", icon: "", onClick: () => {} },
      {
        label: isMultiSelected ? `🗑 删除 ${selectedCount} 张` : "🗑 删除",
        icon: "",
        danger: true,
        onClick: () => requestDelete(targets),
      },
    ];
    return items;
  };

  // Quick apply preset to selected photo
  const catPresets = presets.filter((p) =>
    presetCategory === "all" ? true : p.category === presetCategory
  );

  const quickApplyPreset = (presetId: number, name: string) => {
    applyPreset(presetId);
    const preset = presets.find((p) => p.id === presetId);
    if (preset) {
      setAdjust({
        exposure: preset.params.exposure,
        contrast: preset.params.contrast,
        highlights: preset.params.highlights,
        shadows: preset.params.shadows,
        temperature: preset.params.temperature,
        saturation: preset.params.saturation,
        vibrance: preset.params.vibrance,
        clarity: preset.params.clarity,
        dehaze: preset.params.dehaze,
      });
    }
    toast(`已应用预设: ${name}`, "success");
  };

  // ---- DEVELOP MODE (in-place, LR-style) ----
  if (developPhoto) {
    return (
      <div className="h-full flex flex-col bg-nps-primary">
        {/* Develop toolbar */}
        <div className="flex items-center gap-3 px-4 py-2 border-b border-nps-border shrink-0">
          <button
            type="button"
            onClick={() => setDevelopPath(null)}
            className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary hover:bg-nps-hover active:scale-[0.97] transition-all"
            title="Esc"
          >
            ← 返回图库
          </button>
          <span className="text-[13px] font-semibold text-nps-text">{developPhoto.filename}</span>
          <span className="text-[11px] text-nps-text-dim">
            {developPhoto.format} · {developPhoto.width > 0 ? `${developPhoto.width}×${developPhoto.height}` : "RAW"} · {(developPhoto.fileSize / 1024 / 1024).toFixed(1)} MB
          </span>
          <span className="text-[11px] text-nps-text-dim ml-2">
            {developIdx + 1} / {developList.length}
          </span>
          <div className="ml-auto flex items-center gap-2">
            {/* Star rating in develop mode */}
            <div className="flex items-center gap-0.5">
              {[1, 2, 3, 4, 5].map((star) => (
                <button
                  key={star}
                  type="button"
                  onClick={() => setRating(developPhoto.filePath, developPhoto.rating === star ? 0 : star)}
                  className={`text-lg transition-transform hover:scale-125 ${
                    star <= developPhoto.rating ? "text-yellow-400" : "text-nps-text-dim hover:text-nps-text"
                  }`}
                  title={`${star} ★ (按数字键 ${star})`}
                >
                  ★
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={() => requestDelete([developPhoto.filePath])}
              className="px-2 py-1 rounded text-[11px] bg-red-500/20 text-red-400 border border-red-500/30 hover:bg-red-500/30 active:scale-[0.97] transition-all"
            >
              🗑 删除
            </button>
          </div>
        </div>

        {/* Main image area - centered, fills available space */}
        <div className="flex-1 flex items-center justify-center relative overflow-hidden bg-black/20">
          <DevelopViewer key={developPhoto.filePath} photo={developPhoto} adjust={adjust} />

          {/* Nav arrows */}
          {developIdx > 0 && (
            <button
              type="button"
              onClick={developPrev}
              className="absolute left-4 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-nps-tertiary/80 hover:bg-nps-hover text-nps-text text-xl flex items-center justify-center transition-colors border border-nps-border"
              title="上一张 (←)"
            >
              ‹
            </button>
          )}
          {developIdx < developList.length - 1 && (
            <button
              type="button"
              onClick={developNext}
              className="absolute right-4 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-nps-tertiary/80 hover:bg-nps-hover text-nps-text text-xl flex items-center justify-center transition-colors border border-nps-border"
              title="下一张 (→)"
            >
              ›
            </button>
          )}
        </div>



        {/* Delete Confirmation Dialog */}
        {confirmDelete && (
          <ConfirmDialog
            title="确认删除"
            message={`确定要删除 ${confirmDelete.length} 张照片吗？文件将被移至回收站。`}
            confirmLabel="删除"
            cancelLabel="取消"
            danger
            onConfirm={confirmDeleteAction}
            onCancel={() => setConfirmDelete(null)}
          />
        )}
      </div>
    );
  }

  // ---- GRID MODE (normal library view) ----
  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-nps-border shrink-0">
        <div className="text-[13px] text-nps-text-dim">
          <span className="font-semibold text-nps-text">{list.length}</span> {t("library.photos")}
        </div>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={selectAll}
            className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary hover:bg-nps-hover active:scale-[0.97] transition-all"
            title="Ctrl+A"
          >
            {t("library.selectAll")}
          </button>
          {selectedCount > 0 && (
            <>
              <span className="text-[11px] text-nps-accent font-medium">
                {selectedCount} {t("transfer.selected")}
              </span>
              <button
                type="button"
                onClick={() => requestDelete(photos.filter((p) => p.selected).map((p) => p.filePath))}
                className="px-2.5 py-1 rounded text-[11px] bg-red-500/20 text-red-400 border border-red-500/30 hover:bg-red-500/30 active:scale-[0.97] transition-all"
                title="Delete"
              >
                🗑 {t("organize.deleteSelected")}
              </button>
              <button
                type="button"
                onClick={deselectAll}
                className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary hover:bg-nps-hover active:scale-[0.97] transition-all"
              >
                {t("transfer.deselect")}
              </button>
            </>
          )}
        </div>

        <div className="ml-auto flex items-center gap-3">
          <ZoomSlider />
          <button
            type="button"
            onClick={doImport}
            disabled={importing}
            className="px-3 py-1.5 rounded border border-nps-border bg-nps-tertiary text-[12px] hover:bg-nps-hover active:scale-[0.98] transition-all disabled:opacity-60"
          >
            {importing ? t("library.importing") : `➕ ${t("library.importFolder")}`}
          </button>
        </div>
      </div>

      {/* Quick Presets Strip */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-nps-border bg-nps-secondary/50 shrink-0 overflow-x-auto">
        <span className="text-[10px] text-nps-text-dim font-medium shrink-0 mr-1">快速预设:</span>
        {PRESET_CATEGORIES.map((cat) => (
          <button
            key={cat.id}
            type="button"
            onClick={() => setPresetCategory(cat.id)}
            className={`px-2 py-0.5 rounded-full text-[10px] border transition-all shrink-0 ${
              presetCategory === cat.id
                ? "border-nps-accent bg-nps-accent/15 text-nps-accent font-medium"
                : "border-nps-border bg-nps-tertiary text-nps-text-dim hover:bg-nps-hover"
            }`}
          >
            {cat.icon} {i18n.language.startsWith("zh") ? cat.zh : cat.en}
          </button>
        ))}
        <span className="w-px h-4 bg-nps-border mx-1 shrink-0" />
        {catPresets.slice(0, 8).map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={() => quickApplyPreset(p.id, p.name)}
            className="group relative shrink-0"
            title={`${p.name} (${p.format})`}
          >
            <div
              className="w-6 h-6 rounded-full border-2 border-transparent group-hover:border-nps-accent group-hover:scale-110 transition-all shadow-sm"
              style={{ background: `linear-gradient(135deg, ${p.color}, ${p.color}99)` }}
            />
            <span className="absolute -bottom-4 left-1/2 -translate-x-1/2 text-[8px] text-nps-text-dim opacity-0 group-hover:opacity-100 whitespace-nowrap transition-opacity">
              {p.name}
            </span>
          </button>
        ))}
      </div>

      {/* Photo grid */}
      <div className="flex-1 p-4 overflow-y-auto">
        {list.length === 0 ? (
          <div className="h-full flex items-center justify-center text-nps-text-dim text-sm">
            {t("library.noMatch")}
          </div>
        ) : (
          <div className="thumb-grid" style={{ ["--thumb-size" as string]: `${thumbSize}px` }}>
            {list.map((p) => (
              <PhotoCard
                key={p.filePath}
                photo={p}
                orderedPaths={orderedPaths}
                onContextMenu={handleContextMenu}
                onDoubleClick={(photo) => {
                  select(photo.filePath);
                  setDevelopPath(photo.filePath);
                }}
              />
            ))}
          </div>
        )}
      </div>

      {/* Context Menu */}
      {ctxMenu && (
        <ContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          items={buildMenuItems()}
          onClose={() => setCtxMenu(null)}
        />
      )}

      {/* Delete Confirmation Dialog */}
      {confirmDelete && (
        <ConfirmDialog
          title="确认删除"
          message={`确定要删除 ${confirmDelete.length} 张照片吗？文件将被移至回收站。`}
          confirmLabel="删除"
          cancelLabel="取消"
          danger
          onConfirm={confirmDeleteAction}
          onCancel={() => setConfirmDelete(null)}
        />
      )}
    </div>
  );
}
