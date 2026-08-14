#version 330 core
// PUBLIC DOMAIN CRT STYLED SCAN-LINE SHADER
//
//   by Timothy Lottes
//
// This is more along the style of a really good CGA arcade monitor.
// With RGB inputs instead of NTSC.
// The shadow mask example has the mask rotated 90 degrees for less chromatic aberration.
//
// Left it unoptimized to show the theory behind the algorithm.
//
// It is an example what I personally would want as a display option for pixel art games.
// Please take and use, change, or whatever.
//
// PUBLIC DOMAIN — vendored for play-crt as assets/shaders/crt-lottes.glsl
// This split fragment shader is adapted for GL 3.3 core (glow) from the original
// single-file crt-lottes.glsl. No copyright claim — PUBLIC DOMAIN.
// See assets/shaders/LICENSE.
// Adapted by stripping libretro COMPAT_* macros, using #version 330 core
// with `in vec4 TEX0` / `out vec4 FragColor`, and mapping InputSize/
// TextureSize/OutputSize uniforms directly. warpX/warpY use lottes defaults
// (0.031 / 0.041) — mapped from play-crt CURVATURE 0.20 via crt_gl.rs if
// PARAMETER_UNIFORM is enabled; otherwise defaults are used (simple warp).

in vec4 TEX0;
out vec4 FragColor;

uniform vec2 OutputSize;
uniform vec2 TextureSize;
uniform vec2 InputSize;
uniform sampler2D Texture;

// lottes params — either uniforms (if PARAMETER_UNIFORM defined) or defines
#ifdef PARAMETER_UNIFORM
uniform float hardScan;
uniform float hardPix;
uniform float warpX;
uniform float warpY;
uniform float maskDark;
uniform float maskLight;
uniform float scaleInLinearGamma;
uniform float shadowMask;
uniform float brightBoost;
uniform float hardBloomPix;
uniform float hardBloomScan;
uniform float bloomAmount;
uniform float shape;
#else
#define hardScan -8.0
#define hardPix -3.0
#define warpX 0.031
#define warpY 0.041
#define maskDark 0.5
#define maskLight 1.5
#define scaleInLinearGamma 1.0
#define shadowMask 3.0
#define brightBoost 1.0
#define hardBloomPix -1.5
#define hardBloomScan -2.0
#define bloomAmount 0.15
#define shape 2.0
#endif

#define Source Texture
#define vTexCoord TEX0.xy
#define SourceSize vec4(TextureSize, 1.0 / TextureSize)
#define outsize vec4(OutputSize, 1.0 / OutputSize)

#define DO_BLOOM

// sRGB to Linear.
#ifdef SIMPLE_LINEAR_GAMMA
float ToLinear1(float c) { return c; }
vec3 ToLinear(vec3 c) { return c; }
vec3 ToSrgb(vec3 c) { return pow(c, vec3(1.0 / 2.2)); }
#else
float ToLinear1(float c)
{
    if (scaleInLinearGamma == 0.)
        return c;
    return (c <= 0.04045) ? c/12.92 : pow((c + 0.055)/1.055, 2.4);
}
vec3 ToLinear(vec3 c)
{
    if (scaleInLinearGamma == 0.)
        return c;
    return vec3(ToLinear1(c.r), ToLinear1(c.g), ToLinear1(c.b));
}
float ToSrgb1(float c)
{
    if (scaleInLinearGamma == 0.)
        return c;
    return (c < 0.0031308 ? c*12.92 : 1.055*pow(c, 0.41666) - 0.055);
}
vec3 ToSrgb(vec3 c)
{
    if (scaleInLinearGamma == 0.)
        return c;
    return vec3(ToSrgb1(c.r), ToSrgb1(c.g), ToSrgb1(c.b));
}
#endif

vec3 Fetch(vec2 pos, vec2 off){
  pos=(floor(pos*SourceSize.xy+off)+vec2(0.5,0.5))/SourceSize.xy;
#ifdef SIMPLE_LINEAR_GAMMA
  return ToLinear(brightBoost * pow(texture(Source,pos.xy).rgb, vec3(2.2)));
#else
  return ToLinear(brightBoost * texture(Source,pos.xy).rgb);
#endif
}

vec2 Dist(vec2 pos)
{
    pos = pos*SourceSize.xy;
    return -((pos - floor(pos)) - vec2(0.5));
}

float Gaus(float pos, float scale)
{
    return exp2(scale*pow(abs(pos), shape));
}

vec3 Horz3(vec2 pos, float off)
{
    vec3 b    = Fetch(pos, vec2(-1.0, off));
    vec3 c    = Fetch(pos, vec2( 0.0, off));
    vec3 d    = Fetch(pos, vec2( 1.0, off));
    float dst = Dist(pos).x;
    float scale = hardPix;
    float wb = Gaus(dst-1.0,scale);
    float wc = Gaus(dst+0.0,scale);
    float wd = Gaus(dst+1.0,scale);
    return (b*wb+c*wc+d*wd)/(wb+wc+wd);
}

vec3 Horz5(vec2 pos,float off){
    vec3 a = Fetch(pos,vec2(-2.0, off));
    vec3 b = Fetch(pos,vec2(-1.0, off));
    vec3 c = Fetch(pos,vec2( 0.0, off));
    vec3 d = Fetch(pos,vec2( 1.0, off));
    vec3 e = Fetch(pos,vec2( 2.0, off));
    float dst = Dist(pos).x;
    float scale = hardPix;
    float wa = Gaus(dst - 2.0, scale);
    float wb = Gaus(dst - 1.0, scale);
    float wc = Gaus(dst + 0.0, scale);
    float wd = Gaus(dst + 1.0, scale);
    float we = Gaus(dst + 2.0, scale);
    return (a*wa+b*wb+c*wc+d*wd+e*we)/(wa+wb+wc+wd+we);
}

vec3 Horz7(vec2 pos,float off)
{
    vec3 a = Fetch(pos, vec2(-3.0, off));
    vec3 b = Fetch(pos, vec2(-2.0, off));
    vec3 c = Fetch(pos, vec2(-1.0, off));
    vec3 d = Fetch(pos, vec2( 0.0, off));
    vec3 e = Fetch(pos, vec2( 1.0, off));
    vec3 f = Fetch(pos, vec2( 2.0, off));
    vec3 g = Fetch(pos, vec2( 3.0, off));

    float dst = Dist(pos).x;
    float scale = hardBloomPix;
    float wa = Gaus(dst - 3.0, scale);
    float wb = Gaus(dst - 2.0, scale);
    float wc = Gaus(dst - 1.0, scale);
    float wd = Gaus(dst + 0.0, scale);
    float we = Gaus(dst + 1.0, scale);
    float wf = Gaus(dst + 2.0, scale);
    float wg = Gaus(dst + 3.0, scale);

    return (a*wa+b*wb+c*wc+d*wd+e*we+f*wf+g*wg)/(wa+wb+wc+wd+we+wf+wg);
}

float Scan(vec2 pos, float off)
{
    float dst = Dist(pos).y;
    return Gaus(dst + off, hardScan);
}

float BloomScan(vec2 pos, float off)
{
    float dst = Dist(pos).y;
    return Gaus(dst + off, hardBloomScan);
}

vec3 Tri(vec2 pos)
{
    vec3 a = Horz3(pos,-1.0);
    vec3 b = Horz5(pos, 0.0);
    vec3 c = Horz3(pos, 1.0);
    float wa = Scan(pos,-1.0);
    float wb = Scan(pos, 0.0);
    float wc = Scan(pos, 1.0);
    return a*wa + b*wb + c*wc;
}

vec3 Bloom(vec2 pos)
{
    vec3 a = Horz5(pos,-2.0);
    vec3 b = Horz7(pos,-1.0);
    vec3 c = Horz7(pos, 0.0);
    vec3 d = Horz7(pos, 1.0);
    vec3 e = Horz5(pos, 2.0);

    float wa = BloomScan(pos,-2.0);
    float wb = BloomScan(pos,-1.0);
    float wc = BloomScan(pos, 0.0);
    float wd = BloomScan(pos, 1.0);
    float we = BloomScan(pos, 2.0);

    return a*wa+b*wb+c*wc+d*wd+e*we;
}

vec2 Warp(vec2 pos)
{
    pos  = pos*2.0-1.0;
    pos *= vec2(1.0 + (pos.y*pos.y)*warpX, 1.0 + (pos.x*pos.x)*warpY);
    return pos*0.5 + 0.5;
}

vec3 Mask(vec2 pos)
{
    vec3 mask = vec3(maskDark, maskDark, maskDark);

    if (shadowMask == 1.0)
    {
        float line = maskLight;
        float odd = 0.0;
        if (fract(pos.x*0.166666666) < 0.5) odd = 1.0;
        if (fract((pos.y + odd) * 0.5) < 0.5) line = maskDark;
        pos.x = fract(pos.x*0.333333333);
        if      (pos.x < 0.333) mask.r = maskLight;
        else if (pos.x < 0.666) mask.g = maskLight;
        else                    mask.b = maskLight;
        mask*=line;
    }
    else if (shadowMask == 2.0)
    {
        pos.x = fract(pos.x*0.333333333);
        if      (pos.x < 0.333) mask.r = maskLight;
        else if (pos.x < 0.666) mask.g = maskLight;
        else                    mask.b = maskLight;
    }
    else if (shadowMask == 3.0)
    {
        pos.x += pos.y*3.0;
        pos.x  = fract(pos.x*0.166666666);
        if      (pos.x < 0.333) mask.r = maskLight;
        else if (pos.x < 0.666) mask.g = maskLight;
        else                    mask.b = maskLight;
    }
    else if (shadowMask == 4.0)
    {
        pos.xy  = floor(pos.xy*vec2(1.0, 0.5));
        pos.x  += pos.y*3.0;
        pos.x   = fract(pos.x*0.166666666);
        if      (pos.x < 0.333) mask.r = maskLight;
        else if (pos.x < 0.666) mask.g = maskLight;
        else                    mask.b = maskLight;
    }

    return mask;
}

void main()
{
    vec2 pos = Warp(TEX0.xy*(TextureSize.xy/InputSize.xy))*(InputSize.xy/TextureSize.xy);
    // Border: if Warp pushes outside 0..1, output black (tube overscan)
    if (pos.x < 0.0 || pos.x > 1.0 || pos.y < 0.0 || pos.y > 1.0) {
        FragColor = vec4(0.0, 0.0, 0.0, 1.0);
        return;
    }
    vec3 outColor = Tri(pos);

#ifdef DO_BLOOM
    outColor.rgb += Bloom(pos)*bloomAmount;
#endif

    if (shadowMask > 0.0)
        outColor.rgb *= Mask(gl_FragCoord.xy * 1.000001);

    FragColor = vec4(ToSrgb(outColor.rgb), 1.0);
}
