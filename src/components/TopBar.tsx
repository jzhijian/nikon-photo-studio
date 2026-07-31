import { useTranslation } from "react-i18next";
import { useAppStore, ModuleId } from "../stores/appStore";
import { useLibraryStore } from "../stores/libraryStore";
import { toast } from "../stores/toastStore";
import i18n from "../i18n";

const modules: { id: ModuleId; key: string }[] = [
  { id: "library", key: "nav.library" },
  { id: "organize", key: "nav.organize" },
  { id: "presets", key: "nav.presets" },
  { id: "transfer", key: "nav.transfer" },
];

export default function TopBar() {
  const { t } = useTranslation();
  const {
    activeModule,
    setModule,
    language,
    setLanguage,
    theme,
    toggleTheme,
    leftCollapsed,
    toggleLeftCollapsed,
  } = useAppStore();
  const importFromDialog = useLibraryStore((s) => s.importFromDialog);
  const importing = useLibraryStore((s) => s.importing);
  const selectedPath = useLibraryStore((s) => s.selectedPath);
  const developPath = useLibraryStore((s) => s.developPath);
  const openExport = useAppStore((s) => s.openExport);

  const toggleLang = () => {
    const next = language === "zh-CN" ? "en-US" : "zh-CN";
    setLanguage(next);
    i18n.changeLanguage(next);
  };

  const doImport = async () => {
    const n = await importFromDialog();
    if (n > 0) {
      toast(t("library.importedMsg", { count: n }), "success");
      setModule("library");
    } else {
      toast(t("library.noNewPhotos"), "info");
    }
  };

  return (
    <header className="h-12 bg-nps-secondary border-b border-nps-border flex items-center px-4 gap-2 shrink-0">
      <button
        onClick={toggleLeftCollapsed}
        className="px-2 py-1 rounded border border-nps-border bg-nps-tertiary text-sm hover:bg-nps-hover active:scale-95 transition-all mr-1"
        title={leftCollapsed ? t("panel.expand") : t("panel.collapse")}
      >
        {leftCollapsed ? "⟩" : "⟨"}
      </button>

      <div className="font-bold text-sm text-nps-accent mr-4 whitespace-nowrap">
        NPS <span className="text-nps-text font-normal">| Nikon Photo Studio</span>
      </div>

      <nav className="flex gap-0.5">
        {modules.map((m) => (
          <button
            key={m.id}
            onClick={() => setModule(m.id)}
            className={`px-3.5 py-1.5 rounded text-[13px] transition-all active:scale-[0.97] ${
              activeModule === m.id
                ? "bg-nps-accent text-nps-primary font-semibold"
                : "text-nps-text-dim hover:text-nps-text hover:bg-nps-tertiary"
            }`}
          >
            {t(m.key)}
          </button>
        ))}
      </nav>

      <div className="ml-auto flex items-center gap-2">
        <button
          onClick={toggleTheme}
          className="px-2 py-1 rounded border border-nps-border bg-nps-tertiary text-sm hover:bg-nps-hover active:scale-[0.95] transition-all"
          title={theme === "dark" ? t("theme.switchLight") : t("theme.switchDark")}
        >
          {theme === "dark" ? "☀️" : "🌙"}
        </button>
        <button
          onClick={toggleLang}
          className="px-2 py-0.5 rounded border border-nps-border bg-nps-tertiary text-[11px] hover:bg-nps-hover active:scale-[0.95] transition-all"
        >
          {language === "zh-CN" ? "EN" : "中文"}
        </button>
        <button
          onClick={doImport}
          disabled={importing}
          className="px-3 py-1 rounded border border-nps-border bg-nps-tertiary text-xs hover:bg-nps-hover active:scale-[0.97] transition-all disabled:opacity-60"
        >
          {importing ? t("library.importing") : t("nav.import")}
        </button>
        <button
          onClick={() => {
            if (developPath || selectedPath) openExport();
            else toast(language === "zh-CN" ? "请先选择一张照片" : "Select a photo first", "info");
          }}
          className="px-3 py-1 rounded bg-nps-accent text-nps-primary text-xs font-semibold hover:bg-nps-accent-hover active:scale-[0.97] transition-all"
        >
          {t("nav.export")}
        </button>
        <button
          onClick={() => toast(t("nav.settingsMsg"), "info")}
          className="px-2 py-1 rounded border border-nps-border bg-nps-tertiary text-xs hover:bg-nps-hover active:scale-[0.95] transition-all"
        >
          {"⚙️"}
        </button>
      </div>
    </header>
  );
}
