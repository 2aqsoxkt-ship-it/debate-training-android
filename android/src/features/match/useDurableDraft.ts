import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "../../lib/tauri";
import type {
  TelemetryCommit,
  UseInputTelemetry,
} from "../telemetry/useInputTelemetry";

export type DraftKey = {
  matchId: string;
  windowId: string;
  slot: string;
  fieldKind: "speech" | "question" | "answer";
};
type SavedDraft = {
  key: DraftKey;
  revision: number;
  payload: TelemetryCommit;
  telemetryTrusted: boolean;
};

export function useDurableDraft(
  key: DraftKey | null,
  telemetry: UseInputTelemetry,
) {
  const identity = key ? JSON.stringify(key) : "";
  const latest = useRef(telemetry);
  latest.current = telemetry;
  const state = useRef<{
    identity: string;
    key: DraftKey;
    revision: number;
    ready: boolean;
    lastText: string;
  } | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const [pending, setPending] = useState<SavedDraft | null>(null);
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
  const [error, setError] = useState("");

  useEffect(() => {
    setPending(null);
    setError("");
    if (!identity) {
      state.current = null;
      return;
    }
    const parsed = JSON.parse(identity) as DraftKey;
    const current = {
      identity,
      key: parsed,
      revision: 0,
      ready: false,
      lastText: "",
    };
    state.current = current;
    void call<SavedDraft | null>("load_draft", { key: parsed })
      .then((saved) => {
        if (state.current !== current) return;
        current.ready = true;
        current.revision = saved?.revision ?? 0;
        if (saved?.payload.text) setPending(saved);
      })
      .catch((caught) => {
        if (state.current === current)
          setError(`草稿读取失败，暂不覆盖存档：${String(caught)}`);
      });
    return () => {
      if (state.current === current) state.current = null;
    };
  }, [identity]);

  const flush = useCallback(async () => {
    const current = state.current;
    if (!current) return;
    if (!current.ready) {
      if (latest.current.text)
        throw new Error("草稿读取尚未完成，不能确认保存");
      return;
    }
    if (pendingRef.current) return;
    const payload = latest.current.snapshot();
    if (!payload.text && !current.lastText) {
      await queue.current;
      return;
    }
    const revision = ++current.revision;
    current.lastText = payload.text;
    const save = queue.current
      .catch(() => undefined)
      .then(async () => {
        const accepted = await call<boolean>("save_draft", {
          key: current.key,
          revision,
          payload,
        });
        if (!accepted && state.current === current)
          throw new Error("草稿窗口已关闭或版本已更新");
      })
      .catch((caught) => {
        if (state.current === current) throw caught;
      });
    queue.current = save;
    try {
      await save;
      if (state.current === current) setError("");
    } catch (caught) {
      if (state.current === current) setError(`草稿未保存：${String(caught)}`);
      throw caught;
    }
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void flush().catch(() => undefined);
    }, 500);
    return () => window.clearTimeout(timer);
  }, [telemetry.text, identity, flush]);

  useEffect(() => {
    const save = () => {
      void flush().catch(() => undefined);
    };
    window.addEventListener("pagehide", save);
    return () => window.removeEventListener("pagehide", save);
  }, [flush]);

  function restore() {
    if (
      !pending?.telemetryTrusted ||
      state.current?.identity !== JSON.stringify(pending.key)
    )
      return;
    latest.current.restore(pending.payload);
    if (state.current) state.current.lastText = pending.payload.text;
    pendingRef.current = null;
    setPending(null);
  }
  function discard() {
    if (state.current && pendingRef.current)
      state.current.lastText = pendingRef.current.payload.text;
    pendingRef.current = null;
    setPending(null);
    latest.current.reset();
    void flush().catch(() => undefined);
  }
  return { pending, error, flush, restore, discard };
}
