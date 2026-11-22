cbuffer OcclusionConstants : register(b0)
{
    float4x4 view_projection;
    float4 bounds_center;
    float4 bounds_extent;
};

struct VertexInput
{
    float3 position : POSITION;
};

struct VertexOutput
{
    float4 position : SV_POSITION;
};

VertexOutput OcclusionVS(VertexInput input)
{
    VertexOutput output;
    float3 world_position = bounds_center.xyz + input.position * bounds_extent.xyz;
    output.position = mul(view_projection, float4(world_position, 1.0));
    return output;
}

float4 OcclusionPS() : SV_TARGET
{
    return 0.0;
}
