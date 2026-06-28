import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

type Phase = "idle" | "checking" | "uptodate" | "available" | "downloading" | "installing" | "ready" | "error";

export function UpdaterCard() {
  const [currentVersion, setCurrentVersion] = useState<string>("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<{ downloaded: number; total: number } | null>(null);
  const [error, setError] = useState<string>("");

  useEffect(() => {
    getVersion().then(setCurrentVersion).catch(() => setCurrentVersion("?"));
  }, []);

  const onCheck = async () => {
    setPhase("checking");
    setError("");
    try {
      const u = await check();
      if (u) {
        setUpdate(u);
        setPhase("available");
      } else {
        setPhase("uptodate");
      }
    } catch (e: unknown) {
      setError(String(e));
      setPhase("error");
    }
  };

  const onInstall = async () => {
    if (!update) return;
    setPhase("downloading");
    setError("");
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            setProgress({ downloaded: 0, total });
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            setProgress({ downloaded, total });
            break;
          case "Finished":
            setPhase("installing");
            break;
        }
      });
      setPhase("ready");
    } catch (e: unknown) {
      setError(String(e));
      setPhase("error");
    }
  };

  const onRestart = async () => {
    try {
      await relaunch();
    } catch (e: unknown) {
      setError(`重启失败, 请手动重开应用: ${e}`);
    }
  };

  const mb = (b: number) => (b / 1024 / 1024).toFixed(1);
  const pct = progress && progress.total > 0 ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100)) : 0;

  return (
    <div className="updater-card">
      <div className="updater-row">
        <span className="updater-label">版本</span>
        <span className="updater-version">v{currentVersion}</span>
        {phase === "idle" && (
          <button className="updater-btn" onClick={onCheck}>检查更新</button>
        )}
        {phase === "checking" && <span className="muted small">查询 GitHub...</span>}
        {phase === "uptodate" && (
          <>
            <span className="updater-ok">✓ 已是最新</span>
            <button className="updater-btn ghost" onClick={onCheck}>再查一次</button>
          </>
        )}
        {phase === "available" && update && (
          <>
            <span className="updater-new">→ v{update.version} 可更新</span>
            <button className="updater-btn primary" onClick={onInstall}>下载并安装</button>
          </>
        )}
        {phase === "downloading" && progress && (
          <span className="updater-progress">
            下载中 {mb(progress.downloaded)} / {mb(progress.total)} MB ({pct}%)
          </span>
        )}
        {phase === "installing" && <span className="muted small">安装中...</span>}
        {phase === "ready" && (
          <>
            <span className="updater-ok">✓ 已安装, 重启生效</span>
            <button className="updater-btn primary" onClick={onRestart}>立即重启</button>
          </>
        )}
        {phase === "error" && (
          <>
            <span className="updater-err">更新失败</span>
            <button className="updater-btn ghost" onClick={() => { setPhase("idle"); setError(""); }}>关闭</button>
          </>
        )}
      </div>
      {phase === "available" && update?.body && (
        <div className="updater-notes">
          <div className="updater-notes-title">更新内容</div>
          <pre>{update.body}</pre>
        </div>
      )}
      {phase === "downloading" && (
        <div className="updater-bar"><div className="updater-bar-fill" style={{ width: `${pct}%` }} /></div>
      )}
      {error && <div className="updater-err-msg">{error}</div>}
    </div>
  );
}
