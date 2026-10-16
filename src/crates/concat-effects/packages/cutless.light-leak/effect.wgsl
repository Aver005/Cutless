struct Params { amount: f32, warmth: f32, speed: f32 }

// Тёплое мягкое пятно у края кадра: свет добавляется, а не смешивается,
// и медленно дрейфует по времени. Мягкое окно гасит его за радиусом.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let k = params.amount / 100.0;
    if (k <= 0.0) {
        return c;
    }
    let w = params.warmth / 100.0;
    let tint = mix(vec3<f32>(1.0, 0.45, 0.65), vec3<f32>(1.0, 0.55, 0.1), w);
    let phase = frame.time * params.speed * 0.35;
    let drift = vec2<f32>(0.5 + 0.5 * sin(phase), 0.5 + 0.5 * cos(phase * 1.3));
    let centre = vec2<f32>(0.04, 0.04) + vec2<f32>(0.05, 0.05) * drift;
    let blob = 1.0 - smoothstep(0.1, 0.5, distance(uv, centre));
    return vec4<f32>(c.rgb + tint * (blob * k * 1.5), c.a);
}
