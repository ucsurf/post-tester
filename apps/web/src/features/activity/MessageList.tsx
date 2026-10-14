import { memo, useCallback, useEffect, useRef, useState } from "react";
import { formatClock, formatSize } from "../../lib/format";
import { prettyJson } from "../../lib/text";
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

  const toggle = useCallback((seq: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(seq)) next.delete(seq);
      else next.add(seq);
      return next;
    });
  }, []);

  if (count === 0) return <p className="muted pad">{emptyText}</p>;

  return (
    <div className="messages" role="log" aria-live="off">
      {messages.map((m) => (
        <MessageRow key={m.seq} message={m} open={expanded.has(m.seq)} onToggle={toggle} />
      ))}
      <div ref={end} />
    </div>
  );
}

/** Memoized: entries keep their identity, so a busy stream only renders the new rows. */
const MessageRow = memo(function MessageRow({ message: m, open, onToggle }: { message: MessageEntry; open: boolean; onToggle: (seq: number) => void }) {
  const sent = m.direction === "sent";
  return (
    <div className={`message ${sent ? "sent" : "received"}`}>
      <button type="button" className="message-head" aria-expanded={open} onClick={() => onToggle(m.seq)}>
        <span className="message-dir" title={sent ? "Sent" : "Received"}>
          <Icon name={sent ? "arrowUp" : "arrowDown"} size={12} />
          <span className="sr-only">{sent ? "Sent" : "Received"}</span>
        </span>
        <span className="faint mono message-time">{formatClock(m.at)}</span>
        {m.label && <span className="badge">{m.label}</span>}
        <span className="mono truncate grow">{oneLine(m.payload.preview)}</span>
        <span className="faint mono">{formatSize(m.payload.size)}</span>
      </button>
      {open && (
        <pre className="code-block message-body">
          {prettyJson(m.payload.preview)}
          {m.payload.truncated ? "\n… (truncated)" : ""}
        </pre>
      )}
    </div>
  );
});

function oneLine(text: string): string {
  return text.length > 300 ? `${text.slice(0, 300).replace(/\s+/g, " ")}…` : text.replace(/\s+/g, " ");
}
