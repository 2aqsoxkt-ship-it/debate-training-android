import { useEffect, useRef } from "react";
import { speakerLabel } from "../../lib/presentation";
import { StatusBadge, TeamClock } from "../../components/ui/Primitives";

const WHISTLE_WARNING_MS = 30_000;

export function floorSide(holder: string) {
  if (holder === "affirmative" || holder.startsWith("AFF"))
    return "affirmative";
  if (holder === "negative" || holder.startsWith("NEG")) return "negative";
  return "";
}

export function whistleCount(previousMs: number, currentMs: number) {
  if (previousMs > 0 && currentMs <= 0) return 2;
  if (previousMs > WHISTLE_WARNING_MS && currentMs <= WHISTLE_WARNING_MS) {
    return 1;
  }
  return 0;
}

function playWhistles(count: number) {
  if (count <= 0 || typeof AudioContext === "undefined") return;
  let context: AudioContext;
  try {
    context = new AudioContext();
  } catch {
    return;
  }
  void context.resume().catch(() => undefined);
  for (let index = 0; index < count; index += 1) {
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    const startsAt = context.currentTime + index * 0.28;
    oscillator.type = "sine";
    oscillator.frequency.setValueAtTime(880, startsAt);
    gain.gain.setValueAtTime(0.0001, startsAt);
    gain.gain.exponentialRampToValueAtTime(0.16, startsAt + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, startsAt + 0.18);
    oscillator.connect(gain);
    gain.connect(context.destination);
    oscillator.start(startsAt);
    oscillator.stop(startsAt + 0.2);
  }
  window.setTimeout(
    () => void context.close().catch(() => undefined),
    count * 280 + 300,
  );
}

export function TeamWordPool({
  side,
  label,
  units,
  active,
}: {
  side: "affirmative" | "negative";
  label: string;
  units: number;
  active?: boolean;
}) {
  return (
    <div className={`team-clock ${side} ${active ? "active" : ""}`}>
      <span>{label}</span>
      <strong>{Math.max(0, units).toLocaleString("zh-CN")} 字</strong>
    </div>
  );
}

export function FreeDebatePanel({
  floorHolder,
  affirmativeMs,
  negativeMs,
  meteringMode,
  affirmativeUnits = 0,
  negativeUnits = 0,
  currentSpeaker,
  lastRespondsTo,
  onWaiveRequest,
  waiveDisabled = false,
}: {
  floorHolder: string;
  affirmativeMs: number;
  negativeMs: number;
  meteringMode?: "words" | "time";
  affirmativeUnits?: number;
  negativeUnits?: number;
  currentSpeaker: string;
  lastRespondsTo?: string;
  onWaiveRequest?: () => void;
  waiveDisabled?: boolean;
}) {
  const words = meteringMode === "words";
  const activeSide = floorSide(floorHolder);
  const affirmativeActive = activeSide === "affirmative";
  const negativeActive = activeSide === "negative";
  const previousClocks = useRef({ affirmativeMs, negativeMs });

  useEffect(() => {
    const count = Math.max(
      whistleCount(previousClocks.current.affirmativeMs, affirmativeMs),
      whistleCount(previousClocks.current.negativeMs, negativeMs),
    );
    previousClocks.current = { affirmativeMs, negativeMs };
    if (words) return;
    playWhistles(count);
  }, [affirmativeMs, negativeMs, words]);

  return (
    <section className="free-debate-panel" aria-label="自由辩">
      <header>
        <div>
          <p className="eyebrow">{words ? "团队共享字池" : "团队共享时间"}</p>
          <h2>自由辩交锋</h2>
        </div>
        <StatusBadge
          tone={
            affirmativeActive
              ? "affirmative"
              : negativeActive
                ? "negative"
                : "neutral"
          }
        >
          {activeSide
            ? `${affirmativeActive ? "正方" : "反方"}持权`
            : "等待选择辩位"}
        </StatusBadge>
      </header>
      <div className="free-clock-grid">
        {words ? (
          <>
            <TeamWordPool
              side="affirmative"
              label="正方共享字池"
              units={affirmativeUnits}
              active={affirmativeActive}
            />
            <TeamWordPool
              side="negative"
              label="反方共享字池"
              units={negativeUnits}
              active={negativeActive}
            />
          </>
        ) : (
          <>
            <TeamClock
              side="affirmative"
              label="正方共享"
              ms={affirmativeMs}
              active={affirmativeActive}
            />
            <TeamClock
              side="negative"
              label="反方共享"
              ms={negativeMs}
              active={negativeActive}
            />
          </>
        )}
      </div>
      <div className="free-debate-ruleline">
        <span>四名本方辩手均可发言</span>
        <span>提交后通常换方</span>
        <strong>不可打断</strong>
      </div>
      {onWaiveRequest ? (
        <button
          type="button"
          className="danger-ghost"
          disabled={waiveDisabled || !floorHolder}
          onClick={onWaiveRequest}
        >
          当前方放弃剩余{words ? "字数" : "时间"}
        </button>
      ) : null}
      {words ? (
        <div aria-live="polite">
          <p className={affirmativeUnits <= 0 ? "clock-exhausted" : ""}>
            {affirmativeUnits <= 0
              ? "正方字数额度已用尽，反方可继续使用剩余字数。"
              : `正方剩余 ${affirmativeUnits.toLocaleString("zh-CN")} 字。`}
          </p>
          <p className={negativeUnits <= 0 ? "clock-exhausted" : ""}>
            {negativeUnits <= 0
              ? "反方字数额度已用尽，正方可继续使用剩余字数。"
              : `反方剩余 ${negativeUnits.toLocaleString("zh-CN")} 字。`}
          </p>
        </div>
      ) : null}
      {!words && affirmativeMs <= 0 ? (
        <p className="clock-exhausted">
          正方时间已用尽，反方可继续使用剩余时间。
        </p>
      ) : null}
      {!words && affirmativeMs > 0 && affirmativeMs <= WHISTLE_WARNING_MS ? (
        <p className="clock-warning" role="status">
          正方剩余 30 秒内（一声提示）。
        </p>
      ) : null}
      {!words && negativeMs <= 0 ? (
        <p className="clock-exhausted">
          反方时间已用尽，正方可继续使用剩余时间。
        </p>
      ) : null}
      {!words && negativeMs > 0 && negativeMs <= WHISTLE_WARNING_MS ? (
        <p className="clock-warning" role="status">
          反方剩余 30 秒内（一声提示）。
        </p>
      ) : null}
      <div className="response-brief">
        <span>当前辩位</span>
        <strong>{speakerLabel(currentSpeaker)}</strong>
        {lastRespondsTo ? (
          <p>本轮应回应：{lastRespondsTo}</p>
        ) : (
          <p>首轮可直接提出本方核心争点。</p>
        )}
      </div>
    </section>
  );
}
