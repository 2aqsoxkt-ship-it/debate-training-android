import { speakerLabel } from "../../lib/presentation";

export type ScheduleEventRow = {
  id: string;
  kind: string;
  speaker: string;
  durationSeconds: number;
  wordCap: number;
  interruptible: boolean;
  controller?: string;
  started?: boolean;
};

const KIND: Record<string, string> = {
  opening: "陈词",
  cross_exam: "质询",
  free_debate: "自由辩",
  summary: "总结",
};

export function EventControlPanel({
  schedule,
  cursor,
  mode,
  meteringMode,
  currentWordCap,
  disabled,
  onSetController,
}: {
  schedule: ScheduleEventRow[];
  cursor: number;
  mode: string;
  meteringMode?: "words" | "time";
  currentWordCap?: number;
  disabled?: boolean;
  onSetController: (index: number, toHuman: boolean) => void;
}) {
  const words = meteringMode === "words";
  return (
    <section className="event-control" aria-label="赛程控制权">
      <h3>赛程与控制权</h3>
      <p>
        {mode === "training"
          ? "训练模式：尚未开始的环节可改为「我来发言」或「AI 发言」。"
          : "评分模式：只能接管本方辩位；跨配置接管需二次确认。"}
      </p>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>环节</th>
            <th>辩位</th>
            {!words ? <th>时限</th> : null}
            <th>{words ? "字数上限" : "字数"}</th>
            <th>打断</th>
            <th>控制</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody>
          {schedule.map((event, index) => {
            const started = event.started ?? index < cursor;
            const controller = event.controller ?? "ai";
            const human = controller === "human";
            return (
              <tr key={event.id}>
                <td>{index === cursor ? "当前" : index + 1}</td>
                <td>{KIND[event.kind] ?? event.kind}</td>
                <td>{speakerLabel(event.speaker)}</td>
                {!words ? <td>{event.durationSeconds}s</td> : null}
                <td>
                  {words && index === cursor
                    ? (currentWordCap ?? event.wordCap)
                    : event.wordCap}
                  {words ? " 字" : ""}
                </td>
                <td>{event.interruptible ? "允许" : "禁止"}</td>
                <td>{human ? "人工" : "AI"}</td>
                <td>
                  <button
                    type="button"
                    disabled={disabled || started}
                    onClick={() => onSetController(index, true)}
                  >
                    我来发言
                  </button>
                  <button
                    type="button"
                    disabled={disabled || started}
                    onClick={() => onSetController(index, false)}
                  >
                    AI 发言
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}
