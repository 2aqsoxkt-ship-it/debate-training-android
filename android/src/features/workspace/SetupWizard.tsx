export const SETUP_STEPS = [
  "档案与模式",
  "辩题与材料",
  "赛制来源",
  "赛制编辑与确认",
  "拆题与策略",
  "队伍与辩位",
  "打字校准",
  "阵容与赛程核对",
  "开赛确认",
  "比赛与报告",
];

const PHASE_STEP: Record<string, number> = {
  draft_setup: 0,
  rules_parsed: 3,
  rules_confirmed: 4,
  topic_analyzed: 5,
  calibrated: 7,
  lineup_confirmed: 8,
  ready: 8,
  in_progress: 9,
  recovery_required: 9,
  match_complete: 9,
  scoring: 9,
  report_ready: 9,
};

export function setupStepForPhase(phase: string) {
  return PHASE_STEP[phase] ?? 0;
}

export function SetupStepper({
  phase,
  activeStep,
  mode,
  hasRuleIssues,
  onStepChange,
}: {
  phase: string;
  activeStep: number;
  mode: string;
  hasRuleIssues: boolean;
  onStepChange: (step: number) => void;
}) {
  const reached = Math.max(setupStepForPhase(phase), activeStep);
  return (
    <nav className="setup-stepper" aria-label="赛前准备步骤">
      <div className="stepper-heading">
        <p className="eyebrow">准备进度</p>
        <strong>
          {Math.min(reached + 1, SETUP_STEPS.length)}/{SETUP_STEPS.length}
        </strong>
      </div>
      <ol>
        {SETUP_STEPS.map((label, index) => {
          const available = index <= reached;
          const needsAttention = index === 3 && hasRuleIssues;
          const status = needsAttention
            ? "需修正"
            : index < reached
              ? "已完成"
              : index === reached
                ? "进行中"
                : "未开始";
          return (
            <li key={label} className={needsAttention ? "needs-attention" : ""}>
              <button
                type="button"
                disabled={!available}
                aria-current={index === activeStep ? "step" : undefined}
                onClick={() => onStepChange(index)}
              >
                <span className="step-number">{index + 1}</span>
                <span className="step-copy">
                  <strong>{label}</strong>
                  <small>
                    {index === 6 && mode !== "scoring"
                      ? "训练模式无需测速"
                      : status}
                  </small>
                </span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

export function WizardProgress({ phase }: { phase: string }) {
  const current = setupStepForPhase(phase);
  return (
    <SetupStepper
      phase={phase}
      activeStep={current}
      mode="scoring"
      hasRuleIssues={false}
      onStepChange={() => undefined}
    />
  );
}

const SEATS = [
  "正方一辩",
  "正方二辩",
  "正方三辩",
  "正方四辩",
  "反方一辩",
  "反方二辩",
  "反方三辩",
  "反方四辩",
];

export function SeatSelector({
  mode,
  side,
  selected,
  onChange,
}: {
  mode: string;
  side: string;
  selected: number[];
  onChange: (next: number[]) => void;
}) {
  const permitted = (index: number) =>
    mode !== "scoring" || (side === "negative" ? index >= 4 : index < 4);
  const toggle = (index: number) => {
    if (!permitted(index)) return;
    if (selected.includes(index)) {
      onChange(selected.filter((value) => value !== index));
      return;
    }
    if (mode === "scoring" && selected.length >= 4) return;
    onChange([...selected, index].sort((left, right) => left - right));
  };
  return (
    <fieldset>
      <legend>选择由你控制的辩位</legend>
      <p>
        {mode === "scoring"
          ? "评分模式只能选择本方 1–4 个辩位。"
          : "训练模式可选择任意一名或多名辩手，也可全部交给 AI。"}
      </p>
      <div className="seat-selector">
        {SEATS.map((label, index) => (
          <button
            key={label}
            type="button"
            className={selected.includes(index) ? "selected" : ""}
            aria-pressed={selected.includes(index)}
            disabled={!permitted(index)}
            onClick={() => toggle(index)}
          >
            <strong>{label}</strong>
            <small>{selected.includes(index) ? "人工控制" : "AI 控制"}</small>
          </button>
        ))}
      </div>
    </fieldset>
  );
}
