import { Component, type ErrorInfo, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo): void {
    // 不把原始异常写入界面。日志脱敏在 Rust 侧统一处理。
  }

  render() {
    if (this.state.hasError) {
      return (
        <main className="shell">
          <h1>应用出现问题</h1>
          <p>界面发生未处理错误。本机数据不会因此离开这台电脑。</p>
        </main>
      );
    }

    return this.props.children;
  }
}
