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
  const [showAi, setShowAi] = useState(false);
  const [relaunching, setRelaunching] = useState(false);

  useEffect(() => {
    invoke<ElevationStatus>("check_elevation").then(setElev).catch(() => {});
  }, []);

  const relaunch = async () => {
    setRelaunching(true);
    try {
      await invoke("relaunch_as_admin");
    } catch (e) {
      alert(`重启失败: ${e}\n\n你可以在文件管理器找到此程序,右键 → "以管理员身份运行"`);
      setRelaunching(false);
    }
  };

  // 未提权: 整个主界面被红色遮罩盖住, 只有一个超大按钮
  if (elev && !elev.is_elevated) {
    return (
      <div className="uac-blocker">
        <div className="uac-content">
          <div className="uac-icon">🛡️</div>
          <h1>需要管理员权限</h1>
          <p>明窗要改你电脑的系统设置,需要先以管理员身份打开。</p>
          <button className="uac-btn" onClick={relaunch} disabled={relaunching}>
            {relaunching ? "正在重启..." : "点这里,以管理员身份重启"}
          </button>
          <small>会弹出 Windows 的「用户账户控制」对话框,点「是」即可。</small>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <div className="topbar">
        <div className="topbar-left">明窗 <span className="muted small">让 Windows 重新明亮</span></div>
        <div className="topbar-right">
          <button className="icon-btn" onClick={() => setShowAi(true)} title="问问 AI">
            💬
          </button>
        </div>
      </div>

      <GovernPanel />

      {showAi && (
        <div className="ai-drawer-backdrop" onClick={() => setShowAi(false)}>
          <div className="ai-drawer" onClick={(e) => e.stopPropagation()}>
            <button className="drawer-close" onClick={() => setShowAi(false)}>
              ✕
            </button>
            <AiPanel />
          </div>
        </div>
      )}
    </div>
  );
}
