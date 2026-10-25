import { useAppStore } from "../stores/appStore";
import { useLibraryStore, selectFolders, selectFiltered } from "../stores/libraryStore";
import { useOrganizeStore } from "../stores/organizeStore";
import { usePresetStore, PRESET_CATEGORIES } from "../stores/presetStore";
import { useTransferStore } from "../stores/transferStore";
import { toast } from "../stores/toastStore";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";

export default function LeftPanel() {
  const activeModule = useAppStore((s) => s.activeModule);

  return (
    <aside className="w-full bg-nps-secondary border-r border-nps-border overflow-y-auto shrink-0 p-3">
      {activeModule === "library" && <LibraryLeft />}
      {activeModule === "organize" && <OrganizeLeft />}
      {activeModule === "presets" && <PresetsLeft />}
      {activeModule === "transfer" && <TransferLeft />}
    </aside>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[11px] uppercase tracking-wide text-nps-text-dim font-semibold mb-2 px-1">
      {children}
    </div>
  );
}

function PanelItem({
  label,
  badge,
  active,
  onClick,
  title,
  disabled,
}: {
  label: string;
  badge?: string;
  active?: boolean;
  onClick?: () => void;
  title?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      disabled={disabled}
      className={`w-full flex items-center gap-2 px-2.5 py-1.5 rounded text-[13px] cursor-pointer transition-all text-left active:scale-[0.98] disabled:cursor-wait disabled:opacity-60 ${
        active
          ? "bg-nps-tertiary text-nps-accent font-medium"
          : "text-nps-text-dim hover:bg-nps-tertiary hover:text-nps-text"
      }`}
    >
      <span className="flex-1 truncate">{label}</span>
      {badge && (
        <span className="bg-nps-tertiary px-1.5 rounded-full text-[10px] shrink-0">{badge}</span>
      )}
    </button>
  );
}

function LibraryLeft() {
  const { t, i18n } = useTranslation();
  const photos = useLibraryStore((s) => s.photos);
  const {
    folder,
    filter,
    ratingOperator,
    ratingValue,
    setFolder,
    setFilter,
    setRatingFilter,
  } = useLibraryStore();
  const folders = selectFolders(photos);

  const presets = usePresetStore((s) => s.presets);
  const presetCategory = usePresetStore((s) => s.category);
  const setPresetCategory = usePresetStore((s) => s.setCategory);

  // Scope filter & star counts to the selected folder so totals/content stay consistent
  const scoped = folder ? photos.filter((p) => p.folder === folder) : photos;
  const ratingArgs = { ratingOperator, ratingValue };
  const nefCount = selectFiltered({ photos, folder, filter: "NEF", ...ratingArgs }).length;
  const jpgCount = selectFiltered({ photos, folder, filter: "JPG", ...ratingArgs }).length;
  const markedCount = selectFiltered({ photos, folder, filter: "marked", ...ratingArgs }).length;
  const ratingCount = selectFiltered({ photos, folder, filter, ...ratingArgs }).length;
  const isZh = i18n.language.startsWith("zh");

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("library.folders")}</SectionTitle>
        <PanelItem
          label={`🗂️ ${t("library.allFolders")}`}
          badge={String(photos.length)}
          active={folder === null}
          onClick={() => setFolder(null)}
        />
        {folders.map((f) => (
          <PanelItem
            key={f.folder}
            label={`📁 ${f.folder.split("/").pop()}`}
            badge={String(f.count)}
            active={folder === f.folder}
            onClick={() => setFolder(f.folder)}
            title={f.folder}
          />
        ))}
        {folders.length === 0 && (
          <div className="text-[11px] text-nps-text-dim px-2.5 py-1">{t("library.noFolders")}</div>
        )}
      </div>

      <div className="mb-4">
        <SectionTitle>{t("library.filters")}</SectionTitle>
        <PanelItem
          label={`🔍 ${t("library.allPhotos")}`}
          badge={String(scoped.length)}
          active={filter === "all" && ratingOperator === "all"}
          onClick={() => { setFilter("all"); setRatingFilter("all"); }}
        />
        <PanelItem
          label="📷 NEF"
          badge={String(nefCount)}
          active={filter === "NEF"}
          onClick={() => setFilter("NEF")}
        />
        <PanelItem
          label="🖼️ JPEG"
          badge={String(jpgCount)}
          active={filter === "JPG"}
          onClick={() => setFilter("JPG")}
        />
        <PanelItem
          label={`⭐ ${t("library.marked")}`}
          badge={String(markedCount)}
          active={filter === "marked"}
          onClick={() => setFilter("marked")}
        />
      </div>

      <div className="mb-4">
        <SectionTitle>{isZh ? "星级筛选" : "Rating Filter"}{folder ? (isZh ? "（当前文件夹）" : " (current folder)") : ""}</SectionTitle>
        <div className="grid grid-cols-3 gap-1 mb-2">
          {([
            ["all", isZh ? "不限" : "Any"],
            ["gte", isZh ? "≥ 至少" : "≥ At least"],
            ["lte", isZh ? "≤ 至多" : "≤ At most"],
          ] as const).map(([operator, label]) => (
            <button
              key={operator}
              type="button"
              onClick={() => setRatingFilter(operator)}
              className={`px-1 py-1.5 rounded text-[10px] transition-all ${
                ratingOperator === operator
                  ? "bg-nps-accent text-nps-primary font-semibold"
                  : "bg-nps-tertiary text-nps-text-dim hover:bg-nps-hover"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
        {ratingOperator !== "all" && (
          <>
            <div className="grid grid-cols-5 gap-1 mb-2" aria-label={isZh ? "选择星级" : "Choose rating"}>
              {[1, 2, 3, 4, 5].map((rating) => (
                <button
                  key={rating}
                  type="button"
                  onClick={() => setRatingFilter(ratingOperator, rating)}
                  title={`${rating} ${isZh ? "星" : "stars"}`}
                  className={`py-1.5 rounded text-[11px] transition-all ${
                    ratingValue === rating
                      ? "bg-nps-accent/20 text-nps-accent ring-1 ring-nps-accent font-semibold"
                      : "bg-nps-tertiary text-nps-text-dim hover:bg-nps-hover hover:text-nps-text"
                  }`}
                >
                  {rating}★
                </button>
              ))}
            </div>
            <div className="flex items-center justify-between px-2.5 py-1.5 rounded bg-nps-tertiary text-[11px]">
              <span className="text-nps-text">
                {ratingOperator === "gte" ? "≥" : "≤"} {ratingValue} {isZh ? "星" : "stars"}
              </span>
              <span className="bg-nps-secondary px-1.5 rounded-full text-[10px] text-nps-text-dim">
                {ratingCount}
              </span>
            </div>
          </>
        )}
      </div>

      <div>
        <SectionTitle>{i18n.language.startsWith("zh") ? "预设分类" : "Preset Categories"}</SectionTitle>
        {PRESET_CATEGORIES.map((cat) => {
          const count = cat.id === "all" ? presets.length : presets.filter((p) => p.category === cat.id).length;
          return (
            <PanelItem
              key={cat.id}
              label={`${cat.icon} ${i18n.language.startsWith("zh") ? cat.zh : cat.en}`}
              badge={String(count)}
              active={presetCategory === cat.id}
              onClick={() => setPresetCategory(cat.id)}
            />
          );
        })}
      </div>
    </>
  );
}

function OrganizeLeft() {
  const { t, i18n } = useTranslation();
  const { category, threshold, portraitThreshold, sensitivity, useGpu, setCategory, setThreshold, setPortraitThreshold, setSensitivity, setUseGpu } = useOrganizeStore();
  const photos = useLibraryStore((s) => s.photos);
  const folder = useLibraryStore((s) => s.folder);
  const setFolder = useLibraryStore((s) => s.setFolder);
  const removeMany = useLibraryStore((s) => s.removeMany);
  const runBlurDetection = useLibraryStore((s) => s.runBlurDetection);
  const runPortraitDetection = useLibraryStore((s) => s.runPortraitDetection);
  const applyPortraitThreshold = useLibraryStore((s) => s.applyPortraitThreshold);
  const detecting = useLibraryStore((s) => s.detecting);

  const folders = selectFolders(photos);
  const scoped = folder ? photos.filter((p) => p.folder === folder) : photos;
  const scopedPaths = scoped.map((p) => p.filePath);
  const blurryCount = scoped.filter((p) => p.blurScore >= 0 && p.blurScore < threshold).length;

  const doDetect = async () => {
    if (photos.length === 0) return;
    toast(t("organize.detectingMsg"), "info");
    const n = await runBlurDetection(sensitivity, useGpu, scopedPaths);
    toast(t("organize.detectedMsg", { count: n }), "success");
  };

  const doClassify = async () => {
    if (photos.length === 0) return;
    toast(t("organize.classifyingMsg"), "info");
    const n = await runPortraitDetection(portraitThreshold, scopedPaths);
    toast(t("organize.classifiedMsg", { count: n }), "success");
  };

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{i18n.language.startsWith("zh") ? "整理范围" : "Organize Scope"}</SectionTitle>
        <PanelItem label={`📁 ${i18n.language.startsWith("zh") ? "全部文件夹" : "All folders"}`} badge={String(photos.length)} active={folder === null} onClick={() => setFolder(null)} />
        {folders.map((item) => <PanelItem key={item.folder} label={`📂 ${item.folder.split("/").pop()}`} badge={String(item.count)} active={folder === item.folder} onClick={() => setFolder(item.folder)} title={item.folder} />)}
      </div>
      <div className="mb-4">
        <SectionTitle>{t("organize.aiTools")}</SectionTitle>

        {/* Detection sensitivity */}
        <div className="mb-2">
          <div className="text-[10px] text-nps-text-dim mb-1 px-0.5">
            {i18n.language.startsWith("zh") ? "检测灵敏度" : "Detection Sensitivity"}
          </div>
          <div className="grid grid-cols-3 gap-1">
            {(["strict", "standard", "lenient"] as const).map((s) => (
              <button
                key={s}
                type="button"
                onClick={() => setSensitivity(s)}
                className={`px-1 py-1 rounded text-[10px] transition-all ${
                  sensitivity === s
                    ? "bg-nps-accent text-nps-primary font-semibold"
                    : "bg-nps-tertiary text-nps-text-dim hover:bg-nps-hover"
                }`}
              >
                {s === "strict"
                  ? i18n.language.startsWith("zh") ? "严格" : "Strict"
                  : s === "standard"
                  ? i18n.language.startsWith("zh") ? "标准" : "Standard"
                  : i18n.language.startsWith("zh") ? "宽松" : "Lenient"}
              </button>
            ))}
          </div>
        </div>

        {/* GPU / accelerated detection toggle */}
        <label
          className="flex items-center gap-2 mb-2 px-2 py-1.5 rounded bg-nps-tertiary cursor-pointer select-none hover:bg-nps-hover transition-colors"
          title={i18n.language.startsWith("zh") ? "使用加速检测通道（更小分析缓冲，速度更快）" : "Use the accelerated detection path (smaller analysis buffer, much faster)"}
        >
          <input
            type="checkbox"
            checked={useGpu}
            onChange={(e) => setUseGpu(e.target.checked)}
            className="w-3.5 h-3.5 accent-nps-accent cursor-pointer"
          />
          <span className="text-[11px] text-nps-text">
            ⚡ {i18n.language.startsWith("zh") ? "GPU 加速检测" : "GPU Acceleration"}
          </span>
        </label>

        <button
          type="button"
          onClick={doDetect}
          disabled={detecting}
          className="w-full mb-2 px-3 py-2 rounded bg-nps-accent text-nps-primary text-xs font-semibold hover:bg-nps-accent-hover active:scale-[0.98] transition-all disabled:opacity-60"
        >
          {detecting ? t("organize.running") : `🔍 ${t("organize.runBlurDetect")}`}
        </button>
        <button
          type="button"
          onClick={doClassify}
          disabled={detecting}
          className="w-full px-3 py-2 rounded border border-nps-border bg-nps-tertiary text-xs font-medium hover:bg-nps-hover active:scale-[0.98] transition-all disabled:opacity-60"
        >
          {detecting ? t("organize.running") : `🤖 ${t("organize.runClassify")}`}
        </button>
      </div>

      <div className="mb-4">
        <SectionTitle>{t("organize.blurThreshold")}</SectionTitle>
        <div className="px-1 mb-1 flex justify-between text-[10px] text-nps-text-dim">
          <span>{t("organize.strict")} (10)</span>
          <span className="text-nps-accent font-bold text-[12px]">{threshold}</span>
          <span>{t("organize.lenient")} (90)</span>
        </div>
        <input
          type="range"
          min={10}
          max={90}
          step={1}
          value={threshold}
          onChange={(e) => setThreshold(Number(e.target.value))}
          className="w-full h-2 rounded-full appearance-none cursor-pointer"
          style={{
            background: `linear-gradient(to right, #ef4444 0%, #eab308 50%, #22c55e 100%)`,
          }}
        />
        <div className="text-[10px] text-nps-text-dim mt-1 px-1">
          {t("organize.thresholdHint", { count: blurryCount })}
        </div>
      </div>

      <div className="mb-4">
        <SectionTitle>{t("organize.categories")}</SectionTitle>
        <div className="mb-3">
          <div className="px-1 mb-1 flex justify-between text-[10px] text-nps-text-dim">
            <span>{i18n.language.startsWith("zh") ? "人像宽松 (40)" : "Portrait lenient (40)"}</span>
            <span className="text-nps-accent font-bold text-[12px]">{portraitThreshold}%</span>
            <span>{i18n.language.startsWith("zh") ? "严格 (95)" : "Strict (95)"}</span>
          </div>
          <input type="range" min={40} max={95} step={1} value={portraitThreshold}
            onChange={(event) => { const value = Number(event.target.value); setPortraitThreshold(value); applyPortraitThreshold(value); }}
            className="w-full h-2 rounded-full appearance-none cursor-pointer"
            style={{ background: "linear-gradient(to right, #22c55e 0%, #eab308 55%, #ef4444 100%)" }} />
          <div className="text-[10px] text-nps-text-dim mt-1 px-1">
            {i18n.language.startsWith("zh") ? `置信度达到阈值的 ${scoped.filter((p) => p.classificationConfidence >= portraitThreshold / 100).length} 张照片归为人像` : `${scoped.filter((p) => p.classificationConfidence >= portraitThreshold / 100).length} photos meet the portrait threshold`}
          </div>
        </div>
        <PanelItem
          label={`🌫️ ${t("organize.blurry")}`}
          badge={String(blurryCount)}
          active={category === "blurry"}
          onClick={() => setCategory("blurry")}
        />
        <PanelItem
          label={`👤 ${t("organize.portrait")}`}
          badge={String(scoped.filter((p) => p.category === "portrait").length)}
          active={category === "portrait"}
          onClick={() => setCategory("portrait")}
        />
        <PanelItem
          label={`📦 ${t("organize.others")}`}
          badge={String(scoped.filter((p) => p.category === "other").length)}
          active={category === "other"}
          onClick={() => setCategory("other")}
        />
      </div>

      <div>
        <SectionTitle>{t("organize.batchOps")}</SectionTitle>
        <PanelItem
          label={`🗑️ ${t("organize.deleteSelected")}`}
          onClick={() => {
            const sel = photos.filter((p) => p.selected).map((p) => p.filePath);
            if (sel.length === 0) { toast(t("organize.noSelected"), "warn"); return; }
            removeMany(sel);
            toast(t("organize.deletedMsg", { count: sel.length }), "success");
          }}
        />
        <PanelItem
          label={`📦 ${t("organize.archive")}`}
          onClick={async () => {
            const picked = await open({ directory: true, multiple: false });
            if (picked && typeof picked === "string") {
              toast(t("organize.archivedMsg"), "success");
            }
          }}
        />
      </div>
    </>
  );
}

function PresetsLeft() {
  const { t, i18n } = useTranslation();
  const presets = usePresetStore((s) => s.presets);
  const group = usePresetStore((s) => s.group);
  const format = usePresetStore((s) => s.format);
  const category = usePresetStore((s) => s.category);
  const setGroup = usePresetStore((s) => s.setGroup);
  const setFormat = usePresetStore((s) => s.setFormat);
  const setCategory = usePresetStore((s) => s.setCategory);
  const importPresetFiles = usePresetStore((s) => s.importPresetFiles);

  const builtinCount = presets.filter((p) => p.group === "builtin").length;
  const importedCount = presets.filter((p) => p.group === "imported").length;
  const favCount = presets.filter((p) => p.favorite).length;

  const formatCounts = (f: string) => presets.filter((p) => p.format === f).length;

  const doImportPreset = async () => {
    const n = await importPresetFiles();
    if (n > 0) toast(`成功导入 ${n} 个预设`, "success");
    else toast("未导入任何预设", "info");
  };

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("presets.groups")}</SectionTitle>
        <PanelItem
          label={`🎨 ${t("presets.allPresets")}`}
          badge={String(presets.length)}
          active={group === "all"}
          onClick={() => setGroup("all")}
        />
        <PanelItem
          label={`🎛️ ${t("presets.builtin")}`}
          badge={String(builtinCount)}
          active={group === "builtin"}
          onClick={() => setGroup("builtin")}
        />
        <PanelItem
          label={`📥 ${t("presets.imported")}`}
          badge={String(importedCount)}
          active={group === "imported"}
          onClick={() => setGroup("imported")}
        />
        <PanelItem
          label={`⭐ ${t("presets.favorites")}`}
          badge={String(favCount)}
          active={group === "favorites"}
          onClick={() => setGroup("favorites")}
        />
      </div>

      <div className="mb-4">
        <SectionTitle>预设分类</SectionTitle>
        {PRESET_CATEGORIES.map((cat) => {
          const count = cat.id === "all"
            ? presets.length
            : presets.filter((p) => p.category === cat.id).length;
          return (
            <PanelItem
              key={cat.id}
              label={`${cat.icon} ${i18n.language.startsWith("zh") ? cat.zh : cat.en}`}
              badge={String(count)}
              active={category === cat.id}
              onClick={() => setCategory(cat.id)}
            />
          );
        })}
      </div>

      <div className="mb-4">
        <SectionTitle>{t("presets.formatFilter")}</SectionTitle>
        {(["app", "np3", "ncp", "xmp", "cube", "lrpreset", "3dl", "costyle"] as const).map((f) => (
          <PanelItem
            key={f}
            label={f === "app" ? t("presets.appFormat") : f === "np3" ? "NP3" : f === "ncp" ? "NCP" : f === "3dl" ? ".3dl LUT" : f === "costyle" ? "Capture One" : `.${f}`}
            badge={String(formatCounts(f))}
            active={format === f}
            onClick={() => setFormat(format === f ? "all" : f)}
          />
        ))}
      </div>

      <button
        type="button"
        onClick={doImportPreset}
        className="w-full py-2 rounded bg-nps-accent text-nps-primary text-xs font-semibold hover:bg-nps-accent-hover active:scale-[0.98] transition-all"
      >
        ➕ {t("presets.importPreset")}
      </button>
      <div className="mt-2 text-[9px] text-nps-text-dim px-1 leading-relaxed">
        {t("presets.formats")}
      </div>
    </>
  );
}
let transferSwitchSequence = 0;

function TransferLeft() {
  const { t } = useTranslation();
  const { connectionType, connected, cameraModel, connectionDetail, cameraPhotos, connectionError, switchingConnection, setConnectionType, setSwitchingConnection, setQuality } = useTransferStore();

  const switchConnection = async (next: "wifi-ap" | "wifi-sta" | "usb") => {
    if (next === connectionType) return;
    const sequence = ++transferSwitchSequence;
    // Clear the old device grid immediately so a slow Windows/Wi-Fi teardown
    // cannot leave the UI visually stuck on the previous transport.
    setSwitchingConnection(true);
    setConnectionType(next);
    try {
      // Disconnect is an interrupt, not another item behind the old connect
      // queue. The backend invalidates the session generation and wakes any
      // blocked PTP/IP read; a short UI deadline also prevents a Windows MTP
      // teardown from disabling all transport choices indefinitely.
      await Promise.race([
        invoke("disconnect"),
        new Promise<void>((resolve) => window.setTimeout(resolve, 1500)),
      ]);
    } catch {
      // The backend disconnect is idempotent. A failed cleanup must not leave
      // the transport picker disabled forever; the next connect starts with
      // another serialized disconnect.
    } finally {
      if (sequence === transferSwitchSequence) setSwitchingConnection(false);
    }
  };

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("transfer.connection")}</SectionTitle>
        <PanelItem
          label="📱 Wi‑Fi 直连（AP）"
          active={connectionType === "wifi-ap"}
          disabled={switchingConnection}
          onClick={() => { void switchConnection("wifi-ap"); }}
        />
        <PanelItem
          label="🏠 Wi‑Fi 基础设施（STA）"
          active={connectionType === "wifi-sta"}
          disabled={switchingConnection}
          onClick={() => { void switchConnection("wifi-sta"); }}
        />
        <PanelItem
          label="🔌 USB（MTP/PTP）"
          active={connectionType === "usb"}
          disabled={switchingConnection}
          onClick={() => { void switchConnection("usb"); }}
        />
        {switchingConnection && <div className="mt-2 px-1 text-[10px] text-nps-accent">正在释放上一个连接…</div>}
        <div className="mt-2 px-1 text-[10px] leading-relaxed text-nps-text-dim">
          {connectionType === "wifi-ap" && "电脑连接 NIKON_… 相机热点；相机须停留在“连接到计算机”的等待连接画面。“无 Internet”正常。"}
          {connectionType === "wifi-sta" && "相机和电脑连接同一路由器；先用 Wireless Transmitter Utility 配对，再填写相机 IPv4。"}
          {connectionType === "usb" && "相机菜单中的 USB 模式须设为 MTP/PTP。"}
        </div>
      </div>

      <div className="mb-4">
        <SectionTitle>{t("transfer.cameraInfo")}</SectionTitle>
        {connected ? (
          <>
            <div className="flex items-center gap-2 p-2.5 bg-nps-tertiary rounded mb-2">
              <div className="w-2.5 h-2.5 rounded-full bg-nps-success animate-pulse" />
              <div>
                <div className="text-[13px] font-semibold">{cameraModel}</div>
                <div className="text-[11px] text-nps-text-dim">{connectionDetail}</div>
              </div>
            </div>
            <div className="text-xs space-y-1 px-1">
              <div className="flex justify-between"><span className="text-nps-text-dim">{t("transfer.photoCount")}</span><span>{cameraPhotos.length}</span></div>
            </div>
          </>
        ) : (
          <div className="flex items-center gap-2 p-2.5 bg-nps-tertiary rounded">
            <div className="w-2.5 h-2.5 rounded-full bg-nps-text-dim" />
            <div className="text-[12px] text-nps-text-dim">{t("transfer.notConnected")}</div>
          </div>
        )}
        {connectionError && <div className="mt-2 text-[10px] leading-relaxed text-nps-danger">{connectionError}</div>}
      </div>

      <div className="mb-4">
        <SectionTitle>{t("transfer.downloadSettings")}</SectionTitle>
        <PanelItem
          label={`📷 ${t("transfer.original")}（完整复制）`}
          active
          onClick={() => setQuality("original")}
        />
        <div className="mt-2 px-1 text-[10px] leading-relaxed text-nps-text-dim">下载会按相机中的原始文件名和完整字节复制，不转换、不压缩。</div>
      </div>

      <div>
        <SectionTitle>{t("transfer.nikonCloud")}</SectionTitle>
        <PanelItem label={`☁️ ${t("transfer.downloadFromCloud")}`} onClick={() => toast(t("transfer.cloudMsg"), "info")} />
        <PanelItem label={`🔄 ${t("transfer.syncStatus")}`} onClick={() => toast(t("transfer.syncedMsg"), "success")} />
      </div>
    </>
  );
}
