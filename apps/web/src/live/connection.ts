import type { Event } from "@post-tester/bindings";

export type ConnectionState = "connecting" | "open" | "closed";

export type ConnectionStatus = {
  state: ConnectionState;
  /** Failed attempts since the last successful connection. */
  attempts: number;
  /** When the next attempt starts (epoch ms), while closed. */
  retryAt: number | null;
};

/** Reconnect delay: 0.5 s, 1 s, 2 s … capped at 15 s, with ±20 % jitter. */
export function backoffDelay(attempt: number, random: () => number = Math.random): number {
  const base = Math.min(15_000, 500 * 2 ** Math.max(0, attempt));
  return Math.round(base * (0.8 + 0.4 * random()));
}

/** Accepts a batch (`Event[]`) or a single event; drops anything that is not event-shaped. */
export function parseEventMessage(data: string): Event[] {
  let value: unknown;
  try {
    value = JSON.parse(data);
  } catch {
    return [];
  }
  const list = Array.isArray(value) ? value : [value];
  return list.filter(isEvent);
}

function isEvent(value: unknown): value is Event {
  if (typeof value !== "object" || value === null) return false;
  const e = value as Record<string, unknown>;
  return typeof e.seq === "number" && typeof e.span_id === "string" && typeof e.kind === "object" && e.kind !== null;
}

type SocketLike = Pick<WebSocket, "close"> & {
  onopen: ((ev: unknown) => void) | null;
  onclose: ((ev: unknown) => void) | null;
  onerror: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
};

export type LiveConnectionOptions = {
  url: () => string;
  onEvents: (events: Event[]) => void;
  createSocket?: (url: string) => SocketLike;
};

/** Keeps a WebSocket to `/api/live` open, reconnecting with backoff. */
export class LiveConnection {
  private socket: SocketLike | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = true;
  private status: ConnectionStatus = { state: "closed", attempts: 0, retryAt: null };
  private readonly listeners = new Set<() => void>();

  constructor(private readonly options: LiveConnectionOptions) {}

  getStatus = (): ConnectionStatus => this.status;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  start(): void {
    if (!this.stopped) return;
    this.stopped = false;
    this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.setStatus({ state: "closed", attempts: 0, retryAt: null });
  }

  /** Skips the remaining backoff wait. */
  retryNow(): void {
    if (this.stopped || this.status.state !== "closed") return;
    if (this.timer) clearTimeout(this.timer);
    this.connect();
  }

  private connect(): void {
    this.timer = null;
    this.setStatus({ ...this.status, state: "connecting", retryAt: null });
    const create = this.options.createSocket ?? ((url: string) => new WebSocket(url) as unknown as SocketLike);
    let socket: SocketLike;
    try {
      socket = create(this.options.url());
    } catch {
      this.scheduleReconnect();
      return;
    }
    this.socket = socket;
    socket.onopen = () => this.setStatus({ state: "open", attempts: 0, retryAt: null });
    socket.onmessage = (message) => {
      if (typeof message.data !== "string") return;
      const events = parseEventMessage(message.data);
      if (events.length > 0) this.options.onEvents(events);
    };
    socket.onerror = () => socket.close();
    socket.onclose = () => {
      if (this.socket !== socket) return;
      this.socket = null;
      this.scheduleReconnect();
    };
  }

  private scheduleReconnect(): void {
    if (this.stopped) return;
    const delay = backoffDelay(this.status.attempts);
    this.setStatus({ state: "closed", attempts: this.status.attempts + 1, retryAt: Date.now() + delay });
    this.timer = setTimeout(() => this.connect(), delay);
  }

  private setStatus(status: ConnectionStatus): void {
    this.status = status;
    this.listeners.forEach((listener) => listener());
  }
}
