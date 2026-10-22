//! The platform services `wordcraft-ui-egui` asks for, answered through the host bridge.
//!
//! Two shapes of answer, because the platform offers two:
//!
//! * A file dialog returns a channel the file dialogs machinery polls, so the picker never blocks
//!   the frame the user tapped in ([`file_dialog`]).
//! * The clipboard is a cache: a read is asked for when the cache is older than
//!   [`CLIPBOARD_REFRESH`], and a caller that needs the answer (a paste) waits [`ANSWER_WAIT`] for
//!   it, which on the device is one frame. One long-lived worker does the asking, so a platform
//!   that never answers cannot pile up threads.
//!
//! Pictures arrive as a path: ArkTS packs the system clipboard's `PixelMap` into the app's cache
//! folder, and the bytes are read here and handed over as
//! [`ClipboardPicture::Encoded`](wordcraft_ui_egui::paste_picture::ClipboardPicture::Encoded), the
//! same route the desktop's clipboard takes.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::json;
use wordcraft_ui_egui::paste_picture::ClipboardPicture;
use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest};

use crate::host;

/// How often the clipboard is re-read while something may paste it.
const CLIPBOARD_REFRESH: Duration = Duration::from_millis(400);
/// How long a paste waits for the answer to a read it asked for. One frame on the device, but a
/// clipboard holding a copied *picture* has to be packed into the app's cache first, which took
/// longer than 300 ms on the tablet and left the paste looking at an empty answer.
const ANSWER_WAIT: Duration = Duration::from_millis(1500);
/// How long the background read waits for the platform at all.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// The largest clipboard picture that may be pasted (as `insert.picture` allows).
const MAX_PICTURE_BYTES: u64 = 200 << 20;

#[derive(Default)]
struct Clipboard {
    text: Option<String>,
    html: Option<String>,
    /// The MIME types on the clipboard, so "there is nothing to paste" can say what is there.
    formats: Vec<String>,
    /// Copied files (a file manager's Copy), as sandbox paths.
    files: Vec<PathBuf>,
    /// The clipboard's picture, as PNG bytes, read from the path ArkTS packed it to.
    picture: Option<Vec<u8>>,
    /// When a read was last asked for.
    asked: Option<Instant>,
    /// When an answer last landed (or the read failed), so a waiter can stop waiting.
    answered: Option<Instant>,
}

fn clipboard() -> &'static Mutex<Clipboard> {
    static CLIPBOARD: OnceLock<Mutex<Clipboard>> = OnceLock::new();
    CLIPBOARD.get_or_init(|| Mutex::new(Clipboard::default()))
}

/// The text on the system clipboard, if any.
///
/// A clipboard holding only copied *pictures* (a file manager's Copy, which arrives as `text/uri`
/// records) is reported as their paths: the UI's paste then inserts them as pictures, the same way
/// it does on the desktop, where a copied file puts its path on the clipboard as text.
pub fn clipboard_text() -> Option<String> {
    refresh_clipboard();
    let cache = match clipboard().lock() {
        Ok(cache) => cache,
        Err(_) => return None,
    };
    if let Some(text) = cache.text.clone() {
        return Some(text);
    }
    if !cache.files.is_empty() && cache.files.iter().all(|path| is_picture_file(path)) {
        let paths: Vec<String> = cache.files.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        log::debug!("clipboard holds {} copied picture(s): reporting their paths so a paste inserts them", paths.len());
        return Some(paths.join(
            "
",
        ));
    }
    None
}

/// Whether a path names a picture the engine can insert (the extensions `paste_picture` accepts).
fn is_picture_file(path: &std::path::Path) -> bool {
    const PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "wmf", "emf"];
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| PICTURES.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

/// The HTML on the system clipboard (another program's formatted copy), if any.
pub fn clipboard_html() -> Option<String> {
    refresh_clipboard();
    clipboard().lock().ok().and_then(|c| c.html.clone())
}

/// The MIME types on the system clipboard (empty when the platform doesn't say).
pub fn clipboard_formats() -> Vec<String> {
    refresh_clipboard();
    clipboard().lock().map(|c| c.formats.clone()).unwrap_or_default()
}

/// The picture on the system clipboard — copied pixels, or copied picture files.
pub fn clipboard_picture() -> Option<ClipboardPicture> {
    refresh_clipboard();
    let cache = clipboard().lock().ok()?;
    if let Some(png) = &cache.picture {
        return Some(ClipboardPicture::Encoded(png.clone()));
    }
    if !cache.files.is_empty() {
        return Some(ClipboardPicture::Files(cache.files.clone()));
    }
    None
}

/// Put HTML, its plain-text alternative and (for one equation) its MathML on the clipboard.
///
/// HarmonyOS's pasteboard takes plain text and HTML; MathML has no record of its own there, so
/// other programs get the text and the formatting, not the equation.
pub fn set_clipboard_html(html: &str, text: &str, mathml: &str) -> bool {
    let _ = host::host().request("clipboardWrite", json!({"html": html, "text": text, "mathml": mathml}));
    if let Ok(mut cache) = clipboard().lock() {
        cache.text = Some(text.to_string()).filter(|t| !t.is_empty());
        cache.html = Some(html.to_string()).filter(|h| !h.is_empty());
        let mut formats: Vec<String> = cache.text.iter().map(|_| "text/plain".to_string()).collect();
        if cache.html.is_some() {
            formats.push("text/html".into());
        }
        cache.formats = formats;
        cache.picture = None;
        cache.files.clear();
        let now = Instant::now();
        cache.asked = Some(now);
        cache.answered = Some(now);
    }
    true
}

/// Read the clipboard now, e.g. when the app comes back to the foreground: a copied picture's
/// grant can lapse while another app is in front, so the copy has to be made while it still holds.
pub fn preload_clipboard() {
    if let Ok(mut cache) = clipboard().lock() {
        cache.asked = None;
    }
    refresh_clipboard();
}

/// Ask the platform for the clipboard when the cache is old, and wait for the answer when the
/// caller needs one (a paste does; a "can I paste?" check doesn't).
fn refresh_clipboard() {
    let stale = clipboard().lock().map(|c| c.asked.is_none_or(|asked| asked.elapsed() > CLIPBOARD_REFRESH)).unwrap_or(false);
    if !stale {
        return;
    }
    let asked = Instant::now();
    if let Ok(mut cache) = clipboard().lock() {
        cache.asked = Some(asked);
    }
    ping();
    wait_for_answer(asked);
}

/// Wait until an answer landed after `asked` (or [`ANSWER_WAIT`] passed, when the platform is
/// slow or silent — the cache then answers with what it has).
fn wait_for_answer(asked: Instant) {
    let deadline = asked + ANSWER_WAIT;
    loop {
        let answered = clipboard().lock().ok().and_then(|c| c.answered);
        if answered.is_some_and(|at| at >= asked) || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Tell the clipboard worker to read the clipboard once.
fn ping() {
    static PING: OnceLock<Sender<()>> = OnceLock::new();
    let sender = PING.get_or_init(|| {
        let (sender, receiver) = channel();
        if std::thread::Builder::new().name("clipboard".into()).spawn(move || read_clipboard(receiver)).is_err() {
            log::warn!("no clipboard thread: the clipboard stays empty");
        }
        sender
    });
    if let Err(e) = sender.send(()) {
        log::warn!("the clipboard thread is gone: {e}");
    }
}

/// The one thread that reads the clipboard, so a platform that never answers can't leak threads.
fn read_clipboard(receiver: Receiver<()>) {
    while receiver.recv().is_ok() {
        // One read answers every ping that arrived while the last one was out.
        while receiver.try_recv().is_ok() {}
        let reply = host::host().request("clipboardRead", json!({}));
        let arrived = reply.recv_timeout(READ_TIMEOUT).ok();
        apply(arrived.as_ref());
    }
}

/// Put what the platform said into the cache.
///
/// A failed read is *unknown*, not empty: on HarmonyOS reading the clipboard needs the restricted
/// `ohos.permission.READ_PASTEBOARD` grant, and until it is granted the answer is an error. The
/// cache then keeps what this app last copied, so copying and pasting inside WordCraft keeps
/// working; only an answer that says "nothing is there" empties it.
fn apply(reply: Option<&host::Reply>) {
    let Ok(mut cache) = clipboard().lock() else { return };
    let Some(reply) = reply else {
        cache.answered = Some(Instant::now());
        return;
    };
    if reply.error().is_some() {
        cache.answered = Some(Instant::now());
        return;
    }
    cache.text = reply.string("text").filter(|text| !text.is_empty());
    cache.html = reply.string("html").filter(|html| !html.is_empty());
    cache.formats = reply.strings("formats");
    cache.files = reply.strings("uris").into_iter().map(PathBuf::from).collect();
    cache.picture = reply.string("picture").as_deref().and_then(|path| take_picture(path, MAX_PICTURE_BYTES));
    cache.answered = Some(Instant::now());
}

/// Read a picture ArkTS packed into the cache folder, then delete it; oversized files are refused
/// before they are read (the clipboard comes from other programs).
fn take_picture(path: &str, cap: u64) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    if size == 0 || size > cap {
        log::warn!("clipboard picture is {size} bytes: too large to paste");
        let _ = std::fs::remove_file(path);
        return None;
    }
    let mut bytes = Vec::new();
    match std::io::Read::take(&mut file, cap.saturating_add(1)).read_to_end(&mut bytes) {
        Ok(_) if bytes.len() as u64 <= cap => {}
        _ => {
            let _ = std::fs::remove_file(path);
            return None;
        }
    }
    drop(file);
    let _ = std::fs::remove_file(path);
    Some(bytes)
}

/// Ask the platform for the document to open at launch: a URI another app shared with us (the file
/// manager's "open with"), or the working copy that was open when the app was last closed. The
/// answer arrives on a later frame; `cancelled` means nothing to open.
pub fn startup_document() -> Receiver<host::Reply> {
    host::host().request("startupDocument", json!({}))
}

/// Hand a document to the system's share sheet (HarmonyOS Share Kit). The answer comes later; this
/// only queues the request, so File › Share never blocks a frame.
pub fn share_document(path: &str) -> Result<(), String> {
    let _ = host::host().request("share", json!({"path": path}));
    Ok(())
}

/// What the file dialogs ask of the platform: a picker, answered later through the bridge.
pub fn file_dialog(request: FileDialogRequest) -> Receiver<FileDialogAnswer> {
    let (kind, params) = match request {
        FileDialogRequest::Open { purpose } => ("pickOpen", json!({"purpose": purpose})),
        FileDialogRequest::OpenMany { purpose } => ("pickOpen", json!({"purpose": purpose, "many": true})),
        FileDialogRequest::Save { name, .. } => ("pickSave", json!({"name": name})),
    };
    let reply = host::host().request(kind, params);
    let (sender, receiver) = channel();
    // The dialogs machinery polls the receiver on the UI thread, so the waiting happens here.
    std::thread::spawn(move || {
        let reply = reply.recv_timeout(ASK_TIMEOUT).unwrap_or_default();
        log::debug!("the picker answered: {:?}", reply.0);
        let answer = if let Some(error) = reply.error() {
            Err(error)
        } else if reply.is_cancelled() {
            Ok(Vec::new())
        } else if !reply.strings("paths").is_empty() {
            Ok(reply.strings("paths"))
        } else {
            reply.string("path").map(|path| vec![path]).ok_or_else(|| "the picker answered without a path".to_string())
        };
        let _ = sender.send(answer);
    });
    receiver
}

/// How long a picker may stay open before its asker is told nothing was picked.
const ASK_TIMEOUT: Duration = Duration::from_secs(300);

/// The services the HarmonyOS app hands to the UI.
pub fn services() -> wordcraft_ui_egui::Services {
    wordcraft_ui_egui::Services {
        file_dialog: Some(Box::new(file_dialog)),
        clipboard_text: Some(Box::new(clipboard_text)),
        clipboard_html: Some(Box::new(clipboard_html)),
        clipboard_formats: Some(Box::new(clipboard_formats)),
        clipboard_picture: Some(Box::new(clipboard_picture)),
        set_clipboard_html: Some(Box::new(set_clipboard_html)),
        share_file: Some(Box::new(share_document)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    //! These play ArkTS's side: they take the request the app queued, answer it, and check what the
    //! app then believes about the platform.

    use super::*;
    use crate::host::{BRIDGE_TEST_LOCK, Reply, host};

    /// Drop requests a previous test left behind (one process: `App::new` asks for a startup
    /// document, and other tests' answers may still be in flight).
    fn drain() {
        while host().take().is_some() {}
    }

    /// Take the request the app queued and answer it.
    fn answer_next(json: &str) -> serde_json::Value {
        let line = host().take().expect("a request is waiting for the platform");
        let request: serde_json::Value = serde_json::from_str(&line).expect("a request is JSON");
        let id = request["id"].as_u64().expect("a request has an id");
        host().answer(id, json);
        request
    }

    /// Play ArkTS until a request arrives, then answer it with `json` (a paste asks and waits).
    fn answer_when_asked(json: &str) -> std::thread::JoinHandle<()> {
        let json = json.to_string();
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if let Some(line) = host().take() {
                    let request: serde_json::Value = serde_json::from_str(&line).expect("JSON");
                    let id = request["id"].as_u64().expect("an id");
                    host().answer(id, &json);
                    return;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    }

    #[test]
    fn copying_puts_text_and_html_on_the_clipboard_and_pastes_it_back() {
        let _guard = BRIDGE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        drain();

        // Copy: the platform is asked for both records, and a paste right after reads the cache.
        assert!(set_clipboard_html("<p>Hello</p>", "Hello", ""));
        assert_eq!(clipboard_text().as_deref(), Some("Hello"));
        assert_eq!(clipboard_html().as_deref(), Some("<p>Hello</p>"));
        assert_eq!(clipboard_formats(), vec!["text/plain".to_string(), "text/html".to_string()]);
        let request = answer_next(r#"{"ok":true}"#);
        assert_eq!(request["kind"], "clipboardWrite");
        assert_eq!(request["html"], "<p>Hello</p>");
        assert_eq!(request["text"], "Hello");

        // Age the cache: the next paste asks the platform and waits for the answer.
        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
        }
        let platform = answer_when_asked(r#"{"text":"from the system","html":"<i>from the system</i>","formats":["text/plain","text/html"]}"#);
        assert_eq!(clipboard_text().as_deref(), Some("from the system"));
        assert_eq!(clipboard_html().as_deref(), Some("<i>from the system</i>"));
        platform.join().expect("the platform thread finishes");

        // A clipboard with nothing on it reads as empty, not as the last contents.
        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
        }
        let platform = answer_when_asked(r#"{"text":"","html":"","formats":[]}"#);
        assert_eq!(clipboard_text(), None);
        platform.join().expect("the platform thread finishes");
    }

    #[test]
    fn the_picker_answers_a_path_or_a_cancellation() {
        let _guard = BRIDGE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        drain();

        // Open: the request says what for, and the picked sandbox path comes back.
        let receiver = file_dialog(FileDialogRequest::Open { purpose: "document".into() });
        let request = answer_next(r#"{"path":"/data/storage/el2/base/cache/in.docx"}"#);
        assert_eq!(request["kind"], "pickOpen");
        assert_eq!(request["purpose"], "document");
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).expect("an answer"), Ok(vec!["/data/storage/el2/base/cache/in.docx".to_string()]));

        // Add Font… picks several.
        let receiver = file_dialog(FileDialogRequest::OpenMany { purpose: "font".into() });
        let request = answer_next(r#"{"paths":["/fonts/a.ttf","/fonts/b.otf"]}"#);
        assert_eq!(request["many"], true);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).expect("an answer"),
            Ok(vec!["/fonts/a.ttf".to_string(), "/fonts/b.otf".to_string()])
        );

        // Save suggests a name; cancelling is an empty answer, not an error.
        let receiver = file_dialog(FileDialogRequest::Save { name: "Document1.docx".into(), dir: None });
        let request = answer_next(r#"{"cancelled":true}"#);
        assert_eq!(request["kind"], "pickSave");
        assert_eq!(request["name"], "Document1.docx");
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).expect("an answer"), Ok(Vec::new()));

        // A picker that fails (no document browser on the device) is an error, so the app falls
        // back to its own file browser.
        let receiver = file_dialog(FileDialogRequest::Open { purpose: "picture".into() });
        let _ = answer_next(r#"{"error":"no picker"}"#);
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).expect("an answer"), Err("no picker".to_string()));
    }

    #[test]
    fn a_copied_picture_or_file_is_pasted_from_its_path() {
        let _guard = BRIDGE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        drain();

        // ArkTS packs the clipboard's PixelMap into the cache folder; the bytes go to the UI.
        let dir = std::env::temp_dir().join(format!("wordcraft-ohos-services-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let png = dir.join("clipboard.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nfake").expect("a temporary picture");
        let path = png.to_string_lossy().to_string();

        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
            cache.picture = None;
        }
        let platform = answer_when_asked(&format!(r#"{{"formats":["pixelMap"],"picture":{}}}"#, serde_json::Value::from(path)));
        match clipboard_picture() {
            Some(ClipboardPicture::Encoded(bytes)) => assert_eq!(bytes, b"\x89PNG\r\n\x1a\nfake"),
            other => panic!("expected the packed picture, got {}", other.is_some()),
        }
        platform.join().expect("the platform thread finishes");
        assert!(!png.exists(), "the packed picture is removed once it is read");

        // A file manager's Copy arrives as URIs, and pastes as pictures among those files.
        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
            cache.picture = None;
        }
        let platform = answer_when_asked(r#"{"formats":["text/uri"],"uris":["/data/storage/el2/base/cache/shot.png"]}"#);
        match clipboard_picture() {
            Some(ClipboardPicture::Files(files)) => assert_eq!(files, vec![PathBuf::from("/data/storage/el2/base/cache/shot.png")]),
            other => panic!("expected the copied file, got {}", other.is_some()),
        }
        platform.join().expect("the platform thread finishes");

        // Hostile sizes: a picture bigger than the cap is refused before it is read, and removed.
        let big = dir.join("big.png");
        std::fs::write(&big, vec![0u8; 32]).expect("a temporary picture");
        assert_eq!(take_picture(&big.to_string_lossy(), 8), None);
        assert!(!big.exists(), "an oversized picture is not left in the cache folder");
        assert_eq!(take_picture(&dir.join("missing.png").to_string_lossy(), 8), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copied_pictures_read_as_their_paths() {
        let _guard = BRIDGE_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        drain();
        // A file manager's Copy arrives as text/uri records and no text: a paste must still insert
        // the picture, which the UI does when the clipboard text names a picture file.
        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
            cache.text = None;
            cache.files.clear();
        }
        let platform = answer_when_asked(r#"{"formats":["text/uri"],"uris":["/data/storage/el2/base/cache/shot.png"]}"#);
        assert_eq!(clipboard_text().as_deref(), Some("/data/storage/el2/base/cache/shot.png"));
        platform.join().expect("the platform thread finishes");
        // A copied document is not a picture: pasting it as text would be wrong, so nothing is pasted.
        if let Ok(mut cache) = clipboard().lock() {
            cache.asked = None;
            cache.text = None;
            cache.files.clear();
        }
        let platform = answer_when_asked(r#"{"formats":["text/uri"],"uris":["/data/storage/el2/base/cache/report.docx"]}"#);
        assert_eq!(clipboard_text(), None);
        platform.join().expect("the platform thread finishes");
    }

    #[test]
    fn an_unknown_answer_reads_as_empty() {
        assert_eq!(Reply(None).string("text"), None);
        assert!(Reply(Some(r#"{"cancelled":true}"#.into())).is_cancelled());
    }
}
