import { useAppStore } from "../stores/appStore";
import { useLibraryStore, selectFolders, selectFiltered } from "../stores/libraryStore";
import { useOrganizeStore } from "../stores/organizeStore";
import { usePresetStore, PRESET_CATEGORIES } from "../stores/presetStore";
import { useTransferStore } from "../stores/transferStore";
import { toast } from "../stores/toastStore";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

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
}: {
  label: string;
  badge?: string;
  active?: boolean;
  onClick?: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={`w-full flex items-center gap-2 px-2.5 py-1.5 rounded text-[13px] cursor-pointer transition-all text-left active:scale-[0.98] ${
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
  const { category, threshold, sensitivity, useGpu, setCategory, setThreshold, setSensitivity, setUseGpu } = useOrganizeStore();
  const photos = useLibraryStore((s) => s.photos);
  const removeMany = useLibraryStore((s) => s.removeMany);
  const runBlurDetection = useLibraryStore((s) => s.runBlurDetection);
  const runClassification = useLibraryStore((s) => s.runClassification);
  const detecting = useLibraryStore((s) => s.detecting);

  const blurryCount = photos.filter((p) => p.blurScore >= 0 && p.blurScore < threshold).length;

  const doDetect = async () => {
    if (photos.length === 0) return;
    toast(t("organize.detectingMsg"), "info");
    const n = await runBlurDetection(sensitivity, useGpu);
    toast(t("organize.detectedMsg", { count: n }), "success");
  };

  const doClassify = async () => {
    if (photos.length === 0) return;
    toast(t("organize.classifyingMsg"), "info");
    const n = await runClassification();
    toast(t("organize.classifiedMsg", { count: n }), "success");
  };

  return (
    <>
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
        <PanelItem
          label={`🌫️ ${t("organize.blurry")}`}
          badge={String(blurryCount)}
          active={category === "blurry"}
          onClick={() => setCategory("blurry")}
        />
        <PanelItem
          label={`👤 ${t("organize.portrait")}`}
          badge={String(photos.filter((p) => p.category === "portrait").length)}
          active={category === "portrait"}
          onClick={() => setCategory("portrait")}
        />
        <PanelItem
          label={`🏞️ ${t("organize.landscape")}`}
          badge={String(photos.filter((p) => p.category === "landscape").length)}
          active={category === "landscape"}
          onClick={() => setCategory("landscape")}
        />
        <PanelItem
          label={`📦 ${t("organize.others")}`}
          badge={String(photos.filter((p) => p.category === "other").length)}
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

  const cloudCount = presets.filter((p) => p.group === "cloud").length;
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
          label={`☁️ ${t("presets.nikonCloud")}`}
          badge={String(cloudCount)}
          active={group === "cloud"}
          onClick={() => setGroup("cloud")}
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
        {(["np3", "ncp", "xmp", "cube", "lrpreset", "3dl", "costyle"] as const).map((f) => (
          <PanelItem
            key={f}
            label={f === "np3" ? "NP3" : f === "ncp" ? "NCP" : f === "3dl" ? ".3dl LUT" : f === "costyle" ? "Capture One" : `.${f}`}
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
        支持格式: XMP, .cube, NP3, NCP, .lrpreset, .3dl, .costyle
      </div>
    </>
  );
}
function TransferLeft() {
  const { t } = useTranslation();
  const { connectionType, quality, connected, setConnectionType, setQuality } = useTransferStore();

  return (
    <>
      <div className="mb-4">
        <SectionTitle>{t("transfer.connection")}</SectionTitle>
        <PanelItem
          label="📱 Wi-Fi AP"
          active={connectionType === "wifi-ap"}
          onClick={() => {
            setConnectionType("wifi-ap");
            toast("Wi-Fi AP · Nikon Z30", "success");
          }}
        />
        <PanelItem
          label="🏠 Wi-Fi STA"
          active={connectionType === "wifi-sta"}
          onClick={() => {
            setConnectionType("wifi-sta");
            toast("Wi-Fi STA · Nikon Z5II", "success");
          }}
        />
        <PanelItem
          label="🔌 USB"
          active={connectionType === "usb"}
          onClick={() => {
            setConnectionType("usb");
            toast("USB (MTP/PTP)", "success");
          }}
        />
      </div>

      <div className="mb-4">
        <SectionTitle>{t("transfer.cameraInfo")}</SectionTitle>
        {connected ? (
          <>
            <div className="flex items-center gap-2 p-2.5 bg-nps-tertiary rounded mb-2">
              <div className="w-2.5 h-2.5 rounded-full bg-nps-success animate-pulse" />
              <div>
                <div className="text-[13px] font-semibold">Nikon Z30</div>
                <div className="text-[11px] text-nps-text-dim">{connectionType === "usb" ? "USB (MTP/PTP)" : "192.168.1.1:15740"}</div>
              </div>
            </div>
            <div className="text-xs space-y-1 px-1">
              <div className="flex justify-between"><span className="text-nps-text-dim">{t("transfer.battery")}</span><span>78%</span></div>
              <div className="flex justify-between"><span className="text-nps-text-dim">{t("transfer.storage")}</span><span>128/256 GB</span></div>
              <div className="flex justify-between"><span className="text-nps-text-dim">{t("transfer.photoCount")}</span><span>48</span></div>
            </div>
          </>
        ) : (
          <div className="flex items-center gap-2 p-2.5 bg-nps-tertiary rounded">
            <div className="w-2.5 h-2.5 rounded-full bg-nps-text-dim" />
            <div className="text-[12px] text-nps-text-dim">{t("transfer.notConnected")}</div>
          </div>
        )}
      </div>

      <div className="mb-4">
        <SectionTitle>{t("transfer.downloadSettings")}</SectionTitle>
        {(["original", "8mp", "2mp"] as const).map((q) => (
          <PanelItem
            key={q}
            label={q === "original" ? `📷 ${t("transfer.original")}` : q === "8mp" ? "🖼️ 8MP JPEG" : "🖼️ 2MP JPEG"}
            active={quality === q}
            onClick={() => {
              setQuality(q);
              toast(`${t("transfer.quality")}: ${q === "original" ? t("transfer.original") : q.toUpperCase()}`);
            }}
          />
        ))}
      </div>

      <div>
        <SectionTitle>{t("transfer.nikonCloud")}</SectionTitle>
        <PanelItem label={`☁️ ${t("transfer.downloadFromCloud")}`} onClick={() => toast(t("transfer.cloudMsg"), "info")} />
        <PanelItem label={`🔄 ${t("transfer.syncStatus")}`} onClick={() => toast(t("transfer.syncedMsg"), "success")} />
      </div>
    </>
  );
}
