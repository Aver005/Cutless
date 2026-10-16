// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Каталог моделей распознавания и автопоиск уже скачанных.
//!
//! Две задачи, обе чистые: список моделей, которые можно скачать (Whisper из
//! whisper.cpp, Parakeet и Nemotron от handy-computer), и обход кэша Hugging
//! Face, где движок находит модели, скачанные другими приложениями. Модели
//! handy-computer зеркала у Concat не имеют - тянуть их надо прямо с Hugging
//! Face, поэтому у них `mirrored = false`; путь скачивания собирает
//! [`concat_host::models`].

use std::path::{Path, PathBuf};

/// Семейство модели: по нему выбираются тайм-коды и распознаётся автопоиск.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// Whisper из whisper.cpp.
    Whisper,
    /// Parakeet (NVIDIA, TDT).
    Parakeet,
    /// Nemotron ASR.
    Nemotron,
    /// GigaAM (русский).
    GigaAm,
    /// Canary (NVIDIA).
    Canary,
}

impl Family {
    /// Имя семейства, как его показывает список моделей.
    pub fn name(self) -> &'static str {
        match self {
            Family::Whisper => "whisper",
            Family::Parakeet => "parakeet",
            Family::Nemotron => "nemotron",
            Family::GigaAm => "gigaam",
            Family::Canary => "canary",
        }
    }
}

/// Одна модель, которую настройки предлагают скачать.
pub struct CatalogModel {
    /// Идентификатор, с которым работают настройки и запрос.
    pub id: &'static str,
    /// Имя для списка.
    pub label: &'static str,
    /// Строка под именем: чем одна модель отличается от другой, и лицензия.
    pub blurb: &'static str,
    /// Примерный размер в байтах, для полосы прогресса.
    pub approx_bytes: u64,
    /// SHA-256 готового файла; пустой только у моделей без зеркала и без
    /// готового хэша (у нас таких нет).
    pub sha256: &'static str,
    /// Понимает ли модель только английский.
    pub english_only: bool,
    /// Имя файла на зеркале и у апстрима.
    pub file: &'static str,
    /// Откуда скачивать; для Hugging Face привязано к коммиту.
    pub upstream: &'static str,
    /// Есть ли модель на своём зеркале Concat (у моделей handy-computer нет).
    pub mirrored: bool,
    /// Семейство модели.
    pub family: Family,
}

/// Коммит whisper.cpp, на котором сняты хэши таблицы. Нужен только тесту,
/// который держит апстримы зеркалящихся моделей привязанными к нему.
#[cfg(test)]
const WHISPER_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// Каталог: сначала маленькие Whisper, потом русские модели.
///
/// Whisper - официальные преобразования `ggerganov/whisper.cpp`, зеркалятся
/// на релиз Concat. Parakeet и Nemotron - Q8_0 из `handy-computer` на Hugging
/// Face, без зеркала; имена файлов и коммиты взяты из дерева репозитория,
/// размер и SHA-256 - из его LFS.
pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        id: "tiny.en",
        label: "Tiny (English)",
        blurb: "Fastest draft. Rough on names and punctuation.",
        approx_bytes: 77_704_715,
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        english_only: true,
        file: "ggml-tiny.en.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.en.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "tiny",
        label: "Tiny (Multilingual)",
        blurb: "Fastest draft, any language.",
        approx_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        english_only: false,
        file: "ggml-tiny.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "base.en",
        label: "Base (English)",
        blurb: "The sweet spot: solid captions at ~10x realtime.",
        approx_bytes: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        english_only: true,
        file: "ggml-base.en.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "base",
        label: "Base (Multilingual)",
        blurb: "Solid captions, any language.",
        approx_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        english_only: false,
        file: "ggml-base.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "small.en",
        label: "Small (English)",
        blurb: "Noticeably better wording; a few times slower.",
        approx_bytes: 487_614_201,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        english_only: true,
        file: "ggml-small.en.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.en.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "small",
        label: "Small (Multilingual)",
        blurb: "Best quality offered, any language.",
        approx_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        english_only: false,
        file: "ggml-small.bin",
        upstream: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
        mirrored: true,
        family: Family::Whisper,
    },
    CatalogModel {
        id: "parakeet-tdt-0.6b-v3",
        label: "Parakeet TDT 0.6B v3",
        blurb: "25 European languages, Russian among them; fast and punctuation-aware. CC-BY-4.0 (attribute NVIDIA).",
        approx_bytes: 739_508_576,
        sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
        english_only: false,
        file: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
        upstream: "https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/90f082450fcbacdb54e5900c44ef697c9ea59622/parakeet-tdt-0.6b-v3-Q8_0.gguf",
        mirrored: false,
        family: Family::Parakeet,
    },
    CatalogModel {
        id: "nemotron-3.5-asr-streaming-0.6b",
        label: "Nemotron 3.5 ASR streaming 0.6B",
        blurb: "30+ languages, Russian among them; made for streaming. OpenMDW-1.1.",
        approx_bytes: 751_094_240,
        sha256: "b94545b313b3223fda7b2857a52681da813935c2127643d1e9ff0c23d988089c",
        english_only: false,
        file: "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
        upstream: "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/8139c4ec14bdc45c361adf8d57c27c28e7478272/nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
        mirrored: false,
        family: Family::Nemotron,
    },
];

/// Модель каталога по идентификатору.
pub fn known(id: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|model| model.id == id)
}

/// Семейство по имени файла, только для известных семейств: `*.gguf` и
/// `ggml-*.bin` (whisper.cpp). `None`, если имя не похоже ни на одно.
pub fn family_of_file(name: &str) -> Option<Family> {
    let lower = name.to_ascii_lowercase();
    let model = lower.ends_with(".gguf") || (lower.starts_with("ggml-") && lower.ends_with(".bin"));
    if !model {
        return None;
    }
    let has = |needle: &str| lower.contains(needle);
    if has("parakeet") {
        Some(Family::Parakeet)
    } else if has("nemotron") {
        Some(Family::Nemotron)
    } else if has("gigaam") {
        Some(Family::GigaAm)
    } else if has("canary") {
        Some(Family::Canary)
    } else if has("whisper") || lower.starts_with("ggml-") {
        Some(Family::Whisper)
    } else {
        None
    }
}

/// Похоже ли имя на файл модели распознавания: `*.gguf` или `*.bin`. Для
/// диалога выбора своей модели строже не надо - движок сам скажет, что не так.
pub fn is_model_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gguf") || lower.ends_with(".bin")
}

/// Модель, найденная на диске (в кэше Hugging Face).
#[derive(Clone, Debug, PartialEq)]
pub struct FoundModel {
    /// Путь к файлу модели.
    pub path: PathBuf,
    /// Семейство по имени файла.
    pub family: Family,
    /// Размер файла в байтах.
    pub bytes: u64,
}

/// Имя модели для списка: имя файла без расширения, дефисы на месте.
pub fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("model")
        .to_owned()
}

/// Корень кэша Hugging Face: `HF_HUB_CACHE`, иначе `HF_HOME/hub`, иначе
/// `~/.cache/huggingface/hub` (`HOME`, а на Windows - `USERPROFILE`).
pub fn hf_cache_dir() -> Option<PathBuf> {
    let hub = std::env::var_os("HF_HUB_CACHE");
    let home = std::env::var_os("HF_HOME");
    let user = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    hub_cache(
        hub.as_deref().map(Path::new),
        home.as_deref().map(Path::new),
        user.as_deref().map(Path::new),
    )
}

/// [`hf_cache_dir`] без доступа к среде: приоритет переменных, но проверяемо.
pub fn hub_cache(
    hub: Option<&Path>,
    hf_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let non_empty = |path: &Path| !path.as_os_str().is_empty();
    if let Some(hub) = hub.filter(|path| non_empty(path)) {
        return Some(hub.to_owned());
    }
    if let Some(hf_home) = hf_home.filter(|path| non_empty(path)) {
        return Some(hf_home.join("hub"));
    }
    home.filter(|path| non_empty(path))
        .map(|home| home.join(".cache").join("huggingface").join("hub"))
}

/// Обходит кэш HF и собирает модели известных семейств: папки
/// `models--*--*/snapshots/*/` с файлами `*.gguf` или `ggml-*.bin`.
///
/// Одинаковые имена из разных снимков одного репозитория схлопываются, и
/// список выходит ровным - по имени файла, потом по пути.
pub fn scan_cache(root: &Path) -> Vec<FoundModel> {
    let Ok(repos) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for repo in repos.flatten() {
        let repo_path = repo.path();
        let name = repo.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with("models--") || !repo_path.is_dir() {
            continue;
        }
        let snapshots = repo_path.join("snapshots");
        let Ok(snaps) = std::fs::read_dir(&snapshots) else {
            continue;
        };
        for snap in snaps.flatten() {
            let snap_path = snap.path();
            if !snap_path.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&snap_path) else {
                continue;
            };
            for file in files.flatten() {
                let path = file.path();
                let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(family) = family_of_file(file_name) else {
                    continue;
                };
                let Ok(meta) = std::fs::metadata(&path) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                found.push(FoundModel {
                    path,
                    family,
                    bytes: meta.len(),
                });
            }
        }
    }
    found.sort_by(|a, b| {
        a.path
            .file_name()
            .cmp(&b.path.file_name())
            .then_with(|| a.path.cmp(&b.path))
    });
    found.dedup_by(|a, b| a.path.file_name() == b.path.file_name());
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_says_its_family() {
        assert_eq!(family_of_file("ggml-tiny.en.bin"), Some(Family::Whisper));
        assert_eq!(
            family_of_file("whisper-large-v3.gguf"),
            Some(Family::Whisper)
        );
        assert_eq!(
            family_of_file("parakeet-tdt-0.6b-v3-Q8_0.gguf"),
            Some(Family::Parakeet)
        );
        assert_eq!(
            family_of_file("nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf"),
            Some(Family::Nemotron)
        );
        assert_eq!(family_of_file("gigaam-v3-ctc.gguf"), Some(Family::GigaAm));
        assert_eq!(family_of_file("canary-1b-flash.gguf"), Some(Family::Canary));
        // Not a model file, and not a family we look for.
        assert_eq!(family_of_file("config.json"), None);
        assert_eq!(family_of_file("moonshine-tiny.gguf"), None);
        assert!(is_model_file("model.gguf"));
        assert!(is_model_file("ggml-base.bin"));
        assert!(!is_model_file("README.md"));
    }

    #[test]
    fn the_cache_root_follows_the_environment_in_order() {
        let hub = Path::new("D:/hub-cache");
        let hf_home = Path::new("D:/hf-home");
        let home = Path::new("D:/me");
        assert_eq!(
            hub_cache(Some(hub), Some(hf_home), Some(home)),
            Some(hub.to_path_buf())
        );
        assert_eq!(
            hub_cache(None, Some(hf_home), Some(home)),
            Some(hf_home.join("hub"))
        );
        assert_eq!(
            hub_cache(None, None, Some(home)),
            Some(home.join(".cache").join("huggingface").join("hub"))
        );
        assert_eq!(hub_cache(None, None, None), None);
        // An empty variable is not a path.
        assert_eq!(hub_cache(Some(Path::new("")), None, None), None);
    }

    #[test]
    fn scanning_finds_snapshot_models_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!("concat-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let snap = root
            .join("models--handy-computer--nemotron-3.5-asr-streaming-0.6b-gguf")
            .join("snapshots")
            .join("8139c4ec");
        std::fs::create_dir_all(&snap).expect("a snapshot folder");
        std::fs::write(
            snap.join("nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf"),
            b"model",
        )
        .expect("a model file");
        std::fs::write(snap.join("README.md"), b"notes").expect("a readme");
        // Another repo, a whisper `.bin`.
        let whisper = root
            .join("models--ggerganov--whisper.cpp")
            .join("snapshots")
            .join("main");
        std::fs::create_dir_all(&whisper).expect("a snapshot folder");
        std::fs::write(whisper.join("ggml-base.bin"), b"bin").expect("a model file");
        // A repository with no snapshots at all.
        std::fs::create_dir_all(root.join("models--x--y")).expect("an empty repo");

        let found = scan_cache(&root);
        let names: Vec<String> = found
            .iter()
            .map(|model| display_name(&model.path))
            .collect();
        assert_eq!(names, ["ggml-base", "nemotron-3.5-asr-streaming-0.6b-Q8_0"]);
        assert_eq!(found[0].family, Family::Whisper);
        assert_eq!(found[1].family, Family::Nemotron);
        assert_eq!(found[1].bytes, 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_cache_root_is_simply_no_models() {
        assert!(scan_cache(Path::new("no/such/folder/anywhere")).is_empty());
    }

    #[test]
    fn every_catalog_model_is_asked_of_the_right_sources() {
        for model in CATALOG {
            assert!(model.approx_bytes > 0, "{} has no size", model.id);
            assert!(model.upstream.ends_with(model.file), "{}", model.id);
            assert!(
                known(model.id).is_some(),
                "{} is not in the table",
                model.id
            );
            if model.mirrored {
                let sources = concat_host::models::sources(model.file, model.upstream);
                let (mirror, upstream) = (&sources[0], &sources[1]);
                assert!(mirror.contains(concat_host::models::RELEASE), "{mirror}");
                assert!(mirror.ends_with(model.file));
                assert_eq!(*upstream, model.upstream);
                assert!(model.upstream.contains(WHISPER_COMMIT));
            } else {
                let sources = concat_host::models::sources_upstream(model.file, model.upstream);
                assert_eq!(sources[0], model.upstream);
                assert!(
                    sources
                        .iter()
                        .all(|url| !url.contains(concat_host::models::RELEASE)),
                    "an unmirrored model must not ask for our release"
                );
                assert!(model.upstream.contains("/resolve/"), "pinned to a commit");
            }
        }
    }
}
