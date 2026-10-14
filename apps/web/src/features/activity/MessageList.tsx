import { useEffect, useRef, useState } from "react";
import { formatClock, formatSize } from "../../lib/format";
import { parseJson } from "../../lib/text";
import type { MessageEntry } from "../../live/spans";
import { Icon } from "../../ui/Icon";
import "./activity.css";

type Props = {
  messages: readonly MessageEntry[];
  emptyText?: string;
  /** Keep the newest message in view while new ones arrive. */
  follow?: boolean;
};

/** Live list of WebSocket / SSE / MCP messages with direction arrows; click to expand. */
export function MessageList({ messages, emptyText = "No messages yet.", follow = true }: Props) {
  const end = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<number>>(new Set());
  const count = messages.length;

  useEffect(() => {
    if (follow) end.current?.scrollIntoView({ block: "nearest" });
  }, [count, follow]);

  if (count === 0) return <p className="muted pad">{emptyText}</p>;

  const toggle = (seq: number) => {
    const next = new Set(expanded);
    if (next.has(seq)) next.delete(seq);
    else next.add(seq);
    setExpanded(next);
  };

  return (
    <div className="messages" role="log" aria-live="off">
      {messages.map((m) => {
        const open = expanded.has(m.seq);
        const sent = m.direction === "sent";
        return (
          <div key={m.seq} className={`message ${sent ? "sent" : "received"}`}>
            <button type="button" className="message-head" aria-expanded={open} onClick={() => toggle(m.seq)}>
              <span className="message-dir" title={sent ? "Sent" : "Received"}>
                <Icon name={sent ? "arrowUp" : "arrowDown"} size={12} />
                <span className="sr-only">{sent ? "Sent" : "Received"}</span>
              </span>
              <span className="faint mono message-time">{formatClock(m.at)}</span>
              {m.label && <span className="badge">{m.label}</span>}
              <span className="mono truncate grow">{oneLine(m.payload.preview)}</span>
              <span className="faint mono">{formatSize(m.payload.size)}</span>
            </button>
            {open && <pre className="code-block message-body">{pretty(m.payload.preview)}{m.payload.truncated ? "\n… (truncated)" : ""}</pre>}
          </div>
        );
      })}
      <div ref={end} />
    </div>
  );
}

function oneLine(text: string): string {
  return text.length > 300 ? `${text.slice(0, 300).replace(/\s+/g, " ")}…` : text.replace(/\s+/g, " ");
}

function pretty(text: string): string {
  const parsed = parseJson(text);
  return parsed.ok ? JSON.stringify(parsed.value, null, 2) : text;
}
