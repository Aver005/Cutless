struct Params { angle: f32 }

// Поворот оттенка вокруг серой оси (1,1,1) по матрице Родрига, в линейном свете.
// Вращение сохраняет серую ось и не обрезает блики выше 1.0.
fn effect(uv: vec2<f32>) -> vec4<f32> {
    let c = sample(uv);
    let a = radians(params.angle);
    let cs = cos(a);
    let sn = sin(a);
    let axis = 0.5773502691896258;
    let cross = (sn * axis) * vec3<f32>(c.g - c.b, c.b - c.r, c.r - c.g);
    let grey = ((1.0 - cs) / 3.0) * (c.r + c.g + c.b);
    return vec4<f32>(c.rgb * cs + cross + vec3<f32>(grey), c.a);
}
