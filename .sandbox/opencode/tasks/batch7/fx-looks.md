
# Задача: пакеты hue-shift, rgb-glitch, light-leak, lens-flare, mirror-tile

Пять пакетов для эдитов, вернувшихся в новом формате.

## Разрешённые файлы
Только новые папки `src/crates/concat-effects/packages/cutless.hue-shift/`, `cutless.rgb-glitch/`,
`cutless.light-leak/`, `cutless.lens-flare/`, `cutless.mirror-tile/` и файл `locales-fx-looks.json` в корне worktree.

## 1. `cutless.hue-shift` — «Hue Shift», category Color
- `angle` -180..180, default 0, step 1, unit "°" — поворот оттенка.
- Поворот оттенка, который сохраняет яркость, в линейном свете: вращение вокруг серой оси (матрица Родрига вокруг
  `normalize(vec3(1))` по `radians(angle)`), без обрезки. Если в `shader.rs` для format 2 есть готовая функция
  поворота оттенка в нужном пространстве — используй её и напиши об этом в отчёте.
- `[[replaces]] id = "concat.hue-shift"`: старый `angle` (-180..180, default 60) → `angle = angle`.

## 2. `cutless.rgb-glitch` — «RGB Glitch», category Glitch
Цифровой сбой: горизонтальные полосы сдвигаются, каналы расходятся.
- `amount` 0..100, default 50, step 1, unit "%"; `frequency` 1..30, default 8, step 1, unit "Hz" — как часто меняется
  рисунок сбоя; `bands` 4..64, default 16, step 1 — сколько полос по высоте.
- Сдвиг полосы: `hash(vec2(floor(uv.y*bands), floor(frame.time*frequency)), …)` → сдвиг до `amount/100 * 8 %` ширины,
  только у части полос (порог по второму хэшу). Плюс разъезд R и B на сдвиг/2. Координаты через `mirror()`.
  При amount = 0 — исходная картинка.
- `[[replaces]] id = "concat.rgb-glitch"`: старые `amount` (0..100, default 50), `frequency` (1..30, default 8) →
  те же; `bands = "16"`.

## 3. `cutless.light-leak` — «Light Leak», category Light
Тёплый засвет плёнки, медленно плывёт.
- `amount` 0..100, default 60, step 1, unit "%"; `warmth` 0..100, default 70, step 1, unit "%" (0 — розовый,
  100 — оранжевый); `speed` 0..10, default 1, step 0.1 — скорость дрейфа (0 — стоит).
- Свет **добавляется** (`c.rgb + leak`), большое мягкое пятно у края кадра, движется по `frame.time*speed`. Без обрезки.
  При amount = 0 — тождество.
- `[[replaces]] id = "concat.light-leak"`: старый `amount` (0..100, default 60) → `amount = amount`,
  `warmth = "70"`, `speed = "1"`.

## 4. `cutless.lens-flare` — «Lens Flare», category Light
- `amount` 0..100, default 70, step 1, unit "%"; `x` 0..100, default 70, unit "%"; `y` 0..100, default 30, unit "%"
  (центр блика в кадре); `size` 10..200, default 100, step 1, unit "%".
- Добавляемый свет: яркое ядро (гаусс), горизонтальный луч (вытянутый гаусс) и 2–3 слабых «призрака» на линии через
  центр кадра. Без обрезки. При amount = 0 — тождество.
- `[[replaces]] id = "concat.lens-flare"`: старые `posx` (0..100, default 70), `posy` (0..100, default 30),
  `amount` (0..100, default 70) → `x = posx`, `y = posy`, `amount = amount`, `size = "100"`.

## 5. `cutless.mirror-tile` — «Mirror Tile», category Distort
- `tiles` 1..6, default 2, step 1 — плиток по каждой оси (дробное между целыми тоже должно выглядеть плавно:
  ключами «наезжает»). При 1 — исходная картинка.
- Координата `uv * tiles`, каждая вторая плитка отражена (`mirror()` над `uv * tiles` даёт это само).
- `[[replaces]] id = "concat.mirror-tile"`: старый `tiles` (2..4, default 2) → `tiles = tiles`.

## Пробы
По правилам `_common.md`. Ровная картинка остаётся собой у rgb-glitch и mirror-tile при любых ручках, у hue-shift —
у серого входа. Для light-leak и lens-flare на default центр кадра может быть освещён: либо посчитай `expect`
точно для центрального пикселя при t = 0, либо используй пробы с `params = { amount = 0 }` и серым входом, а для
`at = "max"`/`"min"`/default подбери вход и посчитай честно (это обязательные пробы — без них тест упадёт).
