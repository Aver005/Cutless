// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Приёмы эдитов чистыми функциями: пресеты кривой скорости и удары (наезд, тряска, вспышка).
//! Без ввода-вывода: окно и инструменты агента получают отсюда команды и сами их применяют.

pub mod hits;
pub mod speed;

pub use hits::{HitError, HitKind, Hits};
pub use speed::SpeedPreset;
