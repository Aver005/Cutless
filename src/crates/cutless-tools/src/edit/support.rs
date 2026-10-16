// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Общее для инструментов «edit»: вызовы редактора, разбор состояния, формат чисел и времени.

use serde::Deserialize;
use serde_json::Value;

use crate::{Registry, Tool, ToolClass, ToolContext, ToolError, ToolOutput, ToolSpec};

/// Инструмент из готовой схемы и функции: описание и обработчик одним типом.
pub(crate) struct FnTool {
    spec: ToolSpec,
    run: fn(&mut ToolContext<'_>, Value) -> Result<ToolOutput, ToolError>,
}

impl Tool for FnTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn call(&self, ctx: &mut ToolContext<'_>, input: Value) -> Result<ToolOutput, ToolError> {
        (self.run)(ctx, input)
    }
}

/// Добавляет инструмент в реестр: имя, английское описание, схема входа, класс.
pub(crate) fn add(
    registry: &mut Registry,
    name: &'static str,
    description: &'static str,
    schema: Value,
    class: ToolClass,
    run: fn(&mut ToolContext<'_>, Value) -> Result<ToolOutput, ToolError>,
) {
    registry.register(Box::new(FnTool {
        spec: ToolSpec {
            name,
            description,
            input_schema: schema,
            class,
        },
        run,
    }));
}

/// Состояние проекта, как его отдаёт редактор после чтения и после правки.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    /// Документ целиком, как его держит движок.
    pub project: concat_project::model::Project,
    /// Есть ли что откатить.
    pub can_undo: bool,
    /// Есть ли что повторить.
    pub can_redo: bool,
    /// Имя, размер и частота активной дорожки времени.
    pub settings: Settings,
    /// Id, который выдала создающая команда.
    #[serde(default)]
    pub created_id: Option<String>,
}

/// Настройки сессии: имя проекта и кадр активной дорожки времени.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    /// Имя проекта.
    pub name: String,
    /// Ширина кадра.
    pub width: u32,
    /// Высота кадра.
    pub height: u32,
    /// Числитель частоты кадров.
    pub rate_num: i64,
    /// Знаменатель частоты кадров.
    pub rate_den: i64,
}

/// Разбирает ответ редактора в состояние.
pub(crate) fn parse_view(value: Value) -> Result<View, ToolError> {
    serde_json::from_value(value).map_err(|error| ToolError::Failed(error.to_string()))
}

/// Читает текущее состояние открытого проекта.
pub(crate) fn fetch(ctx: &mut ToolContext<'_>) -> Result<View, ToolError> {
    let path = ctx.project()?;
    parse_view(ctx.call(concat_api::Request::ProjectGet { path })?)
}

/// Применяет одну команду и возвращает новое состояние.
pub(crate) fn apply(
    ctx: &mut ToolContext<'_>,
    command: concat_project::Command,
) -> Result<View, ToolError> {
    let path = ctx.project()?;
    parse_view(ctx.call(concat_api::Request::EditApply {
        path,
        command: Box::new(command),
    })?)
}

/// Собирает несколько команд в одну атомарную правку: один шаг undo.
pub(crate) fn batch(commands: Vec<concat_project::Command>) -> concat_project::Command {
    if commands.len() == 1 {
        commands
            .into_iter()
            .next()
            .unwrap_or_else(|| concat_project::Command::Batch {
                commands: Vec::new(),
            })
    } else {
        concat_project::Command::Batch { commands }
    }
}

/// Клип активной дорожки времени по id или понятная ошибка.
pub(crate) fn find_clip<'a>(
    project: &'a concat_project::model::Project,
    clip_id: &str,
) -> Result<&'a concat_project::model::Clip, ToolError> {
    project
        .active()
        .clip(clip_id)
        .ok_or_else(|| ToolError::Failed(format!("clip {clip_id} is not on the active timeline")))
}

/// Секунды как `mm:ss.mmm`.
pub(crate) fn time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 1000.0).round() as i64;
    let millis = total % 1000;
    let secs = total / 1000;
    format!("{:02}:{:02}.{:03}", secs / 60, secs % 60, millis)
}

/// Частота кадров как число: целое без дробной части, иначе три знака.
pub(crate) fn fps(rate_num: i64, rate_den: i64) -> String {
    let value = rate_num as f64 / rate_den.max(1) as f64;
    if (value - value.round()).abs() < 1e-6 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.3}")
    }
}

/// Число без хвостовых нулей, для коротких ответов.
pub(crate) fn num(value: f64) -> String {
    let mut text = format!("{value:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Частота кадров в виде дроби; невнятный вход даёт 30 кадров в секунду.
pub(crate) fn rate_of(fps: f64) -> (i64, i64) {
    if !fps.is_finite() || fps <= 0.0 {
        return (30, 1);
    }
    if (fps - fps.round()).abs() < 1e-6 {
        return (fps.round() as i64, 1);
    }
    let num = (fps * 1000.0).round() as i64;
    let den = 1000_i64;
    let divisor = gcd(num, den);
    (num / divisor, den / divisor)
}

/// НОД двух положительных чисел.
fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    a.abs().max(1)
}

/// Отпечаток активной дорожки времени: меняется от любой правки, годится как ревизия.
pub(crate) fn revision(project: &concat_project::model::Project) -> u64 {
    use std::hash::{Hash, Hasher};
    let json = serde_json::to_string(project.active()).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}
