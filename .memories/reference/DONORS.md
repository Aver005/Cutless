# DONORS — что и откуда переносим
> Проекты-доноры идей для Cutless. Last updated: 2026-10-16.

Код доноров напрямую не переносим: другой язык (ki-video на TS), другая модель (Remotion в econcar),
другая архитектура (pooprusteek только бинарный). Переносим **идеи, алгоритмы и решения,
проверенные на практике**. Полный разбор доноров — `reference/RESEARCH-2026-10-16.md`.

## ki-video — `E:\Projects\Me\ki-video`

Редактор вертикальных роликов, Bun + React + ffmpeg, техно-демо от 7–8 сентября 2026. База знаний —
`ki-video/.memories/` (особенно `DECISIONS.md`).

| Что | Где у донора | Куда в Cutless | Как переносить |
|---|---|---|---|
| Математика окна кадра: `baseWindow`, `frameToRegion`, `clampFrame`. Окно прижимается к краям, а если шире исходника — центрируется | → `ki-video/src/core/frame.ts` | Ф5.1: утилита в своём крейте «центр + зум → scale + offset клипа» | Переписать на Rust вместе с тестами (`ki-video/tests/frame.test.ts`, 16 тестов как эталон) |
| Жесты рефрейма: тянешь по превью — центр, колесо — зум, `K` — ключ | → `ki-video/src/widgets/preview/ui/Preview.tsx` | Ф5.1: режим в превью окна | Как UX-сценарий |
| `setFrame`: без ключей правит неподвижное значение, с ключами — ключ в текущий момент или новый | → `ki-video/src/entities/project/model/frame.ts` | Ф5.1: логика поверх `SetClipKey`/`SetClipTransform` | Как правило |
| Серия прокрутки или перетаскивание — один шаг undo | → `ki-video/src/entities/project/model/session.ts` | Уже есть в Concat (`apply_within`/`end_gesture`) | Ничего не переносить |
| «Быстрый ТикТок-пресет» | → `ki-video/src/widgets/inspector/ui/QuickPreset.tsx` | Ф5.7: `Batch` из готовых команд | Как набор шагов |
| Звуковые пресеты «Голос чисто», «Игра громко»: highpass, afftdn, acompressor, loudnorm | → `ki-video/src/core/presets.ts` | Ф5.6: цепочки из аудиопакетов Concat | Как значения параметров |
| Пресеты цвета (6 штук) | → `ki-video/src/core/presets.ts` | Ф5: пресеты поверх фильтров Concat | Как значения |
| Импорт SRT и строк «старт конец текст», стили субтитров | → `ki-video/src/core/subtitles.ts` | Ф5.5 | Переписать разбор на Rust, тесты как эталон |

**Не переносим:** плеер превью (у Concat GPU-композитор лучше), граф ffmpeg для экспорта (у Concat
свой конвейер), стор, интерфейс на shadcn.

## econcar и навык promo-reel — монтаж агентом на Remotion

`E:\Projects\VibeCuts\mdpanf\econcar` (пример работы DeepSeek Flash V4.1),
`E:\Projects\VibeCuts\me\diary` (второй пример), методика — `C:\Users\Aver\.claude\skills\promo-reel\`
(`SKILL.md` и `references/{music-sync,capture,remotion-kit,styles,style-packs,copy-and-critics}.md`).

| Что | Где у донора | Куда в Cutless |
|---|---|---|
| Сетка долей: подбор bpm и фазы по онсетам, удары по полосам (бочка 25–150 Гц, малый 1.2–6 кГц, общий), сила и отклонение от сетки | → `econcar/tools/audio.py` | Ф5.2 `cutless-beats` |
| Сборка трека операциями `seg`, `sil`, `tape`, `fade` с кроссфейдом 10 мс | → `econcar/tools/audio.py` (`build`) | Ф5.9 |
| Сюжет на сетке: сцены по долям, `beatsIn(scene)`, `kicks`, `drop`, `snap()` | → `econcar/src/lib/story.ts` | Ф5.2 привязка событий к долям, Ф6 рецепты |
| Время от секунд с округлением, без накопления ошибки (`sec(s)=round(s*fps)`) | → `econcar/src/lib/grid.ts` | Правило для `cutless-beats` |
| `pulse(frame, hits, decay)` — импульс на ударе для любого свойства | → `econcar/src/lib/grid.ts` | Ф5.4 |
| Анимации текста: `Slam` (слова по очереди с шагом), `fitText`, `Typed`, `Scramble`, `Count` | → `econcar/src/lib/type.tsx`, `fx.tsx` | Ф5.3 пакеты анимаций |
| Тряска, вспышка, шторки, зерно, RGB-сдвиг | → `econcar/src/lib/fx.tsx`, `bits.tsx` | Ф5.4 и пакеты эффектов (часть уже есть в Concat) |
| Громкость кривой (тише до удара), приглушение под эффекты (`duck`) | → `econcar/src/lib/mix.tsx` | Ф5.6 |
| Мастеринг: двухпроходный `loudnorm` до −14 LUFS, поправка задержки AAC (−42.667 мс) | → `econcar/tools/master.sh` | Ф5.6, экспорт |
| Контакт-лист кадров для проверки (`remotion still` + `xstack` 4×2) | → `econcar/tools/stills.sh`, `sheet.sh` | Ф2.7 `contact_sheet` |
| Безопасные зоны TikTok (сверху ~200, снизу ~420, справа ~140 px на 1080×1920), лимит 12–15 символов/с, заполненный кадр 0 | навык `promo-reel` | Ф2.8 `lint`, Ф5.7 |
| Пакеты стилей Reglament и NightShift: тема + сюжетный конфиг (`headers[]`, `media[]`, `chips[]`, `cta`) → сцены | → `promo-reel/assets/packs/src/packs/{reglament,nightshift}/` | Ф6.5 |
| Критики-субагенты: моушн + маркетинг, синхронизация с музыкой, тексты + факты | навык `promo-reel`, `references/copy-and-critics.md` | `[IDEA]` встроенная проверка перед экспортом |
| Раздел «Грабли» в README каждого ролика | `econcar/README.md` | Привычка: `LEARNINGS.md` |

**Чего у доноров нет и не будет:** работы с живым видео как с материалом. Её даёт Concat, и это
главная причина, почему Cutless сильнее связки «Remotion + агент».

## pooprusteek — `E:\Projects\Me\pooprusteek`

Терминальный агент на Rust поверх DeepSeek Web, MIT, база знаний — `pooprusteek/.memories/`.

| Что | Где у донора | Куда в Cutless |
|---|---|---|
| Разбор вызовов инструментов во всех известных форматах (DSML DeepSeek, Hermes/Qwen, Claude-подобный, Kimi, Gemma…), уровни доверия, защита от эха | → `pooprusteek/src/agent/tool_parser/` | Ф4.3. Порт или общий крейт |
| Продолжение оборванного вызова на лимите длины | → `pooprusteek/src/agent/continuation.rs` | Ф4.1 |
| Провайдеры OpenAI-, Anthropic-, Gemini-совместимые | → `pooprusteek/src/provider/{openai_compat,anthropic_compat,gemini_compat}.rs` | Ф4.2 (у нас без tokio) |
| Отложенные схемы инструментов и `tool_search`, когда инструментов больше ~12 | → `pooprusteek/src/semantic/`, `.memories/reference/MCP.md` | Ф4.4, если инструментов станет много |
| Политика подтверждений и белые списки | → `pooprusteek/src/tools/`, `src/app/interaction.rs` | Ф4.6 |
| Поведенческий харнесс: сценарии, трассы JSONL, поддельный провайдер | → `pooprusteek/src/harness/`, `.memories/reference/HARNESS.md` | Ф4.9 тесты агента |
| Хранение ключей в системном хранилище (`keyring`) | `pooprusteek` | Ф4.7 (Р9) |

**Ограничения pooprusteek для Cutless** (`DECISIONS.md`):
- крейт только бинарный;
- `fastembed` тянет `ort` rc.12, а у Concat `ort =rc.13`;
- MCP-клиент сводит картинки к строке;
- у DeepSeek Web нет зрения;
- `--acp` — заглушка.
