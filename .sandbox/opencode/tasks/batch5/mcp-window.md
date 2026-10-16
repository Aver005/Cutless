# Задача: `concat-cli mcp` подключается к открытому окну Cutless

Сейчас `cutless_mcp::McpServer` держит собственный `concat_api::Api` (`src/crates/cutless-mcp/src/server.rs`), и агент
правит проект мимо окна. Нужно: если окно разрешило агентам править открытый проект, MCP работает через сокет окна.

## Что сделать
1. `src/crates/cutless-mcp/src/remote.rs` (новый): `RemoteEditor` — реализует `cutless_tools::Editor` поверх
   `std::net::TcpStream`. `connect(address, token) -> Result<RemoteEditor, McpError>`: подключение с таймаутом 2 с,
   строка `auth`, проверка ответа. `dispatch`: сериализует `Request` (`serde_json::to_value`, поле `method` вынимается,
   остальное — `params`), шлёт `{"jsonrpc":"2.0","id":N,"method":…,"params":{…}}` строкой, читает строки до ответа с тем
   же `id` (строки-события без `id` пропускает), `result` → `Ok(Value)`, `error` → `Err(ApiError)` (код из
   `error.data.code`, если он разбирается в `concat_api::ErrorCode`, иначе `Failed`; текст — `error.message`).
   Обрыв связи → `ApiError` с понятным текстом «окно Cutless закрылось» по-английски.
   Если ответ `busy` — до 20 повторов с паузой 250 мс (человек тянет клип мышью), затем ошибка.
2. `src/crates/cutless-mcp/src/window.rs` (новый): `window_endpoint(config_dir) -> Option<(SocketAddr, String)>` —
   читает `settings.json`, возвращает адрес и токен, только если `server.enabled && server.agents`. Свой маленький
   `Deserialize`-тип с `#[serde(default)]`, без зависимости от крейта окна.
3. `McpServer` хранит `Box<dyn cutless_tools::Editor + Send>` вместо `Api`. Конструкторы: `with_default_registry()` —
   сначала окно (если `window_endpoint` дал адрес и `RemoteEditor::connect` удался), иначе своё `Api` как сейчас; в лог
   (stderr) — что выбрано. `with_api` оставить для тестов, добавить `with_editor(registry, Box<dyn Editor + Send>)`.
   В `initialize.instructions` добавить строку: при подключении к окну сначала вызвать `project_on_screen`.
4. `src/crates/cutless-tools/src/edit/project.rs`: новый инструмент `project_on_screen` (Read): вызывает
   `Request::ProjectOnScreen`, если проект один — делает его текущим (`ctx.project = Some(path)`) и отвечает
   «current project: <name> at <path> (open in the Cutless window)», несколько — список путей, ни одного — «no project is
   open in a Cutless window; use project_open or project_create». Зарегистрировать; поправить тест со списком имён
   в `src/crates/cutless-tools/src/edit.rs`, если он требует точного списка.
5. `src/crates/concat-cli/src/main.rs`: флаг `concat-cli mcp --local` — не искать окно (всегда своё `Api`).
6. Тесты: `RemoteEditor` против `std::net::TcpListener` на `127.0.0.1:0` в потоке теста (auth → ответ, вызов → result,
   событие перед ответом пропускается, error → ApiError с кодом, busy → повтор); `window_endpoint` на временной папке
   (нет файла / выключено / agents=false / всё есть); `project_on_screen` с тестовым `Editor` (0, 1, 2 проекта).

## Разрешённые файлы
`src/crates/cutless-mcp/**`, `src/crates/cutless-tools/src/edit/project.rs`, `src/crates/cutless-tools/src/edit.rs`,
`src/crates/concat-cli/src/main.rs`. Зависимости: в `cutless-mcp` можно `concat-host.workspace = true` (для `AppDirs`),
если его нет. Других зависимостей не добавлять.

## Проверка
Из `src/`: `cargo check -p cutless-mcp --tests`, `cargo check -p cutless-tools --tests`, `cargo check -p concat-cli`.
