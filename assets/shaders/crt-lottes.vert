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
// This split vertex shader is adapted for GL 3.3 core (glow) from the original
// single-file crt-lottes.glsl. No copyright claim — PUBLIC DOMAIN.
// See assets/shaders/LICENSE.

layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aTexCoord;

uniform mat4 MVPMatrix;
uniform vec2 InputSize;
uniform vec2 OutputSize;
uniform vec2 TextureSize;

out vec4 TEX0;

void main() {
    TEX0.xy = aTexCoord;
    gl_Position = MVPMatrix * vec4(aPos, 0.0, 1.0);
}
