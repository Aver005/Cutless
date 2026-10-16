struct Params { amount: f32, speed: f32, rotation: f32 }

/// Отражение координаты зеркалом: за краем кадра не будет прозрачного пикселя.
fn mirror(q: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(q * 0.5) * 2.0);
}

/// Дрожание камеры: две синусоиды на ось, лёгкий поворот и зум, чтобы не открылся край.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let t = frame.time;
    let a = params.amount / 100.0;
    let dx = a * (0.6 * sin(t * params.speed * 2.3) + 0.4 * sin(t * params.speed * 3.7 + 1.3));
    let dy = 0.7 * a * (0.6 * sin(t * params.speed * 1.9 + 0.7) + 0.4 * sin(t * params.speed * 3.1 + 2.1));
    let zoom = 1.0 + 2.0 * params.amount / 100.0;
    // Покачивание: угол колеблется вместе с дрожью, а не стоит на месте.
    let angle = radians(params.rotation) * sin(t * params.speed * 1.7 + 0.4);
    let aspect = frame.size.x / frame.size.y;
    // Поворот в квадратных единицах, чтобы не плющить неквадратный кадр.
    let p = (uv - vec2<f32>(0.5)) / zoom * vec2<f32>(aspect, 1.0);
    let c = cos(angle);
    let s = sin(angle);
    let turned = vec2<f32>((p.x * c - p.y * s) / aspect, p.x * s + p.y * c);
    let q = vec2<f32>(0.5) + turned + vec2<f32>(dx, dy);
    return sample(mirror(q));
}
