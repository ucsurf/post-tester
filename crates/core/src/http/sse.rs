//! An incremental Server-Sent Events parser (`text/event-stream`).
//!
//! Feed it chunks as they arrive; it returns each complete event.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// The `event:` field; `None` means the default `message` type.
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    current: SseEvent,
    has_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a chunk of the stream and returns the events it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=newline).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if self.has_data {
                    events.push(std::mem::take(&mut self.current));
                    self.has_data = false;
                } else {
                    self.current = SseEvent::default();
                }
                continue;
            }
            if line.starts_with(':') {
                continue; // comment / keep-alive
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line, ""),
            };
            match field {
                "event" => self.current.event = Some(value.to_string()),
                "data" => {
                    if self.has_data {
                        self.current.data.push('\n');
                    }
                    self.current.data.push_str(value);
                    self.has_data = true;
                }
                "id" => self.current.id = Some(value.to_string()),
                _ => {}
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_events_split_across_chunks() {
        let mut parser = SseParser::new();
        assert!(parser.feed(b"event: tick\nda").is_empty());
        let events = parser.feed(b"ta: 1\ndata: 2\n\n: ping\n\ndata: x\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], SseEvent { event: Some("tick".into()), data: "1\n2".into(), id: None });
        assert_eq!(events[1].data, "x");
    }
}
