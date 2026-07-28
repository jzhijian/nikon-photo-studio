import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../components/Thumbnail", () => ({ prefetchThumbs: vi.fn() }));

import { selectFiltered, type Photo } from "./libraryStore";

const photos: Photo[] = Array.from({ length: 6 }, (_, rating) => ({
  filePath: `C:/photos/${rating}.NEF`,
  filename: `${rating}.NEF`,
  format: "NEF",
  fileSize: 1,
  width: 1,
  height: 1,
  previewable: true,
  folder: "C:/photos",
  category: "other",
  classificationConfidence: -1,
  hasPerson: false,
  blurScore: -1,
  marked: false,
  rating,
  selected: false,
}));

const filter = (ratingOperator: "all" | "gte" | "lte", ratingValue: number) =>
  selectFiltered({ photos, folder: null, filter: "all", ratingOperator, ratingValue });

describe("rating filter", () => {
  it("does not filter when the comparison is disabled", () => {
    expect(filter("all", 3)).toHaveLength(6);
  });

  it("keeps the selected rating and higher ratings", () => {
    expect(filter("gte", 3).map((photo) => photo.rating)).toEqual([3, 4, 5]);
  });

  it("keeps unrated photos and ratings up to the selected rating", () => {
    expect(filter("lte", 3).map((photo) => photo.rating)).toEqual([0, 1, 2, 3]);
  });
});
