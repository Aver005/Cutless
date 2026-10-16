struct Params { tiles: f32 }

// Зеркало: координата за краем кадра отражается обратно, на 0..1 — тождество.
fn mirror(q: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(q * 0.5) * 2.0);
}

// Кадр повторяется плитками, каждая вторая отражена. При tiles = 1 — исходная картинка.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    return sample(mirror(uv * params.tiles));
}
