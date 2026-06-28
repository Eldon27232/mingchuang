import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
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

interface SessionSummary {
  id: string;
  title: string;
  status: string;
  updated_at?: string;
  message_count: number;
}

export function AiPanel() {
  const [config, setConfig] = useState<AiConfig | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [session, setSession] = useState<Session | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [input, setInput] = useState("");
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [editingText, setEditingText] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  const loadConfig = async () => {
    const c = await invoke<AiConfig>("ai_get_config");
    setConfig(c);
    if (!c.api_key) setShowSettings(true);
  };

  const loadSessions = async () => {
    try {
      const ss = await invoke<SessionSummary[]>("ai_list_sessions");
      setSessions(ss);
      return ss;
    } catch (e) {
      console.error("ai_list_sessions failed", e);
      return [];
    }
  };

  // 首次进入: 自动恢复最近一个 session (而不是空白)
  useEffect(() => {
    (async () => {
      await loadConfig();
      const ss = await loadSessions();
      if (ss.length > 0 && !sessionId) {
        const latest = ss[0];
        setSessionId(latest.id);
        const s = await invoke<Session | null>("ai_get_session", { sessionId: latest.id });
        setSession(s);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
      await loadSessions();  // session 标题用首条 user 消息, 发完刷一下
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
    setShowHistory(false);
    await loadSessions();
  };

  const switchToSession = async (id: string) => {
    if (id === sessionId) {
      setShowHistory(false);
      return;
    }
    setSessionId(id);
    const s = await invoke<Session | null>("ai_get_session", { sessionId: id });
    setSession(s);
    setShowHistory(false);
  };

  const deleteSession = async (id: string) => {
    if (!confirm("删除这个对话?(本机文件也会删, 无法找回)")) return;
    try {
      await invoke("ai_delete_session", { sessionId: id });
      const ss = await loadSessions();
      // 如果删的是当前 session, 切到下一个 (或新建)
      if (id === sessionId) {
        if (ss.length > 0) {
          await switchToSession(ss[0].id);
        } else {
          setSessionId(null);
          setSession(null);
        }
      }
    } catch (e) {
      alert(`删除失败: ${e}`);
    }
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
          <button
            className="icon-btn"
            onClick={() => { loadSessions(); setShowHistory(!showHistory); }}
            title="历史对话"
          >
            ☰
          </button>
          <button className="icon-btn" onClick={startNewSession} title="新对话">＋</button>
          <button className="icon-btn" onClick={() => setShowSettings(true)} title="设置">⚙</button>
        </div>
      </div>

      {showHistory && (
        <div className="ai-history-list">
          <div className="ai-history-head">
            <span className="muted small">历史对话 ({sessions.length})</span>
            <button className="icon-btn" onClick={() => setShowHistory(false)} title="关闭">×</button>
          </div>
          {sessions.length === 0 && (
            <div className="muted small" style={{ padding: "20px", textAlign: "center" }}>
              还没有对话, 点上面 + 开一个
            </div>
          )}
          {sessions.map((s) => (
            <div
              key={s.id}
              className={`ai-history-row ${s.id === sessionId ? "current" : ""}`}
              onClick={() => switchToSession(s.id)}
            >
              <div className="ai-history-title">
                {s.title || "新对话"}
                {s.id === sessionId && <span className="ai-history-current-tag"> · 当前</span>}
              </div>
              <div className="ai-history-meta muted small">
                {s.message_count} 条消息
                {s.updated_at && ` · ${new Date(s.updated_at).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}`}
              </div>
              <button
                className="ai-history-del"
                onClick={(e) => { e.stopPropagation(); deleteSession(s.id); }}
                title="删除"
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      )}

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

      {session?.pending_call && (
        <PendingApprovalModal call={session.pending_call} onDecide={approve} />
      )}

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

function AssistantMarkdown({ children }: { children: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ ...props }) => <a {...props} target="_blank" rel="noreferrer" />,
        }}
      >
        {children}
      </ReactMarkdown>
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

  const calls = m.tool_calls ?? [];
  return (
    <div className="ai-msg assistant">
      <div className="ai-msg-role">AI</div>
      {m.content && (
        <div className="ai-msg-text">
          <AssistantMarkdown>{m.content}</AssistantMarkdown>
        </div>
      )}
      <ToolCallList calls={calls} expanded={expanded} onToggle={toggle} />
      {onRetry && (
        <div className="msg-actions">
          <button onClick={onRetry} className="btn-retry">↻ 重试</button>
        </div>
      )}
    </div>
  );
}

/// 单条消息内的 tool_calls 列表渲染。≥2 个时合并成 "▶ 调用了 N 次工具" 收纳条
/// (修复用户反馈: 一轮 AI 调 9 个工具时铺一屏的问题)
function ToolCallList({
  calls, expanded, onToggle,
}: {
  calls: ToolCallView[];
  expanded: Set<string>;
  onToggle: (id: string) => void;
}) {
  const [burstOpen, setBurstOpen] = useState(false);
  if (calls.length === 0) return null;
  // 永远包成 "调用了 N 次工具" 折叠块 (即使 N=1), 这样思考链 (text) 跟工具调用
  // 视觉上严格分开, 不会因为单个 tool 而把卡片直接展开
  const preview = calls.slice(0, 3).map((c) => humanizeToolCall(c.name, c.args)).join(" / ");
  const more = calls.length > 3 ? ` 等 ${calls.length} 步` : "";
  const anyFailed = calls.some((c) => c.status === "failed" || c.status === "denied");
  const allDone = calls.every((c) => c.status === "done");
  return (
    <div className={`ai-toolburst ${anyFailed ? "has-failed" : allDone ? "all-done" : ""}`}>
      <div className="toolburst-head" onClick={() => setBurstOpen(!burstOpen)}>
        <span className="caret">{burstOpen ? "▼" : "▶"}</span>
        <span className="toolburst-count">调用了 {calls.length} 次工具</span>
        <span className="muted small"> · {preview}{more}</span>
      </div>
      {burstOpen && (
        <div className="toolburst-body">
          {calls.map((c) => (
            <ToolCallRow key={c.id} call={c} expanded={expanded.has(c.id)} onToggle={() => onToggle(c.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolCallRow({
  call, expanded, onToggle,
}: { call: ToolCallView; expanded: boolean; onToggle: () => void }) {
  return (
    <div className={`ai-toolcall status-${call.status}`}>
      <div className="toolcall-head" onClick={onToggle}>
        <span className="caret">{expanded ? "▼" : "▶"}</span>
        <span className="toolcall-action">{humanizeToolCall(call.name, call.args)}</span>
        <span className="muted small"> · {statusLabel(call.status)}</span>
      </div>
      {expanded && (
        <div className="toolcall-body">
          {call.review && (() => {
            const v = humanizeVerdict(call.review!.verdict);
            return <div className={`ai-review ${v.cls}`}>{v.icon} {v.text} ({call.review!.reason})</div>;
          })()}
          <details>
            <summary className="muted small">技术细节 ({call.name})</summary>
            <pre className="ai-args">{JSON.stringify(call.args, null, 2)}</pre>
          </details>
          {call.result && <div className="ai-result small">→ {call.result}</div>}
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

function PendingApprovalModal({ call, onDecide }: { call: ToolCallView; onDecide: (d: "approve" | "deny") => void; }) {
  const v = call.review ? humanizeVerdict(call.review.verdict) : null;
  return (
    <div className="modal-backdrop" onClick={(e) => e.stopPropagation()}>
      <div className="modal ai-approval-modal" onClick={(e) => e.stopPropagation()}>
        <h3>⏳ AI 想替你做这件事</h3>
        <div className="ai-approval-action">{humanizeToolCall(call.name, call.args)}</div>
        {v && (
          <div className={`ai-review ${v.cls}`}>
            {v.icon} {v.text}{call.review?.reason ? ` — ${call.review.reason}` : ""}
          </div>
        )}
        <details>
          <summary className="muted small">技术细节 ({call.name})</summary>
          <pre>{JSON.stringify(call.args, null, 2)}</pre>
        </details>
        <p className="muted small">点"好"AI 会立即执行, 点"不要"它知道你拒了, 会换路子或停。</p>
        <div className="modal-buttons">
          <button onClick={() => onDecide("deny")} className="btn-restore">不要</button>
          <button onClick={() => onDecide("approve")} className="btn-exec">好</button>
        </div>
      </div>
    </div>
  );
}

const PROVIDER_PRESETS: Record<string, { base_url: string; model_executor: string; model_reviewer: string; key_link: string }> = {
  anthropic: {
    base_url: "https://api.anthropic.com",
    model_executor: "claude-sonnet-4-6",
    model_reviewer: "claude-haiku-4-5-20251001",
    key_link: "https://console.anthropic.com/",
  },
  openai: {
    base_url: "https://api.openai.com",
    model_executor: "gpt-4o",
    model_reviewer: "gpt-4o-mini",
    key_link: "https://platform.openai.com/api-keys",
  },
};

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

  const onProviderChange = (p: string) => {
    const preset = PROVIDER_PRESETS[p];
    setDraft((d) => ({
      ...d,
      provider: p,
      // 选预设时把 URL/模型默认填上(用户可以再改)
      base_url: preset?.base_url || d.base_url,
      model_executor: preset?.model_executor || d.model_executor,
      model_reviewer: preset?.model_reviewer || d.model_reviewer,
    }));
  };

  const isValid =
    !!draft.provider.trim() &&
    !!draft.api_key.trim() &&
    !!draft.base_url.trim() &&
    !!draft.model_executor.trim() &&
    !!draft.model_reviewer.trim();

  const save = async () => {
    if (!isValid) return;
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

  const keyLink = PROVIDER_PRESETS[draft.provider]?.key_link;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>AI 设置</h3>
        <small className="muted">下面所有都是必填项。</small>

        <label>服务商 *</label>
        <select value={draft.provider} onChange={(e) => onProviderChange(e.target.value)} className="provider-select">
          <option value="anthropic">Anthropic (Claude)</option>
          <option value="openai">OpenAI / 兼容 (DeepSeek / Kimi / 智谱 / Ollama 等)</option>
        </select>

        <label>API 密钥 *</label>
        <input
          type="password"
          value={draft.api_key}
          onChange={(e) => set("api_key", e.target.value)}
          placeholder={draft.provider === "anthropic" ? "sk-ant-..." : "sk-..."}
        />
        {keyLink && (
          <small className="muted">
            没有的话:{" "}
            <a href={keyLink} target="_blank" rel="noreferrer">点这里去注册</a>
          </small>
        )}

        <label>Base URL *</label>
        <input value={draft.base_url} onChange={(e) => set("base_url", e.target.value)} />

        <label>主模型 *(Executor — 处理思考和工具调用)</label>
        <input value={draft.model_executor} onChange={(e) => set("model_executor", e.target.value)} />

        <label>审查模型 *(Reviewer — 小模型,审查危险动作)</label>
        <input value={draft.model_reviewer} onChange={(e) => set("model_reviewer", e.target.value)} />

        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={draft.auto_approve_all}
            onChange={(e) => set("auto_approve_all", e.target.checked)}
          />
          Reviewer 判 safe 时自动放行(否则破坏性动作都要你点确认)
        </label>

        <div className="modal-buttons">
          <button onClick={onClose}>取消</button>
          <button className="btn-exec" onClick={save} disabled={saving || !isValid}>
            {saving ? "..." : isValid ? "保存" : "请填完所有必填项"}
          </button>
        </div>
      </div>
    </div>
  );
}
