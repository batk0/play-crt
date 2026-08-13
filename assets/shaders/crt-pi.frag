#version 330 core
// crt-pi.frag — fragment shader for zork-crt-gui OpenGL path
// Derived from libretro/glsl-shaders crt/shaders/crt-pi.glsl (GPL-2.0+, (C) 2015-2016 davej)
// Adapted from single-file #ifdef FRAGMENT variant to GL 3.3 core for glow.
// Parameters mirror the original #pragma parameter defaults; uniforms can be
// overridden by the Rust side (see src/crt_gl.rs).
// See assets/shaders/crt-pi.glsl for original license and docs.

in vec2 vTexCoord;
in float vFilterWidth;
in vec2 vScreenScale;

out vec4 FragColor;

uniform sampler2D Texture;
uniform vec2 TextureSize;

// crt-pi parameters — 0.20/0.20 curvature is visibly bulged yet readable (orig 0.10/0.25 was faint on 80×24)
uniform float CURVATURE_X = 0.20;
uniform float CURVATURE_Y = 0.20;
uniform float MASK_BRIGHTNESS = 0.70;
uniform float SCANLINE_WEIGHT = 6.0;
uniform float SCANLINE_GAP_BRIGHTNESS = 0.12;
uniform float BLOOM_FACTOR = 1.5;
uniform float INPUT_GAMMA = 2.4;
uniform float OUTPUT_GAMMA = 2.2;

// Feature toggles — mirror original #defines (enabled by default in zork-crt-gui)
#define SCANLINES
#define MULTISAMPLE
#define GAMMA
// #define FAKE_GAMMA
#define CURVATURE
// #define SHARPER
#define MASK_TYPE 1

vec2 distort(vec2 coord) {
    vec2 curvature = vec2(CURVATURE_X, CURVATURE_Y);
    vec2 barrelScale = 1.0 - (0.23 * curvature);
    coord *= vScreenScale;
    coord -= vec2(0.5);
    float rsq = coord.x * coord.x + coord.y * coord.y;
    coord += coord * (curvature * rsq);
    coord *= barrelScale;
    if (abs(coord.x) >= 0.5 || abs(coord.y) >= 0.5)
        return vec2(-1.0);
    coord += vec2(0.5);
    coord /= vScreenScale;
    return coord;
}

float calcScanLineWeight(float dist) {
    return max(1.0 - dist * dist * SCANLINE_WEIGHT, SCANLINE_GAP_BRIGHTNESS);
}

float calcScanLine(float dy) {
    float w = calcScanLineWeight(dy);
#ifdef MULTISAMPLE
    w += calcScanLineWeight(dy - vFilterWidth);
    w += calcScanLineWeight(dy + vFilterWidth);
    w *= 0.3333333;
#endif
    return w;
}

void main() {
#ifdef CURVATURE
    vec2 texcoord = distort(vTexCoord);
    if (texcoord.x < 0.0) {
        FragColor = vec4(0.0);
        return;
    }
#else
    vec2 texcoord = vTexCoord;
#endif

    vec2 texcoordInPixels = texcoord * TextureSize;

#ifdef SHARPER
    vec2 tempCoord = floor(texcoordInPixels) + 0.5;
    vec2 coord = tempCoord / TextureSize;
    vec2 deltas = texcoordInPixels - tempCoord;
    float scanLineWeight = calcScanLine(deltas.y);
    vec2 signs = sign(deltas);
    deltas.x *= 2.0;
    deltas = deltas * deltas;
    deltas.y = deltas.y * deltas.y;
    deltas.x *= 0.5;
    deltas.y *= 8.0;
    deltas /= TextureSize;
    deltas *= signs;
    vec2 tc = coord + deltas;
#else
    float tempY = floor(texcoordInPixels.y) + 0.5;
    float yCoord = tempY / TextureSize.y;
    float dy = texcoordInPixels.y - tempY;
    float scanLineWeight = calcScanLine(dy);
    float signY = sign(dy);
    dy = dy * dy;
    dy = dy * dy;
    dy *= 8.0;
    dy /= TextureSize.y;
    dy *= signY;
    vec2 tc = vec2(texcoord.x, yCoord + dy);
#endif

    vec3 colour = texture(Texture, tc).rgb;

#ifdef SCANLINES
#ifdef GAMMA
#ifdef FAKE_GAMMA
    colour = colour * colour;
#else
    colour = pow(colour, vec3(INPUT_GAMMA));
#endif
#endif
    scanLineWeight *= BLOOM_FACTOR;
    colour *= scanLineWeight;
#ifdef GAMMA
#ifdef FAKE_GAMMA
    colour = sqrt(colour);
#else
    colour = pow(colour, vec3(1.0 / OUTPUT_GAMMA));
#endif
#endif
#endif

#if MASK_TYPE == 0
    FragColor = vec4(colour, 1.0);
#elif MASK_TYPE == 1
    float whichMask = fract(gl_FragCoord.x * 0.5);
    vec3 mask;
    if (whichMask < 0.5)
        mask = vec3(MASK_BRIGHTNESS, 1.0, MASK_BRIGHTNESS);
    else
        mask = vec3(1.0, MASK_BRIGHTNESS, 1.0);
    FragColor = vec4(colour * mask, 1.0);
#else
    float whichMask = fract(gl_FragCoord.x * 0.3333333);
    vec3 mask = vec3(MASK_BRIGHTNESS, MASK_BRIGHTNESS, MASK_BRIGHTNESS);
    if (whichMask < 0.3333333)
        mask.x = 1.0;
    else if (whichMask < 0.6666666)
        mask.y = 1.0;
    else
        mask.z = 1.0;
    FragColor = vec4(colour * mask, 1.0);
#endif
}
