import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HomePage } from "./HomePage";

describe("启动页由用户推进", () => {
  afterEach(() => vi.useRealTimers());

  it("等待不会自动跳转，点击进入后才打开工作台", async () => {
    vi.useFakeTimers();
    render(
      <MemoryRouter initialEntries={["/"]}>
        <Routes>
          <Route path="/" element={<HomePage />} />
          <Route path="/workspace" element={<h1>赛前工作台</h1>} />
        </Routes>
      </MemoryRouter>,
    );
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(screen.queryByRole("heading", { name: "赛前工作台" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "辩论训练智能体" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /进入/ }));
    expect(
      screen.getByRole("heading", { name: "赛前工作台" }),
    ).toBeInTheDocument();
  });
});
