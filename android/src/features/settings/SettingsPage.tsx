import { FormEvent, useEffect, useState } from "react";
import { Link } from "react-router";
import { call } from "../../lib/tauri";

export type ModelSettings = {
  modelId: string;
  baseUrl: string;
  reasoningLevel: string;
  connectionStatus: string;
  lastTestedAt: string;
  lastErrorCategory: string;
  hasApiKey: boolean;
};

const empty: ModelSettings = {
  modelId: "deepseek-flash",
  baseUrl: "https://api.deepseek.com",
  reasoningLevel: "high",
  connectionStatus: "not_configured",
  lastTestedAt: "",
  lastErrorCategory: "",
  hasApiKey: false,
};

function statusLabel(status: string) {
  switch (status) {
    case "not_configured":
      return "未配置密钥";
    case "untested":
      return "已保存，未测试";
    case "testing":
      return "正在测试";
    case "ready":
      return "已连接";
    case "authentication_failed":
      return "认证失败";
    case "network_failed":
      return "网络失败";
    case "timeout":
      return "超时";
    case "model_unavailable":
      return "模型不可用";
    case "insufficient_balance":
      return "DeepSeek 余额不足";
    case "incompatible_response":
      return "响应不兼容";
    default:
      return status;
  }
}

export function SettingsPage() {
  const [settings, setSettings] = useState<ModelSettings>(empty);
  const [apiKey, setApiKey] = useState("");
  const [modelId, setModelId] = useState(empty.modelId);
  const [baseUrl, setBaseUrl] = useState(empty.baseUrl);
  const [reasoningLevel, setReasoningLevel] = useState(empty.reasoningLevel);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [desktop, setDesktop] = useState<boolean | null>(null);

  useEffect(() => {
    void call<ModelSettings>("get_model_settings")
      .then((next) => {
        setSettings(next);
        setModelId(next.modelId);
        setBaseUrl(next.baseUrl);
        setReasoningLevel(next.reasoningLevel);
        setDesktop(true);
      })
      .catch(() => setDesktop(false));
  }, []);

  async function run(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<void> {
    setError("");
    setBusy(true);
    try {
      const next = await call<ModelSettings>(command, args);
      setSettings(next);
      setModelId(next.modelId);
      setBaseUrl(next.baseUrl);
      setReasoningLevel(next.reasoningLevel);
      if (command === "save_api_key" || command === "clear_api_key") {
        setApiKey("");
      }
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setBusy(false);
    }
  }

  function onSaveKey(event: FormEvent) {
    event.preventDefault();
    void run("save_api_key", { apiKey });
  }

  function onSaveModel(event: FormEvent) {
    event.preventDefault();
    void run("update_model_settings", {
      modelId,
      baseUrl,
      reasoningLevel,
    });
  }

  return (
    <main className="workspace">
      <p className="eyebrow">本机设置</p>
      <h1>DeepSeek 连接</h1>
      <p>
        <Link to="/workspace">返回工作台</Link>
      </p>
      {desktop === false ? (
        <p className="error">
          这是浏览器预览，不会读写设备密钥。请在安卓应用内配置连接。
        </p>
      ) : null}
      {error ? <p className="error">{error}</p> : null}
      <section>
        <h2>连接状态</h2>
        <p role="status">
          {statusLabel(settings.connectionStatus)}
          {settings.hasApiKey ? " · 本机已保存密钥" : " · 尚未保存密钥"}
        </p>
        {settings.lastTestedAt ? (
          <p>最近测试：{settings.lastTestedAt}</p>
        ) : null}
        {settings.lastErrorCategory ? (
          <p className="error">错误类别：{settings.lastErrorCategory}</p>
        ) : null}
        <p>
          密钥由 Android Keystore 加密保护，仅在本机保存密文。
          界面、数据库、日志和报告都不会回传明文密钥。
        </p>
        {settings.connectionStatus !== "ready" ? (
          <p className="warning">
            {settings.connectionStatus === "insufficient_balance"
              ? "请先到 DeepSeek 控制台充值，再重新测试连接。"
              : "当前未验证连接。比赛向导仍可使用本地模拟预览，但模拟数据不能代表真实 AI 能力。"}
          </p>
        ) : null}
      </section>
      <section>
        <h2>API Key</h2>
        <form onSubmit={onSaveKey}>
          <label>
            密钥
            <input
              type="password"
              autoComplete="off"
              value={apiKey}
              placeholder={settings.hasApiKey ? "••••••••" : "sk-..."}
              onChange={(event) => setApiKey(event.target.value)}
            />
          </label>
          <button type="submit" disabled={busy || !apiKey.trim()}>
            保存密钥
          </button>
          <button
            type="button"
            disabled={busy || !settings.hasApiKey}
            onClick={() => void run("test_model_connection")}
          >
            测试连接
          </button>
          <button
            type="button"
            disabled={busy || !settings.hasApiKey}
            onClick={() => void run("clear_api_key")}
          >
            清除密钥
          </button>
        </form>
      </section>
      <section>
        <h2>高级设置</h2>
        <form onSubmit={onSaveModel}>
          <label>
            模型 ID
            <input
              value={modelId}
              onChange={(event) => setModelId(event.target.value)}
            />
          </label>
          <label>
            Base URL
            <input
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
            />
          </label>
          <label>
            思考档位
            <select
              value={reasoningLevel}
              onChange={(event) => setReasoningLevel(event.target.value)}
            >
              <option value="low">low</option>
              <option value="high">high</option>
              <option value="max">max</option>
            </select>
          </label>
          <button type="submit" disabled={busy}>
            保存模型设置
          </button>
        </form>
      </section>
    </main>
  );
}
