// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! Taking a video's sound out onto its own lane, and putting it back.
//!
//! One arm per command, exactly as [`super::apply`] routes them here;
//! everything these arms share lives in the parent module.

use super::*;

/// Applies one of this module's commands. Any other is a routing error.
pub(super) fn apply(
    project: &mut Project,
    mint: &mut IdMint,
    command: Command,
) -> Result<Outcome, CommandError> {
    match command {
        Command::DetachAudio { clip_id } => {
            let sounds = detach(project, mint, &clip_id);
            Ok(Outcome {
                applied: !sounds.is_empty(),
                created_id: sounds.into_iter().next(),
            })
        }

        Command::DetachAudioLinked { clip_id } => {
            let sounds = detach(project, mint, &clip_id);
            let timeline = project.active_mut();
            // The group is named after the video: a clip id is never minted twice.
            for id in std::iter::once(&clip_id).chain(&sounds) {
                if let Some(clip) = timeline.clip_mut(id) {
                    links::set_link(clip, Some(&clip_id));
                }
            }
            Ok(Outcome {
                applied: !sounds.is_empty(),
                created_id: sounds.into_iter().next(),
            })
        }

        Command::ReattachAudio { clip_id } => {
            let timeline = project.active_mut();
            let Some(clip) = timeline.clip(&clip_id) else {
                return Ok(Outcome::default());
            };
            let video_id = match (&clip.kind, &clip.detached_from) {
                (ClipKind::Audio, Some(source)) => source.clone(),
                _ => clip.id.clone(),
            };
            if timeline.clip(&video_id).is_none() {
                return Ok(Outcome::default());
            }
            let sounds: Vec<Clip> = timeline
                .clips
                .iter()
                .filter(|other| other.detached_from.as_deref() == Some(video_id.as_str()))
                .map(|other| Clip::clone(other))
                .collect();
            if sounds.is_empty() {
                return Ok(Outcome::default());
            }
            // A set, not a Vec: the retain below tests every clip on the
            // timeline against it.
            let doomed: HashSet<String> = sounds.iter().map(|sound| sound.id.clone()).collect();
            timeline.clips.retain(|other| !doomed.contains(&other.id));
            // A group left with only the video is no group.
            let lone_link = timeline
                .clip(&video_id)
                .and_then(links::link_of)
                .filter(|link| {
                    !timeline
                        .clips
                        .iter()
                        .any(|other| other.id != video_id && links::link_of(other) == Some(*link))
                })
                .is_some();
            let video = timeline.clip_mut(&video_id).expect("checked above");
            video.muted = None;
            video.filters = sounds[0].filters.clone();
            if lone_link {
                links::set_link(video, None);
            }
            // Reaching here means at least one sound clip was deleted.
            Ok(Outcome {
                created_id: None,
                applied: true,
            })
        }

        _ => unreachable!("commands::apply routes only this module's commands here"),
    }
}

/// Pulls a video's sound out into clips of its own, one per stream when the
/// file has several, and says the new clips' ids; empty is a no-op.
fn detach(project: &mut Project, mint: &mut IdMint, clip_id: &str) -> Vec<String> {
    // What comes off, as `(stream, name)`: one sound clip per audio
    // track when the file lists several - a recording that kept the
    // desktop and the microphone apart stays apart, each on a lane
    // of its own - else the one stream the video clip was playing.
    let sounds: Vec<(Option<u32>, String)> = {
        let timeline = project.active();
        let Some(clip) = timeline.clip(clip_id) else {
            return Vec::new();
        };
        let media = project.media_by_id(&clip.media_id);
        let has_audio = clip.kind == ClipKind::Video
            && clip.muted != Some(true)
            && media.is_some_and(|media| media.has_audio)
            && !timeline
                .clips
                .iter()
                .any(|other| other.detached_from.as_deref() == Some(clip_id));
        if !has_audio {
            return Vec::new();
        }
        match media {
            Some(media) if media.audio_tracks.len() > 1 => media
                .audio_tracks
                .iter()
                .enumerate()
                .map(|(position, track)| {
                    let label = if track.title.is_empty() {
                        format!("Track {}", position + 1)
                    } else {
                        track.title.clone()
                    };
                    (Some(track.index), format!("{} · {label}", clip.name))
                })
                .collect(),
            _ => vec![(clip.audio_stream, clip.name.clone())],
        }
    };

    let timeline = project.active_mut();
    let clip = timeline.clip(clip_id).expect("checked above").clone();
    let mut sounds_made = Vec::new();
    for (stream, name) in sounds {
        // A lane free for the whole span, or a fresh one. Each sound
        // placed takes its lane, so the next looks past it.
        let track_id = {
            let end = clip.start + clip.duration;
            let free = timeline.tracks.iter().find(|track| {
                !timeline.clips.iter().any(|other| {
                    other.track_id == track.id
                        && other.start < end
                        && clip.start < other.start + other.duration
                })
            });
            match free {
                Some(track) => track.id.clone(),
                None => {
                    let id = mint.next("t");
                    timeline.tracks.push(Track {
                        id: id.clone(),
                        visible: true,
                        muted: false,
                        extra: Default::default(),
                    });
                    id
                }
            }
        };

        let mut sound = clip.clone();
        sound.id = mint.next("c");
        sound.track_id = track_id;
        sound.name = name;
        sound.kind = ClipKind::Audio;
        sound.video_effects = Vec::new();
        sound.transition_in = None;
        sound.detached_from = Some(clip_id.to_owned());
        sound.muted = None;
        sound.audio_stream = stream;
        sounds_made.push(sound.id.clone());
        timeline.clips.push(Arc::new(sound));
    }

    let video = timeline.clip_mut(clip_id).expect("still present");
    video.muted = Some(true);
    video.filters = Vec::new();
    sounds_made
}
