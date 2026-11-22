
cbuffer SceneConstants : register(b0)
{
    float4x4 view_projection;
    float4x4 inverse_view_projection;
    float4 camera_position;
    float4 sun_direction_intensity;
    float4 sun_color_time;
    float4 ambient_color;
};

Texture2DArray<float4> TerrainAlbedo : register(t0);
Texture2DArray<float4> TerrainArm : register(t1);
Texture2DArray<float4> TerrainAo : register(t2);
SamplerState TerrainSampler : register(s0);

struct VertexInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float3 color : COLOR0;
    float3 texture_data : TEXCOORD0;
    float4 instance_position_scale : INSTANCE_POSITION_SCALE0;
};

struct TerrainPixelInput
{
    float4 position : SV_POSITION;
    float3 normal : NORMAL;
    float3 color : COLOR0;
    float3 world_position : WORLD_POSITION;
    float2 world_xz : TEXCOORD0;
    float terrain_mask : TEXCOORD1;
};

// Set to 1 when rendering into an sRGB render target.
// Set to 0 when the shader must perform sRGB encoding.
#ifndef OUTPUT_IS_SRGB
#define OUTPUT_IS_SRGB 0
#endif

// --------------------------------------------------
// CHEAP FRESNEL
// --------------------------------------------------

float fresnel5(float x)
{
    float x2 = x * x;
    return x2 * x2 * x;
}

// --------------------------------------------------
// TONE MAPPING
// --------------------------------------------------

float3 filmic_color(float3 color)
{
    color = max(color, 0.0);

    float3 mapped =
        color * (2.51 * color + 0.03) /
        (color * (2.43 * color + 0.59) + 0.14);

    mapped = saturate(mapped);

#if OUTPUT_IS_SRGB
    return mapped;
#else
    return lerp(
        mapped * 12.92,
        1.055 * pow(mapped, 1.0 / 2.4) - 0.055,
        step(0.0031308, mapped)
    );
#endif
}

// --------------------------------------------------
// VERTEX SHADER
// --------------------------------------------------

TerrainPixelInput EnvironmentVS(VertexInput input)
{
    TerrainPixelInput output;

    float3 world_position =
        input.position * input.instance_position_scale.w +
        input.instance_position_scale.xyz;

    output.position = mul(
        view_projection,
        float4(world_position, 1.0)
    );

    output.normal = input.normal;
    output.color = input.color;
    output.world_position = world_position;
    output.world_xz = input.texture_data.xy;
    output.terrain_mask = input.texture_data.z;

    return output;
}

// --------------------------------------------------
// WATER MATERIAL
// --------------------------------------------------

float3 RenderWater(
    float3 normal,
    float3 view_direction,
    float3 light_direction,
    float2 world_xz
)
{
    float time = sun_color_time.w;

    // Two directional wave systems
    float phaseA =
        dot(world_xz, float2(0.13, 0.08)) + time * 0.8;

    float phaseB =
        dot(world_xz, float2(-0.07, 0.16)) - time * 0.55;

    float waveA = sin(phaseA);
    float waveB = sin(phaseB);

    // Analytic wave derivatives
    float derivativeX =
        cos(phaseA) * 0.13 * 0.045 +
        cos(phaseB) * -0.07 * 0.035;

    float derivativeZ =
        cos(phaseA) * 0.08 * 0.045 +
        cos(phaseB) * 0.16 * 0.035;

    float3 waterNormal = normalize(
        normal + float3(-derivativeX, 0.0, -derivativeZ)
    );

    float NdotL = saturate(
        dot(waterNormal, light_direction)
    );

    float NdotV = saturate(
        dot(waterNormal, view_direction)
    );

    float3 halfVector = normalize(
        light_direction + view_direction
    );

    float NdotH = saturate(
        dot(waterNormal, halfVector)
    );

    // Approximate power 64 without pow()
    float spec = NdotH * NdotH;
    spec *= spec;
    spec *= spec;
    spec *= spec;
    spec *= spec;
    spec *= spec;

    float fresnel = fresnel5(1.0 - NdotV);

    float waveVariation =
        waveA * waveB * 0.012;

    float3 waterColor =
        float3(0.025, 0.17, 0.21) + waveVariation;

    float3 ambientWater =
        ambient_color.rgb * 0.75;

    float3 directWater =
        sun_color_time.rgb *
        sun_direction_intensity.w *
        NdotL * 0.28;

    float3 reflectedSky =
        lerp(
            float3(0.035, 0.075, 0.13),
            float3(0.24, 0.38, 0.48),
            fresnel
        );

    float3 sunReflection =
        sun_color_time.rgb *
        spec *
        sun_direction_intensity.w *
        2.0;

    float3 result =
        waterColor * (ambientWater + directWater);

    result += reflectedSky * fresnel * 0.45;
    result += sunReflection;

    return result;
}

// --------------------------------------------------
// MAIN ENVIRONMENT PIXEL SHADER
// --------------------------------------------------

float4 EnvironmentPS(TerrainPixelInput input) : SV_TARGET
{
    float3 N = normalize(input.normal);

    float3 L = normalize(
        sun_direction_intensity.xyz
    );

    float3 V = normalize(
        camera_position.xyz - input.world_position
    );

    float sunIntensity =
        max(sun_direction_intensity.w, 0.0);

    float3 finalColor;

    // --------------------------------------------------
    // OBJECTS
    // --------------------------------------------------

    if (input.terrain_mask < 0.5)
    {
        float NdotL = dot(N, L);

        float diffuse = saturate(NdotL);

        float wrappedDiffuse = saturate(
            (NdotL + 0.22) / 1.22
        );

        float moonDiffuse = saturate(
            dot(N, -L)
        );

        float3 ambient =
            ambient_color.rgb *
            lerp(0.55, 0.95, saturate(N.y));

        float3 direct =
            sun_color_time.rgb *
            sunIntensity *
            wrappedDiffuse;

        float3 moon =
            float3(0.18, 0.25, 0.42) *
            ambient_color.a *
            moonDiffuse;

        finalColor =
            input.color *
            (ambient + direct + moon);
    }

    // --------------------------------------------------
    // WATER
    // --------------------------------------------------

    else if (input.terrain_mask > 1.5)
    {
        finalColor = RenderWater(
            N,
            V,
            L,
            input.world_xz
        );
    }

    // --------------------------------------------------
    // TERRAIN
    // --------------------------------------------------

    else
    {
        float primaryMaterial = clamp(round(input.color.r), 0.0, 5.0);
        float secondaryMaterial = clamp(round(input.color.g), 0.0, 5.0);
        float materialBlend = saturate(input.color.b);
        float2 uv = input.world_xz * 0.25;
        float3 primaryLayer = float3(uv, primaryMaterial);
        float3 secondaryLayer = float3(uv, secondaryMaterial);
        float3 albedo = lerp(
            TerrainAlbedo.Sample(TerrainSampler, primaryLayer).rgb,
            TerrainAlbedo.Sample(TerrainSampler, secondaryLayer).rgb,
            materialBlend
        );
        float4 arm = lerp(
            TerrainArm.Sample(TerrainSampler, primaryLayer),
            TerrainArm.Sample(TerrainSampler, secondaryLayer),
            materialBlend
        );
        float ao = lerp(
            TerrainAo.Sample(TerrainSampler, primaryLayer).r,
            TerrainAo.Sample(TerrainSampler, secondaryLayer).r,
            materialBlend
        );
        float roughness = max(arm.g, 0.045);
        float metallic = saturate(arm.b);

        float upFactor = saturate(N.y);
        float3 skyAmbient =
            ambient_color.rgb *
            lerp(0.48, 1.0, upFactor);
        float3 groundBounce =
            float3(0.13, 0.095, 0.065) *
            (1.0 - upFactor) *
            ambient_color.a *
            ao;
        skyAmbient *= ao;

        float NdotL = saturate(dot(N, L));
        float NdotV = max(saturate(dot(N, V)), 0.001);
        float3 H = normalize(L + V);
        float NdotH = saturate(dot(N, H));
        float VdotH = saturate(dot(V, H));
        float alpha = roughness * roughness;
        float alphaSquared = alpha * alpha;
        float distributionDenominator =
            NdotH * NdotH * (alphaSquared - 1.0) + 1.0;
        float distribution =
            alphaSquared / max(3.14159265 * distributionDenominator * distributionDenominator, 0.001);
        float geometryK = (roughness + 1.0) * (roughness + 1.0) * 0.125;
        float geometryView =
            NdotV / (NdotV * (1.0 - geometryK) + geometryK);
        float geometryLight =
            NdotL / (NdotL * (1.0 - geometryK) + geometryK);
        float3 f0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
        float fresnelWeight = pow(1.0 - VdotH, 5.0);
        float3 fresnel = f0 + (1.0 - f0) * fresnelWeight;
        float3 specular =
            distribution * geometryView * geometryLight * fresnel /
            max(4.0 * NdotV * NdotL, 0.001);
        float3 diffuseWeight = (1.0 - fresnel) * (1.0 - metallic);
        float3 direct =
            (diffuseWeight * albedo / 3.14159265 + specular) *
            sun_color_time.rgb *
            sunIntensity *
            NdotL;

        float moonDiffuse = saturate(
            dot(N, -L)
        );
        float3 moon =
            float3(0.18, 0.25, 0.42) *
            ambient_color.a *
            moonDiffuse;
        finalColor =
            albedo * (skyAmbient + groundBounce + moon) +
            direct;
    }

    return float4(
        filmic_color(finalColor),
        1.0
    );
}
