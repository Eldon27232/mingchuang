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
  const [sending, setSending] = useState(false);
  const [pollOn, setPollOn] = useState(false);
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
    if (s && (s.status === "done" || s.status === "failed" || s.status === "waiting_approval" || s.status === "idle")) {
      setPollOn(false);
    }
  };

  useEffect(() => {
    if (!sessionId || !pollOn) return;
    const t = setInterval(() => refreshSession(sessionId), 1000);
    return () => clearInterval(t);
  }, [sessionId, pollOn]);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [session]);

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
    setSending(true);
    setPollOn(true);
    try {
      // 立刻乐观追加用户消息
      setSession((prev) => prev ? { ...prev, messages: [...prev.messages, { role: "user", content: msg }], status: "thinking" } : prev);
      await invoke("ai_send_message", { sessionId: sid, message: msg });
      await refreshSession(sid);
    } catch (e) {
      alert(`发送失败: ${e}`);
    } finally {
      setSending(false);
    }
  };

  const approve = async (decision: "approve" | "deny") => {
    if (!sessionId) return;
    setPollOn(true);
    try {
      await invoke("ai_approve_pending", { sessionId, decision });
      await refreshSession(sessionId);
    } catch (e) {
      alert(`审批失败: ${e}`);
    }
  };

  return (
    <div className="ai-panel">
      <div className="ai-header">
        <h2>AI 助手</h2>
        <div className="ai-header-actions">
          {sessionId && (
            <span className="muted small">
              session={sessionId.slice(0, 14)}… · 状态={session?.status ?? "?"} · tool_calls={session?.tool_call_count ?? 0}
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
          <MessageView key={i} m={m} />
        ))}
        {session?.last_error && (
          <div className="ai-msg err">⚠ {session.last_error}</div>
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
          disabled={sending || !config?.api_key}
        />
        <button onClick={send} disabled={sending || !input.trim() || !config?.api_key}>
          {sending ? "发送中..." : "发送"}
        </button>
      </div>
    </div>
  );
}

function MessageView({ m }: { m: ChatMessage }) {
  const role = m.role;
  const cls = role === "user" ? "user" : role === "assistant" ? "assistant" : "tool";
  return (
    <div className={`ai-msg ${cls}`}>
      <div className="ai-msg-role">{role}</div>
      {m.content && <div className="ai-msg-text">{m.content}</div>}
      {m.tool_calls?.map((c) => (
        <div key={c.id} className={`ai-toolcall status-${c.status}`}>
          <code>{c.name}</code>
          <span className="muted small"> · {c.status}</span>
          {c.review && (
            <div className="ai-review small">
              [Reviewer:{c.review.verdict}] {c.review.reason}
            </div>
          )}
          <pre className="ai-args">{JSON.stringify(c.args, null, 2)}</pre>
          {c.result && <div className="ai-result small">→ {c.result}</div>}
        </div>
      ))}
    </div>
  );
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
      <div className="ai-approval-title">⏳ 待用户审批</div>
      <div>
        AI 想调用 <code>{call.name}</code>:
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
        <div className="muted small">本工具用 Anthropic Claude(下一轮支持 OpenAI 兼容)。</div>

        <label>Provider</label>
        <input value={draft.provider} disabled />

        <label>API key</label>
        <input
          type="password"
          value={draft.api_key}
          onChange={(e) => set("api_key", e.target.value)}
          placeholder="sk-ant-..."
        />
        <div className="muted small">填新值会覆盖;留脱敏值不变。</div>

        <label>Base URL</label>
        <input value={draft.base_url} onChange={(e) => set("base_url", e.target.value)} />

        <label>Executor 模型</label>
        <input
          value={draft.model_executor}
          onChange={(e) => set("model_executor", e.target.value)}
        />

        <label>Reviewer 模型(小模型,做安全审查)</label>
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
          Reviewer 判 safe 时自动放行(否则破坏性动作都要用户点确认)
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
