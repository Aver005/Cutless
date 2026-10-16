// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Мост агента к окну: запросы встроенного сервера по проекту на экране выполняются здесь,
//! в потоке окна. Серия правок агента — один шаг undo, пока человек не тронет проект.

use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use concat_api::{ApiError, Done, ErrorCode, Reply, Request, Response, WindowLink};
use concat_project::{Command, Project};

use crate::host::{Shell, on_ui};
use crate::i18n::{t, tf};
use crate::studio::Studio;

/// Серия кончается, если агент молчит столько времени.
const QUIET: Duration = Duration::from_secs(20);
/// Сколько поток сервера ждёт ответа окна.
const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// Серия правок агента: один шаг undo.
struct Turn {
    /// Имя жеста в истории правок, своё у каждой серии.
    gesture: String,
    /// Ревизия проекта после последней правки агента.
    revision: u64,
    /// Сколько правок в серии.
    edits: usize,
}

/// Состояние агента в окне.
#[derive(Default)]
pub struct AgentPane {
    turn: Option<Turn>,
    turns: u64,
    /// Клипы, которые изменила последняя серия.
    touched: HashSet<String>,
    /// Ревизия, на которой подсветка ещё верна.
    touched_revision: u64,
    quiet: slint::Timer,
}

impl AgentPane {
    /// Агент сейчас правит: серия открыта и человек её не прервал.
    pub fn working(&self, studio_revision: u64) -> bool {
        self.turn
            .as_ref()
            .is_some_and(|turn| turn.revision == studio_revision)
    }

    /// Подсвечивать ли клип: его изменила последняя серия, а человек с тех пор ничего не менял.
    pub fn touched(&self, clip_id: &str, studio_revision: u64) -> bool {
        self.touched_revision == studio_revision && self.touched.contains(clip_id)
    }
}

/// Выполняет запрос сервера над проектом окна.
pub fn answer(studio: &mut Studio, request: Request) -> Response {
    run(studio, request).into()
}

fn run(studio: &mut Studio, request: Request) -> Result<Reply, ApiError> {
    let Some(session) = studio.session.as_ref() else {
        return Err(ApiError::new(
            ErrorCode::NotOpen,
            "no project is open in the window",
        ));
    };
    let view = |studio: &Studio| -> Result<Reply, ApiError> {
        let session = studio
            .session
            .as_ref()
            .ok_or_else(|| refused("the project closed"))?;
        Ok(Reply::View(Box::new(session.view())))
    };
    match request {
        Request::ProjectOpen { .. } | Request::ProjectGet { .. } => view(studio),
        Request::ProjectDocument { .. } => Ok(Reply::Document(session.document())),
        Request::ProjectSave { .. } => {
            studio.save(false);
            Ok(Reply::Done(Done {}))
        }
        Request::EditApply { command, .. } => {
            if studio.pointer_busy() {
                return Err(ApiError::new(
                    ErrorCode::Busy,
                    "the person is dragging in the window; try again in a moment",
                ));
            }
            apply(studio, *command)?;
            view(studio)
        }
        Request::EditUndo { .. } => {
            end_turn(studio);
            studio.undo();
            view(studio)
        }
        Request::EditRedo { .. } => {
            end_turn(studio);
            studio.redo();
            view(studio)
        }
        _ => Err(refused(
            "the window does not serve this for the project on screen",
        )),
    }
}

/// Правка агента: продолжает его серию или открывает новую, если человек менял проект.
fn apply(studio: &mut Studio, command: Command) -> Result<(), ApiError> {
    let continues = studio
        .agent
        .turn
        .as_ref()
        .is_some_and(|turn| turn.revision == studio.revision());
    if !continues {
        end_turn(studio);
        studio.agent.turns += 1;
        studio.agent.turn = Some(Turn {
            gesture: format!("agent:{}", studio.agent.turns),
            revision: studio.revision(),
            edits: 0,
        });
        studio.agent.touched.clear();
    }
    let gesture = studio
        .agent
        .turn
        .as_ref()
        .map(|turn| turn.gesture.clone())
        .unwrap_or_default();
    let before = studio.project().clone();
    studio.apply_agent(&gesture, command).map_err(refused)?;
    let changed = changed_clips(&before, studio.project());
    let revision = studio.revision();
    let pane = &mut studio.agent;
    pane.touched.extend(changed);
    pane.touched_revision = revision;
    if let Some(turn) = pane.turn.as_mut() {
        turn.revision = revision;
        turn.edits += 1;
    }
    pane.quiet.start(slint::TimerMode::SingleShot, QUIET, || {
        Shell::with(|shell, app| {
            end_turn(&mut shell.studio.borrow_mut());
            shell.studio.borrow().publish(&app, &shell.models);
        });
    });
    Ok(())
}

/// Закрывает серию: следующая правка агента — новый шаг undo; итог — тостом.
fn end_turn(studio: &mut Studio) {
    studio.agent.quiet.stop();
    let Some(turn) = studio.agent.turn.take() else {
        return;
    };
    if let Some(session) = studio.session.as_mut() {
        session.end_gesture();
    }
    if turn.edits > 0 {
        studio.notify(&tf("agent.turnDone", &[&turn.edits]), false);
    }
}

/// Клипы активного таймлайна, которых не было или которые изменились.
fn changed_clips(before: &Project, after: &Project) -> Vec<String> {
    let old = before.active();
    after
        .active()
        .clips
        .iter()
        .filter(|clip| old.clip(&clip.id) != Some(clip.as_ref()))
        .map(|clip| clip.id.clone())
        .collect()
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Refused, message.into())
}

/// Подпись индикатора в шапке.
pub fn working_label() -> String {
    t("agent.working")
}

/// Связь встроенного API с окном: запрос уходит в поток окна, ответ ждёт поток сервера.
#[derive(Clone, Default)]
pub struct WindowBridge {
    /// Ответы, которых ждут; закрытие моста роняет их, и ожидание кончается сразу.
    /// Слабые ссылки: запрос, который окно так и не выполнило, тоже отпускает ожидание.
    waiting: Arc<Mutex<Option<Vec<Weak<AnswerSlot>>>>>,
}

type AnswerSlot = Mutex<Option<mpsc::Sender<Response>>>;

impl WindowBridge {
    /// Открытый мост.
    pub fn open() -> WindowBridge {
        WindowBridge {
            waiting: Arc::new(Mutex::new(Some(Vec::new()))),
        }
    }

    /// Закрывает мост перед остановкой сервера: ждущие запросы получают отказ.
    pub fn close(&self) {
        let slots = lock(&self.waiting).take().unwrap_or_default();
        for slot in slots.iter().filter_map(Weak::upgrade) {
            lock(&slot).take();
        }
    }

    /// Регистрирует ожидание ответа; у закрытого моста — None.
    fn wait_slot(&self) -> Option<(Arc<AnswerSlot>, mpsc::Receiver<Response>)> {
        let mut waiting = lock(&self.waiting);
        let list = waiting.as_mut()?;
        list.retain(|slot| slot.strong_count() > 0);
        let (sender, receiver) = mpsc::channel();
        let slot = Arc::new(Mutex::new(Some(sender)));
        list.push(Arc::downgrade(&slot));
        Some((slot, receiver))
    }
}

impl WindowLink for WindowBridge {
    fn call(&self, request: Request) -> Response {
        let gone = || Response::Error(ApiError::failed("the Cutless window is not answering"));
        let Some((slot, receiver)) = self.wait_slot() else {
            return gone();
        };
        on_ui(move |studio, _, _| {
            let response = answer(studio, request);
            if let Some(sender) = lock(&slot).take() {
                let _ = sender.send(response);
            }
        });
        receiver
            .recv_timeout(ANSWER_WAIT)
            .unwrap_or_else(|_| gone())
    }
}

/// Замок, переживший панику в другом потоке.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_bridge_answers_at_once() {
        let bridge = WindowBridge::open();
        bridge.close();
        let response = bridge.call(Request::Version);
        assert!(matches!(response, Response::Error(_)));
    }

    #[test]
    fn closing_drops_every_pending_answer() {
        let bridge = WindowBridge::open();
        let (_slot, receiver) = bridge.wait_slot().expect("open bridge");
        bridge.close();
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn a_request_the_window_never_ran_lets_go() {
        let bridge = WindowBridge::open();
        let (slot, receiver) = bridge.wait_slot().expect("open bridge");
        drop(slot);
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
