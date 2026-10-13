import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MatchTimeline } from "./MatchTimeline";

const item = (id: number, slot = "AFF_1") => ({
  id: String(id),
  slot,
  effectiveText: `发言记录${id}`,
  overflowText: "",
  interruptedTail: "",
});

let frames: Map<number, FrameRequestCallback>;
let scrollTo: ReturnType<typeof vi.fn>;
const originalScrollTo = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollTo",
);

function flushFrames() {
  act(() => {
    const pending = [...frames.values()];
    frames.clear();
    pending.forEach((callback) => callback(0));
  });
}

function setViewport() {
  const log = screen.getByRole("log");
  Object.defineProperties(log, {
    scrollHeight: { configurable: true, value: 1000 },
    clientHeight: { configurable: true, value: 200 },
    scrollTop: { configurable: true, writable: true, value: 0 },
  });
  return log;
}

beforeEach(() => {
  frames = new Map();
  let nextFrame = 0;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    frames.delete(id);
  });
  scrollTo = vi.fn(function (this: HTMLElement, options: ScrollToOptions) {
    if (options.behavior === "smooth") {
      // A real smooth scroll emits intermediate events before reaching the end.
      this.scrollTop = 300;
      fireEvent.scroll(this);
    }
    this.scrollTop = Math.max(0, this.scrollHeight - this.clientHeight);
    fireEvent.scroll(this);
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value: scrollTo,
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  if (originalScrollTo) {
    Object.defineProperty(HTMLElement.prototype, "scrollTo", originalScrollTo);
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  }
});

describe("整场实录", () => {
  it("超过240条仍可查看第一条发言", () => {
    render(
      <MatchTimeline
        items={Array.from({ length: 241 }, (_, index) => ({
          id: String(index),
          slot: "AFF_1",
          effectiveText: `发言记录${index}`,
          overflowText: "",
          interruptedTail: "",
        }))}
      />,
    );
    expect(screen.getByText("发言记录0")).toBeInTheDocument();
    expect(screen.getByText("发言记录240")).toBeInTheDocument();
  });

  it("首次恢复已有历史记录时定位到最新发言", () => {
    render(<MatchTimeline items={[item(0), item(1)]} />);
    const log = setViewport();

    flushFrames();

    expect(scrollTo).toHaveBeenCalled();
    expect(log.scrollTop).toBe(800);
    expect(screen.getByRole("checkbox", { name: "跟随最新" })).toBeChecked();
  });

  it("自动跟随不会被程序滚动的中间事件关闭", () => {
    const { rerender } = render(<MatchTimeline items={[]} />);
    setViewport();
    flushFrames();
    scrollTo.mockClear();

    rerender(<MatchTimeline items={[item(0)]} />);
    flushFrames();

    expect(scrollTo).toHaveBeenCalled();
    expect(screen.getByRole("checkbox", { name: "跟随最新" })).toBeChecked();
  });

  it("在跟随定位执行前人工上翻会取消待执行的滚动", () => {
    const { rerender } = render(<MatchTimeline items={[item(0)]} />);
    const log = setViewport();
    flushFrames();
    scrollTo.mockClear();

    rerender(<MatchTimeline items={[item(0), item(1)]} />);
    log.scrollTop = 200;
    fireEvent.scroll(log);
    flushFrames();

    expect(scrollTo).not.toHaveBeenCalled();
    expect(log.scrollTop).toBe(200);
    expect(
      screen.getByRole("checkbox", { name: "跟随最新" }),
    ).not.toBeChecked();
  });

  it("人工阅读历史时新发言只提醒，回到最新后恢复跟随", () => {
    const { rerender } = render(<MatchTimeline items={[item(0)]} />);
    const log = setViewport();
    flushFrames();
    log.scrollTop = 100;
    fireEvent.scroll(log);
    scrollTo.mockClear();

    rerender(<MatchTimeline items={[item(0), item(1)]} />);
    flushFrames();
    expect(scrollTo).not.toHaveBeenCalled();
    expect(log.scrollTop).toBe(100);

    fireEvent.click(screen.getByRole("button", { name: "有新发言，回到最新" }));
    flushFrames();
    expect(log.scrollTop).toBe(800);
    expect(screen.getByRole("checkbox", { name: "跟随最新" })).toBeChecked();
    expect(
      screen.queryByRole("button", { name: "有新发言，回到最新" }),
    ).toBeNull();
  });

  it("跟随状态下切换队伍筛选仍定位到该队最新发言", () => {
    render(<MatchTimeline items={[item(0), item(1, "NEG_1")]} />);
    setViewport();
    flushFrames();
    scrollTo.mockClear();

    fireEvent.change(screen.getByRole("combobox", { name: "队伍筛选" }), {
      target: { value: "negative" },
    });
    flushFrames();

    expect(screen.queryByText("发言记录0")).toBeNull();
    expect(screen.getByText("发言记录1")).toBeInTheDocument();
    expect(scrollTo).toHaveBeenCalled();
    expect(screen.getByRole("checkbox", { name: "跟随最新" })).toBeChecked();
  });

  it("阅读筛选后的历史时隐藏队伍新发言不会误报", () => {
    const { rerender } = render(<MatchTimeline items={[item(0)]} />);
    const log = setViewport();
    fireEvent.change(screen.getByRole("combobox", { name: "队伍筛选" }), {
      target: { value: "affirmative" },
    });
    flushFrames();
    log.scrollTop = 100;
    fireEvent.scroll(log);
    scrollTo.mockClear();

    rerender(<MatchTimeline items={[item(0), item(1, "NEG_1")]} />);
    flushFrames();

    expect(scrollTo).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "有新发言，回到最新" }),
    ).toBeNull();
    expect(
      screen.getByRole("checkbox", { name: "跟随最新" }),
    ).not.toBeChecked();
  });

  it("卸载组件会取消待执行的定位", () => {
    const { rerender, unmount } = render(<MatchTimeline items={[]} />);
    setViewport();
    flushFrames();
    rerender(<MatchTimeline items={[item(0)]} />);
    expect(frames.size).toBe(1);

    unmount();

    expect(frames.size).toBe(0);
  });

  it("溢出提示不再把字数超限写成超时", () => {
    render(
      <MatchTimeline items={[{ ...item(0), overflowText: "超出的发言" }]} />,
    );
    expect(screen.getByText("超限内容未计入评分")).toBeInTheDocument();
    expect(screen.queryByText("超时内容未计入评分")).toBeNull();
  });
});
