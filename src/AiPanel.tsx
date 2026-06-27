import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
  aborted?: boolean;
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

  useEffect(() => {
    loadConfig();
  }, []);

  const refreshSession = async (id: string) => {
    const s = await invoke<Session | null>("ai_get_session", { sessionId: id });
    setSession(s);
  };

  const isRunning = session?.status === "thinking";
  const isWaiting = session?.status === "waiting_approval";

  useEffect(() => {
    if (!sessionId) return;
    // 一直轮询(简单,下一轮换 event 推送)
    const t = setInterval(() => refreshSession(sessionId), 800);
    return () => clearInterval(t);
  }, [sessionId]);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [session?.messages.length]);

  const startSession = async () => {
    const id = await invoke<string>("ai_create_session");
    setSessionId(id);
    setSession({
      id,
      messages: [],
      pending_call: null,
      status: "idle",
      tool_call_count: 0,
    });
  };

  const send = async () => {
    if (!input.trim()) return;
    let sid = sessionId;
    if (!sid) {
      sid = await invoke<string>("ai_create_session");
      setSessionId(sid);
    }
    const msg = input;
    setInput("");
    setSession((prev) =>
      prev
        ? { ...prev, messages: [...prev.messages, { role: "user", content: msg }], status: "thinking" }
        : prev
    );
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
      alert(`中止失败: ${e}`);
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
      // 先中止,避免编辑中 LLM 又改了 messages
      await invoke("ai_abort_session", { sessionId });
      await invoke("ai_edit_user_message", {
        sessionId,
        msgIndex: idx,
        newContent: text,
      });
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

  // 找到最后一条 assistant 索引,用于显示"重试"
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
        <h2>AI 助手</h2>
        <div className="ai-header-actions">
          {sessionId && (
            <span className="muted small">
              status={session?.status ?? "?"} · tools={session?.tool_call_count ?? 0}
            </span>
          )}
          <button onClick={startSession}>新会话</button>
          <button onClick={() => setShowSettings(true)}>⚙ 设置</button>
        </div>
      </div>

      {showSettings && (
        <SettingsModal
          initial={config!}
          onClose={() => setShowSettings(false)}
          onSaved={(c) => {
            setConfig(c);
            setShowSettings(false);
          }}
        />
      )}

      {!config?.api_key && !showSettings && (
        <div className="banner warn">
          ⚠ 未配置 API key,请点右上"⚙ 设置"填入 Anthropic key 后再用 AI。
        </div>
      )}

      <div className="ai-log" ref={logRef}>
        {(session?.messages ?? []).map((m, i) => (
          <MessageView
            key={i}
            m={m}
            index={i}
            onEdit={startEdit}
            onRetry={i === lastAssistantIdx && !isRunning && !isWaiting ? retry : undefined}
            editing={editingIndex === i}
            editingText={editingText}
            setEditingText={setEditingText}
            onSubmitEdit={submitEdit}
            onCancelEdit={cancelEdit}
          />
        ))}
        {session?.last_error && (
          <div className="ai-msg err">⚠ {session.last_error}</div>
        )}
        {isRunning && (
          <div className="ai-msg assistant thinking">
            <span className="thinking-dots">思考中</span>
          </div>
        )}
      </div>

      {session?.pending_call && (
        <PendingApproval call={session.pending_call} onDecide={approve} />
      )}

      <div className="ai-input">
        <input
          type="text"
          placeholder="自然语言描述电脑问题,例如:123云盘装了卸不掉帮我清"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          disabled={isRunning || !config?.api_key}
        />
        {isRunning || isWaiting ? (
          <button onClick={abort} className="btn-restore">停止</button>
        ) : (
          <button onClick={send} disabled={!input.trim() || !config?.api_key}>发送</button>
        )}
      </div>
    </div>
  );
}

function MessageView({
  m,
  index,
  onEdit,
  onRetry,
  editing,
  editingText,
  setEditingText,
  onSubmitEdit,
  onCancelEdit,
}: {
  m: ChatMessage;
  index: number;
  onEdit: (i: number, text: string) => void;
  onRetry?: () => void;
  editing: boolean;
  editingText: string;
  setEditingText: (s: string) => void;
  onSubmitEdit: () => void;
  onCancelEdit: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const toggle = (id: string) => {
    setExpanded((s) => {
      const n = new Set(s);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });
  };

  // tool role 完全不展示 (这是 tool_result, AI 看的, 用户不需要)
  if (m.role === "tool") return null;

  if (m.role === "user") {
    return (
      <div
        className="ai-msg user"
        onContextMenu={(e) => {
          e.preventDefault();
          if (!editing) onEdit(index, m.content);
        }}
        title="右键重新编辑"
      >
        <div className="ai-msg-role">你</div>
        {editing ? (
          <div className="edit-box">
            <textarea
              value={editingText}
              onChange={(e) => setEditingText(e.target.value)}
              rows={Math.max(2, editingText.split("\n").length)}
              autoFocus
            />
            <div className="edit-buttons">
              <button onClick={onCancelEdit}>取消</button>
              <button className="btn-exec" onClick={onSubmitEdit}>重发</button>
            </div>
          </div>
        ) : (
          <div className="ai-msg-text">{m.content}</div>
        )}
      </div>
    );
  }

  // assistant
  return (
    <div className="ai-msg assistant">
      <div className="ai-msg-role">AI</div>
      {m.content && <div className="ai-msg-text">{m.content}</div>}
      {m.tool_calls?.map((c) => {
        const ex = expanded.has(c.id);
        return (
          <div key={c.id} className={`ai-toolcall status-${c.status}`}>
            <div className="toolcall-head" onClick={() => toggle(c.id)}>
              <span className="caret">{ex ? "▼" : "▶"}</span>
              <code>{c.name}</code>
              <span className="muted small"> · {statusLabel(c.status)}</span>
              {c.result && !ex && <span className="muted small toolcall-summary"> · {c.result}</span>}
            </div>
            {ex && (
              <div className="toolcall-body">
                {c.review && (
                  <div className="ai-review small">
                    [Reviewer:{c.review.verdict}] {c.review.reason}
                  </div>
                )}
                <pre className="ai-args">{JSON.stringify(c.args, null, 2)}</pre>
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
    case "reviewing": return "审查中";
    case "waiting_approval": return "待批准";
    case "approved": return "已批";
    case "denied": return "已拒";
    case "executing": return "执行中";
    case "done": return "完成";
    case "failed": return "失败";
    default: return s;
  }
}

function PendingApproval({
  call,
  onDecide,
}: {
  call: ToolCallView;
  onDecide: (d: "approve" | "deny") => void;
}) {
  return (
    <div className="ai-approval">
      <div className="ai-approval-title">⏳ AI 想做这件事, 等你确认</div>
      <div>
        要调用 <code>{call.name}</code>:
      </div>
      <pre>{JSON.stringify(call.args, null, 2)}</pre>
      {call.review && (
        <div className="ai-review">
          [Reviewer:{call.review.verdict}] {call.review.reason}
        </div>
      )}
      <div className="ai-approval-buttons">
        <button className="btn-exec" onClick={() => onDecide("approve")}>批准并执行</button>
        <button className="btn-restore" onClick={() => onDecide("deny")}>拒绝</button>
      </div>
    </div>
  );
}

function SettingsModal({
  initial,
  onClose,
  onSaved,
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
        <div className="muted small">本工具用 Anthropic Claude。下一轮加 OpenAI 兼容。</div>

        <label>Provider</label>
        <input value={draft.provider} disabled />

        <label>API key</label>
        <input
          type="password"
          value={draft.api_key}
          onChange={(e) => set("api_key", e.target.value)}
          placeholder="sk-ant-..."
        />
        <div className="muted small">填新值会覆盖;含…的脱敏值会保留原 key。</div>

        <label>Base URL</label>
        <input value={draft.base_url} onChange={(e) => set("base_url", e.target.value)} />

        <label>Executor 模型</label>
        <input
          value={draft.model_executor}
          onChange={(e) => set("model_executor", e.target.value)}
        />

        <label>Reviewer 模型(小模型,审查危险动作)</label>
        <input
          value={draft.model_reviewer}
          onChange={(e) => set("model_reviewer", e.target.value)}
        />

        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={draft.auto_approve_all}
            onChange={(e) => set("auto_approve_all", e.target.checked)}
          />
          Reviewer 判 safe 时自动放行(否则破坏性动作都要点确认)
        </label>

        <div className="modal-buttons">
          <button onClick={onClose}>取消</button>
          <button className="btn-exec" onClick={save} disabled={saving}>
            {saving ? "..." : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
