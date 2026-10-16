struct Params { shift: f32, angle: f32 }

/// Отражение координаты зеркалом: за краем кадра не будет прозрачного пикселя.
fn mirror(q: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) - abs(vec2<f32>(1.0) - fract(q * 0.5) * 2.0);
}

/// Разъезд каналов: красный уходит вдоль направления, синий против, зелёный и альфа на месте.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    // Сдвиг в долях ширины по обеим осям: на вертикальном кадре вертикаль не разъезжается сильнее.
    let aspect = frame.size.x / frame.size.y;
    let dir = vec2<f32>(cos(radians(params.angle)), sin(radians(params.angle)) * aspect) * (params.shift / 100.0);
    let red = sample(mirror(uv + dir)).r;
    let blue = sample(mirror(uv - dir)).b;
    return vec4<f32>(red, c.g, blue, c.a);
}
