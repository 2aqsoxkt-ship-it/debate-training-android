/// <reference types="node" />
// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("V3 light arena stylesheet", () => {
  it("covers the actual question, selection and history classes with readable paper colors", () => {
    const css = readFileSync("src/styles/global.css", "utf8");
    const paper = css.slice(css.indexOf("/* V3 interactive panels"));
    for (const selector of [
      ".cross-question",
      ".selection-summary",
      ".cross-history",
    ]) {
      expect(paper).toMatch(
        new RegExp(`\\.match-page \\${selector}(?:\\s*[,\\{])`),
      );
    }
    expect(paper).toMatch(
      /\.match-page \.cross-question\s*\{[^}]*color:\s*#302d27/,
    );
  });

  it("places the light theme after legacy arena rules", () => {
    const css = readFileSync("src/styles/global.css", "utf8");
    const legacy = css.indexOf("/* Match console */");
    const paper = css.indexOf("/* V3 paper referee desk:");
    expect(legacy).toBeGreaterThan(0);
    expect(paper).toBeGreaterThan(legacy);
    expect(css.slice(paper)).toContain("#ece7dc");
    expect(css.slice(paper)).toContain(".match-page .cross-exam-panel");
    expect(css.slice(paper)).not.toContain("background: #111a20");
  });
});
