// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Мост к окну: запросы по проекту на экране выполняет окно, а не своя сессия API.
//! Кадр и экспорт строятся по снимку документа окна.

use concat_host::Session;
use concat_project::{Command, DocumentSettings};

use crate::{Api, ApiError, Done, ErrorCode, Holder, Reply, Request, Response, media, projects};

/// Окно, которое держит проекты на экране и выполняет запросы по ним.
pub trait WindowLink: Send {
    /// Выполняет запрос над проектом окна. Вызывается из потока API, ждёт поток окна.
    fn call(&self, request: Request) -> Response;
}

impl Api {
    /// Связывает API с окном: с этого момента проект на экране правится через окно.
    pub fn link_window(&mut self, link: Box<dyn WindowLink>) {
        self.window = Some(link);
    }

    /// Проект запроса открыт в связанном окне.
    pub(crate) fn on_screen(&self, request: &Request) -> bool {
        self.window.is_some()
            && project_of(request)
                .is_some_and(|path| self.open.holder(path) == Some(Holder::Window))
    }

    /// [`Request::ProjectOnScreen`].
    pub(crate) fn projects_on_screen(&self) -> Vec<concat_host::ProjectInfo> {
        if self.window.is_none() {
            return Vec::new();
        }
        self.open
            .held_by(Holder::Window)
            .iter()
            .filter_map(|path| projects::open(path).ok())
            .collect()
    }

    /// Запрос по проекту окна: правки — окну, кадр и экспорт — по снимку его документа.
    pub(crate) fn through_window(&mut self, request: Request) -> Result<Reply, ApiError> {
        match request {
            Request::PreviewFrame {
                path,
                time,
                output,
                width,
                height,
            } => {
                let session = self.window_session(&path)?;
                let size = width.zip(height);
                match output {
                    Some(output) => Ok(Reply::Written(
                        self.frame_of(&session, time, &output, size)?,
                    )),
                    None => Ok(Reply::Picture(self.picture_of(&session, time, size)?)),
                }
            }
            Request::ExportRun { path, spec } => {
                self.writable(&spec.output)?;
                let session = self.window_session(&path)?;
                let prepared = self.prepare_export(&session, &spec)?;
                Ok(Reply::Started(self.start_export(prepared, &spec)?))
            }
            Request::TemplateSave { path, name } => {
                let session = self.window_session(&path)?;
                Ok(Reply::Template(self.template_of(&session, &name)?))
            }
            // Окно получает готовую команду: пробу файла делаем здесь, вне потока окна.
            Request::MediaImport { path, file } => {
                let item = media::probe(&file)
                    .map_err(ApiError::failed)?
                    .to_new_media();
                self.ask_window(Request::EditApply {
                    path,
                    command: Box::new(Command::AddMedia { item }),
                })
            }
            // Проект закрывает человек, а не агент.
            Request::ProjectClose { .. } => Ok(Reply::Done(Done {})),
            request => self.ask_window(request),
        }
    }

    /// Снимок проекта окна как сессия только для чтения.
    fn window_session(&self, path: &str) -> Result<Session, ApiError> {
        let Reply::View(view) = self.ask_window(Request::ProjectGet {
            path: path.to_owned(),
        })?
        else {
            return Err(ApiError::failed("the window answered project.get oddly"));
        };
        let Reply::Document(document) = self.ask_window(Request::ProjectDocument {
            path: path.to_owned(),
        })?
        else {
            return Err(ApiError::failed(
                "the window answered project.document oddly",
            ));
        };
        let settings = DocumentSettings {
            name: view.settings.name.clone(),
            width: view.settings.width,
            height: view.settings.height,
            rate_num: view.settings.rate_num,
            rate_den: view.settings.rate_den,
        };
        Session::from_document(path, settings, Some(&document)).map_err(ApiError::failed)
    }

    /// Передаёт запрос окну и разворачивает ответ.
    fn ask_window(&self, request: Request) -> Result<Reply, ApiError> {
        let Some(link) = &self.window else {
            return Err(ApiError::new(ErrorCode::NotOpen, "no window is linked"));
        };
        match link.call(request) {
            Response::Result(reply) => Ok(reply),
            Response::Error(error) => Err(error),
        }
    }
}

/// Папка проекта, к которой относится запрос; у запросов без проекта — None.
fn project_of(request: &Request) -> Option<&str> {
    match request {
        Request::ProjectOpen { path }
        | Request::ProjectClose { path, .. }
        | Request::ProjectGet { path }
        | Request::ProjectDocument { path }
        | Request::ProjectSave { path, .. }
        | Request::ProjectSetVideo { path, .. }
        | Request::EditApply { path, .. }
        | Request::EditUndo { path }
        | Request::EditRedo { path }
        | Request::MediaImport { path, .. }
        | Request::TemplateSave { path, .. }
        | Request::ExportRun { path, .. }
        | Request::PreviewFrame { path, .. } => Some(path),
        Request::Version
        | Request::ProjectCreate { .. }
        | Request::ProjectList
        | Request::ProjectOnScreen
        | Request::MediaProbe { .. }
        | Request::CatalogueList { .. }
        | Request::TemplateList
        | Request::TemplateInstantiate { .. }
        | Request::ExportCancel { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::WindowLink;
    use crate::{Api, AppDirs, Done, Holder, OpenProjects, Reply, Request, Response};

    /// Окно-заглушка: запоминает, какие запросы до него дошли.
    struct FakeWindow(Arc<Mutex<Vec<String>>>);

    impl WindowLink for FakeWindow {
        fn call(&self, request: Request) -> Response {
            let method = serde_json::to_value(&request)
                .ok()
                .and_then(|value| {
                    value
                        .get("method")
                        .and_then(|m| m.as_str().map(str::to_owned))
                })
                .unwrap_or_default();
            self.0.lock().expect("calls").push(method);
            Response::Result(Reply::Done(Done {}))
        }
    }

    /// API со связанным окном, которое держит `held`.
    fn linked(held: &str) -> (Api, Arc<Mutex<Vec<String>>>, tempfile::TempDir) {
        let scratch = tempfile::tempdir().expect("scratch");
        let dirs = AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let mut api = Api::with_dirs(dirs, Arc::new(|_| {}));
        let open = OpenProjects::default();
        open.claim(held, Holder::Window).expect("claim");
        api.share_open_projects(open);
        let calls = Arc::new(Mutex::new(Vec::new()));
        api.link_window(Box::new(FakeWindow(Arc::clone(&calls))));
        (api, calls, scratch)
    }

    #[test]
    fn requests_for_the_window_project_reach_the_window() {
        let (mut api, calls, _scratch) = linked("/on/screen");
        let path = "/on/screen".to_owned();
        api.dispatch(Request::EditUndo { path: path.clone() });
        api.dispatch(Request::ProjectGet { path: path.clone() });
        let closed = api.dispatch(Request::ProjectClose { path, save: true });
        assert!(matches!(closed, Response::Result(Reply::Done(_))));
        let other = api.dispatch(Request::ProjectGet {
            path: "/elsewhere".to_owned(),
        });
        assert!(matches!(other, Response::Error(_)));
        api.dispatch(Request::Version);
        assert_eq!(*calls.lock().expect("calls"), ["edit.undo", "project.get"]);
    }

    #[test]
    fn without_a_link_the_window_project_stays_refused() {
        let scratch = tempfile::tempdir().expect("scratch");
        let dirs = AppDirs {
            config: scratch.path().join("config"),
            data: scratch.path().join("data"),
        };
        let mut api = Api::with_dirs(dirs, Arc::new(|_| {}));
        let open = OpenProjects::default();
        open.claim("/on/screen", Holder::Window).expect("claim");
        api.share_open_projects(open);
        let screen = api.dispatch(Request::ProjectOnScreen);
        assert!(matches!(screen, Response::Result(Reply::Projects(list)) if list.is_empty()));
        let get = api.dispatch(Request::ProjectGet {
            path: "/on/screen".to_owned(),
        });
        assert!(matches!(get, Response::Error(_)));
    }
}
