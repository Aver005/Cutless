
# Задача: пакеты-удары shake, rgb-split, invert, flash

Четыре пакета, главные кирпичи эдитов: на них агент и кнопки «На удар» будут ставить ключи по долям музыки.

## Разрешённые файлы
Только новые папки `src/crates/concat-effects/packages/cutless.shake/`, `cutless.rgb-split/`, `cutless.invert/`,
`cutless.flash/` и файл `locales-fx-motion.json` в корне worktree.

## 1. `cutless.shake` — «Shake», category Distort
Дрожание камеры, цикл по `frame.time`.
- `amount` 0..10, default 2, step 0.1, unit "%" — размах сдвига в % кадра (по каждой оси от своего размера).
- `speed` 1..30, default 12, step 1 — насколько часто дрожит.
- `rotation` 0..10, default 0, step 0.1, unit "°" — покачивание поворотом.
- Сдвиг — сумма двух синусов с разной частотой и фазой по каждой оси (не один синус: так дрожь живая), например
  `dx = a*(0.6*sin(t*speed*2.3) + 0.4*sin(t*speed*3.7 + 1.3))`, `dy` — другие частоты (1.9, 3.1) и множитель 0.7.
- Чтобы край кадра не открывался, картинка увеличивается на `1 + 2*amount/100` вокруг центра; координату всё равно
  отражай `mirror()`. При amount = 0 и rotation = 0 — ровно исходная картинка.
- `[[replaces]] id = "concat.shake"`: старые `amount` (px, 2..40, default 12), `speed` (2..30, default 13) →
  `amount = amount / 10.8`, `speed = speed`.
- `[[replaces]] id = "concat.camera-shake"`: старые `amplitude` (1..50, default 15), `speed` (2..30, default 10),
  `rotation` (0..10, default 2) → `amount = amplitude / 10.8`, `speed = speed`, `rotation = rotation`.

## 2. `cutless.rgb-split` — «RGB Split», category Glitch
Разъезд цветовых каналов.
- `shift` 0..3, default 0.6, step 0.05, unit "%" — сдвиг каналов в % ширины кадра.
- `angle` -180..180, default 0, step 1, unit "°" — направление разъезда.
- R читается со сдвигом `+dir*shift`, B — `-dir*shift`, G и альфа — без сдвига; координаты через `mirror()`.
  Читай через `sample_premultiplied` (без ветвлений) и дели на альфу, либо `sample()` трижды до любых `if`.
- `[[replaces]] id = "concat.rgb-split"`: старый `shift` (px, 1..40, default 10) → `shift = shift / 19.2`.

## 3. `cutless.invert` — «Invert», category Color
- `amount` 0..100, default 100, step 1, unit "%" — сила инверсии (ключами — мгновенная инверсия на удар).
- Линейный свет: сначала `c' = min(c.rgb, vec3(1.0))` (блики выше белого становятся белым), инверсия `1 - c'`,
  результат `mix(c.rgb, 1 - c', amount/100)`. Альфа исходная. В `description` честно: блики при инверсии гаснут.
- `[[replaces]] id = "concat.invert"`: ручек не было, `defaults = {}`, `knobs = { amount = "100" }`.

## 4. `cutless.flash` — «Flash», category Light
Белая вспышка — главный акцент дропа.
- `amount` 0..100, default 0, step 1, unit "%" — сила вспышки. По умолчанию 0: эффект в цепочке ничего не делает,
  пока ключи не поднимут amount. `[card] params = { amount = 80 }` — чтобы карточка в каталоге показывала вспышку.
- `frequency` 0..30, default 0, step 0.5, unit "Hz" — стробоскоп: 0 = ровная вспышка, иначе amount включается
  на половину каждого периода (`step(0.5, fract(frame.time * frequency))`; при 0 множитель = 1, без деления на 0).
- Свет **добавляется**, а не смешивается: `out = c.rgb + vec3(a)`, где `a = amount/100 * 1.5` (на максимуме серый
  0.18 становится 1.68 — белее белого, блик 4 остаётся ярче). Альфа исходная.
- `[[replaces]] id = "concat.strobe-flash"`: старые `strength` (0..100, default 80), `frequency` (2..30, default 10)
  → `amount = strength`, `frequency = frequency`.

## Пробы
По правилам `_common.md`. У shake, rgb-split ровная картинка остаётся собой при любых ручках. У invert посчитай:
серый 0.18 при 100 % → 0.82; блик [4,1,0.05] → [0,0,0.95]. У flash default (0) — тождество; `params = { amount = 100 }`
серый → 0.18 + 1.5 = 1.68.
