struct Params { amount: f32 }

/// Инверсия: блик выше белого сначала гаснет до белого, альфа исходная.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let clipped = min(c.rgb, vec3<f32>(1.0));
    return vec4<f32>(mix(c.rgb, vec3<f32>(1.0) - clipped, params.amount / 100.0), c.a);
}
