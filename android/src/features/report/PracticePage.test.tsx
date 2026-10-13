import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { describe, it, expect, vi } from "vitest";
import { PracticePage, type PracticeRecord } from "./PracticePage";
const invoke = vi.fn();
vi.mock("../../lib/tauri", () => ({
  call: (...args: unknown[]) => invoke(...args),
}));
const record: PracticeRecord = {
  id: "practice",
  matchId: "original",
  sourceTurnId: "original-turn",
  revision: 0,
  rewrittenText: "",
  feedback: null,
  context: {
    topic: "原辩题",
    materials: "原材料",
    slot: "AFF_1",
    task: "重写原陈词",
    wordCap: 50,
    originalText: "原有效正文",
    beforeTurns: [["before", "NEG_1", "此前对方正文"]],
  },
};
describe("独立重练页", () => {
  it("打开只读冻结上下文，保存不调用AI，评语只有手动请求且不写原分数", async () => {
    invoke.mockReset();
    let saved = record;
    invoke.mockImplementation((command: string, args: { text?: string }) => {
      if (command === "load_practice") return Promise.resolve(saved);
      if (command === "save_practice") {
        saved = {
          ...saved,
          rewrittenText: args.text ?? "",
          revision: saved.revision + 1,
        };
        return Promise.resolve(saved);
      }
      if (command === "review_practice")
        return Promise.resolve({
          ...saved,
          feedback: JSON.stringify({
            strength: "回应清晰",
            improvement: "补证据",
            confidence: 80,
          }),
        });
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <MemoryRouter initialEntries={["/practice/practice"]}>
        <Routes>
          <Route path="/practice/:practiceId" element={<PracticePage />} />
        </Routes>
      </MemoryRouter>,
    );
    const input = await screen.findByLabelText("重练稿");
    expect(invoke.mock.calls.map(([c]) => c)).toEqual(["load_practice"]);
    fireEvent.change(input, { target: { value: "改进后的论证" } });
    fireEvent.click(
      screen.getByRole("button", { name: "保存练习，不调用 AI" }),
    );
    await waitFor(() =>
      expect(invoke.mock.calls.map(([c]) => c)).toEqual([
        "load_practice",
        "save_practice",
      ]),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /保存并请求/ })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /保存并请求/ }));
    expect(await screen.findByText("回应清晰")).toBeInTheDocument();
    expect(invoke.mock.calls.map(([c]) => c)).toEqual([
      "load_practice",
      "save_practice",
      "save_practice",
      "review_practice",
    ]);
  });
});
