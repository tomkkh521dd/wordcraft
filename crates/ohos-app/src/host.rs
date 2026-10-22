//! Talking to the platform the other way round: Rust asks, ArkTS answers.
//!
//! The shell already pushes values *into* Rust (surface, input, configuration). Opening a file,
//! reading the clipboard and the like need the opposite: Rust asks for something and the answer
//! arrives later, on the ArkTS UI thread.
//!
//! The bridge is deliberately dumb, and pointer-free where it matters:
//!
//! * Rust pushes a JSON request (`{"id": 7, "kind": "pickOpen", "purpose": "document"}`) onto a
//!   queue and keeps a channel for the answer.
//! * ArkTS polls `native.hostTake()` once per frame (it already runs a `displaySync` callback). One
//!   request per call, so a slow picker never blocks the frame.
//! * ArkTS finishes it with `native.hostAnswer(id, json)`; the C ABI hands the bytes to
//!   [`Host::answer`], which passes them through the channel the asker is waiting on.
//!
//! Everything stays on the UI thread's side of the fence except the channel hand-off, and every
//! request carries a deadline, so a picker that never answers cannot leak a waiting caller.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a request may wait before its asker gets "no answer".
const TIMEOUT: Duration = Duration::from_secs(120);

/// What ArkTS answers: the raw JSON object it sent, or nothing when the deadline passed.
#[derive(Clone, Debug, Default)]
pub struct Reply(pub Option<String>);

impl Reply {
    /// A string field of the answer (`{"text": "…"}`, `{"path": "…"}`).
    pub fn string(&self, key: &str) -> Option<String> {
        let text = self.0.as_deref()?;
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        value.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
    }

    /// A list field of the answer (`{"formats": ["text/plain", "text/html"]}`).
    pub fn strings(&self, key: &str) -> Vec<String> {
        let Some(text) = self.0.as_deref() else { return Vec::new() };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
        value
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }

    pub fn is_cancelled(&self) -> bool {
        let Some(text) = self.0.as_deref() else { return true };
        serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v.get("cancelled").and_then(serde_json::Value::as_bool)).unwrap_or(false)
    }

    pub fn error(&self) -> Option<String> {
        self.string("error")
    }
}

/// Tests of the bridge and of the services that use it share one process-wide queue, so they run
/// one at a time (see [`host`](self)).
#[cfg(test)]
pub(crate) static BRIDGE_TEST_LOCK: Mutex<()> = Mutex::new(());

struct Pending {
    sender: Sender<Reply>,
    asked: Instant,
}

/// The queue ArkTS drains and the answers it delivers.
#[derive(Default)]
pub struct Host {
    queue: Mutex<VecDeque<String>>,
    waiting: Mutex<HashMap<u64, Pending>>,
    next_id: Mutex<u64>,
}

static HOST: OnceLock<Host> = OnceLock::new();

/// The one host bridge of the process.
pub fn host() -> &'static Host {
    HOST.get_or_init(Host::default)
}

impl Host {
    /// Ask ArkTS for something. The answer arrives on the returned channel (or times out).
    pub fn request(&self, kind: &str, params: serde_json::Value) -> Receiver<Reply> {
        let (sender, receiver) = channel();
        let mut id = match self.next_id.lock() {
            Ok(mut next) => {
                *next = next.wrapping_add(1);
                *next
            }
            Err(poisoned) => poisoned.into_inner().wrapping_add(1),
        };
        if id == 0 {
            id = 1;
        }
        let mut request = match params {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        request.insert("id".into(), serde_json::Value::from(id));
        request.insert("kind".into(), serde_json::Value::from(kind));
        let line = serde_json::Value::Object(request).to_string();
        if let Ok(mut waiting) = self.waiting.lock() {
            // Give up on anything that has waited too long before adding another.
            waiting.retain(|_, pending| pending.asked.elapsed() < TIMEOUT);
            waiting.insert(id, Pending { sender, asked: Instant::now() });
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(line);
        }
        receiver
    }

    /// The next request for ArkTS, or `None` when there is nothing to do.
    pub fn take(&self) -> Option<String> {
        self.queue.lock().ok()?.pop_front()
    }

    /// ArkTS's answer to request `id`.
    pub fn answer(&self, id: u64, json: &str) {
        let pending = self.waiting.lock().ok().and_then(|mut waiting| waiting.remove(&id));
        if let Some(pending) = pending {
            log::debug!("host answer {id} delivered: {json}");
            let _ = pending.sender.send(Reply(Some(json.to_string())));
        } else {
            log::debug!("answer for an unknown or timed-out request {id}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, because the bridge is process-wide: a request reaches ArkTS's side of the queue
    /// carrying its id and kind, and the answer comes back to the asker.
    #[test]
    fn a_request_reaches_the_platform_and_its_answer_comes_back() {
        let _guard = BRIDGE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let host = host();
        // Tests run in one process: another test's `App::new` may have left a startup question.
        while host.take().is_some() {}
        let receiver = host.request("pickOpen", serde_json::json!({"purpose": "document"}));
        let line = host.take().expect("a request is waiting");
        let request: serde_json::Value = serde_json::from_str(&line).expect("the request is JSON");
        assert_eq!(request["kind"], "pickOpen");
        assert_eq!(request["purpose"], "document");
        let id = request["id"].as_u64().expect("a request has an id");

        host.answer(id, r#"{"path":"/data/storage/el2/base/cache/in.docx"}"#);
        let reply = receiver.recv_timeout(Duration::from_secs(1)).expect("the answer arrives");
        assert_eq!(reply.string("path").as_deref(), Some("/data/storage/el2/base/cache/in.docx"));
        assert!(!reply.is_cancelled());
        assert_eq!(reply.error(), None);

        // Cancelled and error answers read as what they are.
        let cancelled = host.request("pickSave", serde_json::json!({"name": "a.docx"}));
        let line: serde_json::Value = serde_json::from_str(&host.take().expect("waiting")).expect("JSON");
        host.answer(line["id"].as_u64().unwrap_or(0), r#"{"cancelled":true}"#);
        assert!(cancelled.recv_timeout(Duration::from_secs(1)).expect("answer").is_cancelled());

        let failing = host.request("pickOpen", serde_json::json!({}));
        let line: serde_json::Value = serde_json::from_str(&host.take().expect("waiting")).expect("JSON");
        host.answer(line["id"].as_u64().unwrap_or(0), r#"{"error":"no picker"}"#);
        assert_eq!(failing.recv_timeout(Duration::from_secs(1)).expect("answer").error().as_deref(), Some("no picker"));

        // An answer nobody waits for is dropped, not a panic.
        host.answer(999_999, r#"{"text":"nobody"}"#);
        assert!(host.take().is_none(), "the queue is empty again");
    }

    #[test]
    fn a_reply_reads_its_lists() {
        let reply = Reply(Some(r#"{"formats":["text/plain","text/html"],"uris":["/a.png",7,null]}"#.into()));
        assert_eq!(reply.strings("formats"), vec!["text/plain".to_string(), "text/html".to_string()]);
        assert_eq!(reply.strings("uris"), vec!["/a.png".to_string()]);
        assert!(reply.strings("picture").is_empty());
        assert!(Reply(None).strings("formats").is_empty());
        assert!(Reply(Some("not JSON".into())).strings("formats").is_empty());
    }
}
