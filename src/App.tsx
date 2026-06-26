import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ---- 类型 ----
interface Action {
  kind: string;
  target: string;
  reason: string;
  elevate?: boolean;
}

interface Profile {
  id: string;
  name: string;
  vendor: string;
  category: string;
  severity: string;
  actions: Action[];
  fingerprints?: { clsids?: string[] };
  notes?: string;
}

interface PcNamespaceItem {
  clsid: string;
  display_name: string;
  default_icon?: string;
  inproc_server?: string;
  is_system: boolean;
}

interface ActionPlan {
  profile_id: string;
  action_index: number;
  kind: string;
  target: string;
  reason: string;
  elevate: boolean;
  blocked: boolean;
  blocked_reason?: string;
  will_change: string;
}

interface ExecResult {
  plan: ActionPlan;
  snapshot_id?: string;
  success: boolean;
  error?: string;
}

interface SnapshotManifest {
  id: string;
  created_at: string;
  profile_id?: string;
  action_index?: number;
  action_kind: string;
  action_target: string;
  action_reason: string;
  restored_at?: string;
}

// ---- 主组件 ----
export default function App() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [nsItems, setNsItems] = useState<PcNamespaceItem[]>([]);
  const [snapshots, setSnapshots] = useState<SnapshotManifest[]>([]);
  const [activePlans, setActivePlans] = useState<Record<string, ActionPlan[]>>(
    {}
  );
  const [busy, setBusy] = useState<string | null>(null);
  const [toasts, setToasts] = useState<{ id: number; kind: "ok" | "err"; msg: string }[]>([]);

  const refreshAll = async () => {
    try {
      const [p, n, s] = await Promise.all([
        invoke<Profile[]>("list_profiles"),
        invoke<PcNamespaceItem[]>("scan_pc_namespace"),
        invoke<SnapshotManifest[]>("list_snapshots"),
      ]);
      setProfiles(p);
      setNsItems(n);
      setSnapshots(s);
    } catch (e) {
      toast("err", `刷新失败: ${e}`);
    }
  };

  useEffect(() => {
    refreshAll();
  }, []);

  const toast = (kind: "ok" | "err", msg: string) => {
    const id = Date.now();
    setToasts((t) => [...t, { id, kind, msg }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4500);
  };

  const dryRun = async (id: string) => {
    setBusy(`dry:${id}`);
    try {
      const plans = await invoke<ActionPlan[]>("dry_run_profile", { profileId: id });
      setActivePlans((s) => ({ ...s, [id]: plans }));
      toast("ok", `${id}: dry-run 列出 ${plans.length} 个动作`);
    } catch (e) {
      toast("err", `dry-run 失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const execute = async (id: string, idx: number) => {
    if (!confirm(`确认执行 ${id} 的动作 #${idx} ? 会先自动快照,可一键还原。`)) return;
    setBusy(`exec:${id}:${idx}`);
    try {
      const r = await invoke<ExecResult>("execute_profile_action", {
        profileId: id,
        actionIndex: idx,
      });
      if (r.success) {
        toast("ok", `执行成功, 快照 ${r.snapshot_id?.slice(0, 22)}...`);
      } else {
        toast("err", `执行失败: ${r.error ?? "未知"}`);
      }
      await refreshAll();
    } catch (e) {
      toast("err", `RPC 失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const restore = async (sid: string) => {
    if (!confirm(`确认还原快照 ${sid.slice(0, 22)} ... ?`)) return;
    setBusy(`restore:${sid}`);
    try {
      await invoke("restore_snapshot", { snapshotId: sid });
      toast("ok", "还原成功");
      await refreshAll();
    } catch (e) {
      toast("err", `还原失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const matchedProfile = (item: PcNamespaceItem): Profile | undefined =>
    profiles.find((p) =>
      p.fingerprints?.clsids?.some((c) => c.toLowerCase() === item.clsid.toLowerCase())
    );

  const rogueNs = nsItems.filter((it) => !it.is_system);

  return (
    <div className="app">
      <header>
        <h1>
          kuake-fuckyou <span className="ver">v0.0.1 · P0</span>
        </h1>
        <p className="subtitle">
          整治国产流氓软件的 Windows 11 治理工具 · 一次跑完即退出 · 不常驻后台
        </p>
      </header>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.msg}
          </div>
        ))}
      </div>

      {/* 画像档案 */}
      <section>
        <h2>
          画像档案 <span className="count">{profiles.length}</span>
          <button className="reload" onClick={refreshAll}>刷新</button>
        </h2>
        {profiles.map((p) => (
          <div key={p.id} className="profile-card">
            <div className="profile-head">
              <span className={`badge sev-${p.severity}`}>{p.severity}</span>
              <strong>{p.name}</strong>
              <code className="mono">{p.id}</code>
              <span className="muted">· {p.vendor}</span>
              <span className="muted">· {p.actions.length} 个动作</span>
              <button
                className="btn-dryrun"
                disabled={busy?.startsWith("dry:") === true}
                onClick={() => dryRun(p.id)}
              >
                {busy === `dry:${p.id}` ? "..." : "预览动作 (dry-run)"}
              </button>
            </div>
            {activePlans[p.id] && (
              <table className="plans">
                <thead>
                  <tr>
                    <th>#</th>
                    <th>kind</th>
                    <th>目标</th>
                    <th>原因 / 影响</th>
                    <th>执行</th>
                  </tr>
                </thead>
                <tbody>
                  {activePlans[p.id].map((pl) => (
                    <tr key={pl.action_index} className={pl.blocked ? "blocked" : ""}>
                      <td>{pl.action_index}</td>
                      <td className="mono">{pl.kind}</td>
                      <td className="mono small">{pl.target}</td>
                      <td>
                        <div>{pl.reason}</div>
                        <div className={pl.blocked ? "danger small" : "muted small"}>
                          {pl.blocked ? `🚫 ${pl.blocked_reason ?? "blocked"}` : pl.will_change}
                        </div>
                      </td>
                      <td>
                        <button
                          className="btn-exec"
                          disabled={pl.blocked || busy !== null}
                          onClick={() => execute(p.id, pl.action_index)}
                        >
                          {busy === `exec:${p.id}:${pl.action_index}` ? "..." : "执行"}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        ))}
      </section>

      {/* 此电脑命名空间 */}
      <section>
        <h2>
          "此电脑" 命名空间 <span className="count">{nsItems.length} (其中 {rogueNs.length} 非系统)</span>
        </h2>
        {nsItems.length === 0 ? (
          <p className="muted">无</p>
        ) : (
          <table>
            <thead>
              <tr><th style={{ width: "30%" }}>CLSID</th><th>名称</th><th>判定</th><th>画像</th></tr>
            </thead>
            <tbody>
              {nsItems.map((it) => {
                const m = matchedProfile(it);
                return (
                  <tr key={it.clsid}>
                    <td className="mono small">{it.clsid}</td>
                    <td>{it.display_name || <em>(无名)</em>}</td>
                    <td>
                      {it.is_system ? <span className="ok">系统</span>
                        : m ? <span className="danger">已识别流氓</span>
                        : <span className="warn">第三方,未识别</span>}
                    </td>
                    <td>{m ? <code>{m.id}</code> : <span className="muted">—</span>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      {/* 快照 / 还原 */}
      <section>
        <h2>
          操作快照 <span className="count">{snapshots.length}</span>
        </h2>
        {snapshots.length === 0 ? (
          <p className="muted">尚无快照。每次执行动作会自动建快照,可一键还原。</p>
        ) : (
          <table>
            <thead>
              <tr><th>id</th><th>时间</th><th>动作</th><th>目标</th><th>状态</th><th>还原</th></tr>
            </thead>
            <tbody>
              {snapshots.map((s) => (
                <tr key={s.id}>
                  <td className="mono small">{s.id.slice(0, 22)}…</td>
                  <td className="small">{new Date(s.created_at).toLocaleString("zh-CN")}</td>
                  <td><code>{s.action_kind}</code></td>
                  <td className="mono small">{s.action_target}</td>
                  <td>{s.restored_at ? <span className="muted">已还原</span> : <span className="ok">可还原</span>}</td>
                  <td>
                    <button
                      className="btn-restore"
                      disabled={!!s.restored_at || busy !== null}
                      onClick={() => restore(s.id)}
                    >
                      {busy === `restore:${s.id}` ? "..." : "还原"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <footer>
        <small>v0.0.1 P0 · 仅 reg-delete 已接入 · service/file/task/process-kill 等动作下一轮</small>
      </footer>
    </div>
  );
}
