// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Сетка долей на медиа: `SetMediaBeats` кладёт или убирает её одним шагом undo.

use super::*;
use crate::beats;

/// Выполняет `SetMediaBeats`.
pub(super) fn apply(project: &mut Project, command: Command) -> Result<Outcome, CommandError> {
    let Command::SetMediaBeats {
        media_id,
        stream,
        grid,
    } = command
    else {
        return Ok(Outcome::default());
    };
    let Some(media) = project.media.iter_mut().find(|item| item.id == media_id) else {
        return Ok(Outcome::default());
    };
    let applied = beats::set_grid(media, stream, grid.as_ref());
    Ok(Outcome {
        created_id: None,
        applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Editor;
    use crate::beats::BeatGrid;

    fn grid() -> BeatGrid {
        BeatGrid {
            version: beats::VERSION,
            bpm: 128.0,
            phase: 0.3,
            bar: 4,
            downbeat: 0,
            duration: 60.0,
            confidence: 1.8,
            alt_bpm: None,
            steady: true,
            kicks: vec![0.3, 0.77],
            snares: Vec::new(),
            drops: vec![30.3],
        }
    }

    #[test]
    fn a_grid_lands_on_the_media_and_undo_takes_it_back() {
        let mut editor = Editor::new();
        let media_id = editor
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: "/music.wav".to_owned(),
                    name: "music.wav".to_owned(),
                    duration: Some(60.0),
                    kind: MediaKind::Audio,
                    width: None,
                    height: None,
                    frame_rate: None,
                    frame_rate_fraction: None,
                    video_codec: None,
                    audio_codec: Some("pcm".to_owned()),
                    has_audio: true,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .expect("media")
            .created_id
            .expect("id");
        let set = editor
            .apply(Command::SetMediaBeats {
                media_id: media_id.clone(),
                stream: None,
                grid: Some(grid()),
            })
            .expect("set");
        assert!(set.applied);
        let media = editor.project().media_by_id(&media_id).expect("media");
        assert_eq!(beats::grid(media, None), Some(grid()));

        editor.undo();
        let media = editor.project().media_by_id(&media_id).expect("media");
        assert_eq!(beats::grid(media, None), None);
    }

    #[test]
    fn a_grid_with_a_nan_is_refused() {
        let mut bad = grid();
        bad.kicks.push(f64::NAN);
        let command = Command::SetMediaBeats {
            media_id: "m1".to_owned(),
            stream: None,
            grid: Some(bad),
        };
        assert!(command.has_non_finite());
    }
}
