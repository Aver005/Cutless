// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Инструменты движения на настоящем движке: проект с одним 20-секундным видео.

use std::sync::Arc;

use concat_api::{Api, AppDirs, Request};
use concat_project::Command;
use concat_project::commands::NewMedia;
use concat_project::model::{ClipKind, KeyProperty, MediaKind, Project};
use serde_json::{Value, json};

use crate::{Registry, ToolContext, ToolError, ToolOutput};

/// Проект с видео `c…` на 0–20 с; держит папку и API, пока жив.
struct Fixture {
    api: Api,
    path: String,
    clip: String,
    registry: Registry,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("cutless-motion-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        let mut api = Api::with_dirs(
            AppDirs {
                config: root.join("config"),
                data: root.join("data"),
            },
            Arc::new(|_| {}),
        );
        let mut registry = Registry::default();
        crate::edit::register(&mut registry);
        super::register(&mut registry);
        crate::beats::register(&mut registry);
        let mut ctx = ToolContext {
            editor: &mut api,
            project: None,
            jobs: crate::Jobs::default(),
        };
        registry
            .call(
                &mut ctx,
                "project_create",
                json!({ "location": root.to_string_lossy(), "name": "Edit" }),
            )
            .expect("project");
        let path = ctx.project.clone().expect("path");
        let media = ctx
            .call(Request::EditApply {
                path: path.clone(),
                command: Box::new(Command::AddMedia {
                    item: NewMedia {
                        path: "/clips/cs2.mp4".to_owned(),
                        name: "cs2.mp4".to_owned(),
                        duration: Some(20.0),
                        kind: MediaKind::Video,
                        width: Some(1920),
                        height: Some(1080),
                        frame_rate: Some(30.0),
                        frame_rate_fraction: Some("30/1".to_owned()),
                        video_codec: Some("h264".to_owned()),
                        audio_codec: None,
                        has_audio: false,
                        audio_tracks: Vec::new(),
                        origin: None,
                        color_space: Default::default(),
                    },
                }),
            })
            .expect("media");
        let media_id = media["createdId"].as_str().expect("media id").to_owned();
        let placed = registry
            .call(
                &mut ctx,
                "clip_add",
                json!({ "media_id": media_id, "start": 0.0 }),
            )
            .expect("clip");
        let clip = placed.data["createdId"]
            .as_str()
            .expect("clip id")
            .to_owned();
        Fixture {
            api,
            path,
            clip,
            registry,
        }
    }

    fn call(&mut self, name: &str, input: Value) -> Result<ToolOutput, ToolError> {
        let mut ctx = ToolContext {
            editor: &mut self.api,
            project: Some(self.path.clone()),
            jobs: crate::Jobs::default(),
        };
        self.registry.call(&mut ctx, name, input)
    }

    fn project(&mut self) -> Project {
        let mut ctx = ToolContext {
            editor: &mut self.api,
            project: Some(self.path.clone()),
            jobs: crate::Jobs::default(),
        };
        let value = ctx
            .call(Request::ProjectGet {
                path: self.path.clone(),
            })
            .expect("project");
        serde_json::from_value(value["project"].clone()).expect("a project")
    }

    fn undo(&mut self) {
        self.call("undo", json!({})).expect("undo");
    }
}

#[test]
fn clip_keys_take_timeline_seconds_and_refuse_outside_the_clip() {
    let mut fixture = Fixture::new("clip-keys");
    let clip = fixture.clip.clone();
    let output = fixture
        .call(
            "clip_keys",
            json!({
                "clip_id": clip,
                "property": "scale",
                "keys": [{ "time": 5.0, "value": 1.0 }, { "time": 5.5, "value": 1.3, "ease": "out" }]
            }),
        )
        .expect("keys");
    assert!(output.text.contains("2 key(s)"), "{}", output.text);
    let project = fixture.project();
    let held = project.active().clip(&clip).expect("clip");
    let at: Vec<f64> = held.keys_on(KeyProperty::Scale).map(|key| key.at).collect();
    assert_eq!(at, vec![0.25, 0.275]);

    let error = fixture
        .call(
            "clip_keys",
            json!({ "clip_id": clip, "property": "scale", "keys": [{ "time": 25.0, "value": 1.0 }] }),
        )
        .expect_err("outside");
    assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
}

#[test]
fn effect_keys_clamp_into_the_range_and_name_the_parameters() {
    let mut fixture = Fixture::new("effect-keys");
    let clip = fixture.clip.clone();
    fixture
        .call(
            "effect_apply",
            json!({ "clip_ids": [clip], "effect_id": "concat.glow" }),
        )
        .expect("glow");
    let output = fixture
        .call(
            "effect_keys",
            json!({
                "clip_id": clip,
                "effect_id": "concat.glow",
                "param": "amount",
                "keys": [{ "time": 1.0, "value": 0.0 }, { "time": 2.0, "value": 500.0 }]
            }),
        )
        .expect("keys");
    assert!(
        output.text.contains("1 value(s) clamped"),
        "{}",
        output.text
    );
    let error = fixture
        .call(
            "effect_keys",
            json!({ "clip_id": clip, "effect_id": "concat.glow", "param": "size", "keys": [{ "time": 1.0, "value": 1.0 }] }),
        )
        .expect_err("no such parameter");
    assert!(error.to_string().contains("amount"), "{error}");
}

#[test]
fn clip_speed_puts_and_takes_a_ramp_preset() {
    let mut fixture = Fixture::new("speed");
    let clip = fixture.clip.clone();
    let output = fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "preset": "hero" }),
        )
        .expect("hero");
    assert!(output.text.contains("curve speed"), "{}", output.text);
    assert!(
        fixture
            .project()
            .active()
            .clip(&clip)
            .expect("clip")
            .speed_curve
            .is_some()
    );

    fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "constant": true }),
        )
        .expect("constant");
    assert!(
        fixture
            .project()
            .active()
            .clip(&clip)
            .expect("clip")
            .speed_curve
            .is_none()
    );

    let error = fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "speed": 2.0, "preset": "hero" }),
        )
        .expect_err("two at once");
    assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
}

#[test]
fn layer_add_places_a_tuned_layer_in_one_step() {
    let mut fixture = Fixture::new("layer");
    let output = fixture
        .call(
            "layer_add",
            json!({ "effect_id": "concat.glow", "start": 2.0, "duration": 3.0, "params": { "amount": 70 } }),
        )
        .expect("layer");
    let id = output.data["createdId"].as_str().expect("id").to_owned();
    let project = fixture.project();
    let layer = project.active().clip(&id).expect("layer");
    assert_eq!(layer.kind, ClipKind::Layer);
    assert_eq!(layer.video_effects[0].params.get("amount"), Some(&70.0));

    fixture.undo();
    assert!(
        fixture.project().active().clip(&id).is_none(),
        "one undo takes it all"
    );

    let error = fixture
        .call(
            "layer_add",
            json!({ "effect_id": "concat.glow", "start": 0.0, "params": { "nope": 1 } }),
        )
        .expect_err("unknown knob");
    assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
}

#[test]
fn transition_set_needs_a_cut_and_holds_to_both_clips() {
    let mut fixture = Fixture::new("transition");
    let clip = fixture.clip.clone();
    let error = fixture
        .call(
            "transition_set",
            json!({ "clip_id": clip, "transition_id": "concat.dissolve" }),
        )
        .expect_err("no cut yet");
    assert!(error.to_string().contains("needs a cut"), "{error}");

    fixture
        .call("clips_split", json!({ "clip_ids": [clip], "time": 10.0 }))
        .expect("split");
    let project = fixture.project();
    let incoming = project
        .active()
        .clips
        .iter()
        .find(|other| (other.start - 10.0).abs() < 1e-6)
        .expect("the second half")
        .id
        .clone();
    fixture
        .call(
            "transition_set",
            json!({ "clip_id": incoming, "transition_id": "concat.dissolve", "duration": 0.75 }),
        )
        .expect("dissolve");
    let project = fixture.project();
    let transition = project
        .active()
        .clip(&incoming)
        .and_then(|clip| clip.transition_in.clone())
        .expect("a transition");
    assert_eq!(transition.id, "concat.dissolve");
    assert!((transition.duration - 0.75).abs() < 1e-9);
}

#[test]
fn hit_fx_punches_in_one_undo_step() {
    let mut fixture = Fixture::new("hit");
    let clip = fixture.clip.clone();
    let output = fixture
        .call(
            "hit_fx",
            json!({ "clip_id": clip, "kind": "punch", "times": [4.0, 8.0], "strength": 2 }),
        )
        .expect("punch");
    assert!(output.text.contains("one undo step"), "{}", output.text);
    let project = fixture.project();
    let held = project.active().clip(&clip).expect("clip");
    let peak = held
        .keys_on(KeyProperty::Scale)
        .map(|key| key.value)
        .fold(0.0, f64::max);
    assert!((peak - 1.16).abs() < 1e-6, "{peak}");

    fixture.undo();
    let project = fixture.project();
    assert!(project.active().clip(&clip).expect("clip").keys.is_empty());
}

#[test]
fn hit_fx_bw_adds_saturation_and_keys_it() {
    let mut fixture = Fixture::new("bw");
    let clip = fixture.clip.clone();
    fixture
        .call(
            "hit_fx",
            json!({ "clip_id": clip, "kind": "bw", "times": [3.0] }),
        )
        .expect("bw");
    let project = fixture.project();
    let held = project.active().clip(&clip).expect("clip");
    let link = &held.video_effects[0];
    assert_eq!(link.id, "concat.saturation");
    let lowest = link
        .keys_on("amount")
        .iter()
        .map(|key| key.value)
        .fold(0.0, f64::min);
    assert!((lowest + 100.0).abs() < 1e-6, "{lowest}");
}

#[test]
fn the_summary_shows_keys_curves_and_layers() {
    let mut fixture = Fixture::new("summary");
    let clip = fixture.clip.clone();
    fixture
        .call(
            "hit_fx",
            json!({ "clip_id": clip, "kind": "shake", "times": [2.0] }),
        )
        .expect("shake");
    fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "preset": "velocity" }),
        )
        .expect("velocity");
    let summary = fixture
        .call("timeline_summary", json!({}))
        .expect("summary");
    assert!(summary.text.contains("keys scale×"), "{}", summary.text);
    assert!(summary.text.contains("offset_x×"), "{}", summary.text);
    assert!(summary.text.contains("speed-curve"), "{}", summary.text);
}

#[test]
fn a_narrow_catalogue_search_shows_ranges() {
    let mut fixture = Fixture::new("catalogue");
    let output = fixture
        .call("effects_catalogue", json!({ "query": "glow" }))
        .expect("catalogue");
    assert!(
        output.text.contains("amount(0..100=45, keys)"),
        "{}",
        output.text
    );
}

#[test]
fn a_ramp_lands_its_peak_on_a_second_of_the_file() {
    let mut fixture = Fixture::new("ramp");
    let clip = fixture.clip.clone();
    let output = fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "preset": "ramp_to_hit", "hit_source": 12.0 }),
        )
        .expect("ramp");
    assert!(output.text.contains("slow-mo peak at"), "{}", output.text);
    let error = fixture
        .call(
            "clip_speed",
            json!({ "clip_ids": [clip], "preset": "ramp_to_hit", "hit_source": 99.0 }),
        )
        .expect_err("past the file");
    assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
}

#[test]
fn a_layer_hit_moves_its_camera_and_enums_take_no_keys() {
    let mut fixture = Fixture::new("camera");
    let layer = fixture
        .call(
            "layer_add",
            json!({ "effect_id": "cutless.camera", "start": 0.0, "duration": 10.0 }),
        )
        .expect("layer")
        .data["createdId"]
        .as_str()
        .expect("id")
        .to_owned();
    fixture
        .call(
            "hit_fx",
            json!({ "clip_id": layer, "kind": "shake", "times": [2.0] }),
        )
        .expect("shake");
    let project = fixture.project();
    let camera = &project.active().clip(&layer).expect("layer").video_effects[0];
    assert_eq!(camera.id, "cutless.camera");
    for key in ["zoom", "x", "y"] {
        assert!(!camera.keys_on(key).is_empty(), "{key} keyed");
    }
    let error = fixture
        .call(
            "effect_keys",
            json!({ "clip_id": layer, "effect_id": "cutless.camera", "param": "edges", "keys": [{ "time": 1.0, "value": 1.0 }] }),
        )
        .expect_err("an enum holds still");
    assert!(matches!(error, ToolError::BadInput(_)), "{error:?}");
}

/// WAV 16 кГц моно: бочка на каждой доле 128 BPM, первая на 0,2 с, `seconds` секунд.
fn click_track(path: &std::path::Path, seconds: f64) {
    let rate = 16_000u32;
    let total = (seconds * f64::from(rate)) as usize;
    let beat = 60.0 / 128.0;
    let mut samples = vec![0i16; total];
    let mut time = 0.2;
    while time < seconds {
        let start = (time * f64::from(rate)) as usize;
        let mut phase = 0.0_f64;
        for n in 0..(0.25 * f64::from(rate)) as usize {
            let t = n as f64 / f64::from(rate);
            phase +=
                2.0 * std::f64::consts::PI * (50.0 + 100.0 * (-t / 0.03).exp()) / f64::from(rate);
            if let Some(sample) = samples.get_mut(start + n) {
                *sample = (20_000.0 * (-t / 0.06).exp() * phase.sin()) as i16;
            }
        }
        time += beat;
    }
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(path, wav).expect("a wav");
}

#[test]
fn a_music_clip_gets_a_grid_and_hits_land_on_its_kicks() {
    let mut fixture = Fixture::new("beats");
    let wav = std::env::temp_dir().join(format!("cutless-beats-{}.wav", std::process::id()));
    click_track(&wav, 20.0);
    let imported = fixture
        .call("media_import", json!({ "file": wav.to_string_lossy() }))
        .expect("import");
    let media_id = imported
        .text
        .split_whitespace()
        .last()
        .expect("an id")
        .to_owned();
    let music = fixture
        .call("clip_add", json!({ "media_id": media_id, "start": 0.0 }))
        .expect("music")
        .data["createdId"]
        .as_str()
        .expect("id")
        .to_owned();

    let found = fixture
        .call(
            "beats_analyze",
            json!({ "clip_id": music, "wait_seconds": 45 }),
        )
        .expect("a grid");
    assert!(found.text.contains("128.0"), "{}", found.text);
    let again = fixture
        .call("beats_analyze", json!({ "clip_id": music }))
        .expect("stored");
    assert!(again.text.starts_with("stored"), "{}", again.text);

    let bars = fixture
        .call(
            "beats_get",
            json!({ "clip_id": music, "what": "bars", "to": 8.0 }),
        )
        .expect("bars");
    // Бочка на каждой доле: какая из них сильная — неизвестно, но такт — ровно четыре доли.
    let first = bars.data["times"][0].as_f64().expect("a bar");
    let second = bars.data["times"][1].as_f64().expect("a second bar");
    assert!(
        (second - first - 4.0 * 60.0 / 128.0).abs() < 0.01,
        "{}",
        bars.text
    );

    let video = fixture.clip.clone();
    let hit = fixture
        .call(
            "hit_fx",
            json!({ "clip_id": video, "kind": "punch", "on": { "clip_id": music, "what": "kicks", "every": 4 } }),
        )
        .expect("hits on kicks");
    assert!(hit.text.contains("punch x"), "{}", hit.text);
    let summary = fixture
        .call("timeline_summary", json!({}))
        .expect("summary");
    assert!(summary.text.contains("beats 128"), "{}", summary.text);
    let _ = std::fs::remove_file(&wav);
}
