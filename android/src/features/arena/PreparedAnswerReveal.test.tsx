import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PreparedAnswerReveal } from "./PreparedAnswerReveal";
import type { CrossExamSnapshot } from "./CrossExamPanel";

describe("质询回答展示恢复", () => {
  afterEach(() => vi.useRealTimers());

  it("逐字展示失败后停止自动请求，人工重试继续同一前缀", async () => {
    vi.useFakeTimers();
    const onReveal = vi
      .fn()
      .mockRejectedValueOnce(new Error("展示连接失败"))
      .mockResolvedValue({} as CrossExamSnapshot);
    render(
      <PreparedAnswerReveal
        visiblePrefix="已"
        revealedUnits={1}
        totalUnits={3}
        interruptible
        onReveal={onReveal}
        onComplete={vi.fn()}
        onInterrupt={vi.fn()}
      />,
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByRole("alert")).toHaveTextContent("展示连接失败");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(onReveal).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "重试展示" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(onReveal).toHaveBeenCalledTimes(2);
    expect(onReveal.mock.calls.map(([units]) => units)).toEqual([2, 2]);
    expect(screen.getByText("已")).toBeInTheDocument();
  });

  it("自动提交失败只尝试一次，人工重试后可完成", async () => {
    vi.useFakeTimers();
    const onComplete = vi
      .fn()
      .mockRejectedValueOnce(new Error("回答尚未保存"))
      .mockResolvedValue(undefined);
    render(
      <PreparedAnswerReveal
        visiblePrefix="全部"
        revealedUnits={2}
        totalUnits={2}
        interruptible
        onReveal={vi.fn()}
        onComplete={onComplete}
        onInterrupt={vi.fn()}
      />,
    );
    await act(async () => {});
    expect(screen.getByRole("alert")).toHaveTextContent("回答尚未保存");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(onComplete).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "重试提交回答" }));
    await act(async () => {});
    expect(onComplete).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("回答提交未返回时禁止重复点击打断", async () => {
    let resolveComplete: () => void = () => {};
    const onComplete = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveComplete = resolve;
        }),
    );
    render(
      <PreparedAnswerReveal
        visiblePrefix="全部"
        revealedUnits={2}
        totalUnits={2}
        interruptible
        onReveal={vi.fn()}
        onComplete={onComplete}
        onInterrupt={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "打断" })).toBeDisabled();
    await act(async () => {
      resolveComplete();
    });
    expect(onComplete).toHaveBeenCalledTimes(1);
  });

  it("慢 IPC 后不会追赶吐出整段，仍能打断已确认前缀", async () => {
    vi.useFakeTimers();
    let resolveReveal: (snapshot: CrossExamSnapshot) => void = () => {};
    const onReveal = vi.fn<
      (units: number, elapsedMs: number) => Promise<CrossExamSnapshot>
    >(
      () =>
        new Promise<CrossExamSnapshot>((resolve) => {
          resolveReveal = resolve;
        }),
    );
    const onInterrupt = vi.fn();
    const props = {
      visiblePrefix: "已",
      revealedUnits: 1,
      totalUnits: 100,
      interruptible: true,
      unitsPerMinute: 600,
      onReveal,
      onComplete: vi.fn(),
      onInterrupt,
    };
    const { rerender } = render(<PreparedAnswerReveal {...props} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1500);
    });
    expect(onReveal).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "打断" })).toBeEnabled();
    expect(screen.getByText("已")).toBeInTheDocument();
    await act(async () => {
      resolveReveal({} as CrossExamSnapshot);
    });
    rerender(
      <PreparedAnswerReveal
        {...props}
        revealedUnits={2}
        visiblePrefix="已答"
      />,
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(onReveal.mock.calls[1][0]).toBe(3);
    fireEvent.click(screen.getByRole("button", { name: "打断" }));
    expect(onInterrupt).toHaveBeenCalledTimes(1);
  });

  it("同步阻塞时不能自动提交完整回答", async () => {
    const onComplete = vi.fn();
    render(
      <PreparedAnswerReveal
        visiblePrefix="完成"
        revealedUnits={2}
        totalUnits={2}
        interruptible
        synchronizationBlocked
        onReveal={vi.fn()}
        onComplete={onComplete}
        onInterrupt={vi.fn()}
      />,
    );
    await act(async () => {});
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("切到后台暂停，回到前台后只展示下一个单位", async () => {
    vi.useFakeTimers();
    let visibility: DocumentVisibilityState = "visible";
    const spy = vi
      .spyOn(document, "visibilityState", "get")
      .mockImplementation(() => visibility);
    const onReveal = vi.fn<
      (units: number, elapsedMs: number) => Promise<CrossExamSnapshot>
    >(async () => ({}) as CrossExamSnapshot);
    render(
      <PreparedAnswerReveal
        visiblePrefix="答"
        revealedUnits={1}
        totalUnits={100}
        interruptible
        onReveal={onReveal}
        onComplete={vi.fn()}
        onInterrupt={vi.fn()}
      />,
    );
    act(() => {
      visibility = "hidden";
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });
    expect(onReveal).not.toHaveBeenCalled();
    act(() => {
      visibility = "visible";
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(350);
    });
    expect(onReveal.mock.calls[0][0]).toBe(2);
    spy.mockRestore();
  });
});
