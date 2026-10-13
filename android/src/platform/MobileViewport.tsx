import { useLayoutEffect, useRef, type ReactNode } from "react";

/** Resizing changes presentation only: it never remounts a match or its drafts. */
export function MobileViewport({ children }: { children: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const node = root.current;
    if (!node) return;
    const viewport = window.visualViewport;
    let frame: number | undefined;
    let active = true;
    let keyboardWasOpen = false;

    function measure() {
      frame = undefined;
      if (!active || !node) return;
      const focused = document.activeElement;
      const editing =
        focused instanceof HTMLElement &&
        focused.matches(
          "textarea, input:not([type=checkbox]):not([type=radio]), [contenteditable=true]",
        );
      const height = viewport?.height ?? window.innerHeight;
      const keyboardOpen =
        editing &&
        (viewport?.scale ?? 1) === 1 &&
        window.innerHeight - height - (viewport?.offsetTop ?? 0) > 120;
      node.style.setProperty("--visible-height", `${height}px`);
      node.dataset.keyboardOpen = String(keyboardOpen);
      if (keyboardOpen && !keyboardWasOpen && focused instanceof HTMLElement) {
        focused.scrollIntoView?.({ block: "nearest", behavior: "instant" });
      }
      keyboardWasOpen = keyboardOpen;
    }

    function schedule() {
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(measure);
    }

    measure();
    viewport?.addEventListener("resize", schedule);
    viewport?.addEventListener("scroll", schedule);
    window.addEventListener("resize", schedule);
    document.addEventListener("focusin", schedule);
    document.addEventListener("focusout", schedule);
    return () => {
      active = false;
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      viewport?.removeEventListener("resize", schedule);
      viewport?.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      document.removeEventListener("focusin", schedule);
      document.removeEventListener("focusout", schedule);
    };
  }, []);

  return (
    <div ref={root} className="mobile-shell" data-keyboard-open="false">
      {children}
    </div>
  );
}
