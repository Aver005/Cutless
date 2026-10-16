struct Params { amount: f32, frequency: f32, bands: f32 }

// Зеркало: координата за краем кадра отражается обратно, прозрачный пиксель не отдаём.
// На отрезке 0..1 это тождество.
fn mirror(q: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(q * 0.5) * 2.0);
}

// Полосы по высоте съезжают в сторону, R и B расходятся на половину сдвига,
// рисунок сбоя меняется по времени. При amount = 0 картинка не тронута.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let t = floor(frame.time * params.frequency);
    let band = floor(uv.y * params.bands);
    let h = hash(vec2<f32>(band, t), 1.0);
    let gate = step(0.55, hash(vec2<f32>(band, t), 2.0));
    let shift = (h - 0.5) * 2.0 * (params.amount / 100.0 * 0.08) * gate;
    let r = sample(mirror(uv + vec2<f32>(shift * 1.5, 0.0))).r;
    let g = sample(mirror(uv + vec2<f32>(shift, 0.0))).g;
    let b = sample(mirror(uv + vec2<f32>(shift * 0.5, 0.0))).b;
    return vec4<f32>(r, g, b, c.a);
}
