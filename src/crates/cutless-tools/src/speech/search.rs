// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors

//! Поиск фразы по транскрипту: без учёта регистра, ё=е, без пунктуации,
//! плюс нечёткое совпадение по доле слов запроса.

/// Доля совпавших слов запроса, начиная с которой фраза считается найденной.
pub const FUZZY_MIN: f64 = 0.6;

/// Как фраза нашлась в тексте.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Match {
    /// Запрос целиком лежит подстрокой в тексте.
    Exact,
    /// Совпала достаточная доля слов запроса.
    Fuzzy,
}

impl Match {
    /// Метка для ответа инструмента.
    pub fn as_str(self) -> &'static str {
        match self {
            Match::Exact => "exact",
            Match::Fuzzy => "fuzzy",
        }
    }
}

/// Приводит текст к сравнимому виду: нижний регистр, ё→е, без пунктуации,
/// пробелы одиночные.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        let ch = if matches!(ch, 'ё' | 'Ё') { 'е' } else { ch };
        if ch.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            out.extend(ch.to_lowercase());
            space = false;
        } else {
            space = true;
        }
    }
    out
}

/// Находит ли `text` запрос `query`: точной подстрокой или нечётко.
pub fn matches(text: &str, query: &str) -> Option<Match> {
    let query = normalize(query);
    if query.is_empty() {
        return None;
    }
    let text = normalize(text);
    if text.contains(&query) {
        return Some(Match::Exact);
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let wanted: Vec<&str> = query.split_whitespace().collect();
    let found = wanted.iter().filter(|word| words.contains(word)).count();
    let ratio = found as f64 / wanted.len() as f64;
    (ratio >= FUZZY_MIN).then_some(Match::Fuzzy)
}

#[cfg(test)]
mod tests {
    use super::{Match, matches, normalize};

    /// Нормализация: регистр, ё=е, пунктуация прочь, пробелы одиночные.
    #[test]
    fn normalization_flattens_case_yo_and_punctuation() {
        assert_eq!(normalize("Привет, Ёлка!"), "привет елка");
        assert_eq!(normalize("  a   b  "), "a b");
        assert_eq!(normalize("e.g. dot"), "e g dot");
        assert_eq!(normalize(""), "");
    }

    /// Точная подстрока находится независимо от пунктуации и регистра.
    #[test]
    fn an_exact_phrase_is_found_through_punctuation() {
        assert_eq!(
            matches("И вот, наконец, про деньги!", "про деньги"),
            Some(Match::Exact)
        );
        assert_eq!(matches("ЁЖИК", "ежик"), Some(Match::Exact));
    }

    /// Нечёткий поиск: шесть из десяти слов запроса дают совпадение.
    #[test]
    fn a_phrase_is_found_when_most_of_its_words_are_there() {
        // 3 of 4 words: 0.75 ≥ 0.6.
        assert_eq!(
            matches(
                "мы говорили про деньги и планы",
                "говорили деньги планы что-то"
            ),
            Some(Match::Fuzzy)
        );
        // 2 of 4: 0.5 < 0.6.
        assert_eq!(
            matches(
                "мы говорили про деньги и планы",
                "говорили деньги чужие сроки"
            ),
            None
        );
    }

    /// Пустой запрос ничего не находит.
    #[test]
    fn an_empty_query_matches_nothing() {
        assert_eq!(matches("что угодно", "   "), None);
    }
}
