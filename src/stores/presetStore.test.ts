import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({invoke: vi.fn()}));
vi.mock("@tauri-apps/plugin-dialog", () => ({open: vi.fn()}));
vi.mock("./toastStore", () => ({toast: vi.fn()}));
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { usePresetStore } from "./presetStore";
import { toast } from "./toastStore";

describe("preset import failure reporting", () => {
  beforeEach(() => {vi.clearAllMocks(); usePresetStore.setState({presets: []});});
  it("does not count a failed Nikon decode as a successfully imported neutral preset", async () => {
    vi.mocked(open).mockResolvedValue(["C:/looks/cloud.np3"]);
    vi.mocked(invoke).mockRejectedValue("完整 Nikon 解析尚未验证");
    expect(await usePresetStore.getState().importPresetFiles()).toBe(0);
    expect(usePresetStore.getState().presets).toEqual([]);
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("cloud.np3"), "error");
  });
  it("continues importing a valid file after another file fails", async () => {
    vi.mocked(open).mockResolvedValue(["C:/looks/bad.np3", "C:/looks/good.xmp"]);
    vi.mocked(invoke).mockRejectedValueOnce("unsupported")
      .mockResolvedValueOnce({name: "Good", format: "XMP", params: {contrast: 12}});
    expect(await usePresetStore.getState().importPresetFiles()).toBe(1);
    expect(usePresetStore.getState().presets.map(p => p.name)).toEqual(["Good"]);
  });
});
