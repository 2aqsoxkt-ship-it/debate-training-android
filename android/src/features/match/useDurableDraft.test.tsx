import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useInputTelemetry } from "../telemetry/useInputTelemetry";
import { useDurableDraft, type DraftKey } from "./useDurableDraft";
import { fold, toClaimed } from "../telemetry/fold";
import type { SourceEvent } from "../telemetry/types";

const invoke = vi.fn();
vi.mock("../../lib/tauri", () => ({
  call: (...args: unknown[]) => invoke(...args),
}));
const key: DraftKey = {
  matchId: "match",
  windowId: "window",
  slot: "AFF_1",
  fieldKind: "speech",
};
let controls: {
  telemetry: ReturnType<typeof useInputTelemetry>;
  draft: ReturnType<typeof useDurableDraft>;
};
function Harness() {
  const telemetry = useInputTelemetry({ active: false });
  const draft = useDurableDraft(key, telemetry);
  controls = { telemetry, draft };
  return <textarea {...telemetry.bindTextareaProps} />;
}
const events: SourceEvent[] = [
  { kind: "insert", text: "原草稿", source: "typed" },
  { kind: "delete", units: 1 },
  { kind: "insert", text: "稿", source: "typed" },
];
const payload = {
  text: "原草稿",
  events,
  claimed: toClaimed(fold(events, 2500).telemetry),
};
describe("持久草稿", () => {
  beforeEach(() => {
    invoke.mockReset();
  });
  it("清空稿的保存仍在途时，关闭前flush也必须等待它", async () => {
    const releases: Array<() => void> = [];
    invoke.mockImplementation((c: string) =>
      c === "load_draft"
        ? Promise.resolve(null)
        : new Promise<boolean>((resolve) => releases.push(() => resolve(true))),
    );
    render(<Harness />);
    await act(async () => {
      await Promise.resolve();
    });
    act(() => controls.telemetry.setText("曾写过的稿"));
    const first = controls.draft.flush();
    await waitFor(() => expect(releases).toHaveLength(1));
    act(() => controls.telemetry.reset());
    const clearing = controls.draft.flush();
    let closeReady = false;
    const closing = controls.draft.flush().then(() => {
      closeReady = true;
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(closeReady).toBe(false);
    await act(async () => {
      releases[0]();
      await first;
    });
    await waitFor(() => expect(releases).toHaveLength(2));
    await act(async () => {
      releases[1]();
      await Promise.all([clearing, closing]);
    });
    expect(closeReady).toBe(true);
  });
  it("读取未完成时不能把未保存新稿当作已flush", async () => {
    invoke.mockReturnValue(new Promise(() => {}));
    render(<Harness />);
    act(() => controls.telemetry.setText("尚未保存的新稿"));
    await expect(controls.draft.flush()).rejects.toThrow("读取");
  });
  it("读取不会自动恢复、提交或调用模型；恢复保留逆向编辑和有效耗时", async () => {
    invoke.mockResolvedValue({
      key,
      revision: 4,
      payload,
      telemetryTrusted: true,
    });
    render(<Harness />);
    await waitFor(() => expect(controls.draft.pending).not.toBeNull());
    expect(controls.telemetry.text).toBe("");
    expect(invoke.mock.calls.map(([c]) => c)).toEqual(["load_draft"]);
    act(() => controls.draft.restore());
    expect(controls.telemetry.snapshot()).toEqual(payload);
  });
  it("版本递增且保存失败能被切换前的flush发现", async () => {
    invoke.mockImplementation((c: string) =>
      Promise.resolve(c === "load_draft" ? null : false),
    );
    render(<Harness />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    await act(async () => {
      controls.telemetry.setText("新稿");
    });
    await act(async () => {
      await expect(controls.draft.flush()).rejects.toThrow();
    });
    expect(controls.draft.error).toContain("未保存");
    expect(
      invoke.mock.calls.find(([c]) => c === "save_draft")?.[1].revision,
    ).toBe(1);
  });
  it("不恢复会以更高版本清空原草稿，损坏统计不作为标准输入恢复", async () => {
    invoke.mockImplementation((c: string) =>
      Promise.resolve(
        c === "load_draft"
          ? { key, revision: 5, payload, telemetryTrusted: false }
          : true,
      ),
    );
    render(<Harness />);
    await waitFor(() => expect(controls.draft.pending).not.toBeNull());
    act(() => controls.draft.restore());
    expect(controls.telemetry.text).toBe("");
    await act(async () => {
      controls.draft.discard();
    });
    const save = invoke.mock.calls.find(([c]) => c === "save_draft");
    expect(save?.[1].revision).toBe(6);
    expect(save?.[1].payload.text).toBe("");
  });
});
