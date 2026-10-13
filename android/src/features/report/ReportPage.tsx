import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { InlineNotice, StatusBadge } from "../../components/ui/Primitives";
import { call } from "../../lib/tauri";
import { readableRoleText, speakerLabel } from "../../lib/presentation";
import type { PracticeRecord } from "./PracticePage";
import {
  emptySnapshot,
  routeForSnapshot,
  type Snapshot,
} from "../session/types";

function winnerLabel(value: string) {
  if (value === "affirmative") return "正方";
  if (value === "negative") return "反方";
  return value || "待判定";
}

export function ReportPage() {
  const { matchId = "" } = useParams();
  const navigate = useNavigate();
  const [view, setView] = useState<Snapshot>(emptySnapshot);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    async function load() {
      try {
        let next = await call<Snapshot>("workspace_snapshot");
        if (next.matchId !== matchId)
          next = await call<Snapshot>("resume_match", { id: matchId });
        if (!active) return;
        const target = routeForSnapshot(next);
        if (target !== `/report/${matchId}`) {
          navigate(target, { replace: true });
          return;
        }
        setView(next);
      } catch (caught) {
        if (active)
          setError(caught instanceof Error ? caught.message : String(caught));
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [matchId, navigate]);

  async function exportJson() {
    try {
      const json = await call<string>("export_report");
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `debate-report-${matchId}.json`;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  async function backToWorkspace() {
    try {
      await call<Snapshot>("release_completed_match");
      navigate("/workspace", { replace: true });
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  const report = view.report;
  if (!report) {
    return (
      <main className="report-page loading-page">
        <div className="loading-orb" aria-hidden="true" />
        <p>{error || "正在读取评分报告……"}</p>
      </main>
    );
  }

  const improvements = report.stageEvaluations
    .map((evaluation) => evaluation.improvement)
    .filter(Boolean)
    .slice(0, 3);
  if (improvements.length === 0 && report.reviewSummary) {
    improvements.push(report.reviewSummary);
  }
  const maxDimension = Math.max(
    1,
    ...report.dimensions.map((dimension) => dimension.points),
  );

  return (
    <main className="report-page">
      <header className="report-hero">
        <div>
          <p className="eyebrow">赛后复盘报告</p>
          <h1>{view.topic}</h1>
          <div className="report-badges">
            <StatusBadge tone={report.standard ? "success" : "warning"}>
              {report.standard ? "标准比赛" : "非标准比赛"}
            </StatusBadge>
            <StatusBadge>{report.modelId}</StatusBadge>
            <StatusBadge>{report.difficulty}</StatusBadge>
          </div>
          {!report.standard ? <p>{report.nonStandardReason}</p> : null}
        </div>
        <div className="report-actions">
          <button type="button" onClick={() => void exportJson()}>
            导出 JSON
          </button>
          <button type="button" onClick={() => window.print()}>
            打印报告
          </button>
          <button
            type="button"
            className="primary-action"
            onClick={() => void backToWorkspace()}
          >
            返回赛前工作台
          </button>
        </div>
      </header>

      {error ? (
        <InlineNotice tone="danger" title="报告操作失败">
          <p>{error}</p>
        </InlineNotice>
      ) : null}

      <section className="report-score-hero" aria-label="评分总览">
        <article className="primary-score">
          <span>个人总分</span>
          <strong>{report.personal}</strong>
          <small>
            程序 {report.program} + 语义 {report.semantic}
          </small>
        </article>
        <article>
          <span>团队结果</span>
          <strong>{winnerLabel(report.teamWinner)}</strong>
          <small>
            {readableRoleText(report.teamSummary || "暂无团队评语")}
          </small>
        </article>
        <article>
          <span>篇幅控制</span>
          <strong>{report.length}</strong>
          <small>
            规则执行 {report.execution} · 输入控制 {report.inputControl}
          </small>
        </article>
      </section>

      <section className="report-priority-card">
        <div>
          <p className="eyebrow">下一场优先改进</p>
          <h2>把复盘变成可执行动作</h2>
        </div>
        <ol>
          {improvements.map((improvement, index) => (
            <li key={`${improvement}-${index}`}>
              <span>{index + 1}</span>
              <p>{readableRoleText(improvement)}</p>
            </li>
          ))}
        </ol>
      </section>

      <section className="report-card" aria-label="人工发言独立重练">
        <h2>选一轮人工发言重练</h2>
        <p>保留原结果，只使用该轮之前的公开比赛内容。</p>
        {view.transcripts
          .filter((turn) => turn.controller === "human")
          .map((turn) => (
            <article key={turn.id}>
              <strong>{speakerLabel(turn.slot)}</strong>
              <p>{turn.effectiveText}</p>
              <button
                type="button"
                onClick={() => {
                  void call<PracticeRecord>("create_practice", {
                    matchId,
                    sourceTurnId: turn.id,
                  })
                    .then((practice) => navigate(`/practice/${practice.id}`))
                    .catch((e) => setError(String(e)));
                }}
              >
                独立重练此轮
              </button>
            </article>
          ))}
      </section>

      {report.scoringStatus === "SCORING_PENDING" ? (
        <InlineNotice tone="warning" title="语义评审待重试">
          <p>程序分已经保留，当前结果尚未包含完整语义复核。</p>
        </InlineNotice>
      ) : null}

      <div className="report-grid">
        <section className="report-card dimension-card">
          <div className="section-heading">
            <div>
              <p className="eyebrow">能力画像</p>
              <h2>七维表现</h2>
            </div>
            <small>点击维度定位证据</small>
          </div>
          <ul className="dimension-bars">
            {report.dimensions.map((dimension) => (
              <li key={dimension.name}>
                <a href={`#t-${dimension.transcriptId}`}>
                  <span className="dimension-name">{dimension.name}</span>
                  <span className="dimension-track" aria-hidden="true">
                    <span
                      style={{
                        width: `${Math.max(4, (dimension.points / maxDimension) * 100)}%`,
                      }}
                    />
                  </span>
                  <strong>{dimension.points}</strong>
                  <small>原始 {dimension.raw}</small>
                </a>
              </li>
            ))}
          </ul>
        </section>

        <section className="report-card review-card">
          <p className="eyebrow">整场复核</p>
          <h2>表现总结</h2>
          <blockquote>
            {readableRoleText(report.reviewSummary || "暂无整场复核。")}
          </blockquote>
          <dl className="report-meta">
            <div>
              <dt>公式版本</dt>
              <dd>{report.formulaVersion}</dd>
            </div>
            <div>
              <dt>模型</dt>
              <dd>{report.modelId}</dd>
            </div>
            <div>
              <dt>难度</dt>
              <dd>{report.difficulty}</dd>
            </div>
            <div>
              <dt>基准语速</dt>
              <dd>{report.speechRate}</dd>
            </div>
          </dl>
          <p className="report-trace">{readableRoleText(report.trace)}</p>
        </section>
      </div>

      <details className="report-card report-details stage-review" open>
        <summary>
          <span>
            <strong>逐环节评价</strong>
            <small>{report.stageEvaluations.length} 条评价</small>
          </span>
          <span>展开 / 收起</span>
        </summary>
        <div className="details-content">
          {report.stageEvaluations.length === 0 ? (
            <p>暂无逐环节评价。</p>
          ) : null}
          {report.stageEvaluations.map((evaluation) => (
            <article key={evaluation.transcriptId}>
              <h3>
                <a href={`#t-${evaluation.transcriptId}`}>
                  发言 {evaluation.transcriptId.slice(0, 8)}
                </a>
              </h3>
              <div className="evaluation-grid">
                <p>
                  <strong>优势</strong>
                  {readableRoleText(evaluation.strength)}
                </p>
                <p>
                  <strong>改进</strong>
                  {readableRoleText(evaluation.improvement)}
                </p>
              </div>
              <p className="evidence-meta">
                证据：{evaluation.materialSupport} · 置信度{" "}
                {evaluation.confidence}
              </p>
            </article>
          ))}
        </div>
      </details>

      <details
        className="report-card report-details transcript-evidence"
        aria-label="完整发言证据"
      >
        <summary>
          <span>
            <strong>完整发言证据</strong>
            <small>{view.transcripts.length} 条记录</small>
          </span>
          <span>展开 / 收起</span>
        </summary>
        <ol className="details-content">
          {view.transcripts.map((entry) => (
            <li key={entry.id} id={`t-${entry.id}`}>
              <strong>{speakerLabel(entry.slot)}</strong>
              <p>{entry.effectiveText}</p>
              {entry.interruptedTail ? (
                <small>打断后不计入：{entry.interruptedTail}</small>
              ) : null}
              {entry.overflowText ? (
                <small>超限不计入：{entry.overflowText}</small>
              ) : null}
            </li>
          ))}
        </ol>
      </details>
    </main>
  );
}
