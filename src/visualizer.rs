use std::{
    ffi::CStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use glow::HasContext;

use crate::animation_service::{AnimationServiceState, start_service};

include!(concat!(env!("OUT_DIR"), "/visualizer.rs"));

// ---------------------------------------------------------------------------
// Shaders (GLSL 110 / ES 100 for maximum compatibility)
// ---------------------------------------------------------------------------

const VERTEX_SHADER_SRC: &str = r#"
#version 150
in vec2 a_position;
uniform vec2 u_scale;
uniform vec2 u_offset;
void main() {
    // Map chart coords to NDC:
    //   x: [0,1] -> [-1,1]
    //   y: [-1,1] -> [-1,1] (already NDC range)
    vec2 ndc = a_position * u_scale + u_offset;
    gl_Position = vec4(ndc, 0.0, 1.0);
    gl_PointSize = 14.0;
}
"#;

const FRAGMENT_SHADER_SRC: &str = r#"
#version 150
out vec4 frag_color;
uniform vec4 u_color;
void main() {
    frag_color = u_color;
}
"#;

// ---------------------------------------------------------------------------
// Static geometry
// ---------------------------------------------------------------------------

/// Background quad vertices in NDC space (two triangles).
const BACKGROUND_QUAD: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
];

/// Gridline vertices: horizontal lines at y = ±0.25, ±0.5, ±0.75, ±1.0
/// in chart coordinates [0,1]×[-1,1]. Each line is two points (x0,y, x1,y).
const GRIDLINE_VERTICES: [[f32; 2]; 16] = [
    [0.0, -1.0],
    [1.0, -1.0],
    [0.0, -0.75],
    [1.0, -0.75],
    [0.0, -0.5],
    [1.0, -0.5],
    [0.0, -0.25],
    [1.0, -0.25],
    [0.0, 0.25],
    [1.0, 0.25],
    [0.0, 0.5],
    [1.0, 0.5],
    [0.0, 0.75],
    [1.0, 0.75],
    [0.0, 1.0],
    [1.0, 1.0],
];

// ---------------------------------------------------------------------------
// Byte casting
// ---------------------------------------------------------------------------

/// Reinterpret a slice of `[f32; 2]` as `&[u8]` for GL uploads.
fn as_gl_bytes(data: &[[f32; 2]]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

// ---------------------------------------------------------------------------
// Waveform generation (free functions — no GL dependency)
// ---------------------------------------------------------------------------

fn phase_offset_per_fixture(fixture_count: usize) -> f64 {
    if fixture_count > 0 {
        1.0 / fixture_count as f64
    } else {
        1.0
    }
}

fn generate_unit_wave(state: &AnimationServiceState, out: &mut Vec<[f32; 2]>) {
    let offset = phase_offset_per_fixture(state.fixture_count);
    const NUM_POINTS: usize = 1000;
    out.clear();
    out.reserve(NUM_POINTS);
    for i in 0..NUM_POINTS {
        let phase = i as f64 / NUM_POINTS as f64;
        let offset_index = (phase / offset) as usize;
        let y = state.animation.get_unit_value(
            number::Phase::new(phase),
            offset_index,
            &state.clocks.clock_bank,
        );
        out.push([phase as f32, y as f32]);
    }
}

fn generate_scaled_wave(
    state: &AnimationServiceState,
    unit_wave: &[[f32; 2]],
    out: &mut Vec<[f32; 2]>,
) {
    out.clear();
    out.reserve(unit_wave.len());
    for &[x, y] in unit_wave {
        let scaled = state.animation.scale_value(
            &state.clocks.clock_bank,
            state.clocks.audio_envelope,
            y as f64,
        );
        out.push([x, scaled as f32]);
    }
}

fn generate_fixture_dots(state: &AnimationServiceState, out: &mut Vec<[f32; 2]>) {
    let offset = phase_offset_per_fixture(state.fixture_count);
    out.clear();
    out.reserve(state.fixture_count);
    for i in 0..state.fixture_count {
        let phase = i as f64 * offset;
        let y = state.animation.get_value(
            number::Phase::new(phase),
            i,
            &state.clocks.clock_bank,
            state.clocks.audio_envelope,
        );
        out.push([phase as f32, y as f32]);
    }
}

// ---------------------------------------------------------------------------
// CPU tessellation (thick lines via triangle strip extrusion)
// ---------------------------------------------------------------------------

/// Tessellate a line strip into a triangle strip by extruding perpendicular normals.
/// Produces uniform pixel-width lines regardless of slope direction.
fn tessellate_line_strip(
    points: &[[f32; 2]],
    out: &mut Vec<[f32; 2]>,
    line_width_px: f32,
    viewport_width: i32,
    viewport_height: i32,
) {
    out.clear();
    let n = points.len();
    if n < 2 {
        return;
    }
    out.reserve(n * 2);

    let half_w = line_width_px * 0.5;
    // Chart-to-pixel conversion factors:
    //   Chart x [0,1] → NDC [-1,1] via scale_x=2.0 → 1 chart unit = viewport_width/2 pixels
    //   Chart y [-1,1] → NDC [-1,1] via scale_y=1.0 → 1 chart unit = viewport_height/2 pixels
    // So 1 pixel = 2.0/viewport_width chart-x units, 2.0/viewport_height chart-y units.
    let px_to_chart_x = 2.0 / viewport_width as f32;
    let px_to_chart_y = 2.0 / viewport_height as f32;

    for i in 0..n {
        // Compute tangent direction.
        let (tx, ty) = if i == 0 {
            (points[1][0] - points[0][0], points[1][1] - points[0][1])
        } else if i == n - 1 {
            (
                points[n - 1][0] - points[n - 2][0],
                points[n - 1][1] - points[n - 2][1],
            )
        } else {
            (
                points[i + 1][0] - points[i - 1][0],
                points[i + 1][1] - points[i - 1][1],
            )
        };

        // Perpendicular normal: rotate tangent 90° → (-ty, tx).
        let nx = -ty;
        let ny = tx;

        // Convert normal to pixel space, normalize, scale by half_w, convert back.
        let nx_px = nx / px_to_chart_x;
        let ny_px = ny / px_to_chart_y;
        let len_px = (nx_px * nx_px + ny_px * ny_px).sqrt();
        if len_px < 1e-6 {
            // Degenerate segment — emit the point twice to keep strip valid.
            out.push(points[i]);
            out.push(points[i]);
            continue;
        }
        let scale = half_w / len_px;
        let offset_x = nx_px * scale * px_to_chart_x;
        let offset_y = ny_px * scale * px_to_chart_y;

        out.push([points[i][0] + offset_x, points[i][1] + offset_y]);
        out.push([points[i][0] - offset_x, points[i][1] - offset_y]);
    }
}

// ---------------------------------------------------------------------------
// GpuPlotRenderer
// ---------------------------------------------------------------------------

struct GpuPlotRenderer {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    loc_scale: glow::UniformLocation,
    loc_offset: glow::UniformLocation,
    loc_color: glow::UniformLocation,
    // Reusable waveform buffers (owned here to simplify the render API).
    unit_wave: Vec<[f32; 2]>,
    scaled_wave: Vec<[f32; 2]>,
    fixture_dots: Vec<[f32; 2]>,
    tessellated: Vec<[f32; 2]>,
}

impl GpuPlotRenderer {
    /// Initialize GL resources. Called during `RenderingSetup`.
    ///
    /// # Safety
    /// The `get_proc_address` function must return valid GL function pointers.
    unsafe fn new(get_proc_address: &dyn Fn(&CStr) -> *const std::ffi::c_void) -> Self {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };

        // Compile shaders.
        let vs = Self::compile_shader(&gl, glow::VERTEX_SHADER, VERTEX_SHADER_SRC);
        let fs = Self::compile_shader(&gl, glow::FRAGMENT_SHADER, FRAGMENT_SHADER_SRC);

        // Link program.
        let program = unsafe { gl.create_program() }.expect("create program");
        unsafe {
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
        }
        assert!(
            unsafe { gl.get_program_link_status(program) },
            "shader link failed: {}",
            unsafe { gl.get_program_info_log(program) }
        );
        unsafe {
            gl.delete_shader(vs);
            gl.delete_shader(fs);
        }

        let loc_scale =
            unsafe { gl.get_uniform_location(program, "u_scale") }.expect("u_scale uniform");
        let loc_offset =
            unsafe { gl.get_uniform_location(program, "u_offset") }.expect("u_offset uniform");
        let loc_color =
            unsafe { gl.get_uniform_location(program, "u_color") }.expect("u_color uniform");

        // Create VAO (required by OpenGL Core Profile on macOS) and VBO.
        let vao = unsafe { gl.create_vertex_array() }.expect("create VAO");
        let vbo = unsafe { gl.create_buffer() }.expect("create VBO");

        Self {
            gl,
            program,
            vao,
            vbo,
            loc_scale,
            loc_offset,
            loc_color,
            unit_wave: Vec::with_capacity(1000),
            scaled_wave: Vec::with_capacity(1000),
            fixture_dots: Vec::with_capacity(64),
            tessellated: Vec::with_capacity(2000),
        }
    }

    fn compile_shader(gl: &glow::Context, shader_type: u32, source: &str) -> glow::Shader {
        let shader = unsafe { gl.create_shader(shader_type) }.expect("create shader");
        unsafe {
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
        }
        assert!(
            unsafe { gl.get_shader_compile_status(shader) },
            "shader compile failed: {}",
            unsafe { gl.get_shader_info_log(shader) }
        );
        shader
    }

    /// Render the plot. Called during `BeforeRendering`.
    fn render(&mut self, state: &AnimationServiceState, viewport_width: i32, viewport_height: i32) {
        // --- Save GL state that FemtoVG relies on ---
        let saved = unsafe { SavedGlState::save(&self.gl) };

        unsafe {
            self.gl.viewport(0, 0, viewport_width, viewport_height);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.use_program(Some(self.program));
            self.gl.bind_vertex_array(Some(self.vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            // Enable line smoothing for anti-aliased lines.
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.enable(glow::LINE_SMOOTH);
            self.gl.hint(glow::LINE_SMOOTH_HINT, glow::NICEST);
            self.gl.enable(glow::PROGRAM_POINT_SIZE);

            // Projection: map chart [0,1]×[-1,1] to NDC [-1,1]×[-1,1].
            // ndc_x = x * 2.0 - 1.0  →  scale.x = 2.0, offset.x = -1.0
            // ndc_y = y * 1.0 + 0.0  →  scale.y = 1.0, offset.y =  0.0
            self.gl.uniform_2_f32(Some(&self.loc_scale), 2.0, 1.0);
            self.gl.uniform_2_f32(Some(&self.loc_offset), -1.0, 0.0);

            // Enable vertex attrib for a_position (location 0).
            self.gl.enable_vertex_attrib_array(0);
            self.gl
                .vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 0, 0);
        }

        // 1. Background quad (dark gray) — NDC-space, identity projection.
        self.set_identity_projection();
        self.upload_and_draw(
            &BACKGROUND_QUAD,
            glow::TRIANGLES,
            [30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0, 1.0],
        );
        self.set_chart_projection();

        // 2. Gridlines at y = ±0.25, ±0.5, ±0.75, ±1.0.
        self.upload_and_draw(&GRIDLINE_VERTICES, glow::LINES, [0.25, 0.25, 0.25, 0.5]);

        // 3. Generate and draw unit waveform (dark red) with tessellated thick lines.
        generate_unit_wave(state, &mut self.unit_wave);
        self.draw_tessellated_line_strip(
            0,
            [139.0 / 255.0, 0.0, 0.0, 1.0],
            3.0,
            viewport_width,
            viewport_height,
        );

        // 4. Generate and draw scaled waveform (white) with tessellated thick lines.
        generate_scaled_wave(state, &self.unit_wave, &mut self.scaled_wave);
        self.draw_tessellated_line_strip(
            1,
            [1.0, 1.0, 1.0, 1.0],
            3.0,
            viewport_width,
            viewport_height,
        );

        // 5. Generate and draw fixture dots (cyan).
        generate_fixture_dots(state, &mut self.fixture_dots);
        self.upload_and_draw_owned_buf(2, glow::POINTS, [0.0, 1.0, 1.0, 1.0]);

        // --- Restore GL state ---
        unsafe {
            self.gl.disable_vertex_attrib_array(0);
            saved.restore(&self.gl);
        }
    }

    /// Draw a line strip as a tessellated triangle strip for uniform pixel-width lines.
    fn draw_tessellated_line_strip(
        &mut self,
        buf_index: usize,
        color: [f32; 4],
        line_width_px: f32,
        viewport_width: i32,
        viewport_height: i32,
    ) {
        let points = match buf_index {
            0 => &self.unit_wave as &[_],
            1 => &self.scaled_wave as &[_],
            _ => return,
        };
        if points.len() < 2 {
            return;
        }
        tessellate_line_strip(
            points,
            &mut self.tessellated,
            line_width_px,
            viewport_width,
            viewport_height,
        );
        unsafe {
            self.gl.uniform_4_f32(
                Some(&self.loc_color),
                color[0],
                color[1],
                color[2],
                color[3],
            );
            self.gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                as_gl_bytes(&self.tessellated),
                glow::DYNAMIC_DRAW,
            );
            self.gl
                .draw_arrays(glow::TRIANGLE_STRIP, 0, self.tessellated.len() as i32);
        }
    }

    fn set_identity_projection(&self) {
        unsafe {
            self.gl.uniform_2_f32(Some(&self.loc_scale), 1.0, 1.0);
            self.gl.uniform_2_f32(Some(&self.loc_offset), 0.0, 0.0);
        }
    }

    fn set_chart_projection(&self) {
        unsafe {
            self.gl.uniform_2_f32(Some(&self.loc_scale), 2.0, 1.0);
            self.gl.uniform_2_f32(Some(&self.loc_offset), -1.0, 0.0);
        }
    }

    fn upload_and_draw(&self, data: &[[f32; 2]], mode: u32, color: [f32; 4]) {
        if data.is_empty() {
            return;
        }
        unsafe {
            self.gl.uniform_4_f32(
                Some(&self.loc_color),
                color[0],
                color[1],
                color[2],
                color[3],
            );
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, as_gl_bytes(data), glow::DYNAMIC_DRAW);
            self.gl.draw_arrays(mode, 0, data.len() as i32);
        }
    }

    /// Upload and draw from one of the owned buffers (0=unit_wave, 1=scaled_wave, 2=fixture_dots).
    fn upload_and_draw_owned_buf(&self, buf_index: usize, mode: u32, color: [f32; 4]) {
        let data = match buf_index {
            0 => &self.unit_wave,
            1 => &self.scaled_wave,
            2 => &self.fixture_dots,
            _ => return,
        };
        self.upload_and_draw(data, mode, color);
    }

    /// Force framebuffer alpha to 1.0 everywhere. Called during `AfterRendering`
    /// to prevent the window compositor from treating transparent Slint scene
    /// pixels as see-through.
    fn fix_alpha(&self, viewport_width: i32, viewport_height: i32) {
        let gl = &self.gl;
        let saved = unsafe { SavedGlState::save(gl) };
        unsafe {
            gl.viewport(0, 0, viewport_width, viewport_height);
            gl.disable(glow::SCISSOR_TEST);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 0, 0);

            // Only write to the alpha channel.
            gl.color_mask(false, false, false, true);
            gl.disable(glow::BLEND);

            // Draw fullscreen quad with alpha=1.0 (RGB values don't matter).
            gl.uniform_2_f32(Some(&self.loc_scale), 1.0, 1.0);
            gl.uniform_2_f32(Some(&self.loc_offset), 0.0, 0.0);
            gl.uniform_4_f32(Some(&self.loc_color), 0.0, 0.0, 0.0, 1.0);
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                as_gl_bytes(&BACKGROUND_QUAD),
                glow::DYNAMIC_DRAW,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, BACKGROUND_QUAD.len() as i32);

            // Restore color mask and state.
            gl.color_mask(true, true, true, true);
            gl.disable_vertex_attrib_array(0);
            saved.restore(gl);
        }
    }

    fn destroy(&self) {
        unsafe {
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_buffer(self.vbo);
            self.gl.delete_program(self.program);
        }
    }
}

// ---------------------------------------------------------------------------
// GL state save/restore (critical for FemtoVG coexistence)
// ---------------------------------------------------------------------------

struct SavedGlState {
    blend_enabled: bool,
    scissor_enabled: bool,
    viewport: [i32; 4],
    blend_src_rgb: i32,
    blend_dst_rgb: i32,
    blend_src_alpha: i32,
    blend_dst_alpha: i32,
    current_program: Option<glow::Program>,
    bound_vao: Option<glow::VertexArray>,
    bound_vbo: Option<glow::Buffer>,
    bound_texture_2d: Option<glow::Texture>,
    active_texture: i32,
}

impl SavedGlState {
    unsafe fn save(gl: &glow::Context) -> Self {
        Self {
            blend_enabled: unsafe { gl.is_enabled(glow::BLEND) },
            scissor_enabled: unsafe { gl.is_enabled(glow::SCISSOR_TEST) },
            viewport: {
                let mut v = [0i32; 4];
                unsafe { gl.get_parameter_i32_slice(glow::VIEWPORT, &mut v) };
                v
            },
            blend_src_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_RGB) },
            blend_dst_rgb: unsafe { gl.get_parameter_i32(glow::BLEND_DST_RGB) },
            blend_src_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_SRC_ALPHA) },
            blend_dst_alpha: unsafe { gl.get_parameter_i32(glow::BLEND_DST_ALPHA) },
            current_program: unsafe { gl.get_parameter_program(glow::CURRENT_PROGRAM) },
            bound_vao: unsafe { gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING) },
            bound_vbo: unsafe { gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING) },
            bound_texture_2d: unsafe { gl.get_parameter_texture(glow::TEXTURE_BINDING_2D) },
            active_texture: unsafe { gl.get_parameter_i32(glow::ACTIVE_TEXTURE) },
        }
    }

    unsafe fn restore(self, gl: &glow::Context) {
        if self.blend_enabled {
            unsafe { gl.enable(glow::BLEND) };
        } else {
            unsafe { gl.disable(glow::BLEND) };
        }
        if self.scissor_enabled {
            unsafe { gl.enable(glow::SCISSOR_TEST) };
        } else {
            unsafe { gl.disable(glow::SCISSOR_TEST) };
        }
        unsafe {
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.blend_func_separate(
                self.blend_src_rgb as u32,
                self.blend_dst_rgb as u32,
                self.blend_src_alpha as u32,
                self.blend_dst_alpha as u32,
            );
            gl.use_program(self.current_program);
            gl.bind_vertex_array(self.bound_vao);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.bound_vbo);
            gl.active_texture(self.active_texture as u32);
            gl.bind_texture(glow::TEXTURE_2D, self.bound_texture_2d);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the animation visualizer with GPU-accelerated plot rendering.
pub fn run_visualizer() -> Result<()> {
    // Force OpenGL backend for guaranteed GL context access.
    slint::BackendSelector::new()
        .require_opengl()
        .select()
        .map_err(|e| anyhow::anyhow!("Failed to select OpenGL backend: {e}"))?;

    let window = VisualizerWindow::new()?;

    let dirty = Arc::new(AtomicBool::new(true));
    let dirty_writer = dirty.clone();
    let animation_state = start_service(zmq::Context::new(), move || {
        dirty_writer.store(true, Ordering::Relaxed);
    })?;

    // Arc<Mutex<Option<...>>> is required because set_rendering_notifier
    // demands a Send closure, even though all callbacks run on the render thread.
    let renderer: Arc<Mutex<Option<GpuPlotRenderer>>> = Arc::new(Mutex::new(None));
    let renderer_for_notifier = renderer.clone();

    let dirty_for_notifier = dirty.clone();
    let state_for_notifier = animation_state.clone();
    let window_weak_for_notifier = window.as_weak();

    window
        .window()
        .set_rendering_notifier(move |state, graphics_api| {
            match state {
                slint::RenderingState::RenderingSetup => {
                    let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = graphics_api else {
                        return;
                    };
                    let gpu_renderer = unsafe { GpuPlotRenderer::new(get_proc_address) };
                    *renderer_for_notifier.lock().unwrap() = Some(gpu_renderer);
                }
                slint::RenderingState::BeforeRendering => {
                    let mut guard = renderer_for_notifier.lock().unwrap();
                    let Some(gpu_renderer) = guard.as_mut() else {
                        return;
                    };

                    // Get viewport dimensions from the window (already physical pixels).
                    let Some(win) = window_weak_for_notifier.upgrade() else {
                        return;
                    };
                    let size = win.window().size();
                    let w = size.width as i32;
                    let h = size.height as i32;
                    if w <= 0 || h <= 0 {
                        return;
                    }

                    let anim_state = state_for_notifier.lock().unwrap();
                    gpu_renderer.render(&anim_state, w, h);
                }
                slint::RenderingState::AfterRendering => {
                    let guard = renderer_for_notifier.lock().unwrap();
                    let Some(gpu_renderer) = guard.as_ref() else {
                        return;
                    };

                    let Some(win) = window_weak_for_notifier.upgrade() else {
                        return;
                    };
                    let size = win.window().size();
                    let w = size.width as i32;
                    let h = size.height as i32;
                    if w <= 0 || h <= 0 {
                        return;
                    }

                    gpu_renderer.fix_alpha(w, h);
                }
                slint::RenderingState::RenderingTeardown => {
                    if let Some(gpu_renderer) = renderer_for_notifier.lock().unwrap().take() {
                        gpu_renderer.destroy();
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| anyhow::anyhow!("Failed to set rendering notifier: {e:?}"))?;

    // Timer to request repaints when dirty.
    let window_weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(25),
        move || {
            if dirty_for_notifier.swap(false, Ordering::Relaxed)
                && let Some(win) = window_weak.upgrade()
            {
                win.window().request_redraw();
            }
        },
    );

    window.run()?;
    Ok(())
}
