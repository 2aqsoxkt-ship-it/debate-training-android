import { useNavigate } from "react-router";
import splashBackground from "../assets/debate-hall-splash.png";

export function HomePage() {
  const navigate = useNavigate();

  return (
    <main
      className="launch-splash"
      style={{ backgroundImage: `url(${splashBackground})` }}
    >
      <section className="launch-card">
        <p className="eyebrow">DEBATE TRAINING STUDIO</p>
        <h1>辩论训练智能体</h1>
        <p className="launch-lead">
          准备你的辩题、赛程与辩手席位，由你决定何时开始
        </p>
        <button type="button" onClick={() => navigate("/workspace")}>
          进入赛前工作台
        </button>
        <small>比赛记录默认只保存在本机 · 适配安卓手机与平板</small>
      </section>
    </main>
  );
}
