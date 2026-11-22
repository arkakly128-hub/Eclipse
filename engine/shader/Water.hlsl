cbuffer SceneConstants : register(b0)
{
    float4x4 view_projection;
    float4x4 inverse_view_projection;
    float4 camera_position;
    float4 sun_direction_intensity;
    float4 sun_color_time;
    float4 ambient_color;
    float4 ripple_grid;
    float4 environment_state;
};

Texture2D<float> RippleHeight : register(t0);
SamplerState RippleSampler : register(s0);

Texture2DArray<float4> WaterNormalMap : register(t0);
Texture2DArray<float4> WaterRoughnessMap : register(t1);
Texture2DArray<float4> WaterAoMap : register(t2);
Texture2DArray<float4> WaterBaseColorMap : register(t3);
SamplerState WaterMaterialSampler : register(s0);

static const float MAX_WATER_WAVE_HEIGHT = 0.61;
static const float SHORELINE_CLEARANCE = 0.08;

// ============================================================
// INPUT / OUTPUT
// ============================================================

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float3 color : COLOR0;
    float3 texture_data : TEXCOORD0;
    float4 instance_position_scale : INSTANCE_POSITION_SCALE0;
};

struct WaterPixelInput
{
    float4 position : SV_POSITION;
    float3 world_position : TEXCOORD0;
    float3 normal : TEXCOORD1;
    float water_depth : TEXCOORD2;
    float water_type : TEXCOORD3;
};

struct WaveResult
{
    float height;

    float displacement_x;
    float displacement_z;

    float height_dx;
    float height_dz;

    float displacement_x_dx;
    float displacement_x_dz;

    float displacement_z_dx;
    float displacement_z_dz;
};

// ============================================================
// ANALYTICAL WAVE SYSTEM
// ============================================================

void AddWave(
    inout WaveResult result,
    float2 position,
    float2 direction,
    float amplitude,
    float frequency,
    float speed,
    float steepness,
    float time)
{
    float phase =
        dot(position, direction) * frequency
        + time * speed;

    float s = sin(phase);
    float c = cos(phase);

    float derivative = amplitude * frequency * c;

    result.height += amplitude * s;

    result.height_dx += derivative * direction.x;
    result.height_dz += derivative * direction.y;

    float horizontal = amplitude * steepness * c;

    result.displacement_x += direction.x * horizontal;
    result.displacement_z += direction.y * horizontal;

    float horizontal_derivative =
        -amplitude * steepness * frequency * s;

    result.displacement_x_dx +=
        horizontal_derivative * direction.x * direction.x;

    result.displacement_x_dz +=
        horizontal_derivative * direction.x * direction.y;

    result.displacement_z_dx +=
        horizontal_derivative * direction.y * direction.x;

    result.displacement_z_dz +=
        horizontal_derivative * direction.y * direction.y;
}

// ============================================================
// SHARED WATER SURFACE
//
// texture_data.z:
// 0 = Ocean
// 1 = Lake
// 2 = River
// 3 = Shallow water
// ============================================================

WaveResult EvaluateWaves(
    float2 position,
    float time)
{
    WaveResult result = (WaveResult)0;

    AddWave(result, position,
        normalize(float2(1.0, 0.25)),
        0.30, 0.42, 0.85, 0.65, time);

    AddWave(result, position,
        normalize(float2(0.25, 1.0)),
        0.15, 0.78, 1.20, 0.42, time);

    AddWave(result, position,
        normalize(float2(0.8, -0.65)),
        0.08, 1.35, 1.55, 0.22, time);

    AddWave(result, position,
        normalize(float2(-0.4, 1.0)),
        0.045, 2.4, 2.1, 0.12, time);

    AddWave(result, position,
        normalize(float2(0.65, -0.75)),
        0.035, 2.8, 2.6, 0.10, time);

    return result;
}

// ============================================================
// PLAYER RIPPLE
// ============================================================

float SamplePlayerRipple(float2 world_xz)
{
    float grid_size = ripple_grid.w;

    if (grid_size <= 0.0 || ripple_grid.z <= 0.0)
        return 0.0;

    float2 grid_position =
        (world_xz - ripple_grid.xy) / ripple_grid.z;

    if (any(grid_position < 0.0) ||
        any(grid_position >= grid_size))
    {
        return 0.0;
    }

    float2 uv = grid_position / grid_size;

    return RippleHeight.SampleLevel(
        RippleSampler, uv, 0.0);
}

// ============================================================
// WATER VERTEX SHADER
// ============================================================

WaterPixelInput WaterVS(VertexInput input)
{
    WaterPixelInput output;

    float3 base_position = input.position;

    float water_type =
        clamp(round(input.texture_data.z), 0.0, 3.0);

    float time = sun_color_time.w;

    WaveResult wave = EvaluateWaves(base_position.xz, time);

    float ripple =
        SamplePlayerRipple(base_position.xz);

    float sample_distance = 0.25;

    float ripple_dx =
        (SamplePlayerRipple(
            base_position.xz + float2(sample_distance, 0.0))
        - ripple) / sample_distance;

    float ripple_dz =
        (SamplePlayerRipple(
            base_position.xz + float2(0.0, sample_distance))
        - ripple) / sample_distance;

    float shoreline_fade = 1.0 - smoothstep(
        MAX_WATER_WAVE_HEIGHT,
        MAX_WATER_WAVE_HEIGHT + 1.0,
        input.color.g);
    float shoreline_drop =
        (MAX_WATER_WAVE_HEIGHT + SHORELINE_CLEARANCE) * shoreline_fade;
    float wave_visibility = 1.0 - shoreline_fade;

    float3 world_position = base_position;

    world_position.x += wave.displacement_x * wave_visibility;
    world_position.y +=
        (wave.height + ripple) * wave_visibility - shoreline_drop;
    world_position.z += wave.displacement_z * wave_visibility;

    float3 tangent_x = float3(
        1.0 + wave.displacement_x_dx * wave_visibility,
        (wave.height_dx + ripple_dx) * wave_visibility,
        wave.displacement_z_dx * wave_visibility
    );

    float3 tangent_z = float3(
        wave.displacement_x_dz * wave_visibility,
        (wave.height_dz + ripple_dz) * wave_visibility,
        1.0 + wave.displacement_z_dz * wave_visibility
    );

    output.position = mul(
        view_projection,
        float4(world_position, 1.0));

    output.world_position = world_position;

    output.normal = normalize(
        cross(tangent_z, tangent_x));

    output.water_depth = input.color.r;
    output.water_type = water_type;

    return output;
}

// ============================================================
// FAKE ATMOSPHERIC REFLECTION
//
// Mathematical sky approximation.
// No sky texture, cubemap, ray tracing or screen-space reads.
// ============================================================

float3 FakeSkyReflection(
    float3 direction,
    float3 sun_direction,
    float roughness)
{
    direction = normalize(direction);

    // Roughness blurs the reflected direction.
    float3 blurred_direction = normalize(
        lerp(direction, float3(0.0, 1.0, 0.0),
        roughness * 0.48));

    float horizon = 1.0 -
        smoothstep(-0.18, 0.48, blurred_direction.y);

    float day = smoothstep(
        -0.16, 0.24, sun_direction.y);

    float twilight = 1.0 -
        smoothstep(0.04, 0.42, abs(sun_direction.y));

    float3 day_sky = lerp(
        float3(0.035, 0.085, 0.17),
        float3(0.34, 0.52, 0.68),
        horizon);

    float3 dusk_sky = lerp(
        float3(0.055, 0.035, 0.09),
        float3(0.68, 0.27, 0.16),
        horizon);

    float3 night_sky = lerp(
        float3(0.004, 0.009, 0.025),
        float3(0.025, 0.045, 0.075),
        horizon);

    float3 sky = lerp(night_sky, day_sky, day);
    sky = lerp(sky, dusk_sky, twilight);

    // Broad sun glow.
    float sun_dot = saturate(
        dot(blurred_direction, sun_direction));

    float sun_glow =
        pow(sun_dot, lerp(12.0, 55.0, roughness));

    float sun_halo =
        pow(sun_dot, 5.0) * 0.12;

    sky += sun_color_time.rgb *
        (sun_glow + sun_halo) *
        sun_direction_intensity.w;

    return max(sky, 0.0);
}

// ============================================================
// FILMIC OUTPUT
// ============================================================

float3 FilmicSrgb(float3 color)
{
    color = max(color, 0.0);

    float3 mapped =
        color * (2.51 * color + 0.03) /
        (color * (2.43 * color + 0.59) + 0.14);

    mapped = saturate(mapped);

    return lerp(
        mapped * 12.92,
        1.055 * pow(mapped, 1.0 / 2.4) - 0.055,
        step(0.0031308, mapped));
}

// ============================================================
// WATER PIXEL SHADER
// ============================================================

float4 WaterPS(WaterPixelInput input) : SV_TARGET
{
    float time = sun_color_time.w;

    float3 N = normalize(input.normal);
    float2 normal_uv_a =
        input.world_position.xz * 0.12 +
        float2(time * 0.018, time * 0.011);
    float2 normal_uv_b =
        input.world_position.xz * 0.19 +
        float2(-time * 0.013, time * 0.021);
    float3 normal_a =
        WaterNormalMap.Sample(WaterMaterialSampler, float3(normal_uv_a, 0.0)).xyz * 2.0 - 1.0;
    float3 normal_b =
        WaterNormalMap.Sample(WaterMaterialSampler, float3(normal_uv_b, 0.0)).xyz * 2.0 - 1.0;
    float2 detail_normal = (normal_a.xy + normal_b.xy) * 0.16;
    N = normalize(N + float3(detail_normal.x, 0.0, detail_normal.y));
    float texture_roughness =
        WaterRoughnessMap.Sample(WaterMaterialSampler, float3(normal_uv_a, 0.0)).r;
    float texture_ao =
        WaterAoMap.Sample(WaterMaterialSampler, float3(normal_uv_a, 0.0)).r;
    float3 texture_color =
        WaterBaseColorMap.Sample(WaterMaterialSampler, float3(normal_uv_a, 0.0)).rgb;

    float3 V = normalize(
        camera_position.xyz - input.world_position);

    float3 L = normalize(
        sun_direction_intensity.xyz);

    float depth = saturate(input.water_depth);

    int style = (int)round(input.water_type);

    // --------------------------------------------------------
    // NATURAL WATER COLORS
    // --------------------------------------------------------

    float3 deep_color;
    float3 shallow_color;

    if (style == 0)
    {
        // Ocean
        deep_color = float3(0.006, 0.035, 0.065);
        shallow_color = float3(0.025, 0.19, 0.22);
    }
    else if (style == 1)
    {
        // Lake
        deep_color = float3(0.012, 0.045, 0.055);
        shallow_color = float3(0.055, 0.17, 0.15);
    }
    else if (style == 2)
    {
        // River
        deep_color = float3(0.018, 0.052, 0.042);
        shallow_color = float3(0.075, 0.18, 0.12);
    }
    else
    {
        // Shallow water
        deep_color = float3(0.025, 0.085, 0.075);
        shallow_color = float3(0.12, 0.24, 0.18);
    }

    float3 water_color = lerp(
        deep_color,
        shallow_color,
        depth);
    water_color = lerp(water_color, water_color * texture_color * 1.35, 0.12);

    // --------------------------------------------------------
    // SUBTLE SURFACE VARIATION
    // --------------------------------------------------------

    float flow_speed = 0.65;

    float detail_a = sin(
        input.world_position.x * 1.25 +
        time * flow_speed);

    float detail_b = sin(
        input.world_position.z * 1.65 -
        time * flow_speed * 0.73);

    float surface_variation = detail_a * detail_b;

    water_color *=
        1.0 + surface_variation * 0.035;

    // --------------------------------------------------------
    // ROUGHNESS
    // --------------------------------------------------------

    float roughness =
        0.16 + texture_roughness * 0.3 + abs(surface_variation) * 0.04;

    roughness = saturate(roughness);

    // --------------------------------------------------------
    // FRESNEL
    // --------------------------------------------------------

    float NdotV = saturate(dot(N, V));

    float fresnel =
        0.02 + 0.98 *
        pow(1.0 - NdotV, 5.0);

    // --------------------------------------------------------
    // FAKE REFLECTION
    // --------------------------------------------------------

    float3 reflected_direction =
        reflect(-V, N);

    float3 reflected_sky = FakeSkyReflection(
        reflected_direction,
        L,
        roughness);

    float underwater = step(0.5, environment_state.x);
    float reflection_strength = lerp(
        lerp(0.48, 0.22, roughness),
        0.015,
        underwater
    );

    float3 color = lerp(
        water_color,
        reflected_sky,
        fresnel * reflection_strength);

    // --------------------------------------------------------
    // BROAD SPECULAR HIGHLIGHTS
    // --------------------------------------------------------

    float3 H = normalize(L + V);

    float NdotH = saturate(dot(N, H));

    float specular_power =
        lerp(110.0, 24.0, roughness);

    float specular =
        pow(NdotH, specular_power);

    float sun_visibility =
        smoothstep(-0.10, 0.10, L.y);

    float sun_highlight =
        specular *
        sun_direction_intensity.w *
        sun_visibility *
        0.45 *
        (1.0 - underwater);

    color +=
        sun_color_time.rgb *
        sun_highlight *
        lerp(0.75, 0.18, roughness);

    // --------------------------------------------------------
    // AMBIENT LIGHT
    // --------------------------------------------------------

    color +=
        ambient_color.rgb *
        water_color * 0.28 *
        lerp(0.7, 1.0, texture_ao);

    // --------------------------------------------------------
    // NIGHT LIGHTING
    // --------------------------------------------------------

    float night =
        1.0 - smoothstep(-0.20, 0.10, L.y);

    color +=
        float3(0.035, 0.055, 0.095) *
        night * fresnel * 0.35;

    // --------------------------------------------------------
    // FINAL OUTPUT
    // --------------------------------------------------------

    color *= 1.45;

    return float4(
        FilmicSrgb(color),
        1.0);
}