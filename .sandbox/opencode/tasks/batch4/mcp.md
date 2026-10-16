# Задача: MCP-сервер Cutless (`cutless-mcp`) и команда `concat-cli mcp`

Чтобы любой агент (Claude Code, Gemini CLI, Cursor, pooprusteek) мог монтировать в Cutless, нужен MCP-сервер поверх
реестра `cutless_tools::default_registry()` (`src/crates/cutless-tools/src/lib.rs` — прочитай). Инструменты наполняют
другие исполнители параллельно; тебе важен только интерфейс `Registry` / `ToolSpec` / `ToolOutput` / `ToolError` / `ToolContext`.

Спецификация MCP — https://modelcontextprotocol.io/specification (бери актуальную ревизию; если клиент прислал
`protocolVersion`, которую ты знаешь, отвечай ею, иначе своей). Транспорт — **stdio**: JSON-RPC 2.0, одно сообщение в
строке; stdout — только протокол, логи — в stderr.

## Что сделать
1. Новый крейт `src/crates/cutless-mcp` (Cargo.toml по образцу соседних: `version.workspace = true`, `edition.workspace = true`,
   `rust-version.workspace = true`, `license.workspace = true`, `[lints] workspace = true`; зависимости:
   `cutless-tools = { path = "../cutless-tools" }`, `concat-api.workspace = true`, `serde = { version = "1", features = ["derive"] }`,
   `serde_json.workspace = true`, `thiserror.workspace = true`, `log = "0.4"`). Без tokio — синхронный цикл.
2. Методы: `initialize` (serverInfo `{name: "cutless", version}`, `capabilities: {tools: {}}`, `instructions` — 3–5 строк
   по-английски: сначала `project_open`/`project_create`, затем `timeline_summary`, правки откатываются `undo`),
   `notifications/initialized`, `ping`, `tools/list` (name, description, `inputSchema`), `tools/call` →
   `content: [{type: "text", text}, …{type: "image", data, mimeType}]`; ошибка инструмента → результат с `isError: true`
   и текстом (не JSON-RPC error). Неизвестный метод → JSON-RPC error -32601, неверный JSON → -32700.
3. Одна сессия на процесс: `concat_api::Api::new(...)` как `Editor` (события API — в лог) и `ToolContext`, у которого
   `project` живёт между вызовами.
4. `src/crates/concat-cli/src/main.rs`: подкоманда `mcp` (без аргументов) — сервер на stdin/stdout; зависимость в
   `src/crates/concat-cli/Cargo.toml`.
5. Тесты: обмен строками без процесса — initialize → tools/list → tools/call неизвестного → ping; тестовый `Registry`
   с одним игрушечным инструментом.
6. В отчёте — как подключить: Claude Code (`claude mcp add cutless -- <путь>\concat-cli.exe mcp`) и pooprusteek (`/mcp add`).

## Разрешённые файлы
`src/crates/cutless-mcp/**` (новый), `src/crates/concat-cli/src/main.rs`, `src/crates/concat-cli/Cargo.toml`.

## Проверка
Из `src/`: `cargo check -p cutless-mcp --tests`, `cargo check -p concat-cli`.
