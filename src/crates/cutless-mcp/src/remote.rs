// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Редактор поверх сокета окна Cutless: запросы уходят открытому окну,
//! оно правит проект на экране. Формат строки — JSON-RPC 2.0 с `auth`.

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use concat_api::{ApiError, ErrorCode, Event, Request};
use cutless_tools::Jobs;
use serde_json::{Value, json};

use crate::McpError;

/// Сколько ждём подключения и ответа на `auth`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Сколько раз повторяем запрос, пока окно отвечает `busy` (человек тянет клип мышью).
const BUSY_RETRIES: usize = 20;
/// Пауза между повторами при `busy`.
const BUSY_PAUSE: Duration = Duration::from_millis(250);
/// Сколько ждём ответа на запрос: окно само ждёт свой поток до 30 с.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// Редактор, который правит проект через открытое окно Cutless.
pub struct RemoteEditor {
    writer: TcpStream,
    /// Ответы окна; события поток чтения сам кладёт на доску задач.
    replies: Receiver<Value>,
    /// Читает сокет всё время: иначе очередь окна переполнится событиями экспорта.
    reader: Option<JoinHandle<()>>,
    /// id следующего запроса; ответ ищется по нему.
    next_id: u64,
}

impl RemoteEditor {
    /// Подключается к окну по адресу, представляя токен. События задач окна
    /// идут на доску `jobs`. Ошибка — если окно недоступно или не приняло токен.
    pub fn connect(address: SocketAddr, token: &str, jobs: Jobs) -> Result<RemoteEditor, McpError> {
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
            .map_err(|error| McpError::Connect(error.to_string()))?;
        let _ = stream.set_nodelay(true);
        let reading = stream
            .try_clone()
            .map_err(|error| McpError::Connect(error.to_string()))?;
        let (replied, replies) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name("cutless window reader".to_owned())
            .spawn(move || read_lines(reading, &replied, &jobs))
            .map_err(|error| McpError::Connect(error.to_string()))?;
        let mut editor = RemoteEditor {
            writer: stream,
            replies,
            reader: Some(reader),
            next_id: 1,
        };
        let id = editor.mint();
        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "auth",
            "params": { "token": token },
        })
        .to_string();
        // На рукопожатие — короткий срок, чтобы молчащее окно не держало нас.
        editor
            .exchange(&line, id, CONNECT_TIMEOUT)
            .map_err(|error| McpError::Connect(error.to_string()))?;
        Ok(editor)
    }

    /// Следующий id запроса.
    fn mint(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Шлёт запрос и возвращает `result`; при `busy` повторяет.
    fn dispatch_inner(&mut self, request: Request) -> Result<Value, ApiError> {
        let (method, params) = split(request)?;
        let id = self.mint();
        let line =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let mut busy = None;
        for _ in 0..=BUSY_RETRIES {
            match self.exchange(&line, id, REPLY_TIMEOUT) {
                Ok(value) => return Ok(value),
                Err(error) if error.code == ErrorCode::Busy => {
                    busy = Some(error);
                    std::thread::sleep(BUSY_PAUSE);
                }
                Err(error) => return Err(error),
            }
        }
        Err(busy
            .unwrap_or_else(|| ApiError::new(ErrorCode::Busy, "the Cutless window stayed busy")))
    }

    /// Шлёт одну строку и ждёт ответ с этим id не дольше `timeout`.
    fn exchange(&mut self, line: &str, id: u64, timeout: Duration) -> Result<Value, ApiError> {
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(|_error| closed())?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.replies.recv_timeout(left) {
                Ok(value) if value.get("id").and_then(Value::as_u64) == Some(id) => {
                    return outcome(value);
                }
                // Запоздалый ответ на запрос, которого мы уже не ждём.
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(ApiError::failed(
                        "the Cutless window did not answer in time",
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => return Err(closed()),
            }
        }
    }
}

impl Drop for RemoteEditor {
    fn drop(&mut self) {
        // Закрытый сокет будит поток чтения, и он выходит.
        let _ = self.writer.shutdown(Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl cutless_tools::Editor for RemoteEditor {
    fn dispatch(&mut self, request: Request) -> Result<Value, ApiError> {
        self.dispatch_inner(request)
    }
}

/// Поток чтения: строки с id — ответы, без id — события задач окна.
fn read_lines(stream: TcpStream, replies: &Sender<Value>, jobs: &Jobs) {
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("id").is_some() {
            if replies.send(value).is_err() {
                break;
            }
        } else if let Some(event) = event_from(&value) {
            jobs.record(&event);
        }
    }
}

/// Событие из уведомления окна: метод — его имя, `params` — поля.
fn event_from(value: &Value) -> Option<Event> {
    let method = value.get("method")?.as_str()?;
    let mut fields = value.get("params")?.as_object()?.clone();
    fields.insert("event".to_owned(), Value::String(method.to_owned()));
    serde_json::from_value(Value::Object(fields)).ok()
}

/// Метод запроса и его поля как `params`.
fn split(request: Request) -> Result<(String, Value), ApiError> {
    let value =
        serde_json::to_value(request).map_err(|error| ApiError::failed(error.to_string()))?;
    let Value::Object(mut fields) = value else {
        return Err(ApiError::failed("the request is not a JSON object"));
    };
    let method = fields
        .remove("method")
        .and_then(|method| method.as_str().map(str::to_owned))
        .ok_or_else(|| ApiError::failed("the request names no method"))?;
    Ok((method, Value::Object(fields)))
}

/// Ответ окна: `result` или `error` с кодом.
fn outcome(value: Value) -> Result<Value, ApiError> {
    if let Some(result) = value.get("result") {
        return Ok(result.clone());
    }
    let error = value.get("error");
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the Cutless window refused the request")
        .to_owned();
    let code = error
        .and_then(|error| error.get("data"))
        .and_then(|data| data.get("code"))
        .and_then(|code| serde_json::from_value::<ErrorCode>(code.clone()).ok())
        .unwrap_or(ErrorCode::Failed);
    Err(ApiError::new(code, message))
}

/// Окно закрылось: соединение оборвалось.
fn closed() -> ApiError {
    ApiError::failed("the Cutless window closed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutless_tools::Editor;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Сервер на localhost: на каждую строку отдаёт строки из `script`;
    /// пустой список закрывает соединение.
    fn spawn(
        script: impl Fn(&Value) -> Vec<Value> + Send + 'static,
    ) -> (SocketAddr, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            let mut line = String::new();
            while reader.read_line(&mut line).expect("read") > 0 {
                let request: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
                let replies = script(&request);
                if replies.is_empty() {
                    break;
                }
                for reply in replies {
                    writeln!(writer, "{reply}").expect("write");
                }
                writer.flush().expect("flush");
                line.clear();
            }
        });
        (address, handle)
    }

    /// Ответ с тем же id, что у запроса.
    fn reply(request: &Value, result: Value) -> Vec<Value> {
        vec![json!({ "jsonrpc": "2.0", "id": request["id"].clone(), "result": result })]
    }

    /// Ответ об ошибке с тем же id.
    fn refuse(request: &Value, code: i64, name: &str, message: &str) -> Vec<Value> {
        vec![json!({
            "jsonrpc": "2.0",
            "id": request["id"].clone(),
            "error": { "code": code, "message": message, "data": { "code": name } },
        })]
    }

    #[test]
    fn connect_authenticates_and_a_call_returns_its_result() {
        let (address, handle) = spawn(|request| {
            if request["method"] == "auth" {
                reply(request, json!({}))
            } else {
                reply(request, json!({ "path": "/p" }))
            }
        });
        let mut editor =
            RemoteEditor::connect(address, "secret", Jobs::default()).expect("connect");
        let value = editor
            .dispatch(Request::ProjectGet { path: "/p".into() })
            .expect("call");
        assert_eq!(value["path"], "/p");
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn a_refused_auth_is_a_connect_error() {
        let (address, handle) =
            spawn(|request| refuse(request, -32006, "unauthorized", "wrong token"));
        assert!(RemoteEditor::connect(address, "nope", Jobs::default()).is_err());
        handle.join().expect("server");
    }

    #[test]
    fn an_event_before_the_reply_is_skipped() {
        let (address, handle) = spawn(|request| {
            if request["method"] == "auth" {
                return reply(request, json!({}));
            }
            let mut lines = vec![
                json!({ "jsonrpc": "2.0", "method": "export.progress", "params": { "frame": 1 } }),
            ];
            lines.push(json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": { "canUndo": true },
            }));
            lines
        });
        let mut editor = RemoteEditor::connect(address, "t", Jobs::default()).expect("connect");
        let value = editor
            .dispatch(Request::EditUndo { path: "/p".into() })
            .expect("call");
        assert_eq!(value["canUndo"], true);
        // Сервер читает, пока клиент не отключится.
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn an_error_reply_becomes_an_api_error_with_its_code() {
        let (address, handle) = spawn(|request| {
            if request["method"] == "auth" {
                return reply(request, json!({}));
            }
            refuse(request, -32001, "notOpen", "/p is not open")
        });
        let mut editor = RemoteEditor::connect(address, "t", Jobs::default()).expect("connect");
        let error = editor
            .dispatch(Request::EditUndo { path: "/p".into() })
            .expect_err("refused");
        assert_eq!(error.code, ErrorCode::NotOpen);
        assert_eq!(error.message, "/p is not open");
        // Сервер читает, пока клиент не отключится.
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn a_busy_reply_is_retried_until_it_works() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let (address, handle) = spawn(move |request| {
            if request["method"] == "auth" {
                return reply(request, json!({}));
            }
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                refuse(request, -32004, "busy", "the window is busy")
            } else {
                reply(request, json!({ "canUndo": false }))
            }
        });
        let mut editor = RemoteEditor::connect(address, "t", Jobs::default()).expect("connect");
        let value = editor
            .dispatch(Request::EditUndo { path: "/p".into() })
            .expect("retried");
        assert_eq!(value["canUndo"], false);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // Сервер читает, пока клиент не отключится.
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn a_closed_connection_says_the_window_closed() {
        let (address, handle) = spawn(|request| {
            if request["method"] == "auth" {
                reply(request, json!({}))
            } else {
                Vec::new()
            }
        });
        let mut editor = RemoteEditor::connect(address, "t", Jobs::default()).expect("connect");
        let error = editor
            .dispatch(Request::EditUndo { path: "/p".into() })
            .expect_err("closed");
        assert!(error.message.contains("window closed"), "{}", error.message);
        // Сервер читает, пока клиент не отключится.
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn job_events_reach_the_board_even_between_requests() {
        let (address, handle) = spawn(|request| {
            if request["method"] == "auth" {
                return reply(request, json!({}));
            }
            // Ответ, а за ним события экспорта, которых никто не ждёт.
            let mut lines = reply(request, json!({ "job": "j1" }));
            for frame in 1..=300 {
                lines.push(json!({
                    "jsonrpc": "2.0",
                    "method": "export.progress",
                    "params": { "job": "j1", "path": "/p", "frame": frame, "total": 300, "stage": "video" },
                }));
            }
            lines.push(json!({
                "jsonrpc": "2.0",
                "method": "export.done",
                "params": { "job": "j1", "path": "/p", "output": "/out.mp4", "width": 1080, "height": 1920 },
            }));
            lines
        });
        let jobs = Jobs::default();
        let mut editor = RemoteEditor::connect(address, "t", jobs.clone()).expect("connect");
        editor
            .dispatch(Request::Version)
            .expect("the reply comes first");
        // События идут уже после ответа: ждём, пока поток чтения их разберёт.
        let deadline = Instant::now() + Duration::from_secs(5);
        let job = loop {
            match jobs.get("j1") {
                Some(job) if !job.is_running() => break job,
                _ if Instant::now() > deadline => panic!("no export.done on the board"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        assert!(job.line().contains("wrote /out.mp4"), "{}", job.line());
        drop(editor);
        handle.join().expect("server");
    }

    #[test]
    fn the_request_method_and_fields_split_into_a_params_object() {
        let (method, params) = split(Request::ProjectGet { path: "/p".into() }).expect("split");
        assert_eq!(method, "project.get");
        assert_eq!(params, json!({ "path": "/p" }));
        let (method, params) = split(Request::Version).expect("split");
        assert_eq!(method, "version");
        assert_eq!(params, json!({}));
    }
}
