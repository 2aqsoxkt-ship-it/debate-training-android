import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog, Drawer } from "./Overlay";

describe("overlay primitives", () => {
  it("confirms explicitly and closes with Escape", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        open
        title="确认中止比赛"
        description="当前进度会被保存为已中止。"
        confirmLabel="确认中止"
        onClose={onClose}
        onConfirm={onConfirm}
      />,
    );

    const confirm = screen.getByRole("button", { name: "确认中止" });
    expect(screen.getByRole("button", { name: "取消" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("labels a drawer and exposes a close action", () => {
    const onClose = vi.fn();
    render(
      <Drawer open title="比赛资料" onClose={onClose}>
        <p>策略与材料</p>
      </Drawer>,
    );

    expect(screen.getByRole("dialog", { name: "比赛资料" })).toHaveTextContent(
      "策略与材料",
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭比赛资料" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps keyboard navigation inside the dialog in both directions", () => {
    render(
      <>
        <button type="button">页面操作</button>
        <ConfirmDialog
          open
          title="删除档案"
          description="这项操作无法撤销。"
          confirmLabel="确认删除"
          onClose={vi.fn()}
          onConfirm={vi.fn()}
        />
      </>,
    );
    const dialog = screen.getByRole("dialog");
    const cancel = within(dialog).getByRole("button", { name: "取消" });
    const confirm = within(dialog).getByRole("button", { name: "确认删除" });
    confirm.focus();
    fireEvent.keyDown(confirm, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
  });

  it("returns focus to the actual opener after closing a drawer", () => {
    function Example() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            查看帮助
          </button>
          <Drawer open={open} title="使用说明" onClose={() => setOpen(false)}>
            <button type="button">帮助操作</button>
          </Drawer>
        </>
      );
    }
    render(<Example />);
    const opener = screen.getByRole("button", { name: "查看帮助" });
    opener.focus();
    fireEvent.click(opener);
    const close = screen.getByRole("button", { name: "关闭使用说明" });
    const last = screen.getByRole("button", { name: "帮助操作" });
    expect(close).toHaveFocus();
    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(close).toHaveFocus();
    fireEvent.keyDown(close, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
  });
});
