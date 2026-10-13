import { useEffect, useRef, useState } from "react";

/**
 * Projects Rust `remainingMs` for display only.
 * Never mutates match state — caller must submit via authoritative commands on timeout.
 */
export function useAuthoritativeClock(
  remainingMs: number,
  options: {
    frozen?: boolean;
    cursor?: number;
    onTimeout?: () => void;
  } = {},
) {
  const { frozen = false, cursor = 0, onTimeout } = options;
  const [shownMs, setShownMs] = useState(remainingMs);
  const firedRef = useRef(false);
  const onTimeoutRef = useRef(onTimeout);
  onTimeoutRef.current = onTimeout;

  useEffect(() => {
    firedRef.current = false;
    setShownMs(remainingMs);
    if (frozen) {
      return;
    }
    const base = remainingMs;
    const started = performance.now();
    const timer = window.setInterval(() => {
      const next = Math.max(0, base - Math.round(performance.now() - started));
      setShownMs(next);
      if (next <= 0 && !firedRef.current) {
        firedRef.current = true;
        onTimeoutRef.current?.();
      }
    }, 200);
    return () => window.clearInterval(timer);
  }, [remainingMs, frozen, cursor]);

  return shownMs;
}
