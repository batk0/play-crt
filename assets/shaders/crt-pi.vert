#version 330 core
// crt-pi.vert — vertex shader for zork-crt-gui OpenGL path
// Derived from libretro/glsl-shaders crt/shaders/crt-pi.glsl (GPL-2.0+, (C) 2015-2016 davej)
// Split from the single-file #ifdef VERTEX variant for use with glow (GL 3.3 core).
// Original uses MVPMatrix + TexCoord/VertexCoord; this version keeps that interface.
// See assets/shaders/crt-pi.glsl for full license header and parameter docs.

layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aTexCoord;

uniform mat4 MVPMatrix;
uniform vec2 InputSize;
uniform vec2 OutputSize;
uniform vec2 TextureSize;

out vec2 vTexCoord;
out float vFilterWidth;
#if defined(CURVATURE) || 1
// Always pass screenScale; curvature branch reads it. When curvature is disabled
// the fragment shader ignores it. Compute unconditionally to keep interface stable.
out vec2 vScreenScale;
#endif

void main() {
    vTexCoord = aTexCoord;
    vFilterWidth = (InputSize.y / OutputSize.y) / 3.0;
    vScreenScale = TextureSize / InputSize;
    gl_Position = MVPMatrix * vec4(aPos, 0.0, 1.0);
}
