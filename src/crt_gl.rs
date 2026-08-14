//! `crt_gl` — optional OpenGL path for the PUBLIC DOMAIN CRT shader via `glow`.
//!
//! This module compiles `assets/shaders/crt-lottes.vert` / `.frag` (PUBLIC
//! DOMAIN CRT STYLED SCAN-LINE SHADER by Timothy Lottes) into a GL 3.3 core
//! program. Original single-file shader vendored as `crt-lottes.glsl`.
//! It is intentionally **optional and fallible**: if the window cannot
//! provide a GL context or `glow` fails, the caller falls back to the
//! SDL2 CPU path (`crate::crt_pi` + `crate::render`).
//!
//! The CPU path (`crate::crt_pi`) remains a separate MIT implementation that
//! mirrors the former MIT shader's curvature/scanline math; the GL path is
//! PUBLIC DOMAIN (lottes). No copyleft-licensed code remains.
//!
//! Design:
//! - `CrtGl::try_new(&VideoSubsystem, &Window)` — creates GL context + `glow::Context`
//!   + shader program. Returns `Err` on any failure so `main.rs` can continue.
//! - `CrtGl::apply(...)` is a stub for future fullscreen-quad blit. Today the
//!   SDL renderer still draws the frame; the GL path is validated by successful
//!   shader compilation and can be switched to a texture→quad path without
//!   changing the grid/backend code.
//!
//! The shader source is embedded via `include_str!` so `cargo build` validates
//! that the shader files exist.

#![allow(clippy::pedantic)]

use std::rc::Rc;

use glow::HasContext as _;

/// Embedded shader sources — proves at compile time they are present.
pub const VERT_SRC: &str = include_str!("../assets/shaders/crt-lottes.vert");
pub const FRAG_SRC: &str = include_str!("../assets/shaders/crt-lottes.frag");

/// Minimal wrapper around a compiled CRT GL program.
///
/// Holds the `glow::Context` so the context stays current for the window's
/// lifetime. When dropped, the GL program is deleted.
pub struct CrtGl {
    gl: Rc<glow::Context>,
    program: glow::NativeProgram,
}

impl CrtGl {
    /// Try to create a GL context for `window` and compile the CRT shaders.
    /// On success the context is current and `program` is ready for use.
    /// On failure returns `Err(String)` — caller should use the CPU fallback.
    ///
    /// # Errors
    /// Returns `Err` if GL context creation or shader compilation fails.
    pub fn try_new(
        video: &sdl2::VideoSubsystem,
        window: &sdl2::video::Window,
    ) -> Result<Self, String> {
        // Request a core 3.3 context. This must be done before gl_create_context,
        // but SDL2's gl_attr is global — set it now (harmless if already set).
        {
            let gl_attr = video.gl_attr();
            gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
            gl_attr.set_context_version(3, 3);
            let _flags = gl_attr.context_flags();
        }

        let ctx = window
            .gl_create_context()
            .map_err(|e| format!("gl_create_context failed: {e}"))?;
        window
            .gl_make_current(&ctx)
            .map_err(|e| format!("gl_make_current failed: {e}"))?;
        // Load GL via glow's loader that calls SDL_GL_GetProcAddress.
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                video.gl_get_proc_address(s) as *const std::ffi::c_void
            })
        };
        let gl = Rc::new(gl);

        // Compile and link.
        let program = unsafe { compile_program(&gl, VERT_SRC, FRAG_SRC)? };
        // Use once to validate uniforms, then unbind.
        unsafe {
            gl.use_program(Some(program));
            let p = crate::crt_pi::CrtPiParams::default();
            // Lottes uses warpX/warpY instead of CURVATURE_X/Y. Map our
            // 0.20 defaults to lottes defaults (0.031 / 0.041) so --curvature
            // still has an effect on the GL path. If the shader was built
            // without PARAMETER_UNIFORM (default defines), these uniforms won't
            // exist and set_uniform is a no-op — shader uses its #define warp.
            // When PARAMETER_UNIFORM is enabled, warpX/Y are live uniforms.
            let warp_x = p.curvature_x * 0.155;
            let warp_y = p.curvature_y * 0.205;
            set_uniform_f32(&gl, program, "warpX", warp_x);
            set_uniform_f32(&gl, program, "warpY", warp_y);
            // Also set legacy CURVATURE names for any shader that still expects them
            // (no-op if not present).
            set_uniform_f32(&gl, program, "CURVATURE_X", p.curvature_x);
            set_uniform_f32(&gl, program, "CURVATURE_Y", p.curvature_y);
            // Lottes other params — set to defaults so uniform path matches defines
            set_uniform_f32(&gl, program, "hardScan", -8.0);
            set_uniform_f32(&gl, program, "hardPix", -3.0);
            set_uniform_f32(&gl, program, "maskDark", 0.5);
            set_uniform_f32(&gl, program, "maskLight", 1.5);
            set_uniform_f32(&gl, program, "shadowMask", 3.0);
            set_uniform_f32(&gl, program, "brightBoost", 1.0);
            set_uniform_f32(&gl, program, "hardBloomPix", -1.5);
            set_uniform_f32(&gl, program, "hardBloomScan", -2.0);
            set_uniform_f32(&gl, program, "bloomAmount", 0.15);
            set_uniform_f32(&gl, program, "shape", 2.0);
            set_uniform_f32(&gl, program, "scaleInLinearGamma", 1.0);
            // Log which uniforms the GL path will actually use
            if std::env::var("DEBUG").is_ok() {
                eprintln!(
                    "crt-lottes GL uniforms: warpX {warp_x:.3} warpY {warp_y:.3} (from CURVATURE {} / {}; lottes PUBLIC DOMAIN)",
                    p.curvature_x, p.curvature_y
                );
            }
            gl.use_program(None);
        }

        // Keep context alive via window's internal ref; SDL2 ties ctx lifetime
        // to window. We intentionally leak `ctx` for the app lifetime so the
        // context is not destroyed while `glow` holds function pointers.
        std::mem::forget(ctx);

        Ok(Self { gl, program })
    }

    /// Returns the underlying `glow::Context` for advanced use (e.g. fullscreen quad).
    #[allow(dead_code)]
    #[must_use]
    pub fn gl(&self) -> &glow::Context {
        &self.gl
    }

    /// Returns the compiled program handle.
    #[allow(dead_code)]
    #[must_use]
    pub fn program(&self) -> glow::NativeProgram {
        self.program
    }

    /// Future fullscreen-quad hook: bind `program`, set `Texture` to unit 0, draw.
    /// Currently a no-op beyond binding — the SDL Canvas path still presents.
    /// Provided so callers can branch `if let Some(gl) = crt_gl { gl.draw_quad() }`.
    #[allow(dead_code)]
    pub fn bind(&self) {
        unsafe {
            self.gl.use_program(Some(self.program));
        }
    }

    /// Unbind program.
    #[allow(dead_code)]
    pub fn unbind(&self) {
        unsafe {
            self.gl.use_program(None);
        }
    }
}

impl Drop for CrtGl {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
        }
    }
}

// SAFETY: GL calls are unsafe; we check compile/link status and return Err on failure.
unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::NativeProgram, String> {
    let program = gl.create_program().map_err(|e| format!("create_program: {e}"))?;

    let vert = gl
        .create_shader(glow::VERTEX_SHADER)
        .map_err(|e| format!("create vert shader: {e}"))?;
    gl.shader_source(vert, vert_src);
    gl.compile_shader(vert);
    if !gl.get_shader_compile_status(vert) {
        let log = gl.get_shader_info_log(vert);
        gl.delete_shader(vert);
        gl.delete_program(program);
        return Err(format!("crt-lottes vert compile failed: {log}"));
    }

    let frag = gl
        .create_shader(glow::FRAGMENT_SHADER)
        .map_err(|e| format!("create frag shader: {e}"))?;
    gl.shader_source(frag, frag_src);
    gl.compile_shader(frag);
    if !gl.get_shader_compile_status(frag) {
        let log = gl.get_shader_info_log(frag);
        gl.delete_shader(vert);
        gl.delete_shader(frag);
        gl.delete_program(program);
        return Err(format!("crt-lottes frag compile failed: {log}"));
    }

    gl.attach_shader(program, vert);
    gl.attach_shader(program, frag);
    gl.link_program(program);
    // Shaders can be detached/deleted after link; program retains them.
    gl.detach_shader(program, vert);
    gl.detach_shader(program, frag);
    gl.delete_shader(vert);
    gl.delete_shader(frag);

    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(format!("crt-lottes link failed: {log}"));
    }
    Ok(program)
}

unsafe fn set_uniform_f32(gl: &glow::Context, program: glow::NativeProgram, name: &str, value: f32) {
    if let Some(loc) = gl.get_uniform_location(program, name) {
        gl.uniform_1_f32(Some(&loc), value);
    }
}
