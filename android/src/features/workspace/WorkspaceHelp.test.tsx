import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WorkspaceHelp } from "./WorkspaceHelp";

describe("工作台内使用说明", () => {
  it("说明团队字数、模式和交互，不提供跳回启动页的入口", () => {
    const onClose = vi.fn();
    render(<WorkspaceHelp open onClose={onClose} />);
    const help = screen.getByRole("dialog", { name: "使用说明" });
    expect(help).toHaveTextContent("各 4320 字");
    expect(help).toHaveTextContent("各 960 字，全队共享");
    expect(help).toHaveTextContent("自由辩不能打断");
    expect(help).toHaveTextContent("只有质询方能按赛制打断回答");
    expect(help).toHaveTextContent("未提交内容不是磁盘存档");
    expect(screen.queryByRole("link")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "关闭使用说明" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("关闭时不渲染说明", () => {
    render(<WorkspaceHelp open={false} onClose={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
