// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Cutless contributors
struct Params {
    zoom: f32,
    x: f32,
    y: f32,
    rotation: f32,
    tilt: f32,
    blur: f32,
    blur_angle: f32,
    edges: f32,
}

// Виртуальная камера над слоем: наезд, сдвиг, поворот, наклон и смаз движения.
// Обратное отображение: для каждой точки кадра ищем, откуда взять пиксель.
// Работаем в линейном свете, ярче 1.0 не режем, альфу берём у исходного пикселя.

/// Отражение координаты за краем: картинка складывается зеркалом.
fn mirror(q: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(q * 0.5) * 2.0);
}

/// Что за краем кадра: 0 — отражение, иначе растянутый край.
fn wrapped(q: vec2<f32>) -> vec2<f32> {
    let clamped = clamp(q, vec2<f32>(0.0), vec2<f32>(1.0));
    return select(clamped, mirror(q), round(params.edges) < 0.5);
}

/// Откуда взять пиксель выхода `uv`: сдвиг, поворот, наезд и наклон.
fn camera_at(uv: vec2<f32>) -> vec2<f32> {
    let aspect = frame.size.x / frame.size.y;
    // Сдвиг в долях кадра: +x — вправо, +y — вниз.
    var p = uv - vec2<f32>(0.5) - vec2<f32>(params.x, params.y) / 100.0;
    // Квадратные единицы, чтобы поворот не плющил кадр в широком окне.
    p.x *= aspect;
    // Обратный поворот: картинка идёт по часовой на +rotation.
    let a = radians(-params.rotation);
    p = vec2<f32>(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a));
    // Наезд: делим, zoom = 200 показывает половину кадра.
    p /= max(params.zoom / 100.0, 1e-3);
    // Наклон плоскости вокруг горизонтали, фокус 1.5 высоты кадра. Угол взят
    // со знаком минус, чтобы плюс tilt уводил верх от зрителя при y вниз.
    let t = radians(-params.tilt);
    let f = 1.5;
    let y = p.y * f / max(f * cos(t) - p.y * sin(t), 1e-3);
    let x = p.x * (f + y * sin(t)) / f;
    p = vec2<f32>(x, y);
    p.x /= aspect;
    return p + vec2<f32>(0.5);
}

/// Смаз движения: отсчёты вдоль `blur_angle` по отражённой координате.
fn smear(q: vec2<f32>) -> vec4<f32> {
    let len = params.blur * 0.01 * frame.size.x;
    let n = clamp(i32(ceil(len * 0.5)), 1, 48);
    let a = radians(params.blur_angle);
    // Отсчёт в два пикселя, угол против часовой от горизонта.
    let step = vec2<f32>(cos(a), -sin(a)) * 2.0 / frame.size;
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let d = f32(i) - f32(n - 1) * 0.5;
        sum += sample_premultiplied(wrapped(q + step * d));
    }
    return sum / f32(n);
}

fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = smear(camera_at(uv));
    return vec4<f32>(c.rgb / max(c.a, 1e-6), c.a);
}
