
cbuffer SceneConstants : register(b0)
{
    float4x4 view_projection;
    float4x4 inverse_view_projection;
    float4 camera_position;
    float4 sun_direction_intensity;
    float4 sun_color_time;
    float4 ambient_color;
};

struct SkyPixelInput
{
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// --------------------------------------------------
// NOISE
// --------------------------------------------------

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float noise2(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);

    f = f * f * (3.0 - 2.0 * f);

    float a = hash21(i);
    float b = hash21(i + float2(1, 0));
    float c = hash21(i + float2(0, 1));
    float d = hash21(i + float2(1, 1));

    return lerp(
        lerp(a, b, f.x),
        lerp(c, d, f.x),
        f.y
    );
}

// Three octaves: broad clouds with soft detail
float cloudNoise(float2 p)
{
    float result = noise2(p) * 0.57;

    p = p * 2.03 + 17.1;
    result += noise2(p) * 0.29;

    p = p * 2.01 + 11.7;
    result += noise2(p) * 0.14;

    return result;
}

// --------------------------------------------------
// TONE MAPPING
// --------------------------------------------------

float3 skyTone(float3 c)
{
    c = max(c, 0.0);

    c = c * (2.51 * c + 0.03) /
        (c * (2.43 * c + 0.59) + 0.14);

    return pow(saturate(c), 1.0 / 2.2);
}

// --------------------------------------------------
// FULLSCREEN VERTEX
// --------------------------------------------------

SkyPixelInput SkyVS(uint vertex_id : SV_VertexID)
{
    SkyPixelInput o;

    float2 p = float2(
        (vertex_id << 1) & 2,
        vertex_id & 2
    );

    o.position = float4(
        p * float2(2, -2) + float2(-1, 1),
        1, 1
    );

    o.uv = p;

    return o;
}

// --------------------------------------------------
// SKY PIXEL SHADER
// --------------------------------------------------

float4 SkyPS(SkyPixelInput input) : SV_TARGET
{
    float2 ndc = float2(
        input.uv.x * 2.0 - 1.0,
        1.0 - input.uv.y * 2.0
    );

    float4 farPoint = mul(
        inverse_view_projection,
        float4(ndc, 1.0, 1.0)
    );

    float3 worldPoint =
        farPoint.xyz / max(abs(farPoint.w), 0.00001);

    float3 V = normalize(
        worldPoint - camera_position.xyz
    );

    float3 sunDir = normalize(
        sun_direction_intensity.xyz
    );

    float sunHeight = sunDir.y;
    float elevation = V.y;

    float time = sun_color_time.w;

    // --------------------------------------------------
    // DAY / NIGHT / TWILIGHT
    // --------------------------------------------------

    float day = smoothstep(-0.12, 0.22, sunHeight);

    float twilight =
        (1.0 - smoothstep(0.015, 0.34, abs(sunHeight)));

    float night = 1.0 - day;

    // --------------------------------------------------
    // ATMOSPHERIC SKY GRADIENT
    // --------------------------------------------------

    float horizon = 1.0 -
        smoothstep(-0.10, 0.58, elevation);

    float zenithBlend = smoothstep(
        -0.05,
        0.75,
        elevation
    );

    float3 dayZenith =
        float3(0.055, 0.19, 0.39);

    float3 dayHorizon =
        float3(0.64, 0.79, 0.91);

    float3 duskZenith =
        float3(0.12, 0.105, 0.22);

    float3 duskHorizon =
        float3(0.95, 0.43, 0.25);

    float3 nightZenith =
        float3(0.006, 0.012, 0.035);

    float3 nightHorizon =
        float3(0.035, 0.055, 0.10);

    float3 daySky = lerp(
        dayHorizon,
        dayZenith,
        zenithBlend
    );

    float3 duskSky = lerp(
        duskHorizon,
        duskZenith,
        zenithBlend
    );

    float3 nightSky = lerp(
        nightHorizon,
        nightZenith,
        zenithBlend
    );

    float3 sky = lerp(
        daySky,
        duskSky,
        twilight
    );

    sky = lerp(
        sky,
        nightSky,
        night
    );

    // --------------------------------------------------
    // ATMOSPHERIC HORIZON SCATTERING
    // --------------------------------------------------

    float horizonGlow = exp(
        -abs(elevation) * 8.0
    );

    float3 warmScatter =
        float3(1.0, 0.43, 0.20);

    float3 blueScatter =
        float3(0.30, 0.53, 0.78);

    float3 scatterColor = lerp(
        warmScatter,
        blueScatter,
        day
    );

    sky += scatterColor *
        horizonGlow *
        (0.12 + twilight * 0.38);

    // --------------------------------------------------
    // SUN DISK
    // --------------------------------------------------

    float sunAlignment = saturate(
        dot(V, sunDir)
    );

    float sunAngle = acos(
        clamp(sunAlignment, -1.0, 1.0)
    );

    float sunDisk = 1.0 - smoothstep(
        0.0038,
        0.0048,
        sunAngle
    );

    float softHalo = exp(
        -sunAngle * 65.0
    );

    float wideHalo = exp(
        -sunAngle * 12.0
    );

    float3 sunColor = lerp(
        float3(1.0, 0.48, 0.25),
        sun_color_time.rgb,
        day
    );

    float sunVisibility = saturate(
        sunHeight * 8.0 + 0.5
    );

    sky += sunColor *
        (sunDisk * 5.0 +
         softHalo * 0.30 +
         wideHalo * 0.07) *
        sunVisibility *
        sun_direction_intensity.w;

    // --------------------------------------------------
    // MOON
    // --------------------------------------------------

    float moonAlignment = saturate(
        dot(V, -sunDir)
    );

    float moonAngle = acos(
        clamp(moonAlignment, -1.0, 1.0)
    );

    float moonDisk = 1.0 - smoothstep(
        0.0035,
        0.0045,
        moonAngle
    );

    float moonHalo = exp(
        -moonAngle * 30.0
    );

    float moonVisibility = night *
        saturate(ambient_color.a);

    sky += float3(0.58, 0.69, 0.94) *
        (moonDisk * 1.5 + moonHalo * 0.10) *
        moonVisibility;

    // --------------------------------------------------
    // CLOUDS
    // --------------------------------------------------

    // Stable directional projection
    float cloudDenominator = max(
        elevation + 0.22,
        0.22
    );

    float2 cloudUV =
        V.xz / cloudDenominator;

    cloudUV *= 3.2;

    cloudUV += float2(
        time * 0.006,
        time * 0.002
    );

    // Broad cloud formations
    float cloudShape = cloudNoise(cloudUV);

    // Natural soft cloud boundaries
    float cloudMask = smoothstep(
        0.49,
        0.67,
        cloudShape
    );

    float upperSky = smoothstep(
        -0.01,
        0.24,
        elevation
    );

    float cloudCoverage = cloudMask *
        upperSky *
        day;

    // Cloud illumination
    float cloudLight = saturate(
        dot(V, sunDir) * 0.5 + 0.5
    );

    float3 cloudShadow =
        float3(0.36, 0.40, 0.47);

    float3 cloudWhite =
        float3(0.94, 0.96, 1.0);

    float3 cloudSunlit = lerp(
        cloudWhite,
        float3(1.0, 0.78, 0.57),
        twilight * 0.85
    );

    float3 cloudColor = lerp(
        cloudShadow,
        cloudSunlit,
        cloudLight
    );

    // Soft atmospheric cloud blending
    float cloudOpacity = cloudCoverage * 0.52;

    sky = lerp(
        sky,
        cloudColor,
        cloudOpacity
    );

    // --------------------------------------------------
    // NIGHT STARS
    // --------------------------------------------------

    float2 starUV = V.xz /
        max(elevation, 0.15);

    float2 starCell = floor(
        starUV * 260.0
    );

    float starRandom = hash21(starCell);

    float star = smoothstep(
        0.9975,
        0.9995,
        starRandom
    );

    float twinkle = 0.85 +
        0.15 * sin(
            time * 1.5 +
            starRandom * 120.0
        );

    float starVisibility =
        smoothstep(0.08, 0.35, elevation) *
        night;

    sky += float3(0.72, 0.82, 1.0) *
        star *
        twinkle *
        starVisibility;

    // --------------------------------------------------
    // FINAL
    // --------------------------------------------------

    return float4(
        skyTone(sky),
        1.0
    );
}
