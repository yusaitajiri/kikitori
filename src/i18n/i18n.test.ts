import { describe, expect, it } from "vitest";
import en from "./en.json";
import ja from "./ja.json";
import i18n from "./index";

describe("i18n", () => {
  it("ja and en have the same keys", () => {
    expect(Object.keys(ja).sort()).toEqual(Object.keys(en).sort());
  });

  it("core strings match the spec table", () => {
    expect(ja.start).toBe("開始");
    expect(ja.copyForClaude).toBe("Claude用にコピー");
    expect(ja.finishing).toBe("仕上げ中…");
    expect(ja.conversation).toBe("会話として録音");
  });

  it("interpolates single-brace placeholders", () => {
    expect(i18n.t("lag", { n: 12, lng: "ja" })).toBe("遅れ 12秒");
    expect(i18n.t("lag", { n: 12, lng: "en" })).toBe("12s behind");
    expect(i18n.t("shotAdded", { time: "15:16:03", lng: "ja" })).toBe("スクショを挿入しました 15:16:03");
  });
});
