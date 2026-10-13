import { useEffect, useState } from "react";
import { Link, useParams } from "react-router";
import { call } from "../../lib/tauri";
import { speakerLabel } from "../../lib/presentation";
import { countEffectiveUnits } from "../telemetry/units";

export type PracticeRecord = {
  id: string;
  matchId: string;
  sourceTurnId: string;
  revision: number;
  rewrittenText: string;
  feedback: string | null;
  context: {
    topic: string;
    materials: string;
    slot: string;
    task: string;
    wordCap: number;
    originalText: string;
    beforeTurns: [string, string, string][];
  };
};

export function PracticePage() {
  const { practiceId = "" } = useParams();
  const [record, setRecord] = useState<PracticeRecord | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    void call<PracticeRecord>("load_practice", { id: practiceId })
      .then((next) => {
        if (alive) {
          setRecord(next);
          setText(next.rewrittenText);
        }
      })
      .catch((e) => {
        if (alive) setError(String(e));
      });
    return () => {
      alive = false;
    };
  }, [practiceId]);
  async function save(review: boolean) {
    if (!record || busy) return;
    setBusy(true);
    setError("");
    try {
      const saved = await call<PracticeRecord>("save_practice", {
        id: record.id,
        text,
        revision: record.revision,
      });
      setRecord(saved);
      if (review)
        setRecord(
          await call<PracticeRecord>("review_practice", {
            id: saved.id,
            revision: saved.revision,
          }),
        );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  type Feedback = {
    strength?: string;
    improvement?: string;
    materialSupport?: string;
    confidence?: number;
  };
  let feedback: Feedback | null = null;
  try {
    feedback = record?.feedback
      ? (JSON.parse(record.feedback) as Feedback)
      : null;
  } catch {
    /* Preserve the original stored data; do not invent feedback. */
  }
  return (
    <main className="report-page practice-page">
      <h1>独立重练</h1>
      <p>
        练习稿与反馈单独保存，不改变原比赛分数、字池或趋势记录。AI
        反馈仅在点击后请求。
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {record ? (
        <>
          <Link to={`/report/${record.matchId}`}>返回原报告</Link>
          <h2>
            {record.context.topic} · {speakerLabel(record.context.slot)}
          </h2>
          <p>{record.context.task}</p>
          <details>
            <summary>该轮之前的公开实录</summary>
            {record.context.beforeTurns.map(([id, slot, body]) => (
              <article key={id}>
                <strong>{slot}</strong>
                <p>{body}</p>
              </article>
            ))}
          </details>
          <details>
            <summary>原有效发言（仅供对照）</summary>
            <p>{record.context.originalText}</p>
          </details>
          <label>
            重练稿
            <textarea
              aria-label="重练稿"
              value={text}
              disabled={busy}
              onChange={(e) => setText(e.target.value)}
            />
          </label>
          <p>
            {countEffectiveUnits(text)} / {record.context.wordCap} 字
          </p>
          <button
            type="button"
            disabled={
              busy || countEffectiveUnits(text) > record.context.wordCap
            }
            onClick={() => void save(false)}
          >
            保存练习，不调用 AI
          </button>
          <button
            type="button"
            disabled={
              busy ||
              !text.trim() ||
              countEffectiveUnits(text) > record.context.wordCap
            }
            onClick={() => void save(true)}
          >
            保存并请求 AI 反馈（消耗 API）
          </button>
          {busy && (
            <>
              <p role="status">正在保存或等待练习反馈，可以阅读原实录。</p>
              <button
                type="button"
                onClick={() => {
                  void call("cancel_ai_stream").catch((e) =>
                    setError(String(e)),
                  );
                }}
              >
                取消反馈请求
              </button>
            </>
          )}
          {feedback && (
            <section aria-label="练习反馈">
              <h2>练习反馈 · 不计入原比赛</h2>
              <p>{feedback.strength}</p>
              <p>{feedback.improvement}</p>
              <p>{feedback.materialSupport}</p>
              <small>模型置信度 {feedback.confidence ?? "未知"}</small>
            </section>
          )}
        </>
      ) : (
        <p>正在读取独立练习……</p>
      )}
    </main>
  );
}
