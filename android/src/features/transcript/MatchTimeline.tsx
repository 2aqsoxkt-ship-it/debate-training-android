import { useEffect, useMemo, useRef, useState } from "react";
import { speakerLabel as label } from "../../lib/presentation";

type TimelineItem = {
  id: string;
  slot: string;
  effectiveText: string;
  overflowText: string;
  interruptedTail: string;
  responseTargetText?: string;
};

export function MatchTimeline({ items }: { items: TimelineItem[] }) {
  const viewport = useRef<HTMLDivElement>(null);
  const [followLatest, setFollowLatest] = useState(true);
  const [hasNew, setHasNew] = useState(false);
  const [side, setSide] = useState<"all" | "affirmative" | "negative">("all");
  const following = useRef(true);
  const previousVisible = useRef({ side, ids: new Set<string>() });

  const visible = useMemo(() => {
    const filtered = items.filter((item) => {
      if (side === "affirmative") return item.slot.startsWith("AFF");
      if (side === "negative") return item.slot.startsWith("NEG");
      return true;
    });
    return filtered;
  }, [items, side]);

  useEffect(() => {
    const previous = previousVisible.current;
    previousVisible.current = {
      side,
      ids: new Set(visible.map((item) => item.id)),
    };
    if (!followLatest) {
      if (previous.side !== side) setHasNew(false);
      else if (visible.some((item) => !previous.ids.has(item.id)))
        setHasNew(true);
      return;
    }

    setHasNew(false);
    const frame = window.requestAnimationFrame(() => {
      // User scrolling may happen before React commits the follow state change.
      if (!following.current) return;
      viewport.current?.scrollTo({
        top: viewport.current.scrollHeight,
        // Override CSS smooth scrolling: intermediate events look like reading history.
        behavior: "instant",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [followLatest, side, visible]);

  function jumpToLatest() {
    following.current = true;
    setFollowLatest(true);
    setHasNew(false);
    viewport.current?.scrollTo({
      top: viewport.current.scrollHeight,
      behavior: "instant",
    });
  }

  return (
    <section className="match-timeline" aria-label="比赛时间线">
      <header>
        <div>
          <p className="eyebrow">全场公开实录</p>
          <h2>比赛时间线</h2>
        </div>
        <div className="timeline-tools">
          <label>
            队伍筛选
            <select
              value={side}
              onChange={(event) =>
                setSide(
                  event.target.value as "all" | "affirmative" | "negative",
                )
              }
            >
              <option value="all">全部</option>
              <option value="affirmative">正方</option>
              <option value="negative">反方</option>
            </select>
          </label>
          <label className="timeline-follow">
            <input
              type="checkbox"
              checked={followLatest}
              onChange={(event) => {
                following.current = event.target.checked;
                setFollowLatest(event.target.checked);
                if (event.target.checked) jumpToLatest();
              }}
            />
            跟随最新
          </label>
        </div>
      </header>
      <div
        className="timeline-scroll"
        ref={viewport}
        role="log"
        aria-live="polite"
        onScroll={(event) => {
          const node = event.currentTarget;
          const nearBottom =
            node.scrollHeight - node.scrollTop - node.clientHeight < 48;
          if (!nearBottom && followLatest) {
            following.current = false;
            setFollowLatest(false);
          }
          if (nearBottom) setHasNew(false);
        }}
      >
        {visible.length === 0 ? (
          <div className="timeline-empty">
            <strong>等待第一位辩手发言</strong>
            <p>发言、质询、打断和人工接管都会连续记录在这里。</p>
          </div>
        ) : (
          <ol>
            {visible.map((item) => (
              <li
                key={item.id}
                className={
                  item.slot.startsWith("AFF") ? "affirmative" : "negative"
                }
              >
                <div>
                  <strong>{label(item.slot)}</strong>
                  {item.responseTargetText ? (
                    <span>回应：{item.responseTargetText}</span>
                  ) : null}
                </div>
                <p>{item.effectiveText}</p>
                {item.interruptedTail ? (
                  <small>质询方在此处打断回答</small>
                ) : null}
                {item.overflowText ? <small>超限内容未计入评分</small> : null}
              </li>
            ))}
          </ol>
        )}
      </div>
      {hasNew ? (
        <button className="timeline-new" type="button" onClick={jumpToLatest}>
          有新发言，回到最新
        </button>
      ) : null}
    </section>
  );
}
