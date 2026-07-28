import { describe, it, expect, vi, beforeEach } from "vitest";

// Mock Tauri's invoke before the module under test is imported.
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  // The batch path returns cached file paths that the component converts to
  // asset-protocol URLs; identity is enough to exercise the coalescing logic.
  convertFileSrc: (p: string) => p,
}));

import { fetchThumb } from "./Thumbnail";

function defaultImpl(cmd: string, args: { paths?: string[]; path?: string }) {
  if (cmd === "make_thumbnails_batch") {
    return Promise.resolve(
      (args.paths ?? []).map((p) => ({ path: p, url: `data:image/jpeg;base64,FAKE_${p}` }))
    );
  }
  if (cmd === "make_thumbnail") {
    return Promise.resolve(`data:image/jpeg;base64,SINGLE_${args.path}`);
  }
  return Promise.resolve("");
}

// Let the setTimeout(0)-scheduled, async flush run to completion.
const tick = () => new Promise((r) => setTimeout(r, 25));

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation(defaultImpl);
});

describe("thumbnail request coalescing", () => {
  it("coalesces many same-tick requests into a single batch IPC call", async () => {
    const paths = Array.from({ length: 50 }, (_, i) => `C:/photos/coalesce_${i}.nef`);
    const promises = paths.map((p) => fetchThumb(p, 360));
    await tick();
    const results = await Promise.all(promises);

    const batchCalls = invokeMock.mock.calls.filter((c) => c[0] === "make_thumbnails_batch");
    const singleCalls = invokeMock.mock.calls.filter((c) => c[0] === "make_thumbnail");
    expect(batchCalls.length).toBe(1);
    expect(batchCalls[0][1].paths.length).toBe(50);
    expect(singleCalls.length).toBe(0);
    expect(results[0]).toContain("FAKE_");
    expect(results[49]).toContain("coalesce_49.nef");
  });

  it("chunks large request sets (456 photos -> ceil(456/64) batch calls)", async () => {
    const paths = Array.from({ length: 456 }, (_, i) => `C:/photos/big_${i}.nef`);
    const promises = paths.map((p) => fetchThumb(p, 360));
    await tick();
    await Promise.all(promises);

    const batchCalls = invokeMock.mock.calls.filter((c) => c[0] === "make_thumbnails_batch");
    // 456 / 64 = 7.125 -> 8 chunks
    expect(batchCalls.length).toBe(8);
    const total = batchCalls.reduce((n, c) => n + c[1].paths.length, 0);
    expect(total).toBe(456);
  });

  it("serves repeat requests from the JS cache without any new IPC", async () => {
    const p = "C:/photos/cached_repeat.nef";
    await fetchThumb(p, 360);
    await tick();
    invokeMock.mockClear();
    const url = await fetchThumb(p, 360);
    expect(url).toContain("FAKE_");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("dedups concurrent requests for the same path onto one inflight promise", async () => {
    const p = "C:/photos/dup.nef";
    const a = fetchThumb(p, 360);
    const b = fetchThumb(p, 360);
    await tick();
    const [ra, rb] = await Promise.all([a, b]);
    expect(ra).toBe(rb);
    const batchCalls = invokeMock.mock.calls.filter((c) => c[0] === "make_thumbnails_batch");
    expect(batchCalls.length).toBe(1);
    expect(batchCalls[0][1].paths.filter((x: string) => x === p).length).toBe(1);
  });

  it("falls back to a single render when the batch call fails", async () => {
    invokeMock.mockImplementation((cmd: string, args: { path?: string }) => {
      if (cmd === "make_thumbnails_batch") return Promise.reject(new Error("batch boom"));
      if (cmd === "make_thumbnail") return Promise.resolve(`data:image/jpeg;base64,SINGLE_${args.path}`);
      return Promise.resolve("");
    });
    const p = "C:/photos/fallback.nef";
    const prom = fetchThumb(p, 360);
    await tick();
    const url = await prom;
    expect(url).toContain("SINGLE_");
    const singleCalls = invokeMock.mock.calls.filter((c) => c[0] === "make_thumbnail");
    expect(singleCalls.length).toBe(1);
  });
});