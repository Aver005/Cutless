// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! MCP-сервер Cutless: реестр инструментов наружу по stdio.
//! Одна сессия на процесс, JSON-RPC 2.0, по одному сообщению в строке.

mod protocol;
mod server;

pub use server::{McpError, McpServer};
