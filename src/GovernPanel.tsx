import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ScenarioStats {
  pending: number;
  summary: string;
}
interface ScenarioRunResult {
  attempted: number;
  succeeded: number;
  failed: number;
  snapshot_ids: string[];
  summary: string;
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

export function GovernPanel() {
  const [pcStats, setPcStats] = useState<ScenarioStats | null>(null);
  const [kaStats, setKaStats] = useState<ScenarioStats | null>(null);
  const [scStats, setScStats] = useState<ScenarioStats | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotManifest[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const [pc, ka, sc, snaps] = await Promise.all([
        invoke<ScenarioStats>("govern_scan_pc_namespace"),
        invoke<ScenarioStats>("govern_scan_keepalive"),
        invoke<ScenarioStats>("govern_scan_shortcuts"),
        invoke<SnapshotManifest[]>("list_snapshots"),
      ]);
      setPcStats(pc);
      setKaStats(ka);
      setScStats(sc);
      setSnapshots(snaps);
    } catch (e) {
      setLastResult(`刷新失败: ${e}`);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const runScenario = async (cmd: string, name: string) => {
    if (!confirm(`确认执行『${name}』? 所有动作会自动快照, 可一键还原。`)) return;
    setBusy(cmd);
    setLastResult(null);
    try {
      const r = await invoke<ScenarioRunResult>(cmd);
      setLastResult(r.summary);
      await refresh();
    } catch (e) {
      setLastResult(`执行失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const restoreAll = async (ids: string[]) => {
    if (!confirm(`一键还原 ${ids.length} 个快照?`)) return;
    setBusy("restore");
    try {
      for (const id of ids) {
        try {
          await invoke("restore_snapshot", { snapshotId: id });
        } catch (e) {
          console.error("还原失败", id, e);
        }
      }
      setLastResult(`已尝试还原 ${ids.length} 个快照`);
      await refresh();
    } finally {
      setBusy(null);
    }
  };

  const recentSnapshotsIds = snapshots.filter((s) => !s.restored_at && s.restorable !== false).map((s) => s.id);

  return (
    <div className="govern-panel">
      <section className="scenario-grid">
        <ScenarioCard
          title="清『此电脑』"
          desc="删除资源管理器『此电脑』里第三方塞的伪文件夹(网盘居多)。"
          stats={pcStats}
          busy={busy === "govern_clean_pc_namespace"}
          onRun={() => runScenario("govern_clean_pc_namespace", "清『此电脑』伪文件夹")}
        />
        <ScenarioCard
          title="停所有保活服务"
          desc="停止已知国产软件的保活/维护/升级服务并禁止开机自启。"
          stats={kaStats}
          busy={busy === "govern_stop_keepalive"}
          onRun={() => runScenario("govern_stop_keepalive", "停所有保活服务并禁自启")}
        />
        <ScenarioCard
          title="清流氓快捷方式"
          desc="删除桌面/开始菜单上命中流氓画像的 .lnk 快捷方式。"
          stats={scStats}
          busy={busy === "govern_clean_shortcuts"}
          onRun={() => runScenario("govern_clean_shortcuts", "清流氓快捷方式")}
        />
        <FileAssocCard />
      </section>

      {lastResult && <div className="scenario-result">{lastResult}</div>}

      <section>
        <h2>
          操作快照 <span className="count">{snapshots.length}</span>
          {recentSnapshotsIds.length > 0 && (
            <button onClick={() => restoreAll(recentSnapshotsIds)} className="btn-restore">
              一键还原所有未还原 ({recentSnapshotsIds.length})
            </button>
          )}
        </h2>
        {snapshots.length === 0 ? (
          <p className="muted">尚无快照。执行场景会自动建快照。</p>
        ) : (
          <SnapshotsTable snapshots={snapshots} onRefresh={refresh} />
        )}
      </section>
    </div>
  );
}

function ScenarioCard({
  title,
  desc,
  stats,
  busy,
  onRun,
}: {
  title: string;
  desc: string;
  stats: ScenarioStats | null;
  busy: boolean;
  onRun: () => void;
}) {
  return (
    <div className="scenario-card">
      <h3>{title}</h3>
      <p className="muted small">{desc}</p>
      <div className="scenario-stats">
        <div className="stat-pending">{stats?.pending ?? "—"}</div>
        <div className="stat-summary">{stats?.summary ?? "正在扫描..."}</div>
      </div>
      <button
        className="btn-exec scenario-btn"
        disabled={busy || (stats?.pending ?? 0) === 0}
        onClick={onRun}
      >
        {busy ? "执行中..." : "一键执行"}
      </button>
    </div>
  );
}

function FileAssocCard() {
  const [presets, setPresets] = useState<AssocPreset[]>([]);
  const [exePath, setExePath] = useState("");
  const [selectedPresets, setSelectedPresets] = useState<Set<string>>(new Set());
  const [customExts, setCustomExts] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  useEffect(() => {
    invoke<AssocPreset[]>("fileassoc_list_presets").then(setPresets);
  }, []);

  const toggle = (id: string) => {
    setSelectedPresets((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  };

  const allExts = [
    ...presets.filter((p) => selectedPresets.has(p.id)).flatMap((p) => p.extensions),
    ...customExts.split(/[\s,]+/).filter((s) => s.trim()).map((s) => s.trim()),
  ];

  const apply = async () => {
    if (!exePath.trim() || allExts.length === 0) return;
    setBusy(true);
    setResult(null);
    try {
      const r = await invoke<AssocResult>("fileassoc_set_app_defaults", {
        exePath: exePath.trim(),
        extensions: allExts,
      });
      const failed = r.extensions_failed.length;
      setResult(
        `成功设了 ${r.extensions_set.length} 个扩展名 (UserChoice 清了 ${r.userchoice_cleared.length} 个),失败 ${failed} 个。首次打开此类文件 Windows 会让你选打开方式,选 "${r.progid}" 并勾"始终"即生效。`
      );
    } catch (e) {
      setResult(`失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="scenario-card">
      <h3>默认打开方式</h3>
      <p className="muted small">选个 exe + 勾选格式预设,把这些格式的默认打开方式都给它。</p>
      <input
        type="text"
        placeholder="exe 完整路径,例如 C:\Program Files\PotPlayer\PotPlayerMini64.exe"
        value={exePath}
        onChange={(e) => setExePath(e.target.value)}
        className="assoc-input"
      />
      <div className="preset-chips">
        {presets.map((p) => (
          <label key={p.id} className={`chip ${selectedPresets.has(p.id) ? "on" : ""}`}>
            <input
              type="checkbox"
              checked={selectedPresets.has(p.id)}
              onChange={() => toggle(p.id)}
            />
            {p.label} ({p.extensions.length})
          </label>
        ))}
      </div>
      <input
        type="text"
        placeholder="自定义扩展名(逗号或空格分隔, 如 .iso .srt)"
        value={customExts}
        onChange={(e) => setCustomExts(e.target.value)}
        className="assoc-input"
      />
      <div className="muted small">将设置 {allExts.length} 个扩展名</div>
      <button
        className="btn-exec scenario-btn"
        disabled={busy || !exePath.trim() || allExts.length === 0}
        onClick={apply}
      >
        {busy ? "..." : "设为默认"}
      </button>
      {result && <div className="scenario-result small">{result}</div>}
    </div>
  );
}

function SnapshotsTable({ snapshots, onRefresh }: { snapshots: SnapshotManifest[]; onRefresh: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const restore = async (id: string) => {
    if (!confirm(`确认还原 ${id.slice(0, 22)}?`)) return;
    setBusy(id);
    try {
      await invoke("restore_snapshot", { snapshotId: id });
      onRefresh();
    } catch (e) {
      alert(`还原失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };
  return (
    <table>
      <thead>
        <tr>
          <th>时间</th>
          <th>动作</th>
          <th>目标</th>
          <th>状态</th>
          <th>还原</th>
        </tr>
      </thead>
      <tbody>
        {snapshots.slice(0, 30).map((s) => (
          <tr key={s.id}>
            <td className="small">{new Date(s.created_at).toLocaleString("zh-CN")}</td>
            <td><code>{s.action_kind}</code></td>
            <td className="mono small">{s.action_target}</td>
            <td>{s.restored_at ? <span className="muted">已还原</span> : <span className="ok">可还原</span>}</td>
            <td>
              <button
                className="btn-restore"
                disabled={!!s.restored_at || s.restorable === false || busy !== null}
                onClick={() => restore(s.id)}
              >
                {s.restorable === false ? "不可逆" : busy === s.id ? "..." : "还原"}
              </button>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
