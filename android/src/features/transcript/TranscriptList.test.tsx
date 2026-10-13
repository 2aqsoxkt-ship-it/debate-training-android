import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TranscriptList } from "./TranscriptList";

describe("记录列表", () => {
  it("一万条记录只渲染当前窗口", () => {
    const items = Array.from({ length: 10_000 }, (_, index) => ({
      id: String(index),
      text: `记录${index}`,
    }));
    const started = performance.now();
    render(<TranscriptList items={items} start={40} size={20} />);
    const elapsed = performance.now() - started;
    expect(screen.getAllByRole("listitem")).toHaveLength(20);
    expect(screen.getByText("记录40")).toBeInTheDocument();
    expect(screen.queryByText("记录0")).not.toBeInTheDocument();
    expect(screen.queryByText("记录9999")).not.toBeInTheDocument();
    expect(elapsed).toBeLessThan(1_000);
  });
});
