import { describe, expect, it } from "vitest";
import { addView, parseViews, removeView, serializeViews } from "./views";

describe("saved views", () => {
  it("parses, ignores junk and trims names", () => {
    const views = parseViews(
      JSON.stringify([
        { name: " 餐饮 ", query: "acct:expenses:food" },
        { name: "", query: "acct:x" },
        { query: "acct:y" },
        "nope",
      ]),
    );
    expect(views).toEqual([{ name: "餐饮", query: "acct:expenses:food", order: undefined }]);
  });

  it("survives malformed JSON", () => {
    expect(parseViews("{oops")).toEqual([]);
    expect(parseViews(null)).toEqual([]);
    expect(parseViews(JSON.stringify({ name: "x" }))).toEqual([]);
  });

  it("replaces a view with the same name instead of duplicating it", () => {
    const first = addView([], { name: "餐饮", query: "acct:expenses:food" });
    const second = addView(first, { name: "餐饮", query: "acct:expenses:food date:2026-10" });
    expect(second).toHaveLength(1);
    expect(second[0].query).toBe("acct:expenses:food date:2026-10");
  });

  it("removes by name and round-trips through JSON", () => {
    const views = addView(addView([], { name: "a", query: "acct:a" }), { name: "b", query: "acct:b" });
    const restored = parseViews(serializeViews(views));
    expect(restored.map((view) => view.name)).toEqual(["a", "b"]);
    expect(removeView(restored, "a").map((view) => view.name)).toEqual(["b"]);
  });
});
