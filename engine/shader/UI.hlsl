struct UiVertexInput
{
    float2 position : POSITION;
    float2 texcoord : TEXCOORD0;
    float4 color : COLOR0;
    float textureSlot : TEXCOORD1;
};

struct UiPixelInput
{
    float4 position : SV_POSITION;
    float2 texcoord : TEXCOORD0;
    float4 color : COLOR0;
    float textureSlot : TEXCOORD1;
};

Texture2D menuTexture : register(t0);
Texture2D buttonTexture : register(t1);
Texture2D inventoryTexture : register(t2);
Texture2D slotTexture : register(t3);
Texture2D fontTexture : register(t4);
Texture2D redHeartTexture : register(t5);
Texture2D blackHeartTexture : register(t6);
SamplerState uiSampler : register(s0);

UiPixelInput UiVS(UiVertexInput input)
{
    UiPixelInput output;
    output.position = float4(input.position, 0.0, 1.0);
    output.texcoord = input.texcoord;
    output.color = input.color;
    output.textureSlot = input.textureSlot;
    return output;
}

float4 UiPS(UiPixelInput input) : SV_TARGET
{
    if (input.textureSlot > 5.5)
        return blackHeartTexture.Sample(uiSampler, input.texcoord) * input.color;
    if (input.textureSlot > 4.5)
        return redHeartTexture.Sample(uiSampler, input.texcoord) * input.color;
    if (input.textureSlot > 3.5)
    {
        float coverage = fontTexture.Sample(uiSampler, input.texcoord).a;
        return float4(input.color.rgb, input.color.a * coverage);
    }
    if (input.textureSlot > 2.5)
    {
        float4 slot = slotTexture.Sample(uiSampler, input.texcoord);
        return slot * input.color;
    }
    if (input.textureSlot > 1.5)
    {
        float4 inventory = inventoryTexture.Sample(uiSampler, input.texcoord);
        return inventory * input.color;
    }
    if (input.textureSlot > 0.5)
    {
        float4 button = buttonTexture.Sample(uiSampler, input.texcoord);
        if (input.color.a > 0.5)
            button.rgb = saturate(button.rgb * 1.35 + float3(0.28, 0.16, 0.02));
        return button;
    }
    if (input.textureSlot > -0.5)
        return menuTexture.Sample(uiSampler, input.texcoord);
    return input.color;
}
