import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MobileViewport } from "./MobileViewport";

const originalViewport = Object.getOwnPropertyDescriptor(
  window,
  "visualViewport",
);

afterEach(() => {
  if (originalViewport)
    Object.defineProperty(window, "visualViewport", originalViewport);
  else Reflect.deleteProperty(window, "visualViewport");
  vi.restoreAllMocks();
});

describe("移动窗口适配", () => {
  it("旋转和键盘变化保留同一个输入节点、草稿和组件状态", () => {
    const viewport = Object.assign(new EventTarget(), {
      height: 768,
      offsetTop: 0,
      scale: 1,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: viewport,
    });
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
      callback(0);
      return 1;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
    const scroll = vi.fn();
    const { container } = render(
      <MobileViewport>
        <textarea aria-label="草稿" defaultValue="不要清空" />
      </MobileViewport>,
    );
    const input = screen.getByRole("textbox");
    input.scrollIntoView = scroll;
    act(() => {
      input.focus();
      viewport.height = 420;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(container.firstElementChild).toHaveAttribute(
      "data-keyboard-open",
      "true",
    );
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("textbox")).toBe(input);
    expect(input).toHaveValue("不要清空");
    act(() => {
      viewport.height = 768;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(container.firstElementChild).toHaveAttribute(
      "data-keyboard-open",
      "false",
    );
    expect(screen.getByRole("textbox")).toBe(input);
  });

  it("缩放、没有输入焦点以及不支持 visualViewport 时不误判键盘", () => {
    const viewport = Object.assign(new EventTarget(), {
      height: 400,
      offsetTop: 0,
      scale: 2,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: viewport,
    });
    const { container } = render(
      <MobileViewport>
        <p>比赛内容</p>
      </MobileViewport>,
    );
    expect(container.firstElementChild).toHaveAttribute(
      "data-keyboard-open",
      "false",
    );
  });

  it("卸载时清理监听，不因迟到的视口回调修改旧界面", () => {
    const viewport = Object.assign(new EventTarget(), {
      height: 768,
      offsetTop: 0,
      scale: 1,
    });
    Object.defineProperty(window, "visualViewport", {
      configurable: true,
      value: viewport,
    });
    const remove = vi.spyOn(viewport, "removeEventListener");
    const { unmount } = render(
      <MobileViewport>
        <p>比赛内容</p>
      </MobileViewport>,
    );
    unmount();
    expect(remove).toHaveBeenCalledWith("resize", expect.any(Function));
    expect(remove).toHaveBeenCalledWith("scroll", expect.any(Function));
  });
});
