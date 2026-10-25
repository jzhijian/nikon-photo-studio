import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { usePresetStore, PRESET_CATEGORIES, type PresetCategory, type PresetInfo } from "../../stores/presetStore";
import ConfirmDialog from "../../components/ConfirmDialog";
import CloudRecipesPanel from "./CloudRecipesPanel";
import NikonRenderPanel from "./NikonRenderPanel";
import { toast } from "../../stores/toastStore";

/** Preset Manager only: no photo strip, image preview, develop controls or export. */
export default function PresetsView() {
  const { t, i18n } = useTranslation();
  const presets = usePresetStore((s) => s.presets);
  const group = usePresetStore((s) => s.group);
  const format = usePresetStore((s) => s.format);
  const category = usePresetStore((s) => s.category);
  const selectedId = usePresetStore((s) => s.selectedId);
  const renamingId = usePresetStore((s) => s.renamingId);
  const select = usePresetStore((s) => s.select);
  const setCategory = usePresetStore((s) => s.setCategory);
  const toggleFavorite = usePresetStore((s) => s.toggleFavorite);
  const assignCategory = usePresetStore((s) => s.assignCategory);
  const rename = usePresetStore((s) => s.rename);
  const startRename = usePresetStore((s) => s.startRename);
  const removePreset = usePresetStore((s) => s.removePreset);
  const importPresetFiles = usePresetStore((s) => s.importPresetFiles);
  const [renameText, setRenameText] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<PresetInfo | null>(null);
  const [categoryTarget, setCategoryTarget] = useState<number | null>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  const list = presets.filter((preset) => {
    if (group === "favorites" && !preset.favorite) return false;
    if (group !== "all" && group !== "favorites" && preset.group !== group) return false;
    if (format !== "all" && preset.format !== format) return false;
    if (category !== "all" && preset.category !== category) return false;
    return true;
  });

  useEffect(() => {
    if (renamingId !== null) {
      renameRef.current?.focus();
      renameRef.current?.select();
    }
  }, [renamingId]);

  const doImport = async () => {
    const count = await importPresetFiles();
    toast(count > 0 ? `成功导入 ${count} 个预设` : "没有导入预设", count > 0 ? "success" : "info");
  };

  const beginRename = (preset: PresetInfo) => {
    setRenameText(preset.name);
    startRename(preset.id);
  };

  const commitRename = () => {
    if (renamingId !== null) rename(renamingId, renameText);
  };

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-3 px-5 py-3 border-b border-nps-border shrink-0">
        <div>
          <div className="text-[14px] font-semibold">预设管理</div>
          <div className="text-[10px] text-nps-text-dim mt-0.5">导入、筛选、重命名、分类、收藏和删除预设</div>
        </div>
        <span className="px-2 py-0.5 rounded-full bg-nps-tertiary text-[11px] text-nps-text-dim">{list.length} 个</span>
        <button type="button" onClick={doImport} className="ml-auto px-3 py-1.5 rounded bg-nps-accent text-nps-primary text-[11px] font-semibold hover:bg-nps-accent-hover">
          ＋ 导入预设
        </button>
      </div>

      <CloudRecipesPanel />
      <NikonRenderPanel />
      <div className="px-5 py-3 border-b border-nps-border bg-nps-secondary/40 shrink-0">
        <div className="flex gap-1.5 flex-wrap">
          {PRESET_CATEGORIES.map((item) => (
            <button key={item.id} type="button" onClick={() => setCategory(item.id)} className={`px-2.5 py-1 rounded-full text-[11px] border transition-colors ${category === item.id ? "border-nps-accent bg-nps-accent/15 text-nps-accent" : "border-nps-border bg-nps-tertiary text-nps-text-dim hover:bg-nps-hover"}`}>
              {item.icon} {i18n.language.startsWith("zh") ? item.zh : item.en}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-5">
        {list.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-nps-text-dim">
            <div className="text-4xl mb-3">🎨</div>
            <div className="text-sm">当前筛选条件下没有预设</div>
          </div>
        ) : (
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-3">
            {list.map((preset) => (
              <div key={preset.id} onClick={() => select(preset.id)} className={`relative group rounded-xl border p-3 bg-nps-card cursor-pointer transition-all ${selectedId === preset.id ? "border-nps-accent ring-1 ring-nps-accent/40" : "border-nps-border hover:border-nps-accent/60"}`}>
                <div className="h-16 rounded-lg mb-3 shadow-inner" style={{ background: `linear-gradient(135deg, ${preset.color}, ${preset.color}88)` }} />
                {renamingId === preset.id ? (
                  <input ref={renameRef} value={renameText} onChange={(event) => setRenameText(event.target.value)} onBlur={commitRename} onKeyDown={(event) => { if (event.key === "Enter") commitRename(); if (event.key === "Escape") startRename(null); }} onClick={(event) => event.stopPropagation()} className="w-full px-1.5 py-1 rounded bg-nps-tertiary border border-nps-accent text-[12px] outline-none" />
                ) : <div className="text-[12px] font-medium truncate">{preset.name}</div>}
                <div className="flex items-center gap-1.5 mt-1 text-[9px] text-nps-text-dim uppercase">
                  <span>{preset.format === "app" ? t("presets.appFormat") : preset.format}</span><span>·</span><span>{preset.group === "builtin" ? "内置" : "已导入"}</span>
                </div>
                <div className="mt-2 flex items-center gap-1">
                  <button type="button" onClick={(event) => { event.stopPropagation(); setCategoryTarget(categoryTarget === preset.id ? null : preset.id); }} className="px-1.5 py-0.5 rounded bg-nps-tertiary text-[9px] text-nps-text-dim hover:text-nps-text">分类</button>
                  <button type="button" onClick={(event) => { event.stopPropagation(); beginRename(preset); }} className="px-1.5 py-0.5 rounded bg-nps-tertiary text-[9px] text-nps-text-dim hover:text-nps-text">重命名</button>
                  <button type="button" onClick={(event) => { event.stopPropagation(); toggleFavorite(preset.id); }} className={`ml-auto text-[14px] ${preset.favorite ? "text-yellow-400" : "text-nps-text-dim hover:text-yellow-400"}`} title="收藏">★</button>
                  <button type="button" onClick={(event) => { event.stopPropagation(); setDeleteTarget(preset); }} className="text-[12px] text-nps-text-dim hover:text-red-400" title="删除">🗑</button>
                </div>
                {categoryTarget === preset.id && (
                  <div onClick={(event) => event.stopPropagation()} className="absolute z-20 left-3 right-3 bottom-10 p-1.5 rounded-lg bg-nps-secondary border border-nps-border shadow-2xl">
                    {PRESET_CATEGORIES.filter((item) => item.id !== "all").map((item) => (
                      <button key={item.id} type="button" onClick={() => { assignCategory(preset.id, item.id as PresetCategory); setCategoryTarget(null); }} className={`w-full text-left px-2 py-1 rounded text-[10px] ${preset.category === item.id ? "bg-nps-accent/15 text-nps-accent" : "text-nps-text-dim hover:bg-nps-hover"}`}>
                        {item.icon} {i18n.language.startsWith("zh") ? item.zh : item.en}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {deleteTarget && <ConfirmDialog title="删除预设" message={`确定删除预设“${deleteTarget.name}”吗？`} confirmLabel="删除" cancelLabel="取消" danger onConfirm={() => { removePreset(deleteTarget.id); setDeleteTarget(null); toast("预设已删除", "success"); }} onCancel={() => setDeleteTarget(null)} />}
    </div>
  );
}
