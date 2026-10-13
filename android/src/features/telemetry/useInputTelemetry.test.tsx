import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  useInputTelemetry,
  type UseInputTelemetryOptions,
} from "./useInputTelemetry";
import { fold } from "./fold";

function Harness({
  onReady,
  visible = true,
  options,
}: {
  onReady: (value: ReturnType<typeof useInputTelemetry>) => void;
  visible?: boolean;
  options?: UseInputTelemetryOptions;
}) {
  const telemetry = useInputTelemetry({ wordCap: 20, ...options });
  onReady(telemetry);
  return visible ? (
    <textarea aria-label="回答" {...telemetry.bindTextareaProps} />
  ) : null;
}

describe("useInputTelemetry", () => {
  afterEach(() => vi.restoreAllMocks());

  it("遗漏 compositionstart/end 时仍能提交；未确认候选词不作为保存的最终文本", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(<Harness onReady={(value) => (telemetry = value)} />);
    const el = view.getByLabelText("回答") as HTMLTextAreaElement;
    act(() => {
      el.value = "pinyin";
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertCompositionText",
          isComposing: true,
        }),
      );
    });
    expect(telemetry!.isComposing()).toBe(true);
    expect(telemetry!.snapshot().text).toBe("");
    act(() => {
      el.value = "拼音";
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertText",
          isComposing: false,
        }),
      );
    });
    expect(telemetry!.isComposing()).toBe(false);
    expect(telemetry!.snapshot().text).toBe("拼音");
    expect(telemetry!.snapshot().claimed.forwardTypedUnits).toBe(2);
  });

  it("超限再删回限内，仍保留全部正向输入、逆向次数和可验证文本", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(
      <Harness
        onReady={(value) => (telemetry = value)}
        options={{ wordCap: 2 }}
      />,
    );
    const el = view.getByLabelText("回答") as HTMLTextAreaElement;
    for (const [value, type] of [
      ["甲乙丙丁", "insertText"],
      ["甲丁", "deleteContentForward"],
    ]) {
      act(() => {
        el.value = value;
        el.dispatchEvent(
          new InputEvent("input", { bubbles: true, inputType: type }),
        );
      });
    }
    expect(telemetry!.snapshot().claimed).toMatchObject({
      forwardTypedUnits: 4,
      effectivePositiveUnits: 2,
      reverseUnits: 2,
      reverseActions: 1,
    });
    expect(fold(telemetry!.snapshot().events).telemetry.finalText).toBe("甲丁");
  });

  it("安卓候选词提交后重复的 input 不重复计数，回放等于屏幕文本", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(<Harness onReady={(value) => (telemetry = value)} />);
    const el = view.getByLabelText("回答") as HTMLTextAreaElement;
    act(() => {
      el.dispatchEvent(
        new CompositionEvent("compositionstart", { bubbles: true }),
      );
      el.value = "手机能力";
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertCompositionText",
          isComposing: true,
        }),
      );
      el.dispatchEvent(
        new CompositionEvent("compositionend", {
          bubbles: true,
          data: "手机能力",
        }),
      );
      el.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertText",
          data: "手机能力",
        }),
      );
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertText",
          data: "手机能力",
        }),
      );
    });
    const saved = telemetry!.snapshot();
    expect(saved.text).toBe("手机能力");
    expect(fold(saved.events).telemetry.finalText).toBe(saved.text);
    expect(saved.claimed.forwardTypedUnits).toBe(4);
  });

  it("没有 beforeinput 的安卓 input 仍能回放，取消的 beforeinput 不凭空插字", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(<Harness onReady={(value) => (telemetry = value)} />);
    const el = view.getByLabelText("回答") as HTMLTextAreaElement;
    act(() => {
      el.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertText",
          data: "取消",
        }),
      );
    });
    expect(telemetry!.snapshot().claimed.forwardTypedUnits).toBe(0);
    act(() => {
      el.value = "电脑也有";
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertText",
          data: null,
        }),
      );
    });
    const saved = telemetry!.snapshot();
    expect(fold(saved.events).telemetry.finalText).toBe("电脑也有");
    expect(saved.claimed.forwardTypedUnits).toBe(4);
  });

  it("中间替换、标点删除、粘贴都按实际位置回放而非末尾，保留来源", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(<Harness onReady={(value) => (telemetry = value)} />);
    const el = view.getByLabelText("回答") as HTMLTextAreaElement;
    function input(value: string, inputType: string) {
      act(() => {
        el.value = value;
        el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
      });
    }
    input("甲，乙丙丁", "insertText");
    input("甲，替换丁", "insertReplacementText");
    input("甲替换丁", "deleteContentForward");
    input("甲粘贴替换丁", "insertFromPaste");
    const saved = telemetry!.snapshot();
    expect(fold(saved.events).telemetry.finalText).toBe(saved.text);
    expect(saved.claimed.reverseUnits).toBe(2);
    expect(saved.claimed.reverseActions).toBe(2);
    expect(saved.claimed.pasteUnits).toBe(2);
    expect(saved.claimed.effectivePositiveUnits).toBe(4);
  });

  it("只统计合法输入窗口，排除前置AI等待和提交等待，保留失败提交草稿", () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const onReady = (value: ReturnType<typeof useInputTelemetry>) =>
      (telemetry = value);
    const view = render(
      <Harness
        onReady={onReady}
        options={{ active: false, sessionKey: "free:1" }}
      />,
    );
    now = 300000;
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: true, sessionKey: "free:1" }}
      />,
    );
    now += 2000;
    act(() => {
      const el = view.getByLabelText("回答") as HTMLTextAreaElement;
      view.getByLabelText("回答").dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertText",
          data: "真人草稿",
        }),
      );
      el.value = "真人草稿";
      el.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertText",
          data: "真人草稿",
        }),
      );
    });
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(2000);
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: false, sessionKey: "free:1" }}
      />,
    );
    now += 600000;
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(2000);
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: true, sessionKey: "free:1" }}
      />,
    );
    now += 3000;
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(5000);
    expect(telemetry?.snapshot().claimed.forwardTypedUnits).toBe(4);
    expect(telemetry?.text).toBe("真人草稿");
    expect(telemetry?.snapshot().events).not.toContainEqual({
      kind: "pause",
      ms: 600000,
    });
  });

  it("同一自由辩环节的新回合独立计时，质询问答也按输入窗口分别计时", () => {
    let now = 0;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const onReady = (value: ReturnType<typeof useInputTelemetry>) =>
      (telemetry = value);
    const view = render(
      <Harness
        onReady={onReady}
        options={{ active: true, sessionKey: "event-1:round-1:question" }}
      />,
    );
    now = 2000;
    act(() => telemetry?.setText("上一轮"));
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(2000);
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: false, sessionKey: "event-1:round-1:answer" }}
      />,
    );
    now += 300000;
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: true, sessionKey: "event-1:round-1:answer" }}
      />,
    );
    now += 1000;
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(1000);
    expect(telemetry?.text).toBe("");
    view.rerender(
      <Harness
        onReady={onReady}
        options={{ active: true, sessionKey: "event-1:round-2:answer" }}
      />,
    );
    now += 500;
    expect(telemetry?.snapshot().claimed.inputElapsedMs).toBe(500);
  });
  it("异步载入的质询输入框也会附加遥测监听", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const onReady = (value: ReturnType<typeof useInputTelemetry>) =>
      (telemetry = value);
    const view = render(<Harness visible={false} onReady={onReady} />);
    view.rerender(<Harness visible onReady={onReady} />);
    const textarea = view.getByLabelText("回答");
    act(() => {
      textarea.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertText",
          data: "有效输入",
        }),
      );
      (textarea as HTMLTextAreaElement).value = "有效输入";
      textarea.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertText",
          data: "有效输入",
        }),
      );
    });
    expect(telemetry?.snapshot().claimed.forwardTypedUnits).toBe(4);
  });

  it("beforeinput 没有 data 时仍从 paste 事件记录粘贴来源", () => {
    let telemetry: ReturnType<typeof useInputTelemetry> | undefined;
    const view = render(<Harness onReady={(value) => (telemetry = value)} />);
    const textarea = view.getByLabelText("回答") as HTMLTextAreaElement;

    act(() => {
      textarea.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          inputType: "insertFromPaste",
          data: null,
        }),
      );
      const paste = new Event("paste", { bubbles: true }) as ClipboardEvent;
      Object.defineProperty(paste, "clipboardData", {
        value: { getData: () => "粘贴文本" },
      });
      textarea.dispatchEvent(paste);
      textarea.value = "粘贴文本";
      textarea.dispatchEvent(
        new InputEvent("input", {
          bubbles: true,
          inputType: "insertFromPaste",
          data: null,
        }),
      );
    });

    const snapshot = telemetry?.snapshot();
    expect(snapshot?.claimed.pasteUnits).toBe(4);
    expect(snapshot?.events).toContainEqual({
      kind: "replace",
      start: 0,
      end: 0,
      text: "粘贴文本",
      source: "pasted",
    });
  });
});
