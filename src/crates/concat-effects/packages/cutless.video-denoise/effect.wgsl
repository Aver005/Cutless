struct Params { strength: f32 }

// Двусторонний фильтр: усредняет соседей 7×7, похожих по цвету, а края не трогает.
// Сила задаёт, насколько далёкий по цвету сосед ещё считается шумом.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let s = params.strength / 100.0;
    if (s <= 0.0) {
        return c;
    }
    let t = texel();
    let range = 0.03 + s * 0.2;
    let falloff = 1.0 / (2.0 * range * range);
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var y: i32 = -3; y <= 3; y++) {
        for (var x: i32 = -3; x <= 3; x++) {
            let n = sample(uv + vec2<f32>(f32(x), f32(y)) * t).rgb;
            let d = n - c.rgb;
            let w = exp(-f32(x * x + y * y) / 4.5 - dot(d, d) * falloff);
            sum += n * w;
            total += w;
        }
    }
    return vec4<f32>(sum / total, c.a);
}
