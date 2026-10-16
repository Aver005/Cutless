// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты агента: каждое действие редактора — инструмент с именем, описанием и схемой входа.
//! Один реестр обслуживает и MCP, и встроенный чат; правки идут через [`Editor`].

use serde::de::DeserializeOwned;
use serde_json::Value;

pub mod edit;
pub mod pauses;
pub mod speech;

/// Куда уходят запросы редактору: сейчас API без окна, на этапе моста — открытое окно.
pub trait Editor {
    /// Выполняет один запрос API: ответ как JSON или ошибка API.
    fn dispatch(&mut self, request: concat_api::Request) -> Result<Value, concat_api::ApiError>;
}

impl Editor for concat_api::Api {
    fn dispatch(&mut self, request: concat_api::Request) -> Result<Value, concat_api::ApiError> {
        answer(concat_api::Api::dispatch(self, request))
    }
}

/// Ответ API в виде JSON — так его отдают и своя сессия, и сокет окна.
pub fn answer(response: concat_api::Response) -> Result<Value, concat_api::ApiError> {
    match response {
        concat_api::Response::Result(reply) => serde_json::to_value(reply)
            .map_err(|error| concat_api::ApiError::failed(error.to_string())),
        concat_api::Response::Error(error) => Err(error),
    }
}

/// Насколько инструмент опасен: от этого зависит, спрашивать ли человека.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolClass {
    /// Только читает: сводки, транскрипт, кадры.
    Read,
    /// Правит проект; откатывается Ctrl+Z.
    Edit,
    /// Необратимо или вне проекта: удаление медиа, запись файла поверх.
    Danger,
}

/// Описание инструмента для модели: имя, что делает, JSON-схема входа.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    /// Имя для вызова, `snake_case`.
    pub name: &'static str,
    /// Что делает и когда звать — для модели, по-английски.
    pub description: &'static str,
    /// JSON-схема входа.
    pub input_schema: Value,
    /// Класс опасности.
    pub class: ToolClass,
}

/// Картинка в ответе инструмента (кадр, контакт-лист).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolImage {
    /// MIME-тип, например `image/png`.
    pub mime: String,
    /// Содержимое в base64.
    pub base64: String,
}

/// Ответ инструмента: короткий текст для модели, данные и картинки.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolOutput {
    /// Короткий текст для модели.
    pub text: String,
    /// Структурные данные (для интерфейса и тестов).
    pub data: Value,
    /// Картинки, если инструмент их отдаёт.
    pub images: Vec<ToolImage>,
}

impl ToolOutput {
    /// Ответ из одной строки текста.
    pub fn text(text: impl Into<String>) -> ToolOutput {
        ToolOutput {
            text: text.into(),
            ..ToolOutput::default()
        }
    }
}

/// Почему инструмент не выполнился; текст показывается модели как есть.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Инструмента с таким именем нет.
    #[error("неизвестный инструмент: {0}")]
    Unknown(String),
    /// Вход не подходит под схему.
    #[error("неверный вход: {0}")]
    BadInput(String),
    /// Инструменту нужен проект, а он не открыт.
    #[error("нет открытого проекта: сначала project_open или project_create")]
    NoProject,
    /// Редактор отказал; текст его ошибки.
    #[error("{0}")]
    Editor(String),
    /// Другая ошибка выполнения.
    #[error("{0}")]
    Failed(String),
}

/// Состояние сессии агента: редактор и проект, с которым он работает.
pub struct ToolContext<'a> {
    /// Редактор, которому уходят запросы.
    pub editor: &'a mut dyn Editor,
    /// Папка открытого проекта; инструменты, которым нужен проект, берут её отсюда.
    pub project: Option<String>,
}

impl ToolContext<'_> {
    /// Папка открытого проекта или ошибка «нет проекта».
    pub fn project(&self) -> Result<String, ToolError> {
        self.project.clone().ok_or(ToolError::NoProject)
    }

    /// Запрос редактору; ответ — JSON ответа или ошибка редактора текстом.
    pub fn call(&mut self, request: concat_api::Request) -> Result<Value, ToolError> {
        self.editor
            .dispatch(request)
            .map_err(|error| ToolError::Editor(error.to_string()))
    }
}

/// Разбирает вход инструмента в его тип; ошибка понятна модели.
pub fn input<T: DeserializeOwned>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(|error| ToolError::BadInput(error.to_string()))
}

/// Один инструмент.
pub trait Tool: Send + Sync {
    /// Описание инструмента.
    fn spec(&self) -> ToolSpec;
    /// Выполняет инструмент.
    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError>;
}

/// Все инструменты, по имени.
#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    /// Добавляет инструмент; имя должно быть новым.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.spec().name;
        debug_assert!(self.find(name).is_none(), "инструмент {name} уже есть");
        self.tools.push(tool);
    }

    /// Описания всех инструментов в порядке регистрации.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|tool| tool.spec()).collect()
    }

    /// Вызывает инструмент по имени.
    pub fn call(
        &self,
        ctx: &mut ToolContext<'_>,
        name: &str,
        input: Value,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .find(name)
            .ok_or_else(|| ToolError::Unknown(name.to_owned()))?;
        tool.call(ctx, input)
    }

    fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .map(|tool| tool.as_ref())
            .find(|tool| tool.spec().name == name)
    }
}

/// Реестр со всеми инструментами Cutless.
pub fn default_registry() -> Registry {
    let mut registry = Registry::default();
    edit::register(&mut registry);
    speech::register(&mut registry);
    pauses::register(&mut registry);
    registry
}
