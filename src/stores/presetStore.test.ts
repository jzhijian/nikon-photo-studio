import { beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({invoke: vi.fn()}));
vi.mock("@tauri-apps/plugin-dialog", () => ({open: vi.fn()}));
vi.mock("./toastStore", () => ({toast: vi.fn()}));
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { usePresetStore } from "./presetStore";
import { toast } from "./toastStore";

const initialPresets = usePresetStore.getState().presets;

describe("built-in preset provenance", () => {
  it("identifies the built-in adjustments without claiming cloud or imported files", () => {
    expect(initialPresets.map(preset => preset.id)).toEqual(Array.from({length: 12}, (_, i) => i + 1));
    expect(initialPresets.every(preset => preset.group === "builtin" && preset.format === "app")).toBe(true);
    expect(initialPresets.every(preset => !preset.sourcePath && !/nikon/i.test(preset.name))).toBe(true);
    expect(initialPresets.filter(preset => preset.group === "imported")).toHaveLength(0);
    expect(initialPresets.find(preset => preset.id === 5)?.params).toMatchObject({contrast: 25, saturation: 30, sharpening: 20});
    expect(initialPresets.find(preset => preset.id === 6)?.params).toMatchObject({contrast: -5, saturation: -5, sharpening: 10});
  });
});

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
  it("offers supported imports and retains existing user presets when appending a file", async () => {
    const existing = {...initialPresets[0], id: 300, name: "My existing file", group: "imported" as const, format: "np3" as const, sourcePath: "C:/looks/existing.np3"};
    usePresetStore.setState({presets: [existing]});
    vi.mocked(open).mockResolvedValue(["C:/looks/new.xmp"]);
    vi.mocked(invoke).mockResolvedValue({name: "New file", format: "XMP", params: {contrast: 12}});
    expect(await usePresetStore.getState().importPresetFiles()).toBe(1);
    const offered = vi.mocked(open).mock.calls[0][0]?.filters?.[0].extensions;
    expect(offered).toEqual(["xmp", "cube", "lrpreset", "3dl", "costyle"]);
    expect(offered).not.toContain("np3");
    expect(offered).not.toContain("ncp");
    expect(usePresetStore.getState().presets[0]).toEqual(existing);
    expect(usePresetStore.getState().presets[1]).toMatchObject({name: "New file", group: "imported", format: "xmp", sourcePath: "C:/looks/new.xmp", params: {contrast: 12}});
  });
});
