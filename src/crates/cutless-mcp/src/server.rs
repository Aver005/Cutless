// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сервер MCP поверх реестра инструментов.
//! Одна сессия на процесс: редактор — окно Cutless или своя сессия API.

use std::io::{BufRead, Write};
use std::sync::Arc;

use cutless_tools::{Editor, Registry, ToolContext, ToolError, ToolOutput};
use serde_json::{Value, json};

use crate::protocol::{self, Call, RpcError};
use crate::{remote::RemoteEditor, window::window_endpoint};

/// Почему сервер не запустился или не смог писать.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// Папки приложения не нашлись: некуда положить настройки и модели.
    #[error("не удалось определить папки приложения: {0}")]
    Dirs(String),
    /// К окну Cutless не подключиться.
    #[error("окно Cutless недоступно: {0}")]
    Connect(String),
    /// Строку не удалось прочитать или записать.
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),
}

/// Сессия MCP: реестр инструментов, редактор и открытый проект.
pub struct McpServer {
    registry: Registry,
    /// Кто выполняет запросы: окно Cutless или своя сессия API.
    editor: Box<dyn Editor + Send>,
    /// Папка открытого проекта; её ставит `project_open`/`project_create`.
    project: Option<String>,
    /// Версия протокола, согласованная в `initialize`.
    protocol_version: String,
}

impl McpServer {
    /// Сервер со всеми инструментами Cutless. Сначала пробует открытое окно
    /// (если оно разрешило агентам правки), иначе работает со своим API.
    pub fn with_default_registry() -> Result<McpServer, McpError> {
        McpServer::select_editor(false)
    }

    /// Сервер только со своим API, без поиска окна: для `mcp --local`.
    pub fn with_default_registry_local() -> Result<McpServer, McpError> {
        McpServer::select_editor(true)
    }

    /// Выбирает редактора: окно Cutless, если оно есть и `local` не запрещает,
    /// иначе свою сессию API. Что выбрано — пишем в лог.
    fn select_editor(local: bool) -> Result<McpServer, McpError> {
        if !local
            && let Ok(dirs) = concat_api::AppDirs::locate()
            && let Some((address, token)) = window_endpoint(&dirs.config)
        {
            match RemoteEditor::connect(address, &token) {
                Ok(editor) => {
                    log::info!(target: "cutless_mcp", "работаем через окно Cutless на {address}");
                    return Ok(McpServer::with_editor(
                        cutless_tools::default_registry(),
                        Box::new(editor),
                    ));
                }
                Err(error) => {
                    log::info!(
                        target: "cutless_mcp",
                        "окно Cutless не подошло ({error}), работаем без него"
                    );
                }
            }
        }
        let events: concat_api::EventSink = Arc::new(|event| {
            log::info!(target: "cutless_mcp", "событие API: {event:?}");
        });
        let api = concat_api::Api::new(events).map_err(McpError::Dirs)?;
        log::info!(target: "cutless_mcp", "работаем со своим API, без окна");
        Ok(McpServer::with_editor(
            cutless_tools::default_registry(),
            Box::new(api),
        ))
    }

    /// Сервер над готовым редактором: окно, своя сессия или тест.
    pub fn with_editor(registry: Registry, editor: Box<dyn Editor + Send>) -> McpServer {
        McpServer {
            registry,
            editor,
            project: None,
            protocol_version: protocol::DEFAULT_PROTOCOL_VERSION.to_owned(),
        }
    }

    /// Сервер над готовым API: для тестов и для встраивания.
    pub fn with_api(registry: Registry, api: concat_api::Api) -> McpServer {
        McpServer::with_editor(registry, Box::new(api))
    }

    /// Обрабатывает одну строку и возвращает строку ответа.
    /// Уведомление ответа не даёт, поэтому `None`.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let call = match Call::parse(line) {
            Ok(call) => call,
            Err(error) => return Some(protocol::error(&Value::Null, &error)),
        };
        let Some(id) = call.id.clone() else {
            // Уведомление: выполняем, но не отвечаем.
            let _ = self.dispatch(&call);
            return None;
        };
        Some(match self.dispatch(&call) {
            Ok(value) => protocol::result(&id, value),
            Err(error) => protocol::error(&id, &error),
        })
    }

    /// Читает строки, пока вход не кончится, и отвечает на каждой.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> Result<(), McpError> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle(&line) {
                writeln!(output, "{reply}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// Один метод MCP. Уведомления тоже здесь, их ответ отбрасывается.
    fn dispatch(&mut self, call: &Call) -> Result<Value, RpcError> {
        match call.method.as_str() {
            "initialize" => self.initialize(&call.params),
            "notifications/initialized" => Ok(Value::Null),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(self.tools_list()),
            "tools/call" => self.tools_call(&call.params),
            other => Err(RpcError::method_not_found(format!(
                "неизвестный метод: {other}"
            ))),
        }
    }

    /// Рукопожатие: согласует версию протокола и отдаёт возможности сервера.
    fn initialize(&mut self, params: &Value) -> Result<Value, RpcError> {
        self.protocol_version = negotiated_version(params);
        Ok(json!({
            "protocolVersion": self.protocol_version,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": protocol::SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": protocol::INSTRUCTIONS,
        }))
    }

    /// Список инструментов: имя, описание и схема входа.
    fn tools_list(&self) -> Value {
        let tools: Vec<Value> = self
            .registry
            .specs()
            .iter()
            .map(|spec| {
                json!({
                    "name": spec.name,
                    "description": spec.description,
                    "inputSchema": spec.input_schema,
                })
            })
            .collect();
        json!({ "tools": tools })
    }

    /// Вызов инструмента. Ошибка инструмента — результат с `isError`.
    fn tools_call(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("tools/call без имени инструмента"))?;
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(Value::Object(_)) => params["arguments"].clone(),
            Some(_) => return Err(RpcError::invalid_params("arguments — объект JSON")),
        };
        let mut ctx = ToolContext {
            editor: self.editor.as_mut(),
            project: self.project.clone(),
        };
        let outcome = self.registry.call(&mut ctx, name, arguments);
        // Инструмент мог открыть или закрыть проект: запоминаем.
        self.project = ctx.project.clone();
        Ok(match outcome {
            Ok(output) => tool_result(&output),
            Err(error) => tool_failure(&error),
        })
    }
}

/// Версия протокола: названная клиентом, если сервер её знает, иначе своя.
fn negotiated_version(params: &Value) -> String {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    match requested {
        Some(version) if protocol::SUPPORTED_PROTOCOL_VERSIONS.contains(&version) => {
            version.to_owned()
        }
        _ => protocol::DEFAULT_PROTOCOL_VERSION.to_owned(),
    }
}

/// Ответ инструмента как содержимое MCP: текст, затем картинки.
fn tool_result(output: &ToolOutput) -> Value {
    let mut content = vec![json!({ "type": "text", "text": output.text })];
    for image in &output.images {
        content.push(json!({
            "type": "image",
            "data": image.base64,
            "mimeType": image.mime,
        }));
    }
    json!({ "content": content, "isError": false })
}

/// Ошибка инструмента как результат с `isError`: запрос цел, инструмент сказал «нет».
fn tool_failure(error: &ToolError) -> Value {
    json!({
        "content": [{ "type": "text", "text": error.to_string() }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cutless_tools::{Tool, ToolClass, ToolSpec};

    /// Игрушечный инструмент: возвращает текст входа.
    struct Echo;

    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "echo",
                description: "Echoes the text back.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "text": { "type": "string" } },
                    "required": ["text"],
                }),
                class: ToolClass::Read,
            }
        }

        fn call(&self, _ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
            let text = input
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(ToolOutput::text(text.to_owned()))
        }
    }

    /// Сервер с одним инструментом над API на временной папке.
    fn server() -> (McpServer, tempfile::TempDir) {
        let scratch = tempfile::tempdir().expect("временная папка");
        let dirs = concat_api::AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let api = concat_api::Api::with_dirs(dirs, Arc::new(|_event| {}));
        let mut registry = Registry::default();
        registry.register(Box::new(Echo));
        (McpServer::with_api(registry, api), scratch)
    }

    /// Строка ответа как JSON.
    fn value(line: &str) -> Value {
        serde_json::from_str(line).expect("ответ — JSON")
    }

    /// Шлёт запрос и разбирает ответ.
    fn ask(server: &mut McpServer, id: i64, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let reply = server.handle(&line.to_string()).expect("ответ на запрос");
        value(&reply)
    }

    #[test]
    fn a_request_parses_by_method_and_keeps_its_id() {
        let call =
            Call::parse(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"x"}}"#)
                .expect("запрос");
        assert_eq!(call.id, Some(json!(7)));
        assert_eq!(call.method, "tools/call");
        assert_eq!(call.params["name"], "x");
    }

    #[test]
    fn parsing_says_why_a_line_is_not_a_request() {
        assert_eq!(Call::parse("{ not json").expect_err("не JSON").code, -32700);
        assert_eq!(Call::parse("[1,2]").expect_err("не объект").code, -32600);
        assert_eq!(
            Call::parse(r#"{"jsonrpc":"2.0","id":1}"#)
                .expect_err("без метода")
                .code,
            -32600
        );
        let note = Call::parse(r#"{"jsonrpc":"2.0","method":"ping"}"#).expect("уведомление");
        assert_eq!(note.id, None);
    }

    #[test]
    fn initialize_accepts_a_known_version_and_describes_the_server() {
        let (mut server, _scratch) = server();
        let answer = ask(
            &mut server,
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            }),
        );
        assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(answer["result"]["serverInfo"]["name"], "cutless");
        assert!(answer["result"]["capabilities"]["tools"].is_object());
        assert!(answer["result"]["instructions"].is_string());
    }

    #[test]
    fn an_unknown_version_gets_the_servers_own() {
        let (mut server, _scratch) = server();
        let answer = ask(
            &mut server,
            1,
            "initialize",
            json!({ "protocolVersion": "1900-01-01" }),
        );
        assert_eq!(
            answer["result"]["protocolVersion"],
            protocol::DEFAULT_PROTOCOL_VERSION
        );
    }

    #[test]
    fn tools_list_has_name_description_and_schema() {
        let (mut server, _scratch) = server();
        let answer = ask(&mut server, 2, "tools/list", json!({}));
        let tools = answer["result"]["tools"].as_array().expect("список");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
        assert!(tools[0]["description"].is_string());
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn calling_a_tool_returns_its_text() {
        let (mut server, _scratch) = server();
        let answer = ask(
            &mut server,
            3,
            "tools/call",
            json!({ "name": "echo", "arguments": { "text": "привет" } }),
        );
        assert_eq!(answer["result"]["isError"], false);
        assert_eq!(answer["result"]["content"][0]["type"], "text");
        assert_eq!(answer["result"]["content"][0]["text"], "привет");
    }

    #[test]
    fn an_unknown_tool_is_a_result_with_is_error_not_a_transport_error() {
        let (mut server, _scratch) = server();
        let answer = ask(&mut server, 4, "tools/call", json!({ "name": "nope" }));
        assert!(answer.get("error").is_none());
        assert_eq!(answer["result"]["isError"], true);
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .expect("текст");
        assert!(text.contains("nope"), "{text}");
    }

    #[test]
    fn ping_answers_with_an_empty_result() {
        let (mut server, _scratch) = server();
        let answer = ask(&mut server, 5, "ping", json!({}));
        assert_eq!(answer["result"], json!({}));
    }

    #[test]
    fn an_unknown_method_is_a_json_rpc_error() {
        let (mut server, _scratch) = server();
        let answer = ask(&mut server, 6, "does/not/exist", json!({}));
        assert_eq!(answer["error"]["code"], -32601);
    }

    #[test]
    fn a_bad_line_is_a_parse_error_with_a_null_id() {
        let (mut server, _scratch) = server();
        let reply = server.handle("{ not json").expect("ответ");
        let answer = value(&reply);
        assert_eq!(answer["error"]["code"], -32700);
        assert_eq!(answer["id"], Value::Null);
    }

    #[test]
    fn a_notification_gets_no_answer() {
        let (mut server, _scratch) = server();
        let line = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(server.handle(&line.to_string()).is_none());
    }

    #[test]
    fn a_full_exchange_over_lines_answers_in_order() {
        let (mut server, _scratch) = server();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
            "\n",
        );
        let mut output = Vec::new();
        server
            .serve(std::io::Cursor::new(input), &mut output)
            .expect("работает");
        let lines: Vec<Value> = String::from_utf8(output)
            .expect("utf-8")
            .lines()
            .map(value)
            .collect();
        assert_eq!(lines.len(), 4, "уведомление ответа не даёт");
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["id"], 2);
        assert_eq!(lines[2]["id"], 3);
        assert_eq!(lines[3]["id"], 4);
        assert_eq!(lines[0]["result"]["serverInfo"]["name"], "cutless");
        assert_eq!(lines[2]["result"]["isError"], true);
        assert_eq!(lines[3]["result"], json!({}));
    }
}
