import {
  useId,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";

function useOverlayFocus(open: boolean, onClose: () => void) {
  const container = useRef<HTMLElement | null>(null);
  const initialFocus = useRef<HTMLButtonElement | null>(null);

  useLayoutEffect(() => {
    if (!open) return;
    // Capture the opener before focusing the dialog, not after React autoFocus.
    const previousFocus = document.activeElement;
    initialFocus.current?.focus();
    return () => {
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) {
        previousFocus.focus();
      }
    };
  }, [open]);

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !container.current) return;
    const focusable = Array.from(
      container.current.querySelectorAll<HTMLElement>(
        "button, a[href], input, select, textarea, [tabindex]",
      ),
    ).filter(
      (element) =>
        element.tabIndex >= 0 &&
        !element.matches(":disabled") &&
        !element.closest('[hidden], [inert], [aria-hidden="true"]') &&
        window.getComputedStyle(element).display !== "none" &&
        window.getComputedStyle(element).visibility !== "hidden",
    );
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) {
      event.preventDefault();
      container.current.focus();
    } else if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return { container, initialFocus, onKeyDown };
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  onClose,
  onConfirm,
}: {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const focus = useOverlayFocus(open, onClose);
  const titleId = useId();
  if (!open) return null;

  return (
    <div className="overlay-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        ref={focus.container}
        className="confirm-dialog"
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-labelledby={titleId}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={focus.onKeyDown}
      >
        <p className="eyebrow">需要确认</p>
        <h2 id={titleId}>{title}</h2>
        <p>{description}</p>
        <div className="dialog-actions">
          <button ref={focus.initialFocus} type="button" onClick={onClose}>
            取消
          </button>
          <button type="button" className="danger-action" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </section>
    </div>
  );
}

export function Drawer({
  open,
  title,
  onClose,
  children,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const focus = useOverlayFocus(open, onClose);
  if (!open) return null;

  return (
    <div className="overlay-backdrop drawer-backdrop" onMouseDown={onClose}>
      <aside
        ref={focus.container}
        className="app-drawer"
        role="dialog"
        tabIndex={-1}
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={focus.onKeyDown}
      >
        <header>
          <div>
            <p className="eyebrow">辅助信息</p>
            <h2>{title}</h2>
          </div>
          <button
            type="button"
            ref={focus.initialFocus}
            onClick={onClose}
            aria-label={`关闭${title}`}
          >
            ×
          </button>
        </header>
        <div className="drawer-content">{children}</div>
      </aside>
    </div>
  );
}
