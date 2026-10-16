// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Короткие читаемые титры из слов распознавания.
//!
//! Nemotron отдаёт всю фразу одним сегментом, поэтому одну длинную плашку
//! режем по словам: по длине, времени, паузам и концам предложений.

use concat_speech::transcribe::{Segment, Word};

/// Настройки нарезки: сколько влезает в титр и где рвать.
#[derive(Clone, Debug)]
pub struct LineRules {
    /// Сколько символов влезает в один титр.
    pub max_chars: usize,
    /// Сколько секунд держится один титр.
    pub max_seconds: f64,
    /// Пауза между словами, после которой начинается новый титр.
    pub pause_seconds: f64,
    /// Нижняя граница показа: короткий титр растягивается до неё.
    pub min_seconds: f64,
    /// После запятой рвём, только если строка уже длиннее этого.
    pub comma_after_chars: usize,
}

impl Default for LineRules {
    /// Умолчания под вертикальное видео: 32 символа и 2,5 секунды на титр.
    fn default() -> Self {
        Self {
            max_chars: 32,
            max_seconds: 2.5,
            pause_seconds: 0.45,
            min_seconds: 0.6,
            comma_after_chars: 18,
        }
    }
}

/// Готовый титр: когда показать и что написать. Таймкоды — секунды от начала окна.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionLine {
    /// Секунда от начала окна, когда титр появляется.
    pub start: f64,
    /// Секунда от начала окна, когда титр исчезает.
    pub end: f64,
    /// Текст титра.
    pub text: String,
}

/// Режет сегменты на титры: сегмент со словами — по их таймкодам, без слов —
/// поровну по времени.
pub fn caption_lines(segments: &[Segment], rules: &LineRules) -> Vec<CaptionLine> {
    let mut lines = Vec::new();
    for segment in segments {
        let words = segment_words(segment);
        lines.extend(group_words(&words, rules));
    }
    stretch_short(&mut lines, rules);
    lines
}

/// Слова сегмента. Без своих слов режем текст на слова и раздаём им время
/// сегмента поровну — так работает модель без покадровых таймкодов.
fn segment_words(segment: &Segment) -> Vec<Word> {
    if !segment.words.is_empty() {
        return segment.words.clone();
    }
    let tokens: Vec<&str> = segment.text.split_whitespace().collect();
    let count = tokens.len();
    if count == 0 {
        return Vec::new();
    }
    let step = (segment.end - segment.start).max(0.0) / count as f64;
    tokens
        .into_iter()
        .enumerate()
        .map(|(index, text)| Word {
            start: segment.start + index as f64 * step,
            end: segment.start + (index + 1) as f64 * step,
            text: text.to_owned(),
        })
        .collect()
}

/// Собирает слова в титры, рвя перед словом по правилам.
fn group_words(words: &[Word], rules: &LineRules) -> Vec<CaptionLine> {
    let mut lines = Vec::new();
    let mut current: Vec<&Word> = Vec::new();
    for word in words {
        if breaks_before(&current, word, rules) {
            lines.push(build_line(&current));
            current.clear();
        }
        current.push(word);
    }
    if !current.is_empty() {
        lines.push(build_line(&current));
    }
    lines
}

/// Пора ли начать новый титр перед словом `next`.
fn breaks_before(current: &[&Word], next: &Word, rules: &LineRules) -> bool {
    let Some(last) = current.last() else {
        return false;
    };
    let chars = line_chars(current) + 1 + next.text.trim().chars().count();
    if chars > rules.max_chars {
        return true;
    }
    if next.end - current[0].start > rules.max_seconds {
        return true;
    }
    if ends_sentence(&last.text) {
        return true;
    }
    if ends_comma(&last.text) && line_chars(current) > rules.comma_after_chars {
        return true;
    }
    next.start - last.end >= rules.pause_seconds
}

/// Строит титр из слов: текст через пробел, начало первого и конец последнего.
fn build_line(words: &[&Word]) -> CaptionLine {
    let start = words.first().map_or(0.0, |word| word.start);
    let end = words.last().map_or(start, |word| word.end);
    let text = words
        .iter()
        .map(|word| word.text.trim())
        .collect::<Vec<_>>()
        .join(" ");
    CaptionLine { start, end, text }
}

/// Длина строки в символах, как её увидит зритель: слова и пробелы между ними.
fn line_chars(words: &[&Word]) -> usize {
    let chars: usize = words
        .iter()
        .map(|word| word.text.trim().chars().count())
        .sum();
    chars + words.len().saturating_sub(1)
}

/// Точка, восклицание, вопрос или многоточие в конце слова.
fn ends_sentence(text: &str) -> bool {
    ends_with(text, |ch| matches!(ch, '.' | '!' | '?' | '…'))
}

/// Запятая в конце слова.
fn ends_comma(text: &str) -> bool {
    ends_with(text, |ch| ch == ',')
}

/// Последний значимый символ слова: закрывающие кавычки и скобки не считаем.
fn ends_with(text: &str, test: impl Fn(char) -> bool) -> bool {
    text.trim_end_matches(['"', '\'', '»', '”', ')', ']', '}'])
        .chars()
        .last()
        .is_some_and(test)
}

/// Растягивает слишком короткие титры до минимума, но не дальше начала
/// следующего.
fn stretch_short(lines: &mut [CaptionLine], rules: &LineRules) {
    for index in 0..lines.len() {
        let start = lines[index].start;
        if lines[index].end - start >= rules.min_seconds {
            continue;
        }
        let min_end = start + rules.min_seconds;
        let limit = lines
            .get(index + 1)
            .map_or(min_end, |next| next.start.min(min_end));
        lines[index].end = limit.max(lines[index].end);
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptionLine, LineRules, caption_lines};
    use concat_speech::transcribe::{Segment, Word};

    /// Одно слово с таймкодами.
    fn word(text: &str, start: f64, end: f64) -> Word {
        Word {
            start,
            end,
            text: text.to_owned(),
        }
    }

    /// Сегмент с заданными словами.
    fn segment(text: &str, start: f64, end: f64, words: Vec<Word>) -> Segment {
        Segment {
            start,
            end,
            text: text.to_owned(),
            words,
        }
    }

    /// Тексты титров, для сравнения.
    fn texts(lines: &[CaptionLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    /// Фраза из тридцати слов: титров несколько, каждый не длиннее 32 символов.
    #[test]
    fn a_long_phrase_becomes_several_short_captions() {
        let owned: Vec<String> = (0..30).map(|index| format!("word{index}")).collect();
        let words: Vec<Word> = owned
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let at = index as f64 * 0.3;
                word(text, at, at + 0.25)
            })
            .collect();
        let lines = caption_lines(
            &[segment(&owned.join(" "), 0.0, 9.0, words)],
            &LineRules::default(),
        );
        assert!(lines.len() > 1, "{lines:?}");
        assert!(lines.iter().all(|line| line.text.chars().count() <= 32));
        assert!(lines.iter().all(|line| !line.text.is_empty()));
    }

    /// Пауза между словами начинает новый титр.
    #[test]
    fn a_pause_starts_a_new_caption() {
        let lines = caption_lines(
            &[segment(
                "hello there world",
                0.0,
                3.0,
                vec![
                    word("hello", 0.0, 0.5),
                    word("there", 0.55, 0.9),
                    word("world", 2.0, 2.4),
                ],
            )],
            &LineRules::default(),
        );
        assert_eq!(texts(&lines), ["hello there", "world"]);
    }

    /// Точка в конце слова заканчивает титр.
    #[test]
    fn a_sentence_ends_the_caption() {
        let lines = caption_lines(
            &[segment(
                "hello world. next one",
                0.0,
                2.0,
                vec![
                    word("hello", 0.0, 0.3),
                    word("world.", 0.35, 0.7),
                    word("next", 0.75, 1.0),
                    word("one", 1.05, 1.3),
                ],
            )],
            &LineRules::default(),
        );
        assert_eq!(texts(&lines), ["hello world.", "next one"]);
    }

    /// Слишком короткий титр растягивается до минимума в 0,6 с.
    #[test]
    fn a_short_caption_is_held_for_the_minimum() {
        let lines = caption_lines(
            &[segment(
                "hi there",
                0.0,
                2.0,
                vec![word("hi", 0.0, 0.1), word("there", 0.12, 0.2)],
            )],
            &LineRules::default(),
        );
        assert_eq!(lines.len(), 1);
        assert!(
            (lines[0].end - lines[0].start - 0.6).abs() < 1e-9,
            "{:?}",
            lines[0]
        );
    }

    /// Короткий титр растягивается не дальше начала следующего.
    #[test]
    fn a_short_caption_never_reaches_into_the_next() {
        let lines = caption_lines(
            &[segment(
                "oh no. stop",
                0.0,
                2.0,
                vec![
                    word("oh", 0.0, 0.05),
                    word("no.", 0.06, 0.12),
                    word("stop", 0.5, 0.9),
                ],
            )],
            &LineRules::default(),
        );
        assert_eq!(texts(&lines), ["oh no.", "stop"]);
        assert!((lines[0].end - 0.5).abs() < 1e-9, "{:?}", lines[0]);
        assert!(lines[0].end <= lines[1].start);
    }

    /// Сегмент без слов режется по времени, а текст сохраняется по словам.
    #[test]
    fn a_segment_without_words_is_cut_evenly() {
        let lines = caption_lines(
            &[segment(
                "one two three four five six seven eight nine ten",
                0.0,
                4.0,
                Vec::new(),
            )],
            &LineRules::default(),
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| line.text.chars().count() <= 32));
        let joined: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(
            joined.join(" "),
            "one two three four five six seven eight nine ten"
        );
    }

    /// Кириллица считается по символам, а не по байтам: границей рвём ровно
    /// там, где кончаются 32 символа.
    #[test]
    fn cyrillic_is_measured_in_characters() {
        let lines = caption_lines(
            &[segment(
                "Привет мир как дела сегодня хорошо",
                0.0,
                2.0,
                vec![
                    word("Привет", 0.0, 0.3),
                    word("мир", 0.35, 0.6),
                    word("как", 0.65, 0.9),
                    word("дела", 0.95, 1.2),
                    word("сегодня", 1.25, 1.6),
                    word("хорошо", 1.65, 2.0),
                ],
            )],
            &LineRules::default(),
        );
        assert_eq!(texts(&lines), ["Привет мир как дела сегодня", "хорошо"]);
    }

    /// Ведущие пробелы и лишние пробелы в словах нормализуются.
    #[test]
    fn leading_spaces_in_words_are_normalized() {
        let lines = caption_lines(
            &[segment(
                " Hello  world.",
                0.0,
                1.0,
                vec![word(" Hello", 0.0, 0.4), word("  world.", 0.5, 0.9)],
            )],
            &LineRules::default(),
        );
        assert_eq!(texts(&lines), ["Hello world."]);
    }
}
