import { HashRouter, Route, Routes } from "react-router";
import { SettingsPage } from "../features/settings/SettingsPage";
import { MatchPage } from "../features/match/MatchPage";
import { ReportPage } from "../features/report/ReportPage";
import { PracticePage } from "../features/report/PracticePage";
import { WorkspacePage } from "../features/workspace/WorkspacePage";
import { HomePage } from "./HomePage";

export function AppRouter() {
  return (
    <HashRouter>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/workspace" element={<WorkspacePage />} />
        <Route path="/match/:matchId" element={<MatchPage />} />
        <Route path="/report/:matchId" element={<ReportPage />} />
        <Route path="/practice/:practiceId" element={<PracticePage />} />
        <Route path="/settings" element={<SettingsPage />} />
      </Routes>
    </HashRouter>
  );
}
