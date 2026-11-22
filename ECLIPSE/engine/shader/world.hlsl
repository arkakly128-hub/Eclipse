
cbuffer SceneConstants : register(b0)
{
    float4x4 view_projection;
    float4x4 inverse_view_projection;
    float4 camera_position;
    float4 sun_direction_intensity;
    float4 sun_color_time;
    float4 ambient_color;
};

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float3 color : COLOR0;
    float3 texture_data : TEXCOORD0;
    float4 instance_position_scale : INSTANCE_POSITION_SCALE0;
};

struct WorldPixelInput
{
    float4 position : SV_POSITION;
    float3 normal : NORMAL;
    float3 albedo : COLOR0;
    float3 world_position : WORLD_POSITION;
    float2 local_xz : TEXCOORD0;
    float height : TEXCOORD1;
};

// ============================================================
// COLOR MAPPING
// ============================================================

float3 world_filmic_srgb(float3 color)
{
    color = max(color, 0.0);

    float3 mapped =
        color * (2.51 * color + 0.03) /
        (color * (2.43 * color + 0.59) + 0.14);

    mapped = saturate(mapped);

    return lerp(
        mapped * 12.92,
        1.055 * pow(mapped, 1.0 / 2.4) - 0.055,
        step(0.0031308, mapped)
    );
}

// ============================================================
// PROCEDURAL MATERIAL DETAIL
// Cheap hash-based value noise.
// ============================================================

float WorldHash(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);

    return frac(p.x * p.y);
}

float WorldNoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);

    f = f * f * (3.0 - 2.0 * f);

    float a = WorldHash(i);
    float b = WorldHash(i + float2(1.0, 0.0));
    float c = WorldHash(i + float2(0.0, 1.0));
    float d = WorldHash(i + float2(1.0, 1.0));

    return lerp(
        lerp(a, b, f.x),
        lerp(c, d, f.x),
        f.y
    );
}

// ============================================================
// VERTEX SHADER
// ============================================================

WorldPixelInput WorldVS(VertexInput input)
{
    WorldPixelInput output;

    float scale = input.instance_position_scale.w;

    float3 world_position =
        input.position * scale +
        input.instance_position_scale.xyz;

    output.position = mul(
        view_projection,
        float4(world_position, 1.0)
    );

    // Assumes uniform instance scaling.
    output.normal = normalize(input.normal);

    output.albedo = input.color;
    output.world_position = world_position;

    output.local_xz = world_position.xz;
    output.height = world_position.y;

    return output;
}

// ============================================================
// PIXEL SHADER
// ============================================================

float4 WorldPS(WorldPixelInput input) : SV_TARGET
{
    float3 N = normalize(input.normal);

    float3 V = normalize(
        camera_position.xyz - input.world_position
    );

    float3 L = normalize(
        sun_direction_intensity.xyz
    );

    float3 albedo = max(input.albedo, 0.0);

    // --------------------------------------------------------
    // MATERIAL CLASSIFICATION
    // Uses supplied vertex colors.
    // --------------------------------------------------------

    float green_dominance =
        albedo.g - max(albedo.r, albedo.b);

    float foliage_mask = smoothstep(
        0.025, 0.20, green_dominance
    );

    float maximum_channel =
        max(albedo.r, max(albedo.g, albedo.b));

    float minimum_channel =
        min(albedo.r, min(albedo.g, albedo.b));

    float saturation =
        maximum_channel > 0.001
        ? (maximum_channel - minimum_channel) /
          maximum_channel
        : 0.0;

    float rock_mask =
        (1.0 - smoothstep(0.15, 0.48, saturation))
        * (1.0 - foliage_mask);

    // --------------------------------------------------------
    // NATURAL MULTI-SCALE COLOR VARIATION
    // --------------------------------------------------------

    float2 world_uv = input.world_position.xz;

    float macro_noise =
        WorldNoise(world_uv * 0.075);

    float medium_noise =
        WorldNoise(world_uv * 0.42);

    float fine_noise =
        WorldNoise(world_uv * 2.2);

    float terrain_variation =
        (macro_noise - 0.5) * 0.13 +
        (medium_noise - 0.5) * 0.075 +
        (fine_noise - 0.5) * 0.035;

    // Foliage gets slightly less color variation.
    terrain_variation *=
        lerp(1.0, 0.55, foliage_mask);

    albedo *= 1.0 + terrain_variation;

    // Subtle height influence.
    float height_variation =
        saturate((input.height + 2.0) * 0.025);

    albedo *= lerp(
        0.94,
        1.025,
        height_variation
    );

    // --------------------------------------------------------
    // SLOPE RESPONSE
    // --------------------------------------------------------

    float slope =
        saturate(1.0 - N.y);

    float exposed_surface =
        smoothstep(0.25, 0.85, slope);

    // Rock gets a slight cooler variation on steep surfaces.
    float3 rock_tint =
        float3(0.91, 0.95, 1.0);

    albedo = lerp(
        albedo,
        albedo * rock_tint,
        exposed_surface * rock_mask * 0.25
    );

    // --------------------------------------------------------
    // SUNLIGHT
    // --------------------------------------------------------

    float NdotL = dot(N, L);

    float wrapped_diffuse =
        saturate((NdotL + 0.22) / 1.22);

    float day_factor =
        smoothstep(-0.18, 0.12, L.y);

    float sun_intensity =
        sun_direction_intensity.w *
        day_factor;

    float3 direct_light =
        sun_color_time.rgb *
        sun_intensity *
        wrapped_diffuse;

    // --------------------------------------------------------
    // HEMISPHERICAL AMBIENT
    // --------------------------------------------------------

    float hemisphere =
        saturate(N.y * 0.5 + 0.5);

    float3 sky_ambient =
        ambient_color.rgb;

    float3 ground_ambient =
        ambient_color.rgb *
        float3(0.68, 0.63, 0.53);

    float3 ambient =
        lerp(
            ground_ambient,
            sky_ambient,
            hemisphere
        );

    // --------------------------------------------------------
    // NIGHT / MOON LIGHT
    // --------------------------------------------------------

    float moon_factor =
        1.0 - day_factor;

    float moon_diffuse =
        saturate(dot(N, -L));

    float3 moon_light =
        float3(0.12, 0.17, 0.27) *
        ambient_color.a *
        moon_diffuse *
        moon_factor;

    // --------------------------------------------------------
    // MATERIAL ROUGHNESS
    // --------------------------------------------------------

    float roughness = lerp(
        0.72,
        0.88,
        foliage_mask
    );

    roughness = lerp(
        roughness,
        0.62,
        rock_mask
    );

    // --------------------------------------------------------
    // SPECULAR LIGHT
    // --------------------------------------------------------

    float3 H = normalize(L + V);

    float NdotH =
        saturate(dot(N, H));

    float specular_power =
        lerp(96.0, 12.0, roughness);

    float specular =
        pow(NdotH, specular_power);

    float NdotV =
        saturate(dot(N, V));

    float fresnel =
        0.04 +
        0.96 * pow(1.0 - NdotV, 5.0);

    float material_specular =
        lerp(0.28, 0.045, foliage_mask);

    material_specular =
        lerp(
            material_specular,
            0.18,
            rock_mask
        );

    float3 specular_light =
        sun_color_time.rgb *
        specular *
        fresnel *
        material_specular *
        sun_intensity;

    // --------------------------------------------------------
    // FOLIAGE LIGHT TRANSMISSION
    // Subtle backlighting, not an artificial glow.
    // --------------------------------------------------------

    float backlight =
        saturate(dot(-N, L));

    float foliage_transmission =
        foliage_mask *
        backlight *
        sun_intensity *
        0.12;

    float3 transmission =
        sun_color_time.rgb *
        foliage_transmission *
        albedo;

    // --------------------------------------------------------
    // FINAL LIGHTING
    // --------------------------------------------------------

    float3 lighting =
        ambient +
        direct_light +
        moon_light;

    float3 color =
        albedo * lighting;

    color += specular_light;
    color += transmission;

    // Subtle ambient foliage lift.
    color +=
        ambient_color.rgb *
        foliage_mask *
        0.025;

    // --------------------------------------------------------
    // FINAL COLOR
    // --------------------------------------------------------

    return float4(
        world_filmic_srgb(color),
        1.0
    );
}
