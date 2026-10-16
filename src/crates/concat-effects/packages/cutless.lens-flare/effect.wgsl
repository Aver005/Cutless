struct Params { amount: f32, x: f32, y: f32, size: f32 }

// Мягкое окно: 1 в ядре пятна и 0 за внешним радиусом, чтобы свет не тёк бесконечно.
fn flare_window(d: f32, outer: f32) -> f32 {
    return 1.0 - smoothstep(outer * 0.6, outer, d);
}

// Добавляемый свет: яркое гауссово ядро, горизонтальный луч и три призрака
// на линии через центр кадра. При amount = 0 картинка не тронута.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    if (k <= 0.0) {
        return c;
    }
    let s = params.size / 100.0;
    let centre = vec2<f32>(params.x, params.y) / 100.0;
    let to_centre = vec2<f32>(0.5) - centre;

    let d = distance(uv, centre);
    let core_sigma = 0.045 * s;
    let core = exp(-d * d / (2.0 * core_sigma * core_sigma)) * flare_window(d, 0.13 * s);

    let rel = uv - centre;
    let ray_sigma = 0.012 * s;
    let ray = exp(-rel.y * rel.y / (2.0 * ray_sigma * ray_sigma)) * exp(-abs(rel.x) * 3.0)
        * flare_window(abs(rel.x), 0.6);

    var glow = vec3<f32>(1.0, 0.96, 0.86) * (core * 1.4) + vec3<f32>(1.0, 0.9, 0.7) * (ray * 0.5);
    for (var i = 1; i <= 3; i++) {
        let fi = f32(i);
        let ghost_at = centre + to_centre * (1.0 + fi * 0.7);
        let gd = distance(uv, ghost_at);
        let ghost_sigma = 0.05 * s;
        let ghost = exp(-gd * gd / (2.0 * ghost_sigma * ghost_sigma)) * flare_window(gd, 0.09 * s);
        let tint = mix(vec3<f32>(1.0, 0.6, 0.7), vec3<f32>(0.5, 0.7, 1.0), fi / 3.0);
        glow += tint * (ghost * (0.3 / fi));
    }
    return vec4<f32>(c.rgb + glow * k, c.a);
}
