import { useEffect } from "react";
import { useAppStore } from "./stores/appStore";
import { useLibraryStore } from "./stores/libraryStore";
import TopBar from "./components/TopBar";
import LeftPanel from "./components/LeftPanel";
import RightPanel from "./components/RightPanel";
import FilmStrip from "./components/FilmStrip";
import ToastHost from "./components/Toast";
import Lightbox from "./components/Lightbox";
import LibraryView from "./modules/library/LibraryView";
import OrganizeView from "./modules/organize/OrganizeView";
import PresetsView from "./modules/presets/PresetsView";
import TransferView from "./modules/transfer/TransferView";

/** Left sidebar with a draggable resize handle and collapse support. */
function LeftSidebar() {
  const leftWidth = useAppStore((s) => s.leftWidth);
  const leftCollapsed = useAppStore((s) => s.leftCollapsed);
  const setLeftWidth = useAppStore((s) => s.setLeftWidth);

  const startDrag = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = leftWidth;
    const onMove = (ev: PointerEvent) => setLeftWidth(startW + (ev.clientX - startX));
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.cursor = "";
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    document.body.style.cursor = "col-resize";
  };

  return (
    <div
      className="relative flex shrink-0 transition-[width] duration-150 ease-out"
      style={{ width: leftCollapsed ? 0 : leftWidth }}
    >
      <div className="w-full overflow-hidden">
        <LeftPanel />
      </div>
      {!leftCollapsed && (
        <div
          onPointerDown={startDrag}
          className="absolute top-0 right-0 w-1.5 h-full cursor-col-resize hover:bg-nps-accent/60 active:bg-nps-accent transition-colors z-10"
          title="Drag to resize panel"
        />
      )}
    </div>
  );
}

function App() {
  const activeModule = useAppStore((s) => s.activeModule);
  const setDevelopPath = useLibraryStore((s) => s.setDevelopPath);
  // Library and Organize both own an in-place develop viewer. Presets and
  // Transfer do not, so only those modules close it.
  useEffect(() => {
    if (activeModule !== "library" && activeModule !== "organize") setDevelopPath(null);
  }, [activeModule, setDevelopPath]);

  return (
    <div className="flex flex-col h-screen bg-nps-primary text-nps-text">
      <TopBar />
      <div className="flex flex-1 overflow-hidden">
        <LeftSidebar />
        <main className="flex-1 overflow-hidden">
          {activeModule === "library" && <LibraryView />}
          {activeModule === "organize" && <OrganizeView />}
          {activeModule === "presets" && <PresetsView />}
          {activeModule === "transfer" && <TransferView />}
        </main>
        <RightPanel />
      </div>
      {(activeModule === "library" || activeModule === "organize") && <FilmStrip />}
      <Lightbox />
      <ToastHost />
    </div>
  );
}

export default App;
