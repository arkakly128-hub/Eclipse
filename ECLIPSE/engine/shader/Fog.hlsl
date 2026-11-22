
// ============================================================================
// HYPEROPTIC RE8 CINEMATIC VOLUMETRIC FOG
// ============================================================================
// Standalone DirectX 11 HLSL
// Shader Model 5.0
//
// Features:
// - Analytical height-integrated atmospheric fog
// - Two independent fog density layers
// - Procedural animated density noise
// - Beer-Lambert transmittance
// - Depth-aware scene compositing
// - Cinematic cold atmospheric coloration
//
// No Unity dependencies.
// No ShaderLab.
// No ray marching.
// No volumetric light shafts.
// ============================================================================


// ============================================================================
// CONSTANT BUFFERS
// ============================================================================

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


// ============================================================================
// INPUT TEXTURES
// ============================================================================

Texture2DMS<float> SceneDepth : register(t0);
Texture2D<float4> SceneColor : register(t1);

SamplerState LinearClamp : register(s0);


// ============================================================================
// VERTEX INPUT
// ============================================================================

struct VSOutput
{
    float4 Position : SV_POSITION;
    float2 UV       : TEXCOORD0;
};


// ============================================================================
// FULLSCREEN VERTEX SHADER
// ============================================================================

VSOutput FogVS(uint vertex_id : SV_VertexID)
{
    VSOutput output;

    float2 position = float2(
        (vertex_id << 1) & 2,
        vertex_id & 2
    );
    output.Position = float4(
        position * float2(2.0, -2.0) + float2(-1.0, 1.0),
        0.0,
        1.0
    );
    output.UV = position;

    return output;
}


// ============================================================================
// PROCEDURAL NOISE
// ============================================================================

float Hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));

    p += dot(p, p + 45.32);

    return frac(p.x * p.y);
}


float ValueNoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);

    f = f * f * (3.0 - 2.0 * f);

    float a = Hash21(i);
    float b = Hash21(i + float2(1, 0));
    float c = Hash21(i + float2(0, 1));
    float d = Hash21(i + float2(1, 1));

    return lerp(
        lerp(a, b, f.x),
        lerp(c, d, f.x),
        f.y
    );
}


float FogNoise(float3 worldPosition)
{
    float2 wind =
        float2(0.8, 0.2) * (sun_color_time.w * 0.018);

    float2 p =
        worldPosition.xz * 0.035 + wind;

    float n1 = ValueNoise(p);

    float n2 = ValueNoise(
        p * 2.13 + float2(17.3, 9.7)
    );

    return n1 * 0.7 + n2 * 0.3;
}


// ============================================================================
// WORLD POSITION RECONSTRUCTION
// ============================================================================

float3 ReconstructWorldPosition(
    float2 uv,
    float depth
)
{
    // DirectX viewport UV: top-left origin.
    // NDC: Y-up, Z range 0 to 1.

    float2 ndc;

    ndc.x = uv.x * 2.0 - 1.0;
    ndc.y = 1.0 - uv.y * 2.0;

    float4 clipPosition = float4(
        ndc,
        depth,
        1.0
    );

    float4 worldPosition = mul(
        inverse_view_projection,
        clipPosition
    );

    return worldPosition.xyz /
        max(abs(worldPosition.w), 0.000001);
}


// ============================================================================
// ANALYTICAL HEIGHT FOG INTEGRATION
// ============================================================================

float IntegrateHeightFog(
    float cameraY,
    float surfaceY,
    float distanceToSurface,
    float density,
    float baseHeight,
    float falloff
)
{
    float heightDifference =
        surfaceY - cameraY;

    float a = falloff * heightDifference;

    float integral;

    if (abs(a) < 0.001)
    {
        integral = 1.0;
    }
    else
    {
        integral = (1.0 - exp(-a)) / a;
    }

    float cameraDensity = density *
        exp(clamp(
            -falloff * (cameraY - baseHeight),
            -10.0,
            10.0
        ));

    return cameraDensity *
        distanceToSurface *
        integral;
}


// ============================================================================
// PIXEL SHADER
// ============================================================================

float3 UnderwaterComposite(float3 sceneColor, float2 uv, float distanceToSurface)
{
    float attenuation = exp(-distanceToSurface * 0.035);
    float3 underwaterColor = lerp(
        float3(0.008, 0.055, 0.085),
        sceneColor,
        attenuation
    );
    float vignette = 1.0 - smoothstep(
        0.28,
        0.82,
        length((uv - 0.5) * float2(1.0, 0.8))
    );
    return underwaterColor * (0.72 + vignette * 0.28);
}

float4 FogPS(VSOutput input) : SV_TARGET
{
    float2 uv = input.UV;

    float2 sceneUV = uv;
    if (environment_state.x > 0.5)
    {
        float distortion =
            sin(uv.y * 42.0 + sun_color_time.w * 1.8) *
            cos(uv.x * 31.0 - sun_color_time.w * 1.2) *
            0.0025;
        sceneUV = saturate(uv + float2(distortion, -distortion * 0.7));
    }
    float4 sceneColor = SceneColor.SampleLevel(LinearClamp, sceneUV, 0);

    uint depthWidth;
    uint depthHeight;
    uint depthSamples;
    SceneDepth.GetDimensions(depthWidth, depthHeight, depthSamples);
    int2 pixel = min(
        int2(input.Position.xy),
        int2(depthWidth - 1, depthHeight - 1)
    );
    float rawDepth = 0.0;
    for (uint sampleIndex = 0; sampleIndex < depthSamples; ++sampleIndex)
        rawDepth += SceneDepth.Load(pixel, sampleIndex);
    rawDepth /= max(depthSamples, 1);

    if (rawDepth >= 0.999999)
    {
        if (environment_state.x > 0.5)
        {
            sceneColor.rgb = UnderwaterComposite(sceneColor.rgb, uv, 80.0);
        }
        else
        {
            float day = smoothstep(-0.15, 0.20, sun_direction_intensity.y);
            float3 skyFog = lerp(
                float3(0.045, 0.065, 0.11),
                float3(0.54, 0.67, 0.76),
                day
            );
            sceneColor.rgb = lerp(sceneColor.rgb, skyFog, 0.08);
        }
        return sceneColor;
    }

    float3 worldPosition =
        ReconstructWorldPosition(uv, rawDepth);
    float3 ray = worldPosition - camera_position.xyz;
    float distanceToSurface = length(ray);
    float3 midpoint = camera_position.xyz + ray * 0.5;

    float groundOpticalDepth = IntegrateHeightFog(
        camera_position.y,
        worldPosition.y,
        distanceToSurface,
        0.012,
        0.0,
        0.042
    );
    float upperOpticalDepth = IntegrateHeightFog(
        camera_position.y,
        worldPosition.y,
        distanceToSurface,
        0.004,
        24.0,
        0.018
    );
    float noise = FogNoise(midpoint);
    float noiseMultiplier = lerp(
        1.0,
        0.35 + noise * 1.3,
        0.35
    );
    float opticalDepth =
        (groundOpticalDepth + upperOpticalDepth) *
        noiseMultiplier;
    opticalDepth = max(opticalDepth, 0.0);
    float fogAmount = min(1.0 - exp(-opticalDepth), 0.82);

    float day = smoothstep(-0.15, 0.20, sun_direction_intensity.y);
    float twilight = 1.0 - smoothstep(
        0.03,
        0.38,
        abs(sun_direction_intensity.y)
    );
    float3 fogColor = lerp(
        float3(0.045, 0.065, 0.11),
        float3(0.54, 0.67, 0.76),
        day
    );
    fogColor = lerp(fogColor, float3(0.82, 0.38, 0.20), twilight);
    fogColor += ambient_color.rgb * 0.12;
    sceneColor.rgb = lerp(sceneColor.rgb, fogColor, fogAmount);
    if (environment_state.x > 0.5)
        sceneColor.rgb = UnderwaterComposite(
            sceneColor.rgb,
            uv,
            distanceToSurface
        );

    return sceneColor;
}
