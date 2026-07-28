import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { useLibraryStore } from "../../stores/libraryStore";
import type { Photo } from "../../stores/libraryStore";
import { useOrganizeStore } from "../../stores/organizeStore";
import PhotoCard from "../../components/PhotoCard";
import DevelopViewer from "../../components/DevelopViewer";
import { prefetchThumbs } from "../../components/Thumbnail";
import ZoomSlider from "../../components/ZoomSlider";
import ConfirmDialog from "../../components/ConfirmDialog";
import ContextMenu, { type MenuItem } from "../../components/ContextMenu";
import { toast } from "../../stores/toastStore";

export default function OrganizeView() {
  const { t } = useTranslation();
  const photos = useLibraryStore((s) => s.photos);
  const thumbSize = useLibraryStore((s) => s.thumbSize);
  const adjust = useLibraryStore((s) => s.adjust);
  const category = useOrganizeStore((s) => s.category);
  const threshold = useOrganizeStore((s) => s.threshold);
  const importFromDialog = useLibraryStore((s) => s.importFromDialog);
  const selectMany = useLibraryStore((s) => s.selectMany);
  const deselectAll = useLibraryStore((s) => s.deselectAll);
  const removeMany = useLibraryStore((s) => s.removeMany);
  const select = useLibraryStore((s) => s.select);
  const setRating = useLibraryStore((s) => s.setRating);
  const developPath = useLibraryStore((s) => s.developPath);
  const setDevelopPath = useLibraryStore((s) => s.setDevelopPath);
  const [archiving, setArchiving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; photo: Photo } | null>(null);

  const list = useMemo(() => photos.filter((photo) => {
    if (category === "blurry") return photo.blurScore >= 0 && photo.blurScore < threshold;
    return photo.category === category;
  }), [photos, category, threshold]);
  const orderedPaths = useMemo(() => list.map((photo) => photo.filePath), [list]);
  const selectedCount = photos.filter((photo) => photo.selected).length;
  const developPhoto = developPath ? list.find((photo) => photo.filePath === developPath) ?? null : null;
  const developIndex = developPhoto ? list.findIndex((photo) => photo.filePath === developPhoto.filePath) : -1;

  const signature = orderedPaths.join("|");
  useEffect(() => {
    if (orderedPaths.length > 0) void prefetchThumbs(orderedPaths, 360);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  // If the active category/threshold changes and no longer contains the open
  // photo, return to this Organize grid rather than leaking into Library.
  useEffect(() => {
    if (developPath && !list.some((photo) => photo.filePath === developPath)) setDevelopPath(null);
  }, [developPath, list, setDevelopPath]);

  const requestDelete = useCallback((paths: string[]) => {
    if (paths.length > 0) setConfirmDelete(paths);
  }, []);

  const confirmDeleteAction = async () => {
    if (!confirmDelete) return;
    for (const path of confirmDelete) {
      try { await invoke("move_to_trash", { path }); } catch { /* keep UI responsive */ }
    }
    const count = confirmDelete.length;
    if (developPath && confirmDelete.includes(developPath)) setDevelopPath(null);
    removeMany(confirmDelete);
    setConfirmDelete(null);
    toast(t("organize.deletedMsg", { count }), "success");
  };

  const openDevelop = (photo: Photo) => {
    select(photo.filePath);
    setDevelopPath(photo.filePath);
  };

  const moveDevelop = (offset: number) => {
    const next = list[developIndex + offset];
    if (next) openDevelop(next);
  };

  const menuItems = (): MenuItem[] => {
    if (!ctxMenu) return [];
    const photo = ctxMenu.photo;
    const selectedTargets = photo.selected && selectedCount > 1
      ? photos.filter((item) => item.selected).map((item) => item.filePath)
      : [photo.filePath];
    return [
      { label: "在整理中打开大图", icon: "🔍", onClick: () => openDevelop(photo) },
      { label: "", divider: true, onClick: () => {} },
      ...[1, 2, 3, 4, 5].map((rating) => ({
        label: `${"★".repeat(rating)} ${rating} 星`,
        onClick: () => setRating(photo.filePath, rating),
      })),
      { label: "", divider: true, onClick: () => {} },
      {
        label: selectedTargets.length > 1 ? `删除选中的 ${selectedTargets.length} 张照片` : "删除这张照片",
        icon: "🗑",
        danger: true,
        onClick: () => requestDelete(selectedTargets),
      },
    ];
  };

  const archiveToDir = async () => {
    const target = await open({ directory: true, multiple: false });
    if (!target || typeof target !== "string") return;
    const paths = selectedCount > 0
      ? photos.filter((photo) => photo.selected).map((photo) => photo.filePath)
      : orderedPaths;
    setArchiving(true);
    try {
      await invoke("archive_photos", { paths, destDir: target });
      toast(`已归档 ${paths.length} 张照片`, "success");
      deselectAll();
    } catch (error) {
      toast(`归档失败：${String(error)}`, "error");
    } finally {
      setArchiving(false);
    }
  };

  if (photos.length === 0) {
    return (
      <div className="h-full flex items-center justify-center text-center p-8">
        <div>
          <div className="text-5xl mb-4">🗂️</div>
          <h2 className="text-xl font-bold mb-2">{t("organize.emptyTitle")}</h2>
          <p className="text-sm text-nps-text-dim mb-6">{t("organize.emptyDesc")}</p>
          <button type="button" onClick={importFromDialog} className="px-6 py-2.5 rounded-lg bg-nps-accent text-nps-primary font-semibold text-sm">
            📥 {t("library.importFolder")}
          </button>
        </div>
      </div>
    );
  }

  if (developPhoto) {
    return (
      <div className="h-full flex flex-col bg-nps-primary">
        <div className="flex items-center gap-3 px-4 py-2 border-b border-nps-border shrink-0">
          <button type="button" onClick={() => setDevelopPath(null)} className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary hover:bg-nps-hover">
            ← 返回整理
          </button>
          <span className="text-[13px] font-semibold">{developPhoto.filename}</span>
          <span className="text-[11px] text-nps-text-dim">{developIndex + 1} / {list.length} · 当前整理结果</span>
          <div className="ml-auto flex items-center gap-2">
            <button type="button" onClick={() => requestDelete([developPhoto.filePath])} className="px-2 py-1 rounded text-[11px] bg-red-500/20 text-red-400 border border-red-500/30">
              🗑 删除
            </button>
          </div>
        </div>
        <div className="flex-1 relative overflow-hidden bg-black/20">
          <DevelopViewer key={developPhoto.filePath} photo={developPhoto} adjust={adjust} />
          {developIndex > 0 && <button type="button" onClick={() => moveDevelop(-1)} className="absolute left-4 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-nps-tertiary/80 border border-nps-border">‹</button>}
          {developIndex < list.length - 1 && <button type="button" onClick={() => moveDevelop(1)} className="absolute right-4 top-1/2 -translate-y-1/2 w-10 h-10 rounded-full bg-nps-tertiary/80 border border-nps-border">›</button>}
        </div>
        {confirmDelete && <DeleteDialog paths={confirmDelete} onConfirm={confirmDeleteAction} onCancel={() => setConfirmDelete(null)} />}
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-4 py-2.5 border-b border-nps-border shrink-0 flex-wrap">
        <div className="text-[13px] text-nps-text-dim"><span className="font-semibold text-nps-text">{list.length}</span> {category === "blurry" ? t("organize.blurryBelow", { threshold }) : t("organize.inCategory")}</div>
        <div className="flex items-center gap-2 ml-2">
          <button type="button" onClick={() => selectMany(orderedPaths)} className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary hover:bg-nps-hover">{t("organize.selectAll")}</button>
          {selectedCount > 0 && <button type="button" onClick={() => requestDelete(photos.filter((photo) => photo.selected).map((photo) => photo.filePath))} className="px-2.5 py-1 rounded text-[11px] bg-red-500/20 text-red-400 border border-red-500/30">🗑 {t("organize.deleteSelected")}</button>}
          {selectedCount > 0 && <button type="button" onClick={deselectAll} className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary">{t("transfer.deselect")}</button>}
          <button type="button" onClick={archiveToDir} disabled={archiving || list.length === 0} className="px-2.5 py-1 rounded text-[11px] border border-nps-border bg-nps-tertiary disabled:opacity-50">{archiving ? "归档中…" : `📦 ${t("organize.archive")}`}</button>
        </div>
        <div className="ml-auto"><ZoomSlider /></div>
      </div>
      <div className="flex-1 p-4 overflow-y-auto">
        {list.length === 0 ? <div className="h-full flex items-center justify-center text-nps-text-dim text-sm">{t("organize.noneFound")}</div> : (
          <div className="thumb-grid" style={{ ["--thumb-size" as string]: `${thumbSize}px` }}>
            {list.map((photo) => <PhotoCard key={photo.filePath} photo={photo} orderedPaths={orderedPaths} onContextMenu={(event, item) => setCtxMenu({ x: event.clientX, y: event.clientY, photo: item })} onDoubleClick={openDevelop} />)}
          </div>
        )}
      </div>
      {ctxMenu && <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={menuItems()} onClose={() => setCtxMenu(null)} />}
      {confirmDelete && <DeleteDialog paths={confirmDelete} onConfirm={confirmDeleteAction} onCancel={() => setConfirmDelete(null)} />}
    </div>
  );
}

function DeleteDialog({ paths, onConfirm, onCancel }: { paths: string[]; onConfirm: () => void; onCancel: () => void }) {
  return <ConfirmDialog title="确认删除" message={`确定删除 ${paths.length} 张照片吗？文件将移入回收站。`} confirmLabel="删除" cancelLabel="取消" danger onConfirm={onConfirm} onCancel={onCancel} />;
}
