# MOTION — эффекты для эдитов: что сделано по пунктам 1–6
> Быстрые победы из `EDIT-FX.md`. Last updated: 2026-10-16 (пункты 1, 2, 3, 5, 6 в коде; пункт 4 — без окна).

Порядок владельца: 1 → 2 → 3 → 5 → 4 → 6. Перед кодом проект прошёл аудит трёх агентов, после — ревью трёх.
Решения аудита и ревью вписаны ниже; грабли — `LEARNINGS.md` → Ключи и удары.

## Пункт 1. Ключи у «ударных» параметров

`animate = true`: glow, bloom-pulse, chromatic-aberration, motion-blur (length, angle), gaussian-blur, vignette,
fisheye, pixelate, film-grain. Рендер играет ключи любого параметра (`AppliedFilter::params_at`); флаг включает
ромбики в инспекторе. У glow `min` снижен с 10 до 0 — покой для удара (проба минимума стала тождеством). → апстрим.

## Пункт 3 и 5. Новые пакеты `cutless.*` (пачка 7, исполнители DeepSeek)

| Пакет | Ручки | Заменяет старый |
|---|---|---|
| `cutless.shake` | amount % кадра, speed, rotation ° (качается) | `concat.shake`, `concat.camera-shake` |
| `cutless.rgb-split` | shift % ширины, angle | `concat.rgb-split` |
| `cutless.invert` | amount % (блики выше белого гаснут) | `concat.invert` |
| `cutless.flash` | amount (покой 0, свет добавляется), frequency (0 — без строба) | `concat.strobe-flash` |
| `cutless.hue-shift` | angle (поворот вокруг серой оси, линейный свет) | `concat.hue-shift` |
| `cutless.rgb-glitch` | amount, frequency, bands | `concat.rgb-glitch` |
| `cutless.light-leak` | amount, warmth, speed | `concat.light-leak` |
| `cutless.lens-flare` | amount, x, y, size | `concat.lens-flare` |
| `cutless.mirror-tile` | tiles (дробное — плавно) | `concat.mirror-tile` |
| `cutless.camera` | zoom 100–400 %, x/y −50…50 % кадра, rotation ±180, tilt ±30 (перспектива), blur % ширины, blur_angle, edges (enum, без ключей) | — |

Все в линейном свете, края — зеркалом, `sample()` до ветвлений, пробы на GPU зелёные (`every_package_probe_holds`
с `CONCAT_REQUIRE_GPU=1`). Строки — в 25 локалях через `.sandbox/opencode/merge-locales.py`. Переводы на 23 языка
(кроме ru/en) никто не вычитывал.

## Команда `SetKeySpan` (`concat-project/src/commands/keyspans.rs`)

Заменяет ключи свойства клипа или параметра эффекта на отрезке `from..=to` готовым списком. Нужна, потому что
`SetClipKey`/`SetEffectKey` склеивают ключи ближе `KEY_EPSILON` = 0,002 доли клипа (на 60 с — 3,6 кадра).
`clip_keys` и `effect_keys` тоже пишут через неё (отрезок в одну точку). Нормализует через `Clip::tidy`. → апстрим.

## Крейт `cutless-motion` (чистые функции, зависит только от `concat-project`)

- `speed::SpeedPreset` — velocity, montage, hero, bullet, jump_cut, flash_in, flash_out, ramp_to_hit.
  `ramp_hit_for_share` — где поставить пик рампы, чтобы на него пришлась доля пройденного исходника.
- `hits::commands(kind, &Hits)` — удары punch, shake, flash, bw, rgb, glow, blur. Как устроено:
  - моменты прижимаются к сетке кадров и внутрь клипа; окно — кадр до удара и `3·decay` после;
  - близкие удары сливаются в одно окно: огибающая — максимум импульсов, тряска — сумма;
  - импульс `exp(-d/(decay/2.3))`, тряска `sin(55.2·t+h)`, `0.7·cos(45.6·t+h)` (донор econcar в секундах);
  - база — значение в снимке до удара (`Clip::value_at`, у эффекта — умолчание пакета из каталога);
  - тряска клипа увеличивает масштаб на `1 + 2·max|сдвиг|`, чтобы не открыть край;
  - на слое-обработке наезд и тряска двигают `cutless.camera` (у слоя нет трансформа);
  - нет эффекта в цепочке — добавляется в покое (`UpdateClip`) и качается тем же `Batch`.
- Сила 0.1…3 (дроп = 2), `decay` в секундах — кадр…2 с, по умолчанию кадры донора при 24 fps.

| Вид | Клип | Слой |
|---|---|---|
| punch | `Scale` ×(1 + 0.08·сила) | `cutless.camera` zoom |
| shake | `OffsetX/Y` ± `Scale` | камера x, y, zoom |
| flash | `cutless.flash` amount → 100 | то же |
| bw | `concat.saturation` amount → −100 | то же |
| rgb | `cutless.rgb-split` shift +0.8 | то же |
| glow | `concat.glow` amount +60 | то же |
| blur | `concat.zoom-blur` amount +35 | то же |

## Пункт 2 и 6. Инструменты агента (`cutless-tools/src/motion/`)

`clip_keys`, `effect_keys` (только float/int), `clip_speed` (speed / preset / points / constant; `hit_source` —
секунда файла для пика рампы), `layer_add` (параметры в одной команде, `AddLayerClip.params`), `transition_set`,
`clip_look` (opacity, blend, flip), `hit_fx`. Время — секунды таймлайна. Всего инструментов — 45.
`effects_catalogue` при ≤ 12 найденных показывает `key(min..max=default, keys)`. `timeline_summary` показывает
ключи с окнами (`scale×32@00:04.000-00:05.033`), `speed-curve`, `transition-in`, ключи эффектов.
Инструкция сервера (`cutless-mcp/src/protocol.rs`) описывает порядок работы над эдитом.

## Что не сделано

- **Пункт 4, интерфейс:** чипы пресетов скорости в `SpeedPanel` окна — следующий шаг (пресеты для агента готовы).
- Кнопка «На удар» для человека — нет; генераторы готовы в `cutless-motion` для окна.
- Сетки долей нет (Ф5.2): моменты ударов агент берёт у человека или из транскрипта.
- Ключи хранятся долями клипа: после смены скорости удары растягиваются вместе с клипом — порядок «сначала скорость,
  потом удары» записан в описаниях инструментов.
- Первый ключ отрезка всегда линейный: участок от прежнего ключа пользователя слегка меняет форму.
