import { describe, expect, it } from "vitest";
import { ERROR_CODES } from "./error-codes";

describe("错误码契约", () => {
  it("使用稳定且不重复的编码", () => {
    expect(new Set(ERROR_CODES).size).toBe(ERROR_CODES.length);
    expect(ERROR_CODES.every((code) => /^E_[A-Z0-9_]+$/.test(code))).toBe(true);
  });
});
