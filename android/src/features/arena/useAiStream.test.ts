import { describe, expect, it } from "vitest";
import { acceptsStreamEvent } from "./useAiStream";

describe("acceptsStreamEvent", () => {
  it("绑定 started 的 requestId，并丢弃迟到的旧请求事件", () => {
    const started = acceptsStreamEvent(
      {
        kind: "started",
        requestId: "req-new",
        eventId: "evt-1",
      },
      "evt-1",
      "",
    );
    expect(started.accept).toBe(true);
    expect(started.nextBoundRequestId).toBe("req-new");

    const lateDelta = acceptsStreamEvent(
      {
        kind: "text_delta",
        requestId: "req-old",
        eventId: "evt-1",
        text: "迟到",
      },
      "evt-1",
      "req-new",
    );
    expect(lateDelta.accept).toBe(false);

    const ownDelta = acceptsStreamEvent(
      {
        kind: "text_delta",
        requestId: "req-new",
        eventId: "evt-1",
        text: "本轮",
      },
      "evt-1",
      "req-new",
    );
    expect(ownDelta.accept).toBe(true);
  });

  it("丢弃其他 eventId 的事件", () => {
    const other = acceptsStreamEvent(
      {
        kind: "text_delta",
        requestId: "req-1",
        eventId: "evt-other",
        text: "错环节",
      },
      "evt-1",
      "req-1",
    );
    expect(other.accept).toBe(false);
  });
});
