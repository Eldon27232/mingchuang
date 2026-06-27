import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AiPanel } from "./AiPanel";
import { GovernPanel } from "./GovernPanel";

interface ElevationStatus {
  is_elevated: boolean;
  message: string;
}

export default function App() {
  const [elev, setElev] = useState<ElevationStatus | null>(null);
  const [tab, setTab] = useState<"govern" | "ai">("govern");

  useEffect(() => {
    invoke<ElevationStatus>("check_elevation").then(setElev).catch(() => {});
  }, []);

  return (
    <div className="app">
      <header>
        <h1>
          kuake-fuckyou <span className="ver">v0.0.1</span>
        </h1>
        <p className="subtitle">
          整治国产流氓软件的 Windows 11 治理工具 · 一次跑完即退出 · 不常驻后台
        </p>
      </header>

      {elev && (
        <div className={`banner ${elev.is_elevated ? "ok" : "warn"}`}>
          <span className="badge-small">{elev.is_elevated ? "✓ 管理员" : "⚠ 未提权"}</span>
          <span>{elev.message}</span>
        </div>
      )}

      <div className="tabs">
        <button className={tab === "govern" ? "tab active" : "tab"} onClick={() => setTab("govern")}>
          治理面板
        </button>
        <button className={tab === "ai" ? "tab active" : "tab"} onClick={() => setTab("ai")}>
          AI 助手 ✨
        </button>
      </div>

      {tab === "govern" && <GovernPanel />}
      {tab === "ai" && <AiPanel />}

      <footer>
        <small>v0.0.1 · 一键场景治理 + 默认打开方式 + AI 双 agent · 不常驻</small>
      </footer>
    </div>
  );
}
