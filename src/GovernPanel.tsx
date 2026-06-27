import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { humanizeAction } from "./labels";

interface ScenarioStats {
  pending: number;
  summary: string;
  display_names: string[];
}
interface ScenarioRunResult {
  attempted: number;
  succeeded: number;
  failed: number;
  snapshot_ids: string[];
  summary: string;
}
interface InstalledApp {
  key: string;
  display_name: string;
  category: string;
  exe_path: string;
}
interface AssocPreset {
  id: string;
  label: string;
  extensions: string[];
}
interface AssocResult {
  exe: string;
  progid: string;
  extensions_set: string[];
  extensions_failed: [string, string][];
  userchoice_cleared: string[];
}
interface SnapshotManifest {
  id: string;
  created_at: string;
  action_kind: string;
  action_target: string;
  action_reason: string;
  restored_at?: string;
  restorable?: boolean;
}

const CAT_ICON: Record<string, string> = {
  video: "🎬", music: "🎵", archive: "📦", image: "🖼️", doc: "📄",
};
const PRESET_ICON: Record<string, string> = {
  music: "🎵", video: "🎬", archive: "📦", image: "🖼️", doc: "📄",
};

export function GovernPanel() {
  const [pc, setPc] = useState<ScenarioStats | null>(null);
  const [ka, setKa] = useState<ScenarioStats | null>(null);
  const [sc, setSc] = useState<ScenarioStats | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotManifest[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [scanning, setScanning] = useState(true);
  const [recentBatch, setRecentBatch] = useState<{ summary: string; snapshotIds: string[] } | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showUndo, setShowUndo] = useState(false);

  const refresh = async () => {
    setScanning(true);
    // 分别 invoke 而不是 Promise.all,某一个慢不影响其他先显示
    invoke<ScenarioStats>("govern_scan_pc_namespace").then(setPc).catch(e => console.error("scan_pc", e));
    invoke<ScenarioStats>("govern_scan_keepalive").then(setKa).catch(e => console.error("scan_ka", e));
    invoke<ScenarioStats>("govern_scan_shortcuts").then(setSc).catch(e => console.error("scan_sc", e));
    invoke<SnapshotManifest[]>("list_snapshots").then(setSnapshots).catch(e => console.error("list_snap", e));
    // 总 timeout 标记 scanning 结束(实际上各自 setState 后视图就 OK)
    setTimeout(() => setScanning(false), 8000);
  };

  useEffect(() => { refresh(); }, []);

  // 任意一个 scan 完成就视作 scan 结束
  useEffect(() => {
    if (pc && ka && sc) setScanning(false);
  }, [pc, ka, sc]);

  const totalPending = (pc?.pending || 0) + (ka?.pending || 0) + (sc?.pending || 0);
  const anyScanned = pc !== null || ka !== null || sc !== null;
  const allDisplayNames = [
    ...(pc?.display_names || []),
    ...(ka?.display_names || []),
    ...(sc?.display_names || []),
  ];
  const uniqueDisplayNames = Array.from(new Set(allDisplayNames));

  const oneClickAll = async () => {
    if (totalPending === 0) return;
    setBusy("oneclick");
    try {
      const collected: string[] = [];
      const summaries: string[] = [];
      if ((pc?.pending || 0) > 0) {
        const r = await invoke<ScenarioRunResult>("govern_clean_pc_namespace");
        collected.push(...r.snapshot_ids);
        summaries.push(`清掉「我的电脑」${r.succeeded} 项`);
      }
      if ((ka?.pending || 0) > 0) {
        const r = await invoke<ScenarioRunResult>("govern_stop_keepalive");
        collected.push(...r.snapshot_ids);
        summaries.push(`关掉后台 ${r.succeeded / 2 | 0} 个`);
      }
      if ((sc?.pending || 0) > 0) {
        const r = await invoke<ScenarioRunResult>("govern_clean_shortcuts");
        collected.push(...r.snapshot_ids);
        summaries.push(`清了 ${r.succeeded} 个快捷方式`);
      }
      setRecentBatch({
        summary: summaries.join(" · "),
        snapshotIds: collected,
      });
      await refresh();
    } catch (e) {
      alert(`执行失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const runOne = async (cmd: string) => {
    setBusy(cmd);
    try {
      const r = await invoke<ScenarioRunResult>(cmd);
      setRecentBatch({ summary: r.summary, snapshotIds: r.snapshot_ids });
      await refresh();
    } catch (e) {
      alert(`执行失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const undoBatch = async () => {
    if (!recentBatch) return;
    setBusy("undo");
    try {
      for (const id of recentBatch.snapshotIds) {
        try { await invoke("restore_snapshot", { snapshotId: id }); } catch {}
      }
      setRecentBatch(null);
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="govern-panel">
      {/* === 一键体检主按钮 === */}
      <div className="hero-card">
        <div className="hero-title">一键体检 + 清理</div>
        <div className="hero-sub">扫一扫,清掉国产软件塞进系统的垃圾</div>
        {scanning && !anyScanned && (
          <div className="muted small">
            <span className="thinking-dots">正在扫描你的电脑</span>
          </div>
        )}
        {totalPending > 0 && (
          <div className="hero-names">
            发现:
            {uniqueDisplayNames.slice(0, 6).map((n, i) => (
              <span key={i} className="vendor-chip">{n}</span>
            ))}
            {uniqueDisplayNames.length > 6 && (
              <span className="muted small">等 {uniqueDisplayNames.length} 项</span>
            )}
          </div>
        )}
        <button
          className="hero-btn"
          disabled={busy !== null || !anyScanned || totalPending === 0}
          onClick={oneClickAll}
        >
          {busy === "oneclick" ? "正在清理..." :
            !anyScanned ? "扫描中..." :
              totalPending === 0 ? "✓ 你的电脑很干净,无需清理" :
                `清掉这 ${totalPending} 项问题`}
        </button>
      </div>

      {/* === 撤销条 === */}
      {recentBatch && (
        <div className="undo-bar">
          <span>✓ 刚才{recentBatch.summary} · 不满意可以撤回</span>
          <div>
            <button onClick={undoBatch} disabled={busy === "undo"}>
              {busy === "undo" ? "撤回中..." : "撤回这次"}
            </button>
            <button onClick={() => setRecentBatch(null)} className="ghost">关闭</button>
          </div>
        </div>
      )}

      {/* === 默认打开方式 (核心功能 ①) === */}
      <FileAssocCard />

      {/* === 高级:分别处理 === */}
      <div className="advanced-toggle" onClick={() => setShowAdvanced(!showAdvanced)}>
        {showAdvanced ? "▼" : "▶"} 高级:分别处理
      </div>
      {showAdvanced && (
        <div className="scenario-grid">
          <SimpleCard
            title="清「我的电脑」里的图标"
            stats={pc}
            busy={busy === "govern_clean_pc_namespace"}
            runLabel="清掉这些图标"
            onRun={() => runOne("govern_clean_pc_namespace")}
          />
          <SimpleCard
            title="关掉国产软件的后台"
            stats={ka}
            busy={busy === "govern_stop_keepalive"}
            runLabel="关掉这些后台"
            onRun={() => runOne("govern_stop_keepalive")}
          />
          <SimpleCard
            title="清桌面/开始菜单快捷方式"
            stats={sc}
            busy={busy === "govern_clean_shortcuts"}
            runLabel="清掉这些快捷方式"
            onRun={() => runOne("govern_clean_shortcuts")}
          />
        </div>
      )}

      {/* === 历史记录 === */}
      <div className="history-toggle" onClick={() => setShowUndo(!showUndo)}>
        ↶ 历史操作 ({snapshots.length})
      </div>
      {showUndo && (
        <div className="history-list">
          {snapshots.slice(0, 20).map((s) => (
            <HistoryItem key={s.id} s={s} onRefresh={refresh} />
          ))}
          {snapshots.length === 0 && <p className="muted">还没有操作过任何东西。</p>}
        </div>
      )}
    </div>
  );
}

function SimpleCard({
  title, stats, busy, runLabel, onRun,
}: {
  title: string;
  stats: ScenarioStats | null;
  busy: boolean;
  runLabel: string;
  onRun: () => void;
}) {
  const empty = stats?.pending === 0;
  return (
    <div className={`simple-card ${empty ? "empty" : ""}`}>
      <h4>{title}</h4>
      <div className="simple-stats">
        {empty ? (
          <span className="ok">✓ 干净, 无需处理</span>
        ) : (
          <>
            <span className="big-num">{stats?.pending ?? "—"}</span>
            <span className="muted small">{stats?.summary || "..."}</span>
          </>
        )}
      </div>
      {stats?.display_names && stats.display_names.length > 0 && (
        <div className="vendor-list">
          {stats.display_names.map((n, i) => (
            <span key={i} className="vendor-chip small-chip">{n}</span>
          ))}
        </div>
      )}
      <button
        className="btn-exec simple-btn"
        disabled={empty || busy}
        onClick={onRun}
      >
        {busy ? "..." : runLabel}
      </button>
    </div>
  );
}

function HistoryItem({ s, onRefresh }: { s: SnapshotManifest; onRefresh: () => void }) {
  const [busy, setBusy] = useState(false);
  const restore = async () => {
    setBusy(true);
    try {
      await invoke("restore_snapshot", { snapshotId: s.id });
      onRefresh();
    } catch (e) {
      alert(`撤回失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };
  const time = new Date(s.created_at).toLocaleString("zh-CN", {
    month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit",
  });
  return (
    <div className={`history-item ${s.restored_at ? "restored" : ""}`}>
      <div className="history-info">
        <div className="history-action">{humanizeAction(s.action_kind, s.action_target)}</div>
        <div className="history-time">{time}</div>
      </div>
      <button
        disabled={!!s.restored_at || s.restorable === false || busy}
        onClick={restore}
        className="btn-restore"
      >
        {s.restored_at ? "已撤回" : s.restorable === false ? "不可撤回" : busy ? "..." : "撤回"}
      </button>
    </div>
  );
}

function FileAssocCard() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [presets, setPresets] = useState<AssocPreset[]>([]);
  const [selectedApp, setSelectedApp] = useState<InstalledApp | null>(null);
  const [selectedPresets, setSelectedPresets] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [customApps, setCustomApps] = useState<InstalledApp[]>([]);

  useEffect(() => {
    invoke<InstalledApp[]>("fileassoc_detect_installed_apps").then(setApps);
    invoke<AssocPreset[]>("fileassoc_list_presets").then(setPresets);
  }, []);

  const togglePreset = (id: string) => {
    setSelectedPresets((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  };

  const onPickApp = (app: InstalledApp) => {
    setSelectedApp(app);
    // 自定义 app 不预选 category,系统 app 预选
    if (!customApps.find(c => c.key === app.key)) {
      setSelectedPresets(new Set([app.category]));
    }
  };

  const pickCustomExe = async () => {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "可执行文件", extensions: ["exe"] }],
      });
      if (!picked || typeof picked !== "string") return;
      const exePath = picked as string;
      const nameWithExt = exePath.split(/[\\/]/).pop() || exePath;
      const stem = nameWithExt.replace(/\.exe$/i, "");
      const custom: InstalledApp = {
        key: `custom:${exePath}`,
        display_name: stem,
        category: "custom",
        exe_path: exePath,
      };
      setCustomApps((arr) => {
        if (arr.find((a) => a.exe_path === exePath)) return arr;
        return [...arr, custom];
      });
      onPickApp(custom);
    } catch (e) {
      alert(`选择文件失败: ${e}`);
    }
  };

  const allApps = [...apps, ...customApps];

  const allExts = presets.filter((p) => selectedPresets.has(p.id)).flatMap((p) => p.extensions);

  const apply = async () => {
    if (!selectedApp || allExts.length === 0) return;
    setBusy(true);
    setResult(null);
    try {
      const r = await invoke<AssocResult>("fileassoc_set_app_defaults", {
        exePath: selectedApp.exe_path,
        extensions: allExts,
      });
      setResult(
        `搞定。下次双击这类文件时,Windows 可能问一次「用哪个软件打开」,选「${selectedApp.display_name}」并勾「始终」就一劳永逸。`
      );
    } catch (e) {
      setResult(`失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="hero-card assoc-hero">
      <div className="hero-title">换默认打开方式</div>
      <div className="hero-sub">选个软件 + 选格式, 让 Windows 默认用它打开</div>

      <div className="step-label">① 选个你信任的软件</div>
      <div className="app-grid">
        {allApps.map((a) => (
          <div
            key={a.key}
            className={`app-tile ${selectedApp?.key === a.key ? "selected" : ""}`}
            onClick={() => onPickApp(a)}
            title={a.exe_path}
          >
            <div className="app-icon">{CAT_ICON[a.category] || "🛠️"}</div>
            <div className="app-name">{a.display_name}</div>
          </div>
        ))}
        <div className="app-tile add-custom" onClick={pickCustomExe} title="从文件管理器挑一个 exe">
          <div className="app-icon">＋</div>
          <div className="app-name">我自己选 exe</div>
        </div>
      </div>
      {apps.length === 0 && customApps.length === 0 && (
        <p className="muted small">没在本机找到推荐的应用,点 + 自己选一个 exe。</p>
      )}

      {selectedApp && (
        <>
          <div className="step-label">② 选要交给它的文件类型</div>
          <div className="preset-chips">
            {presets.map((p) => (
              <label key={p.id} className={`chip ${selectedPresets.has(p.id) ? "on" : ""}`}>
                <input
                  type="checkbox"
                  checked={selectedPresets.has(p.id)}
                  onChange={() => togglePreset(p.id)}
                />
                {PRESET_ICON[p.id] || ""} {p.label} ({p.extensions.length} 种)
              </label>
            ))}
          </div>

          <button
            className="hero-btn"
            disabled={busy || allExts.length === 0}
            onClick={apply}
          >
            {busy ? "..." : `把这 ${allExts.length} 种文件交给 ${selectedApp.display_name}`}
          </button>
        </>
      )}

      {result && <div className="result-box">{result}</div>}
    </div>
  );
}
