struct Params { amount: f32, frequency: f32 }

/// Вспышка: белый свет добавляется к кадру; частота делает стробоскоп, 0 — ровная вспышка.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let a = params.amount / 100.0 * 1.5;
    let gate = select(1.0, step(0.5, fract(frame.time * params.frequency)), params.frequency > 0.0);
    return vec4<f32>(c.rgb + vec3<f32>(a * gate), c.a);
}
