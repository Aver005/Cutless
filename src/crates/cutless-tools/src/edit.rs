// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты группы «edit»: проект, медиатека, монтаж, титры, эффекты и вывод.

mod clips;
mod effects;
mod media;
mod output;
mod project;
mod support;
mod text;

use crate::Registry;

/// Регистрирует инструменты группы.
pub fn register(registry: &mut Registry) {
    project::register(registry);
    media::register(registry);
    clips::register(registry);
    text::register(registry);
    effects::register(registry);
    output::register(registry);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use concat_api::{Api, ApiError, AppDirs, Request, Response};
    use serde_json::{Value, json};

    use crate::{Registry, ToolContext, ToolError};

    /// Свежий реестр только с инструментами группы «edit».
    fn registry() -> Registry {
        let mut registry = Registry::default();
        super::register(&mut registry);
        registry
    }

    /// Пустая временная папка, названная по тесту.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cutless-edit-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// Настоящий API на временной папке: правки идут через движок.
    fn api(root: &std::path::Path) -> Api {
        let dirs = AppDirs {
            config: root.join("config"),
            data: root.join("data"),
        };
        Api::with_dirs(dirs, Arc::new(|_| {}))
    }

    /// Тестовый редактор: отвечает заранее заданным ответом и помнит запросы.
    struct Mock {
        reply: Response,
        seen: Vec<Request>,
    }

    impl crate::Editor for Mock {
        fn dispatch(&mut self, request: Request) -> Result<serde_json::Value, ApiError> {
            self.seen.push(request);
            crate::answer(self.reply.clone())
        }
    }

    /// Все 26 имён, что группа обязана зарегистрировать.
    const NAMES: [&str; 26] = [
        "project_open",
        "project_create",
        "project_on_screen",
        "project_save",
        "project_settings",
        "timeline_summary",
        "clip_detail",
        "undo",
        "redo",
        "media_import",
        "media_list",
        "clips_split",
        "clip_trim",
        "clips_move",
        "clips_remove",
        "clip_add",
        "clip_volume",
        "clip_transform",
        "text_add",
        "text_update",
        "text_style_track",
        "effects_catalogue",
        "effect_apply",
        "effect_remove",
        "frame_at",
        "export_start",
    ];

    /// По одному корректному входу на инструмент, для проверки разбора.
    fn sample_inputs() -> Vec<(&'static str, Value)> {
        vec![
            ("project_open", json!({ "path": "/p" })),
            (
                "project_create",
                json!({ "location": "/tmp", "name": "New" }),
            ),
            ("project_on_screen", json!({})),
            ("project_save", json!({})),
            ("project_settings", json!({ "width": 1080, "fps": 25.0 })),
            ("timeline_summary", json!({})),
            ("clip_detail", json!({ "clip_id": "c1" })),
            ("undo", json!({})),
            ("redo", json!({})),
            ("media_import", json!({ "file": "/tmp/a.mp4" })),
            ("media_list", json!({})),
            ("clips_split", json!({ "clip_ids": ["c1"], "time": 1.5 })),
            (
                "clip_trim",
                json!({ "clip_id": "c1", "edge": "end", "delta": -0.5 }),
            ),
            (
                "clips_move",
                json!({ "moves": [{ "clip_id": "c1", "start": 2.0 }] }),
            ),
            ("clips_remove", json!({ "clip_ids": ["c1"] })),
            ("clip_add", json!({ "media_id": "m1", "start": 0.0 })),
            ("clip_volume", json!({ "clip_ids": ["c1"], "volume": 0.8 })),
            ("clip_transform", json!({ "clip_id": "c1", "scale": 1.5 })),
            ("text_add", json!({ "text": "Hi", "start": 0.0 })),
            ("text_update", json!({ "clip_id": "c1", "text": "Bye" })),
            ("text_style_track", json!({ "clip_id": "c1" })),
            ("effects_catalogue", json!({ "kind": "filter" })),
            (
                "effect_apply",
                json!({ "clip_ids": ["c1"], "effect_id": "concat.blur" }),
            ),
            (
                "effect_remove",
                json!({ "clip_id": "c1", "effect_id": "concat.blur" }),
            ),
            ("frame_at", json!({ "time": 0.5 })),
            ("export_start", json!({ "output": "/tmp/out.mp4" })),
        ]
    }

    #[test]
    fn the_group_registers_every_tool_once() {
        let specs = registry().specs();
        let mut names: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
        names.sort_unstable();
        let mut expected = NAMES.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected);
        assert!(specs.iter().all(|spec| !spec.description.is_empty()));
        assert!(
            specs
                .iter()
                .all(|spec| spec.input_schema.get("type").is_some())
        );
    }

    #[test]
    fn every_tool_parses_a_correct_input() {
        let registry = registry();
        for (name, input) in sample_inputs() {
            let root = scratch(&format!("parse-{name}"));
            let mut api = api(&root);
            let mut ctx = ToolContext {
                editor: &mut api,
                project: Some(root.to_string_lossy().into_owned()),
            };
            // project_create writes a folder, so it gets a real location.
            let input = if name == "project_create" {
                json!({ "location": root.to_string_lossy(), "name": "Sample" })
            } else {
                input
            };
            // Против пустого API инструмент либо упрётся в редактор, либо
            // честно отработает, но разбор входа не должен упасть.
            let error = registry.call(&mut ctx, name, input).err();
            if let Some(ToolError::BadInput(message)) = &error {
                panic!("{name} rejected its own sample input: {message}");
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn a_missing_required_field_is_bad_input() {
        let registry = registry();
        let root = scratch("bad-input");
        let mut api = api(&root);
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
        };
        for (name, input) in [
            ("clip_trim", json!({ "clip_id": "c1", "delta": 0.5 })),
            ("text_add", json!({ "start": 0.0 })),
            ("clips_split", json!({ "clip_ids": ["c1"] })),
            ("clip_transform", json!({ "clip_id": "c1" })),
        ] {
            match registry.call(&mut ctx, name, input) {
                Err(ToolError::BadInput(_)) => {}
                other => panic!("{name} should be bad input, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_tool_without_a_project_says_so() {
        let registry = registry();
        let mut mock = Mock {
            reply: Response::Error(ApiError::failed("unused")),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: None,
        };
        match registry.call(&mut ctx, "timeline_summary", json!({})) {
            Err(ToolError::NoProject) => {}
            other => panic!("expected NoProject, got {other:?}"),
        }
    }

    /// Проект, как его называет окно в ответе `project.onScreen`.
    fn on_screen(path: &str, name: &str) -> concat_host::ProjectInfo {
        concat_host::ProjectInfo {
            path: path.to_owned(),
            name: name.to_owned(),
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
            opened_at: 0,
        }
    }

    /// Ответ окна со списком открытых проектов.
    fn window_reply(projects: Vec<concat_host::ProjectInfo>) -> Response {
        Response::Result(concat_api::Reply::Projects(projects))
    }

    #[test]
    fn project_on_screen_with_nothing_open_says_so() {
        let registry = registry();
        let mut mock = Mock {
            reply: window_reply(Vec::new()),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: None,
        };
        let output = registry
            .call(&mut ctx, "project_on_screen", json!({}))
            .expect("ran");
        assert!(
            output.text.contains("no project is open"),
            "{}",
            output.text
        );
        assert!(ctx.project.is_none());
        assert_eq!(mock.seen, [Request::ProjectOnScreen]);
    }

    #[test]
    fn project_on_screen_with_one_project_makes_it_current() {
        let registry = registry();
        let mut mock = Mock {
            reply: window_reply(vec![on_screen("/p/One", "One")]),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: None,
        };
        let output = registry
            .call(&mut ctx, "project_on_screen", json!({}))
            .expect("ran");
        assert_eq!(ctx.project.as_deref(), Some("/p/One"));
        assert_eq!(
            output.text,
            "current project: One at /p/One (open in the Cutless window)"
        );
    }

    #[test]
    fn project_on_screen_with_several_lists_the_paths() {
        let registry = registry();
        let mut mock = Mock {
            reply: window_reply(vec![on_screen("/p/One", "One"), on_screen("/p/Two", "Two")]),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: None,
        };
        let output = registry
            .call(&mut ctx, "project_on_screen", json!({}))
            .expect("ran");
        assert!(output.text.contains("/p/One"), "{}", output.text);
        assert!(output.text.contains("/p/Two"), "{}", output.text);
        assert!(ctx.project.is_none(), "не выбираем, какой из нескольких");
    }

    #[test]
    fn clip_trim_builds_the_trim_command_it_was_asked_for() {
        let registry = registry();
        let mut mock = Mock {
            reply: Response::Error(ApiError::failed("stop")),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: Some("/p".to_owned()),
        };
        let _ = registry.call(
            &mut ctx,
            "clip_trim",
            json!({ "clip_id": "c7", "edge": "end", "delta": -0.25, "ripple": true }),
        );
        match mock.seen.as_slice() {
            [Request::EditApply { command, .. }] => match command.as_ref() {
                concat_project::Command::TrimClip {
                    clip_id,
                    edge,
                    delta,
                    ripple,
                } => {
                    assert_eq!(clip_id, "c7");
                    assert_eq!(*edge, concat_project::commands::TrimEdge::End);
                    assert_eq!(*delta, -0.25);
                    assert!(*ripple);
                }
                other => panic!("expected TrimClip, got {other:?}"),
            },
            other => panic!("expected one EditApply, got {other:?}"),
        }
    }

    #[test]
    fn a_project_is_created_edited_saved_and_reopened() {
        let root = scratch("flow");
        let location = root.join("projects");
        std::fs::create_dir_all(&location).expect("location");
        let path;
        {
            let mut api = api(&root);
            let mut ctx = ToolContext {
                editor: &mut api,
                project: None,
            };

            let created = registry()
                .call(
                    &mut ctx,
                    "project_create",
                    json!({
                        "location": location.to_string_lossy(),
                        "name": "Promo",
                        "width": 1080,
                        "height": 1920,
                        "fps": 30
                    }),
                )
                .expect("created");
            path = ctx.project.clone().expect("remembers the project");
            assert!(path.contains("Promo"));
            assert!(created.text.contains("1080x1920 @30"));

            let added = registry()
                .call(
                    &mut ctx,
                    "text_add",
                    json!({ "text": "Привет", "start": 0.0, "duration": 2.0 }),
                )
                .expect("added");
            let clip_id = added.data["createdId"].as_str().expect("minted").to_owned();
            assert!(!clip_id.is_empty());

            let media = registry()
                .call(&mut ctx, "media_list", json!({}))
                .expect("media");
            assert_eq!(media.text, "no media in the bin");

            let summary = registry()
                .call(&mut ctx, "timeline_summary", json!({}))
                .expect("summary");
            assert!(summary.text.starts_with("project \"Promo\" rev "));
            assert!(summary.text.contains("1080x1920 @30"));
            assert!(summary.text.contains("text \"Привет\""));

            let detail = registry()
                .call(&mut ctx, "clip_detail", json!({ "clip_id": clip_id }))
                .expect("detail");
            assert_eq!(detail.data["kind"], "text");

            registry()
                .call(
                    &mut ctx,
                    "project_settings",
                    json!({ "width": 720, "height": 1280, "fps": 24.0 }),
                )
                .expect("settings");
            let summary = registry()
                .call(&mut ctx, "timeline_summary", json!({}))
                .expect("summary");
            assert!(summary.text.contains("720x1280 @24"));

            registry()
                .call(&mut ctx, "project_save", json!({}))
                .expect("saved");
        }

        let mut api = api(&root);
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
        };
        let reopened = registry()
            .call(&mut ctx, "project_open", json!({ "path": path }))
            .expect("reopened");
        assert!(reopened.text.contains("720x1280 @24"));
        let summary = registry()
            .call(&mut ctx, "timeline_summary", json!({}))
            .expect("summary");
        assert!(summary.text.contains("text \"Привет\""));
    }

    #[test]
    fn a_missing_required_member_of_a_batch_reports_bad_input() {
        let registry = registry();
        let mut mock = Mock {
            reply: Response::Error(ApiError::failed("stop")),
            seen: Vec::new(),
        };
        let mut ctx = ToolContext {
            editor: &mut mock,
            project: Some("/p".to_owned()),
        };
        assert!(matches!(
            registry.call(&mut ctx, "clips_move", json!({ "moves": [] })),
            Err(ToolError::BadInput(_))
        ));
        assert!(matches!(
            registry.call(&mut ctx, "clips_remove", json!({ "clip_ids": [] })),
            Err(ToolError::BadInput(_))
        ));
        assert!(matches!(
            registry.call(&mut ctx, "clip_volume", json!({ "clip_ids": ["c1"] })),
            Err(ToolError::BadInput(_))
        ));
    }

    #[test]
    fn title_style_spreads_to_its_track_and_keeps_words() {
        let root = scratch("text-track");
        let mut api = api(&root);
        let tools = registry();
        let location = root.join("projects");
        std::fs::create_dir_all(&location).expect("location");
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
        };
        tools
            .call(
                &mut ctx,
                "project_create",
                json!({ "location": location.to_string_lossy(), "name": "Titles" }),
            )
            .expect("created");

        let first = tools
            .call(
                &mut ctx,
                "text_add",
                json!({ "text": "First", "start": 0.0, "duration": 1.0, "style": { "color": "#ff0000", "fontSize": 0.2 } }),
            )
            .expect("first");
        let first_id = first.data["createdId"].as_str().expect("id").to_owned();

        let second = tools
            .call(
                &mut ctx,
                "text_add",
                json!({ "text": "Second", "start": 2.0, "duration": 1.0 }),
            )
            .expect("second");
        let second_id = second.data["createdId"].as_str().expect("id").to_owned();

        // Оба заголовка на одной дорожке.
        let first_track = tools
            .call(&mut ctx, "clip_detail", json!({ "clip_id": first_id }))
            .expect("detail")
            .data["trackId"]
            .clone();
        let second_track = tools
            .call(&mut ctx, "clip_detail", json!({ "clip_id": second_id }))
            .expect("detail")
            .data["trackId"]
            .clone();
        assert_eq!(
            first_track, second_track,
            "both titles should share a track"
        );

        tools
            .call(&mut ctx, "text_style_track", json!({ "clip_id": first_id }))
            .expect("style spread");

        let detail = tools
            .call(&mut ctx, "clip_detail", json!({ "clip_id": second_id }))
            .expect("detail");
        assert_eq!(detail.data["text"]["color"], "#ff0000");
        assert_eq!(detail.data["text"]["content"], "Second");

        tools
            .call(
                &mut ctx,
                "text_update",
                json!({ "clip_id": second_id, "text": "Third" }),
            )
            .expect("update");
        let detail = tools
            .call(&mut ctx, "clip_detail", json!({ "clip_id": second_id }))
            .expect("detail");
        assert_eq!(detail.data["text"]["content"], "Third");
        assert_eq!(detail.data["text"]["color"], "#ff0000", "style is kept");
    }

    #[test]
    fn splitting_trimming_moving_and_removing_a_title() {
        let root = scratch("clips");
        let mut api = api(&root);
        let registry = registry();
        let location = root.join("projects");
        std::fs::create_dir_all(&location).expect("location");
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
        };
        registry
            .call(
                &mut ctx,
                "project_create",
                json!({ "location": location.to_string_lossy(), "name": "Clips" }),
            )
            .expect("created");
        let added = registry
            .call(
                &mut ctx,
                "text_add",
                json!({ "text": "Cut me", "start": 0.0, "duration": 4.0 }),
            )
            .expect("added");
        let clip_id = added.data["createdId"].as_str().expect("id").to_owned();

        registry
            .call(
                &mut ctx,
                "clip_trim",
                json!({ "clip_id": clip_id, "edge": "end", "delta": -1.0 }),
            )
            .expect("trim");
        let detail = registry
            .call(&mut ctx, "clip_detail", json!({ "clip_id": clip_id }))
            .expect("detail");
        assert_eq!(detail.data["duration"], 3.0);

        registry
            .call(
                &mut ctx,
                "clips_split",
                json!({ "clip_ids": [clip_id], "time": 1.0 }),
            )
            .expect("split");
        let summary = registry
            .call(&mut ctx, "timeline_summary", json!({}))
            .expect("summary");
        assert_eq!(summary.data["clips"], 2);

        registry
            .call(
                &mut ctx,
                "clip_volume",
                json!({ "clip_ids": [clip_id], "volume": 0.5, "fade_in": 0.2 }),
            )
            .expect("volume");
        registry
            .call(
                &mut ctx,
                "clip_transform",
                json!({ "clip_id": clip_id, "scale": 1.5, "offset_y": -0.3 }),
            )
            .expect("transform");
        registry
            .call(
                &mut ctx,
                "clips_move",
                json!({ "moves": [{ "clip_id": clip_id, "start": 0.5 }] }),
            )
            .expect("move");
        // После разреза справа вплотную лежит хвост: редактор не даёт наложиться, клип
        // остаётся на месте, и инструмент обязан сказать об этом модели.
        let moved = registry
            .call(
                &mut ctx,
                "clips_move",
                json!({ "moves": [{ "clip_id": clip_id, "start": 0.5 }] }),
            )
            .expect("move again");
        assert!(moved.text.contains("stays at"), "{}", moved.text);
        let detail = registry
            .call(&mut ctx, "clip_detail", json!({ "clip_id": clip_id }))
            .expect("detail");
        assert_eq!(detail.data["start"], 0.0);
        registry.call(&mut ctx, "undo", json!({})).expect("undo");
        registry.call(&mut ctx, "redo", json!({})).expect("redo");

        registry
            .call(
                &mut ctx,
                "clips_remove",
                json!({ "clip_ids": [clip_id], "ripple": true }),
            )
            .expect("remove");
        let summary = registry
            .call(&mut ctx, "timeline_summary", json!({}))
            .expect("summary");
        assert_eq!(summary.data["clips"], 1);
    }

    #[test]
    fn an_effect_is_appended_then_removed() {
        let root = scratch("effects");
        let mut api = api(&root);
        let registry = registry();
        let location = root.join("projects");
        std::fs::create_dir_all(&location).expect("location");
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
        };
        registry
            .call(
                &mut ctx,
                "project_create",
                json!({ "location": location.to_string_lossy(), "name": "Fx" }),
            )
            .expect("created");
        let added = registry
            .call(
                &mut ctx,
                "text_add",
                json!({ "text": "Look", "start": 0.0, "duration": 2.0 }),
            )
            .expect("added");
        let clip_id = added.data["createdId"].as_str().expect("id").to_owned();

        let catalogue = registry
            .call(&mut ctx, "effects_catalogue", json!({ "kind": "filter" }))
            .expect("catalogue");
        assert!(
            catalogue
                .text
                .split('\n')
                .all(|line| line.contains("[filter]"))
        );
        assert!(!catalogue.text.is_empty());

        registry
            .call(
                &mut ctx,
                "effect_apply",
                json!({ "clip_ids": [clip_id], "effect_id": "concat.sepia" }),
            )
            .expect("apply");
        registry
            .call(
                &mut ctx,
                "effect_apply",
                json!({ "clip_ids": [clip_id], "effect_id": "concat.grain" }),
            )
            .expect("apply second");
        let detail = registry
            .call(&mut ctx, "clip_detail", json!({ "clip_id": clip_id }))
            .expect("detail");
        let effects = detail.data["videoEffects"].as_array().expect("chain");
        assert_eq!(
            effects.len(),
            2,
            "the second apply must append, not replace"
        );

        registry
            .call(
                &mut ctx,
                "effect_remove",
                json!({ "clip_id": clip_id, "effect_id": "concat.sepia" }),
            )
            .expect("remove");
        let detail = registry
            .call(&mut ctx, "clip_detail", json!({ "clip_id": clip_id }))
            .expect("detail");
        let effects = detail.data["videoEffects"].as_array().expect("chain");
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0]["id"], "concat.grain");
    }
}
