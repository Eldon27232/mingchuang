import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { humanizeToolCall, humanizeVerdict } from "./labels";

interface ToolCallView {
  id: string;
  name: string;
  args: any;
  review?: { verdict: string; reason: string } | null;
  status: string;
  result?: string | null;
}
interface ChatMessage {
  role: string;
  content: string;
  tool_calls?: ToolCallView[] | null;
  tool_call_id?: string | null;
}
interface Session {
  id: string;
  messages: ChatMessage[];
  pending_call?: ToolCallView | null;
  status: string;
  tool_call_count: number;
  last_error?: string | null;
}
interface AiConfig {
  provider: string;
  api_key: string;
  base_url: string;
  model_executor: string;
  model_reviewer: string;
  auto_approve_all: boolean;
}

export function AiPanel() {
  const [config, setConfig] = useState<AiConfig | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [session, setSession] = useState<Session | null>(null);
  const [input, setInput] = useState("");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [editingText, setEditingText] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  const loadConfig = async () => {
    const c = await invoke<AiConfig>("ai_get_config");
    setConfig(c);
    if (!c.api_key) setShowSettings(true);
  };

  useEffect(() => { loadConfig(); }, []);

  const refreshSession = async (id: string) => {
    const s = await invoke<Session | null>("ai_get_session", { sessionId: id });
    setSession(s);
  };

  const isRunning = session?.status === "thinking";
  const isWaiting = session?.status === "waiting_approval";

  useEffect(() => {
    if (!sessionId) return;
    const t = setInterval(() => refreshSession(sessionId), 800);
    return () => clearInterval(t);
  }, [sessionId]);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [session?.messages.length]);

  const send = async () => {
    if (!input.trim()) return;
    let sid = sessionId;
    if (!sid) {
      sid = await invoke<string>("ai_create_session");
      setSessionId(sid);
    }
    const msg = input;
    setInput("");
    setSession((prev) => prev
      ? { ...prev, messages: [...prev.messages, { role: "user", content: msg }], status: "thinking" }
      : prev);
    try {
      await invoke("ai_send_message", { sessionId: sid, message: msg });
      await refreshSession(sid);
    } catch (e) {
      alert(`发送失败: ${e}`);
      await refreshSession(sid);
    }
  };

  const abort = async () => {
    if (!sessionId) return;
    try {
      await invoke("ai_abort_session", { sessionId });
      await refreshSession(sessionId);
    } catch (e) {
      alert(`停止失败: ${e}`);
    }
  };

  const retry = async () => {
    if (!sessionId) return;
    try {
      await invoke("ai_retry_last", { sessionId });
      await refreshSession(sessionId);
    } catch (e) {
      alert(`重试失败: ${e}`);
    }
  };

  const startEdit = (i: number, text: string) => {
    setEditingIndex(i);
    setEditingText(text);
  };
  const cancelEdit = () => {
    setEditingIndex(null);
    setEditingText("");
  };
  const submitEdit = async () => {
    if (!sessionId || editingIndex === null) return;
    const text = editingText;
    const idx = editingIndex;
    cancelEdit();
    try {
      await invoke("ai_abort_session", { sessionId });
      await invoke("ai_edit_user_message", { sessionId, msgIndex: idx, newContent: text });
      await refreshSession(sessionId);
    } catch (e) {
      alert(`编辑失败: ${e}`);
    }
  };

  const approve = async (decision: "approve" | "deny") => {
    if (!sessionId) return;
    try {
      await invoke("ai_approve_pending", { sessionId, decision });
      await refreshSession(sessionId);
    } catch (e) {
      alert(`审批失败: ${e}`);
    }
  };

  const startNewSession = async () => {
    const id = await invoke<string>("ai_create_session");
    setSessionId(id);
    setSession({ id, messages: [], pending_call: null, status: "idle", tool_call_count: 0 });
  };

  const lastAssistantIdx = (() => {
    if (!session) return -1;
    for (let i = session.messages.length - 1; i >= 0; i--) {
      if (session.messages[i].role === "assistant") return i;
    }
    return -1;
  })();

  return (
    <div className="ai-panel">
      <div className="ai-header">
        <h2>问问 AI</h2>
        <div className="ai-header-actions">
          <button className="icon-btn" onClick={startNewSession} title="新会话">＋</button>
          <button className="icon-btn" onClick={() => setShowSettings(true)} title="设置">⚙</button>
        </div>
      </div>

      {showSettings && (
        <SettingsModal
          initial={config!}
          onClose={() => setShowSettings(false)}
          onSaved={(c) => { setConfig(c); setShowSettings(false); }}
        />
      )}

      {!config?.api_key && !showSettings && (
        <div className="banner warn">
          ⚠ 先点右上 ⚙ 填入 API 密钥才能用。没有的话点开有说明。
        </div>
      )}

      <div className="ai-log" ref={logRef}>
        {(session?.messages ?? []).map((m, i) => (
          <MessageView
            key={i} m={m} index={i}
            onEdit={startEdit}
            onRetry={i === lastAssistantIdx && !isRunning && !isWaiting ? retry : undefined}
            editing={editingIndex === i}
            editingText={editingText}
            setEditingText={setEditingText}
            onSubmitEdit={submitEdit}
            onCancelEdit={cancelEdit}
          />
        ))}
        {session?.last_error && <div className="ai-msg err">⚠ {session.last_error}</div>}
        {isRunning && (
          <div className="ai-msg assistant thinking">
            <span className="thinking-dots">思考中</span>
          </div>
        )}
      </div>

      {session?.pending_call && <PendingApproval call={session.pending_call} onDecide={approve} />}

      <div className="ai-input">
        <input
          type="text"
          placeholder="123云盘装了卸不掉,帮我清"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }}
          disabled={isRunning || !config?.api_key}
        />
        {isRunning || isWaiting ? (
          <button onClick={abort} style={{ background: "var(--danger)" }}>停止</button>
        ) : (
          <button onClick={send} disabled={!input.trim() || !config?.api_key}>发送</button>
        )}
      </div>
    </div>
  );
}

function MessageView({
  m, index, onEdit, onRetry, editing, editingText, setEditingText, onSubmitEdit, onCancelEdit,
}: {
  m: ChatMessage; index: number;
  onEdit: (i: number, t: string) => void;
  onRetry?: () => void;
  editing: boolean;
  editingText: string;
  setEditingText: (s: string) => void;
  onSubmitEdit: () => void;
  onCancelEdit: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggle = (id: string) => {
    setExpanded((s) => { const n = new Set(s); n.has(id) ? n.delete(id) : n.add(id); return n; });
  };

  if (m.role === "tool") return null;

  if (m.role === "user") {
    return (
      <div className="ai-msg user" onContextMenu={(e) => { e.preventDefault(); if (!editing) onEdit(index, m.content); }} title="右键改">
        <div className="ai-msg-role">你</div>
        {editing ? (
          <div className="edit-box">
            <textarea value={editingText} onChange={(e) => setEditingText(e.target.value)}
              rows={Math.max(2, editingText.split("\n").length)} autoFocus />
            <div className="edit-buttons">
              <button onClick={onCancelEdit}>取消</button>
              <button className="btn-exec" onClick={onSubmitEdit}>重发</button>
            </div>
          </div>
        ) : <div className="ai-msg-text">{m.content}</div>}
      </div>
    );
  }

  return (
    <div className="ai-msg assistant">
      <div className="ai-msg-role">AI</div>
      {m.content && <div className="ai-msg-text">{m.content}</div>}
      {m.tool_calls?.map((c) => {
        const ex = expanded.has(c.id);
        const humanized = humanizeToolCall(c.name, c.args);
        return (
          <div key={c.id} className={`ai-toolcall status-${c.status}`}>
            <div className="toolcall-head" onClick={() => toggle(c.id)}>
              <span className="caret">{ex ? "▼" : "▶"}</span>
              <span className="toolcall-action">{humanized}</span>
              <span className="muted small"> · {statusLabel(c.status)}</span>
            </div>
            {ex && (
              <div className="toolcall-body">
                {c.review && (() => {
                  const v = humanizeVerdict(c.review.verdict);
                  return <div className={`ai-review ${v.cls}`}>{v.icon} {v.text} ({c.review.reason})</div>;
                })()}
                <details>
                  <summary className="muted small">技术细节 ({c.name})</summary>
                  <pre className="ai-args">{JSON.stringify(c.args, null, 2)}</pre>
                </details>
                {c.result && <div className="ai-result small">→ {c.result}</div>}
              </div>
            )}
          </div>
        );
      })}
      {onRetry && (
        <div className="msg-actions">
          <button onClick={onRetry} className="btn-retry">↻ 重试</button>
        </div>
      )}
    </div>
  );
}

function statusLabel(s: string): string {
  switch (s) {
    case "reviewing": return "检查中";
    case "waiting_approval": return "等你确认";
    case "approved": return "已批";
    case "denied": return "已拒";
    case "executing": return "执行中";
    case "done": return "完成";
    case "failed": return "失败";
    default: return s;
  }
}

function PendingApproval({ call, onDecide }: { call: ToolCallView; onDecide: (d: "approve" | "deny") => void; }) {
  const v = call.review ? humanizeVerdict(call.review.verdict) : null;
  return (
    <div className="ai-approval">
      <div className="ai-approval-title">⏳ AI 想替你做这件事</div>
      <div>{humanizeToolCall(call.name, call.args)}</div>
      {v && <div className={`ai-review ${v.cls}`}>{v.icon} {v.text}{call.review?.reason ? ` — ${call.review.reason}` : ""}</div>}
      <details>
        <summary className="muted small">技术细节 ({call.name})</summary>
        <pre>{JSON.stringify(call.args, null, 2)}</pre>
      </details>
      <div className="ai-approval-buttons">
        <button className="btn-restore" onClick={() => onDecide("deny")}>不要</button>
        <button className="btn-exec" onClick={() => onDecide("approve")}>好</button>
      </div>
    </div>
  );
}

function SettingsModal({
  initial, onClose, onSaved,
}: {
  initial: AiConfig;
  onClose: () => void;
  onSaved: (c: AiConfig) => void;
}) {
  const [draft, setDraft] = useState<AiConfig>(initial);
  const [saving, setSaving] = useState(false);
  const set = <K extends keyof AiConfig>(k: K, v: AiConfig[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  const save = async () => {
    setSaving(true);
    try {
      await invoke("ai_set_config", { cfg: draft });
      const reloaded = await invoke<AiConfig>("ai_get_config");
      onSaved(reloaded);
    } catch (e) {
      alert(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>AI 设置</h3>

        <label>🔑 API 密钥</label>
        <input
          type="password"
          value={draft.api_key}
          onChange={(e) => set("api_key", e.target.value)}
          placeholder="sk-ant-... 或 sk-..."
        />
        <small className="muted">
          没有的话:{" "}
          <a href="https://console.anthropic.com/" target="_blank" rel="noreferrer">claude.ai</a>{" "}
          注册并创建 key, 或填其他兼容服务的 key (DeepSeek/Kimi 等)。
        </small>

        <details>
          <summary>🛠 高级设置(给开发者)</summary>

          <label>服务商 (anthropic 或 openai)</label>
          <input value={draft.provider} onChange={(e) => set("provider", e.target.value)} />

          <label>Base URL</label>
          <input value={draft.base_url} onChange={(e) => set("base_url", e.target.value)} />

          <label>Executor 模型</label>
          <input value={draft.model_executor} onChange={(e) => set("model_executor", e.target.value)} />

          <label>Reviewer 模型(小模型, 审查危险动作)</label>
          <input value={draft.model_reviewer} onChange={(e) => set("model_reviewer", e.target.value)} />

          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={draft.auto_approve_all}
              onChange={(e) => set("auto_approve_all", e.target.checked)}
            />
            安全的破坏性动作自动放行
          </label>
        </details>

        <div className="modal-buttons">
          <button onClick={onClose}>取消</button>
          <button className="btn-exec" onClick={save} disabled={saving}>{saving ? "..." : "保存"}</button>
        </div>
      </div>
    </div>
  );
}
