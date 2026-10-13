import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAuthoritativeClock } from "./useAuthoritativeClock";

describe("useAuthoritativeClock", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("投影剩余时间并在归零时回调，不自行改比赛状态", () => {
    const onTimeout = vi.fn();
    const { result } = renderHook(() =>
      useAuthoritativeClock(1_000, { onTimeout, cursor: 0 }),
    );
    expect(result.current).toBe(1_000);
    act(() => {
      vi.advanceTimersByTime(1_200);
    });
    expect(result.current).toBe(0);
    expect(onTimeout).toHaveBeenCalledTimes(1);
  });

  it("冻结时不推进显示", () => {
    const onTimeout = vi.fn();
    const { result } = renderHook(() =>
      useAuthoritativeClock(5_000, { frozen: true, onTimeout }),
    );
    act(() => {
      vi.advanceTimersByTime(3_000);
    });
    expect(result.current).toBe(5_000);
    expect(onTimeout).not.toHaveBeenCalled();
  });
});
