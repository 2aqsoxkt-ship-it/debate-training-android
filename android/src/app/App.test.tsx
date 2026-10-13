import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { ErrorBoundary } from "./ErrorBoundary";

describe("应用壳", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("展示安卓本地优先说明，不要求 Windows 运行环境", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { name: "辩论训练智能体" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/默认只保存在本机/)).toBeInTheDocument();
    expect(screen.getByText(/安卓手机与平板/)).toBeInTheDocument();
    expect(screen.queryByText(/WebView2 Runtime/)).not.toBeInTheDocument();
  });

  it("错误边界不展示原始异常", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});

    function Boom(): null {
      throw new Error("api_key=sk-shouldnotshow1234");
    }

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    expect(
      screen.getByRole("heading", { name: "应用出现问题" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/sk-shouldnotshow/)).not.toBeInTheDocument();
  });
});
