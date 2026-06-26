import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
  notes?: string;
}

interface PcNamespaceItem {
  clsid: string;
  display_name: string;
  default_icon?: string;
  inproc_server?: string;
  is_system: boolean;
}

export default function App() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [nsItems, setNsItems] = useState<PcNamespaceItem[]>([]);
  const [errors, setErrors] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      invoke<Profile[]>("list_profiles")
        .then(setProfiles)
        .catch((e) => setErrors((es) => [...es, `list_profiles: ${e}`])),
      invoke<PcNamespaceItem[]>("scan_pc_namespace")
        .then(setNsItems)
        .catch((e) => setErrors((es) => [...es, `scan_pc_namespace: ${e}`])),
    ]).finally(() => setLoading(false));
  }, []);

  const rogueNsItems = nsItems.filter((it) => !it.is_system);
  const matchedProfiles = (item: PcNamespaceItem): Profile | undefined =>
    profiles.find((p) =>
      // @ts-ignore — fingerprints 透传后端 JSON
      (p as any).fingerprints?.clsids?.some?.(
        (c: string) => c.toLowerCase() === item.clsid.toLowerCase()
      )
    );

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

      {errors.length > 0 && (
        <div className="error">
          <strong>错误:</strong>
          <ul>
            {errors.map((e, i) => (
              <li key={i}>{e}</li>
            ))}
          </ul>
        </div>
      )}

      {loading && <div className="loading">加载中...</div>}

      <section>
        <h2>
          已加载的画像档案 <span className="count">{profiles.length}</span>
        </h2>
        {profiles.length === 0 ? (
          <p className="muted">无</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>id</th>
                <th>名称</th>
                <th>厂商</th>
                <th>类别</th>
                <th>严重度</th>
                <th>动作数</th>
              </tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <tr key={p.id}>
                  <td className="mono">{p.id}</td>
                  <td>{p.name}</td>
                  <td>{p.vendor}</td>
                  <td>{p.category}</td>
                  <td className={`sev-${p.severity}`}>{p.severity}</td>
                  <td>{p.actions.length}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section>
        <h2>
          "此电脑" 命名空间项{" "}
          <span className="count">
            {nsItems.length} (其中 {rogueNsItems.length} 项非系统)
          </span>
        </h2>
        <p className="muted">
          只读扫描 ·{" "}
          <code>
            HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace
          </code>
        </p>
        {nsItems.length === 0 ? (
          <p className="muted">无</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th style={{ width: "30%" }}>CLSID</th>
                <th>名称</th>
                <th>判定</th>
                <th>匹配画像</th>
              </tr>
            </thead>
            <tbody>
              {nsItems.map((it) => {
                const profile = matchedProfiles(it);
                return (
                  <tr key={it.clsid}>
                    <td className="mono">{it.clsid}</td>
                    <td>{it.display_name || <em>(无名)</em>}</td>
                    <td>
                      {it.is_system ? (
                        <span className="ok">系统</span>
                      ) : profile ? (
                        <span className="danger">已识别流氓</span>
                      ) : (
                        <span className="warn">第三方,未识别</span>
                      )}
                    </td>
                    <td>
                      {profile ? (
                        <code>{profile.id}</code>
                      ) : (
                        <span className="muted">—</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      <footer>
        <small>
          v0.0.1 骨架 · P0 安全地基 (快照/dry-run/还原) 与 P1 动作执行下一轮起步
        </small>
      </footer>
    </div>
  );
}
