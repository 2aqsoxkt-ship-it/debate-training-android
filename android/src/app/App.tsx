import { ErrorBoundary } from "./ErrorBoundary";
import { AppRouter } from "./router";
import { MobileViewport } from "../platform/MobileViewport";

export function App() {
  return (
    <ErrorBoundary>
      <MobileViewport>
        <AppRouter />
      </MobileViewport>
    </ErrorBoundary>
  );
}
