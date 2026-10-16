// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Форма сообщений MCP: одна строка JSON-RPC 2.0 на вход и на выход.

use serde_json::{Value, json};

/// Версия, которой сервер отвечает, когда клиент не назвал знакомой.
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";

/// Версии протокола, которые сервер понимает; клиент получает свою.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Имя сервера в `serverInfo`.
pub const SERVER_NAME: &str = "cutless";

/// Подсказка модели: с чего начать, чем смотреть и что правки откатываются.
pub const INSTRUCTIONS: &str = "Open a project before editing: project_open takes a folder that already holds one, or project_create takes a location and a name. Call timeline_summary to see tracks, clips and ids before you change anything. Every edit is one undo step, and undo rolls the last change back. Times are seconds everywhere.";

/// Одно входящее сообщение: id, метод и параметры.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// id запроса; у уведомления его нет.
    pub id: Option<Value>,
    /// Имя метода MCP.
    pub method: String,
    /// Параметры метода; у методов без них — null.
    pub params: Value,
}

/// Отказ разобрать сообщение; `code` — номер JSON-RPC.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    /// Номер ошибки JSON-RPC.
    pub code: i64,
    /// Текст для клиента.
    pub message: String,
}

impl RpcError {
    /// Строка не разобралась как JSON, -32700.
    pub fn parse(message: impl Into<String>) -> RpcError {
        RpcError {
            code: -32700,
            message: message.into(),
        }
    }

    /// JSON разобрался, но это не запрос, -32600.
    pub fn invalid_request(message: impl Into<String>) -> RpcError {
        RpcError {
            code: -32600,
            message: message.into(),
        }
    }

    /// Метода с таким именем нет, -32601.
    pub fn method_not_found(message: impl Into<String>) -> RpcError {
        RpcError {
            code: -32601,
            message: message.into(),
        }
    }

    /// Параметры метода не подходят, -32602.
    pub fn invalid_params(message: impl Into<String>) -> RpcError {
        RpcError {
            code: -32602,
            message: message.into(),
        }
    }
}

impl Call {
    /// Разбирает одну строку. Не JSON — ошибка -32700; JSON не похож на
    /// запрос — -32600.
    pub fn parse(line: &str) -> Result<Call, RpcError> {
        let value: Value =
            serde_json::from_str(line).map_err(|error| RpcError::parse(error.to_string()))?;
        let Value::Object(mut object) = value else {
            return Err(RpcError::invalid_request("сообщение — объект JSON"));
        };
        let id = object.remove("id");
        let method = object
            .remove("method")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| RpcError::invalid_request("сообщение называет метод"))?;
        let params = object.remove("params").unwrap_or(Value::Null);
        Ok(Call { id, method, params })
    }
}

/// Строка ответа с результатом.
pub fn result(id: &Value, value: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string()
}

/// Строка ответа с ошибкой.
pub fn error(id: &Value, error: &RpcError) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": error.code, "message": error.message },
    })
    .to_string()
}
