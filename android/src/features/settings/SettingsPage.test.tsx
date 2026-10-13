import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

describe("模型设置页", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("无密钥时不显示已连接，并且响应里没有密钥字段", async () => {
    invoke.mockResolvedValue({
      modelId: "deepseek-flash",
      baseUrl: "https://api.deepseek.com",
      reasoningLevel: "high",
      connectionStatus: "not_configured",
      lastTestedAt: "",
      lastErrorCategory: "",
      hasApiKey: false,
    });
    render(
      <MemoryRouter>
        <SettingsPage />
      </MemoryRouter>,
    );
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("未配置密钥");
    });
    expect(screen.queryByText("已连接")).not.toBeInTheDocument();
    expect(JSON.stringify(invoke.mock.results[0].value)).not.toMatch(/sk-/);
  });

  it("余额不足时给出充值后重试的明确提示", async () => {
    invoke.mockResolvedValue({
      modelId: "deepseek-flash",
      baseUrl: "https://api.deepseek.com",
      reasoningLevel: "high",
      connectionStatus: "insufficient_balance",
      lastTestedAt: "2026-10-03T10:00:00Z",
      lastErrorCategory: "insufficient_balance",
      hasApiKey: true,
    });
    render(
      <MemoryRouter>
        <SettingsPage />
      </MemoryRouter>,
    );
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("DeepSeek 余额不足");
    });
    expect(screen.getByText(/充值，再重新测试连接/)).toBeInTheDocument();
  });
});
