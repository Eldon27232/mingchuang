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
interface AssocApp {
  key: string;
  display_name: string;
  exe_path: string;
  extensions: string[];
}
interface AssocManifest {
  apps: AssocApp[];
}
interface AssocResult {
  exe: string;
  progid: string;
  extensions_set: string[];
  extensions_failed: [string, string][];
  extensions_need_manual: string[];
}
interface ApplyAllResult {
  total_apps: number;
  applied_extensions: number;
  failed: [string, string, string][];
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
  video: "🎬", music: "🎵", archive: "📦", image: "🖼️", doc: "📄", custom: "🛠️",
};
const PRESET_ICON: Record<string, string> = {
  music: "🎵", video: "🎬", archive: "📦", image: "🖼️", doc: "📄",
};

export function GovernPanel() {
  const [pc, setPc] = useState<ScenarioStats | null>(null);
  const [ka, setKa] = useState<ScenarioStats | null>(null);
  const [snapshots, setSnapshots] = useState<SnapshotManifest[]>([]);
  const [manifest, setManifest] = useState<AssocManifest | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [scanning, setScanning] = useState(true);
  const [recentBatch, setRecentBatch] = useState<{ summary: string; snapshotIds: string[] } | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showUndo, setShowUndo] = useState(false);

  const refresh = async () => {
    setScanning(true);
    invoke<ScenarioStats>("govern_scan_pc_namespace").then(setPc).catch(e => console.error("scan_pc", e));
    invoke<ScenarioStats>("govern_scan_keepalive").then(setKa).catch(e => console.error("scan_ka", e));
    invoke<SnapshotManifest[]>("list_snapshots").then(setSnapshots).catch(e => console.error("list_snap", e));
    invoke<AssocManifest>("fileassoc_get_manifest").then(setManifest).catch(e => console.error("manifest", e));
    setTimeout(() => setScanning(false), 8000);
  };

  useEffect(() => { refresh(); }, []);
  useEffect(() => {
    if (pc && ka) setScanning(false);
  }, [pc, ka]);

  const totalPending = (pc?.pending || 0) + (ka?.pending || 0);
  const anyScanned = pc !== null || ka !== null;
  const uniqueDisplayNames = Array.from(new Set([
    ...(pc?.display_names || []),
    ...(ka?.display_names || []),
  ]));

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
        summaries.push(`关掉后台 ${(r.succeeded / 2) | 0} 个`);
      }
      setRecentBatch({ summary: summaries.join(" · "), snapshotIds: collected });
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

  const applyAllAssoc = async () => {
    setBusy("apply_all");
    try {
      const r = await invoke<ApplyAllResult>("fileassoc_apply_all");
      if (r.failed.length === 0) {
        alert(`✓ 已为 ${r.total_apps} 个应用恢复了 ${r.applied_extensions} 种文件的默认打开方式。`);
      } else {
        alert(`部分恢复: ${r.applied_extensions} 种成功, ${r.failed.length} 种失败。`);
      }
    } catch (e) {
      alert(`恢复失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="govern-panel">
      {/* 用户已有设置 → 顶部恢复按钮 */}
      {manifest && manifest.apps.length > 0 && (
        <div className="restore-bar">
          <span>你之前为 {manifest.apps.length} 个软件设置过默认打开方式 · {manifest.apps.reduce((sum, a) => sum + a.extensions.length, 0)} 种文件类型</span>
          <button onClick={applyAllAssoc} disabled={busy === "apply_all"}>
            {busy === "apply_all" ? "恢复中..." : "一键恢复"}
          </button>
        </div>
      )}

      {/* 一键体检主按钮 */}
      <div className="hero-card">
        <div className="hero-title">一键体检 + 清理</div>
        <div className="hero-sub">扫一扫,清掉国产软件塞进系统的垃圾</div>
        {scanning && !anyScanned && (
          <div className="muted small"><span className="thinking-dots">正在扫描你的电脑</span></div>
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

      {recentBatch && (
        <div className="undo-bar">
          <span>✓ 刚才{recentBatch.summary} · 不满意可以撤回</span>
          <div>
            <button onClick={undoBatch} disabled={busy === "undo"}>{busy === "undo" ? "撤回中..." : "撤回这次"}</button>
            <button onClick={() => setRecentBatch(null)} className="ghost">关闭</button>
          </div>
        </div>
      )}

      {/* 默认打开方式 */}
      <FileAssocCard manifest={manifest} refresh={refresh} />

      {/* 高级 */}
      <div className="advanced-toggle" onClick={() => setShowAdvanced(!showAdvanced)}>
        {showAdvanced ? "▼" : "▶"} 高级:分别处理
      </div>
      {showAdvanced && (
        <div className="scenario-grid">
          <SimpleCard title="清「我的电脑」里的图标" stats={pc} busy={busy === "govern_clean_pc_namespace"} runLabel="清掉这些图标" onRun={() => runOne("govern_clean_pc_namespace")} />
          <SimpleCard title="关掉国产软件的后台" stats={ka} busy={busy === "govern_stop_keepalive"} runLabel="关掉这些后台" onRun={() => runOne("govern_stop_keepalive")} />
        </div>
      )}

      {/* 历史 */}
      <div className="history-toggle" onClick={() => setShowUndo(!showUndo)}>↶ 历史操作 ({snapshots.length})</div>
      {showUndo && (
        <div className="history-list">
          {snapshots.slice(0, 20).map((s) => <HistoryItem key={s.id} s={s} onRefresh={refresh} />)}
          {snapshots.length === 0 && <p className="muted">还没有操作过任何东西。</p>}
        </div>
      )}
    </div>
  );
}

function SimpleCard({ title, stats, busy, runLabel, onRun }: {
  title: string; stats: ScenarioStats | null; busy: boolean; runLabel: string; onRun: () => void;
}) {
  const empty = stats?.pending === 0;
  return (
    <div className={`simple-card ${empty ? "empty" : ""}`}>
      <h4>{title}</h4>
      <div className="simple-stats">
        {empty ? <span className="ok">✓ 干净, 无需处理</span> : <>
          <span className="big-num">{stats?.pending ?? "—"}</span>
          <span className="muted small">{stats?.summary || "..."}</span>
        </>}
      </div>
      {stats?.display_names && stats.display_names.length > 0 && (
        <div className="vendor-list">
          {stats.display_names.map((n, i) => <span key={i} className="vendor-chip small-chip">{n}</span>)}
        </div>
      )}
      <button className="btn-exec simple-btn" disabled={empty || busy} onClick={onRun}>{busy ? "..." : runLabel}</button>
    </div>
  );
}

function HistoryItem({ s, onRefresh }: { s: SnapshotManifest; onRefresh: () => void }) {
  const [busy, setBusy] = useState(false);
  const restore = async () => {
    setBusy(true);
    try { await invoke("restore_snapshot", { snapshotId: s.id }); onRefresh(); }
    catch (e) { alert(`撤回失败: ${e}`); } finally { setBusy(false); }
  };
  const time = new Date(s.created_at).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
  return (
    <div className={`history-item ${s.restored_at ? "restored" : ""}`}>
      <div className="history-info">
        <div className="history-action">{humanizeAction(s.action_kind, s.action_target)}</div>
        <div className="history-time">{time}</div>
      </div>
      <button disabled={!!s.restored_at || s.restorable === false || busy} onClick={restore} className="btn-restore">
        {s.restored_at ? "已撤回" : s.restorable === false ? "不可撤回" : busy ? "..." : "撤回"}
      </button>
    </div>
  );
}

function FileAssocCard({ manifest, refresh }: { manifest: AssocManifest | null; refresh: () => Promise<void> }) {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [presets, setPresets] = useState<AssocPreset[]>([]);
  const [customApps, setCustomApps] = useState<InstalledApp[]>([]);
  const [editing, setEditing] = useState<{ app: InstalledApp; extensions: Set<string> } | null>(null);

  useEffect(() => {
    invoke<InstalledApp[]>("fileassoc_detect_installed_apps").then(setApps);
    invoke<AssocPreset[]>("fileassoc_list_presets").then(setPresets);
  }, []);

  const pickCustomExe = async () => {
    try {
      const picked = await open({ multiple: false, directory: false, filters: [{ name: "可执行文件", extensions: ["exe"] }] });
      if (!picked || typeof picked !== "string") return;
      const exePath = picked as string;
      const nameWithExt = exePath.split(/[\\/]/).pop() || exePath;
      const stem = nameWithExt.replace(/\.exe$/i, "");
      const custom: InstalledApp = { key: `custom:${exePath}`, display_name: stem, category: "custom", exe_path: exePath };
      setCustomApps((arr) => arr.find((a) => a.exe_path === exePath) ? arr : [...arr, custom]);
      openEditFor(custom);
    } catch (e) { alert(`选择失败: ${e}`); }
  };

  const openEditFor = (app: InstalledApp) => {
    const existing = manifest?.apps.find((a) => a.exe_path === app.exe_path);
    const initialExts = new Set<string>(existing?.extensions || []);
    if (initialExts.size === 0 && app.category !== "custom") {
      const preset = presets.find((p) => p.id === app.category);
      if (preset) preset.extensions.forEach((e) => initialExts.add(e));
    }
    setEditing({ app, extensions: initialExts });
  };

  const allApps = [...apps, ...customApps];
  // 计算所有 app 的扩展名分布,用于显示
  const appExtMap: Record<string, string[]> = {};
  for (const app of manifest?.apps || []) {
    appExtMap[app.exe_path] = app.extensions;
  }

  return (
    <div className="hero-card assoc-hero">
      <div className="hero-title">默认打开方式</div>
      <div className="hero-sub">选个应用,把视频/音乐/图片等文件类型分配给它</div>

      <div className="app-grid">
        {allApps.map((a) => {
          const exts = appExtMap[a.exe_path] || [];
          return (
            <div key={a.key} className="app-tile" onClick={() => openEditFor(a)} title={a.exe_path}>
              <div className="app-icon">{CAT_ICON[a.category] || "🛠️"}</div>
              <div className="app-name">{a.display_name}</div>
              {exts.length > 0 && <div className="app-ext-count">{exts.length} 种文件</div>}
            </div>
          );
        })}
        <div className="app-tile add-custom" onClick={pickCustomExe} title="从文件管理器挑一个 exe">
          <div className="app-icon">＋</div>
          <div className="app-name">添加其他应用</div>
        </div>
      </div>

      {/* 当前用户的分配清单 */}
      {manifest && manifest.apps.length > 0 && (
        <div className="assoc-list">
          <div className="step-label">已设置的默认打开方式</div>
          {manifest.apps.map((app) => (
            <div key={app.exe_path} className="assoc-row">
              <span className="assoc-name">{app.display_name}</span>
              <span className="assoc-exts">
                {app.extensions.slice(0, 8).map((e, i) => <code key={i}>{e}</code>)}
                {app.extensions.length > 8 && <span className="muted small">+{app.extensions.length - 8}</span>}
              </span>
            </div>
          ))}
        </div>
      )}

      {editing && (
        <EditAssocModal
          app={editing.app}
          initialExtensions={editing.extensions}
          presets={presets}
          manifest={manifest}
          onCancel={() => setEditing(null)}
          onSaved={async () => { setEditing(null); await refresh(); }}
        />
      )}
    </div>
  );
}

function EditAssocModal({ app, initialExtensions, presets, manifest, onCancel, onSaved }: {
  app: InstalledApp;
  initialExtensions: Set<string>;
  presets: AssocPreset[];
  manifest: AssocManifest | null;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(initialExtensions);
  const [custom, setCustom] = useState("");
  const [busy, setBusy] = useState(false);

  const toggleExt = (ext: string) => {
    setSelected((s) => { const n = new Set(s); n.has(ext) ? n.delete(ext) : n.add(ext); return n; });
  };
  const togglePreset = (p: AssocPreset) => {
    const allOn = p.extensions.every((e) => selected.has(e));
    setSelected((s) => {
      const n = new Set(s);
      for (const e of p.extensions) { allOn ? n.delete(e) : n.add(e); }
      return n;
    });
  };

  // 找冲突:某个 ext 当前被其他 app 拥有
  const conflicts: { ext: string; owner: string }[] = [];
  for (const ext of selected) {
    for (const other of manifest?.apps || []) {
      if (other.exe_path !== app.exe_path && other.extensions.includes(ext)) {
        conflicts.push({ ext, owner: other.display_name });
      }
    }
  }

  const [resultSummary, setResultSummary] = useState<string | null>(null);
  const [needManual, setNeedManual] = useState<string[]>([]);

  const save = async () => {
    setBusy(true);
    const extsArr = Array.from(selected);
    if (custom.trim()) {
      for (const c of custom.split(/[\s,]+/)) {
        const t = c.trim();
        if (t) extsArr.push(t.startsWith(".") ? t : `.${t}`);
      }
    }
    try {
      await invoke("fileassoc_upsert_app", {
        app: {
          key: app.key,
          display_name: app.display_name,
          exe_path: app.exe_path,
          extensions: extsArr,
        },
      });
      // 立即 apply, 拿到逐个扩展名是否成功
      const result = await invoke<{ failed: [string, string, string][]; applied_extensions: number }>(
        "fileassoc_apply_all"
      );
      // 用 set_app_defaults 走最新的逻辑路径单独检查 need_manual
      const single = await invoke<AssocResult>("fileassoc_set_app_defaults", {
        exePath: app.exe_path,
        extensions: extsArr,
      });
      if (single.extensions_need_manual.length === 0) {
        setResultSummary(`✓ ${single.extensions_set.length} 种文件类型已强制设为 ${app.display_name},不用任何手动操作`);
        setTimeout(() => onSaved(), 1500);
      } else {
        setResultSummary(`${single.extensions_set.length} 种已强制设好, 但 ${single.extensions_need_manual.length} 种 Windows 不接受我们的哈希,需要手动选`);
        setNeedManual(single.extensions_need_manual);
      }
      void result;
    } catch (e) {
      alert(`保存失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const removeApp = async () => {
    if (!confirm(`移除 ${app.display_name} 的所有分配?(不会还原系统已有关联,只是从清单删除)`)) return;
    setBusy(true);
    try {
      await invoke("fileassoc_remove_app", { key: app.key });
      onSaved();
    } catch (e) {
      alert(`移除失败: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  if (resultSummary) {
    const allOk = needManual.length === 0;
    return (
      <div className="modal-backdrop" onClick={onSaved}>
        <div className="modal" onClick={(e) => e.stopPropagation()}>
          <h3>{allOk ? "完成" : "大部分已设, 少数需要手动"}</h3>
          <p>{resultSummary}</p>
          {allOk ? (
            <p className="muted small">
              已经直接写入 Windows 的 UserChoice 哈希,下次双击就用 <strong>{app.display_name}</strong> 打开,
              不需要任何手动确认。
            </p>
          ) : (
            <>
              <p className="muted small">
                这几种 Windows 系统版本不接受我们的哈希算法(可能 Win11 太新或太老):
              </p>
              <div className="ext-grid">
                {needManual.map((e, i) => (
                  <span key={i} className="chip on">{e}</span>
                ))}
              </div>
              <p className="muted small">点下面按钮跳到 Windows 默认应用页手动选一下。</p>
            </>
          )}
          <div className="modal-buttons">
            {!allOk && (
              <button
                className="btn-exec"
                onClick={async () => {
                  try {
                    await invoke("fileassoc_open_settings", { ext: needManual[0]?.slice(1) ?? null });
                  } catch (e) {
                    alert(`打开失败: ${e}`);
                  }
                  onSaved();
                }}
              >
                打开 Windows 默认应用设置
              </button>
            )}
            <button className="btn-exec" onClick={onSaved}>知道了</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal modal-wide" onClick={(e) => e.stopPropagation()}>
        <h3>设置 {app.display_name} 打开哪些文件</h3>
        <div className="muted small">勾选预设或单独勾扩展,保存后立即应用并加入你的设置清单。</div>

        <div className="step-label">快捷预设</div>
        <div className="preset-chips">
          {presets.map((p) => {
            const allOn = p.extensions.every((e) => selected.has(e));
            return (
              <label key={p.id} className={`chip ${allOn ? "on" : ""}`} onClick={(e) => { e.preventDefault(); togglePreset(p); }}>
                {PRESET_ICON[p.id] || ""} {p.label} ({p.extensions.length})
              </label>
            );
          })}
        </div>

        <div className="step-label">所有扩展(共 {selected.size} 个已选)</div>
        <div className="ext-grid">
          {Array.from(new Set(presets.flatMap((p) => p.extensions))).sort().map((ext) => (
            <label key={ext} className={`chip ${selected.has(ext) ? "on" : ""}`}>
              <input type="checkbox" checked={selected.has(ext)} onChange={() => toggleExt(ext)} />
              {ext}
            </label>
          ))}
        </div>

        <input
          type="text"
          placeholder="还想加的扩展名(逗号分隔, 如 .iso .srt)"
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          className="assoc-input"
        />

        {conflicts.length > 0 && (
          <div className="conflict-warn">
            ⚠ 冲突 — 这些扩展名已经分给了别的应用,保存会**改成{app.display_name}**:
            <ul>
              {conflicts.slice(0, 5).map((c, i) => <li key={i}><code>{c.ext}</code> 当前: {c.owner}</li>)}
              {conflicts.length > 5 && <li>等 {conflicts.length} 个冲突</li>}
            </ul>
          </div>
        )}

        <div className="modal-buttons">
          {manifest?.apps.some((a) => a.exe_path === app.exe_path) && (
            <button className="btn-restore" onClick={removeApp} disabled={busy}>从清单移除</button>
          )}
          <button onClick={onCancel}>取消</button>
          <button className="btn-exec" onClick={save} disabled={busy || selected.size === 0}>
            {busy ? "..." : `保存并应用(${selected.size} 个扩展)`}
          </button>
        </div>
      </div>
    </div>
  );
}
