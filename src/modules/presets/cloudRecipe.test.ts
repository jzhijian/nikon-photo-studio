import { describe, expect, it } from "vitest";
import { assessCloudRecipe, normalizeCloudRecipe, parseRecipeId } from "./cloudRecipe";
import fixture from "../../../tests/fixtures/cloud-mate-film.json";

describe("real Nikon public recipe contract", () => {
  it("keeps China and international recipe namespaces separate", () => {
    const id = fixture.recipe.id;
    const cn = `https://imagingcloud.nikon.com.cn/recipe/release/detail/?id=${id}`;
    expect(parseRecipeId(cn, "china")).toBe(id);
    expect(() => parseRecipeId(cn, "international")).toThrow();
    expect(() => parseRecipeId(`https://imagingcloud.nikon.com/recipe/release/detail/?id=${id}`, "china")).toThrow();
    expect(normalizeCloudRecipe(fixture, "china").sourceUrl).toBe(cn);
  });
  it("preserves fractional native values and distinguishes missing curve data", () => {
    const recipe = normalizeCloudRecipe(fixture);
    expect(recipe.name).toBe("Mate Film by Choi matae");
    expect(recipe.pictureControl.sharpening).toBe("+0.75");
    expect(recipe.pictureControl.contrast).toBe("gamma");
    const assessment = assessCloudRecipe(recipe);
    expect(assessment.kind).toBe("flexible-color");
    expect(assessment.canWriteZ30).toBe(false);
    expect(assessment.canRenderFaithfully).toBe(false);
    expect(assessment.missing).toContain("curve-data");
  });
  it("does not mistake shooting metadata for instructions or an executable preset", () => {
    const recipe = normalizeCloudRecipe(fixture);
    expect(recipe.captureReference.whiteBalance).toBe("AUTO1");
    expect(assessCloudRecipe(recipe).canWriteZ30).toBe(false);
  });
  it("does not declare a traditional metadata-only recipe ready to write", () => {
    const recipe = normalizeCloudRecipe({...fixture, recipe: {...fixture.recipe,
      pictureControl: {baseName: "STANDARD", sharpening: "3.00"}}});
    expect(assessCloudRecipe(recipe).kind).toBe("traditional");
    expect(assessCloudRecipe(recipe).canWriteZ30).toBe(false);
  });
  it("rejects malformed responses and service errors instead of returning neutral params", () => {
    for (const input of [{}, {error_code: "AUTH_REQUIRED"}, {recipe: {id: "x"}}]) {
      expect(() => normalizeCloudRecipe(input)).toThrow();
    }
  });
  it("only accepts public recipe IDs and Nikon detail links", () => {
    const id = fixture.recipe.id;
    expect(parseRecipeId(id)).toBe(id);
    expect(parseRecipeId(`https://imagingcloud.nikon.com/recipe/release/detail/?id=${id}`)).toBe(id);
    for (const input of ["../settings", `https://evil.test/?id=${id}`, "file:///C:/secret", ""]) {
      expect(() => parseRecipeId(input)).toThrow();
    }
  });
});
