import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface SentryState {
  started_at: string;
  updated_at: string;
  last_alert_at?: string;
  paused_until?: string;
  alerts_total: number;
  monitored_pids: number;
  last_inspection_at?: string;
  last_tamper_check_at?: string;
  last_inspection_findings: number;
}
interface SentryStatus {
  installed: boolean;
  autostart_enabled: boolean;
  running: boolean;
  state?: SentryState;
}
interface AlertEvent {
  ts: string;
  pid: number;
  image_name: string;
  up_bps: number;
}
interface WhitelistEntry {
  image_name: string;
  signer_cn?: string;
  reason?: string;
}
interface WhitelistFile {
  version: number;
  entries: WhitelistEntry[];
}
interface InspectionConfig {
  inspection_enabled: boolean;
  tamper_alert_enabled: boolean;
  inspection_interval_minutes: number;
}
interface InspectionEvent {
  ts: string;
  kind: string;
  category: string;
  label: string;
  detail: string;
}

export function SentryPanel() {
  const [status, setStatus] = useState<SentryStatus | null>(null);
  const [events, setEvents] = useState<AlertEvent[]>([]);
  const [whitelist, setWhitelist] = useState<WhitelistFile | null>(null);
  const [inspCfg, setInspCfg] = useState<InspectionConfig | null>(null);
  const [inspEvents, setInspEvents] = useState<InspectionEvent[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [newEntry, setNewEntry] = useState({ image_name: "", reason: "" });

  const refresh = async () => {
    try {
      const [s, e, w, ic, ie] = await Promise.all([
        invoke<SentryStatus>("sentry_get_status"),
        invoke<AlertEvent[]>("sentry_list_events", { limit: 50 }),
        invoke<WhitelistFile>("sentry_get_whitelist"),
        invoke<InspectionConfig>("sentry_get_inspection_config"),
        invoke<InspectionEvent[]>("sentry_list_inspection_events", { limit: 30 }),
      ]);
      setStatus(s);
      setEvents(e);
      setWhitelist(w);
      setInspCfg(ic);
      setInspEvents(ie);
    } catch (err) {
      console.error("sentry refresh failed", err);
    }
  };

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, []);

  const wrap = async (cmd: string, fn: () => Promise<void>) => {
    setBusy(cmd);
    try { await fn(); await refresh(); }
    catch (e) { alert(`失败: ${e}`); }
    finally { setBusy(null); }
  };

  const toggleInspection = async () => {
    if (!inspCfg) return;
    await wrap("toggle_insp", async () => {
      await invoke("sentry_set_inspection_config", {
        config: { ...inspCfg, inspection_enabled: !inspCfg.inspection_enabled },
      });
    });
  };
  const toggleTamper = async () => {
    if (!inspCfg) return;
    await wrap("toggle_tamper", async () => {
      await invoke("sentry_set_inspection_config", {
        config: { ...inspCfg, tamper_alert_enabled: !inspCfg.tamper_alert_enabled },
      });
    });
  };
  const setInspInterval = async (minutes: number) => {
    if (!inspCfg) return;
    await wrap("interval", async () => {
      await invoke("sentry_set_inspection_config", {
        config: { ...inspCfg, inspection_interval_minutes: minutes },
      });
    });
  };
  const runInspNow = () => wrap("insp_now", async () => { await invoke("sentry_run_inspection_now"); });

  const enableAutostart = () => wrap("enable", async () => { await invoke("sentry_enable_autostart"); });
  const disableAutostart = () => wrap("disable", async () => { await invoke("sentry_disable_autostart"); });
  const startNow = () => wrap("start", async () => { await invoke("sentry_start_now"); });
  const pauseHour = () => wrap("pause", async () => { await invoke("sentry_pause", { minutes: 60 }); });
  const resume = () => wrap("resume", async () => { await invoke("sentry_resume"); });
  const stop = () => wrap("stop", async () => { await invoke("sentry_stop"); });

  const addEntry = async () => {
    if (!newEntry.image_name.trim()) return;
    await wrap("add", async () => {
      await invoke("sentry_whitelist_add", {
        imageName: newEntry.image_name.trim(),
        reason: newEntry.reason.trim() || null,
      });
      setNewEntry({ image_name: "", reason: "" });
    });
  };
  const removeEntry = async (name: string) => {
    await wrap("rm:" + name, async () => {
      await invoke("sentry_whitelist_remove", { imageName: name });
    });
  };

  if (!status) {
    return <div className="sentry-panel"><div className="muted">加载中...</div></div>;
  }

  const isPaused = !!status.state?.paused_until && new Date(status.state.paused_until) > new Date();
  const fmtBps = (bps: number) => `${(bps / 1_000_000).toFixed(1)} Mbps`;
  const formatInterval = (m: number) => {
    if (m < 60) return `${m} 分钟`;
    if (m < 1440) return `${m / 60} 小时`;
    return `${m / 1440} 天`;
  };

  return (
    <div className="sentry-panel">
      <h2>🛡 守护后台</h2>
      <p className="muted small">
        监控所有非系统/非白名单进程的上行带宽,持续 15 秒 &gt; 8 Mbps 弹通知。
        独立后台进程,不耗 GUI 内存。
      </p>

      {/* 状态卡 */}
      <div className="sentry-status-grid">
        <div className={`status-tile ${status.running ? "ok" : "warn"}`}>
          <div className="status-label">运行状态</div>
          <div className="status-value">
            {status.running ? (isPaused ? "已暂停" : "✓ 监控中") : "✗ 未运行"}
          </div>
        </div>
        <div className={`status-tile ${status.autostart_enabled ? "ok" : "muted-tile"}`}>
          <div className="status-label">开机自启</div>
          <div className="status-value">{status.autostart_enabled ? "✓ 已开启" : "未开启"}</div>
        </div>
        <div className="status-tile">
          <div className="status-label">累计告警</div>
          <div className="status-value">{status.state?.alerts_total ?? 0}</div>
        </div>
        <div className="status-tile">
          <div className="status-label">监视进程数</div>
          <div className="status-value">{status.state?.monitored_pids ?? 0}</div>
        </div>
      </div>

      {/* 控制按钮 */}
      <div className="sentry-controls">
        {!status.autostart_enabled ? (
          <button className="btn-exec" onClick={enableAutostart} disabled={busy !== null}>
            {busy === "enable" ? "..." : "✓ 启用守护并开机自启"}
          </button>
        ) : (
          <button onClick={disableAutostart} disabled={busy !== null}>
            {busy === "disable" ? "..." : "关闭自启 + 停止守护"}
          </button>
        )}
        {status.autostart_enabled && !status.running && (
          <button onClick={startNow} disabled={busy !== null}>
            {busy === "start" ? "..." : "现在就启动"}
          </button>
        )}
        {status.running && !isPaused && (
          <button onClick={pauseHour} disabled={busy !== null}>
            {busy === "pause" ? "..." : "暂停 1 小时"}
          </button>
        )}
        {isPaused && (
          <button onClick={resume} disabled={busy !== null}>
            {busy === "resume" ? "..." : "立即恢复监控"}
          </button>
        )}
        {status.running && (
          <button onClick={stop} disabled={busy !== null} className="btn-restore">
            {busy === "stop" ? "..." : "停止本次"}
          </button>
        )}
      </div>

      {!status.installed && (
        <div className="banner warn">
          ⚠ 没找到 mingchuang-sentry.exe(应该跟主程序在同目录)。
          请先 cargo build 编译它。
        </div>
      )}

      {/* 巡检 / 偷改告警 */}
      {inspCfg && (
        <div className="inspection-section">
          <h3 className="section-h">巡检 + 偷改告警</h3>
          <p className="muted small">
            两个独立开关, 默认都关。PCDN 监控独立。守护进程在跑时才生效。
          </p>

          <div className="inspection-toggles">
            <label className="insp-toggle">
              <input
                type="checkbox"
                checked={inspCfg.inspection_enabled}
                disabled={busy === "toggle_insp"}
                onChange={toggleInspection}
              />
              <div className="insp-toggle-text">
                <strong>定时巡检</strong>
                <span className="muted small">
                  每 {formatInterval(inspCfg.inspection_interval_minutes)} 跑一次,
                  扫"此电脑"被塞图标 / 应用偷加自启 / 默认打开方式被改回
                </span>
              </div>
            </label>

            {inspCfg.inspection_enabled && (
              <div className="insp-interval">
                <span className="muted small">频率:</span>
                {[15, 60, 360, 1440].map((m) => (
                  <button
                    key={m}
                    className={inspCfg.inspection_interval_minutes === m ? "seg-btn on" : "seg-btn"}
                    onClick={() => setInspInterval(m)}
                    disabled={busy === "interval"}
                  >
                    {formatInterval(m)}
                  </button>
                ))}
                <button onClick={runInspNow} disabled={busy === "insp_now"} className="seg-btn">
                  {busy === "insp_now" ? "..." : "立即巡检一次"}
                </button>
              </div>
            )}

            <label className="insp-toggle">
              <input
                type="checkbox"
                checked={inspCfg.tamper_alert_enabled}
                disabled={busy === "toggle_tamper"}
                onChange={toggleTamper}
              />
              <div className="insp-toggle-text">
                <strong>偷改告警</strong>
                <span className="muted small">
                  每 60 秒快查一次, 默认打开方式 / 自启项被偷改了立刻弹通知
                  (需要先做过一次定时巡检建基准)
                </span>
              </div>
            </label>
          </div>

          {status.state?.last_inspection_at && (
            <p className="muted small">
              上次巡检: {new Date(status.state.last_inspection_at).toLocaleString("zh-CN")} ·
              {" "}本轮发现 {status.state.last_inspection_findings} 项变化
            </p>
          )}

          {inspEvents.length > 0 && (
            <div className="event-list" style={{ marginTop: "10px" }}>
              {inspEvents.slice(0, 10).map((ev, i) => (
                <div key={i} className="event-row">
                  <span className="event-time">
                    {new Date(ev.ts).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                  </span>
                  <span className={`insp-badge ${ev.category}`}>{ev.category}</span>
                  <span className="insp-label">{ev.label}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 告警历史 */}
      <h3 className="section-h">最近 PCDN 告警 ({events.length})</h3>
      {events.length === 0 ? (
        <p className="muted">还没有告警。守护启动后,如果有进程偷偷上传超过 8 Mbps 持续 15 秒会被记录在这里。</p>
      ) : (
        <div className="event-list">
          {events.slice(0, 20).map((ev, i) => (
            <div key={i} className="event-row">
              <span className="event-time">{new Date(ev.ts).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</span>
              <code className="event-name">{ev.image_name || `PID ${ev.pid}`}</code>
              <span className="event-bps">{fmtBps(ev.up_bps)}</span>
              <button onClick={async () => { await invoke("sentry_whitelist_add", { imageName: ev.image_name, reason: "从告警加白" }); await refresh(); }}>
                加白
              </button>
            </div>
          ))}
        </div>
      )}

      {/* 白名单 */}
      <h3 className="section-h">用户白名单 ({whitelist?.entries.length ?? 0})</h3>
      <p className="muted small">这些进程超带宽不会告警。明窗内置了 60+ 常见软件(浏览器/IDE/游戏平台/通讯/同步盘等)。</p>

      <div className="wl-add">
        <input
          type="text"
          placeholder="进程 exe 文件名,例如 mygame.exe"
          value={newEntry.image_name}
          onChange={(e) => setNewEntry({ ...newEntry, image_name: e.target.value })}
        />
        <input
          type="text"
          placeholder="备注(可选)"
          value={newEntry.reason}
          onChange={(e) => setNewEntry({ ...newEntry, reason: e.target.value })}
        />
        <button className="btn-exec" onClick={addEntry} disabled={busy === "add" || !newEntry.image_name.trim()}>
          加入
        </button>
      </div>

      {whitelist && whitelist.entries.length > 0 && (
        <div className="wl-list">
          {whitelist.entries.map((e, i) => (
            <div key={i} className="wl-row">
              <code>{e.image_name}</code>
              {e.reason && <span className="muted small">— {e.reason}</span>}
              <button onClick={() => removeEntry(e.image_name)} disabled={busy === "rm:" + e.image_name} className="btn-restore">
                {busy === "rm:" + e.image_name ? "..." : "移除"}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
