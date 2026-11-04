//! The OpenGL calls.
//!
//! Every call here is `unsafe` in `glow` because it needs a current OpenGL context. The host's painter
//! (egui) guarantees that while it runs our callback, and nothing in this module keeps a context alive or
//! shares objects between threads, so the blocks below only have to use handles created from the same
//! context.
#![allow(unsafe_code)]

use glow::HasContext as _;
use ink_brush::{Accumulate, MaskKind, STAMP_STRIDE, StrokeBatch};

use crate::shaders;

/// Canvas units covered by one tile of the grain texture.
const GRAIN_PERIOD: f32 = 256.0;
/// Pixels added around a stroke's bounding box so antialiased edges are not cut off.
const EDGE_MARGIN: i32 = 2;
/// The mean of the marker's grain texture, used when it is not loaded.
const SCREEN_TEXTURE_AVERAGE: f32 = 0.65;
/// Screen points that one tile of a screen-space texture covers.
const SCREEN_TILE_POINTS: f32 = 64.0;
/// Keeps rectangle arithmetic far from `i32` overflow.
const COORD_LIMIT: f32 = 1.0e7;

/// Why the renderer could not start or draw.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("OpenGL {0} is too old: the renderer needs 3.3 or OpenGL ES 3.0")]
    Unsupported(String),
    #[error("the {stage} shader did not compile: {log}")]
    Shader { stage: &'static str, log: String },
    #[error("the shader program did not link: {0}")]
    Link(String),
    #[error("could not create a {0}: {1}")]
    Create(&'static str, String),
    #[error("an offscreen framebuffer is incomplete (status {0:#x})")]
    Incomplete(u32),
}

/// A rectangle in framebuffer pixels. Like OpenGL, `y` counts up from the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    fn intersect(self, other: Self) -> Option<Self> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.width).min(other.x + other.width);
        let y1 = (self.y + self.height).min(other.y + other.height);
        (x1 > x0 && y1 > y0).then_some(Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    fn moved(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

/// Where the frame ends up: the host's framebuffer, the canvas area inside it, and the part of that area
/// the host allows drawing to.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub framebuffer: Option<glow::Framebuffer>,
    pub viewport: PixelRect,
    pub clip: PixelRect,
}

/// How canvas coordinates map to pixels of the canvas area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Pixels per canvas unit.
    pub zoom: f32,
    /// Where canvas (0, 0) lands, in pixels from the top left of the canvas area. Canvas y points up.
    pub origin: [f32; 2],
    /// How far the paper is turned about the origin, in radians, counterclockwise as it is seen.
    pub angle: f32,
}

impl View {
    /// Where the canvas point (`x`, `y`) lands, in pixels from the top left of the canvas area.
    fn pixel(self, x: f32, y: f32) -> (f32, f32) {
        let (sin, cos) = self.angle.sin_cos();
        let (vx, vy) = (x * self.zoom, -y * self.zoom);
        (
            self.origin[0] + vx * cos + vy * sin,
            self.origin[1] - vx * sin + vy * cos,
        )
    }
}

/// A stroke to draw and the opacity of the layer it is on.
#[derive(Debug, Clone, Copy)]
pub struct Placed<'a> {
    pub batch: &'a StrokeBatch,
    pub opacity: f32,
}

/// Everything one frame needs.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub screen: Screen,
    pub view: View,
    /// Physical pixels per point of the host's interface, for textures laid out in screen points.
    pub pixels_per_point: f32,
    /// Changes whenever the committed strokes do; the cached texture is redrawn when it or the view changes.
    pub revision: u64,
    /// The strokes already in the document, in paint order.
    pub committed: &'a [Placed<'a>],
    /// The strokes being drawn or moved right now, over the committed ones: the stroke a pen is writing, or the
    /// selection while it is dragged.
    pub live: &'a [Placed<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommittedKey {
    revision: u64,
    view: [u32; 4],
    size: [i32; 2],
}

#[derive(Debug)]
struct Target {
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
}

#[derive(Debug)]
struct Targets {
    width: i32,
    height: i32,
    /// One stroke's coverage, a single channel.
    mask: Target,
    /// All committed strokes, premultiplied RGBA.
    committed: Target,
}

type Uniform = Option<glow::UniformLocation>;

#[derive(Debug)]
struct StampProgram {
    program: glow::Program,
    view: Uniform,
    size: Uniform,
    hardness: Uniform,
    mask_mode: Uniform,
    force_alpha: Uniform,
}

#[derive(Debug)]
struct FillProgram {
    program: glow::Program,
    view: Uniform,
    size: Uniform,
}

#[derive(Debug)]
struct CompositeProgram {
    program: glow::Program,
    size: Uniform,
    origin: Uniform,
    view: Uniform,
    color: Uniform,
    grain: Uniform,
    reveal: Uniform,
    gain: Uniform,
    screen_mode: Uniform,
    screen_scale: Uniform,
}

#[derive(Debug)]
struct BlitProgram {
    program: glow::Program,
    size: Uniform,
    origin: Uniform,
}

/// One run of the stamp program over a stroke's stamps.
struct Stamps {
    texture: Option<glow::Texture>,
    atlas: bool,
    accumulate: Accumulate,
    /// Whether the run writes the red and green channels of the mask.
    channel: [bool; 2],
    /// A fixed alpha for every stamp, or a negative number to use each stamp's own.
    force_alpha: f32,
}

/// Where one stroke is composited.
struct Dest {
    framebuffer: Option<glow::Framebuffer>,
    viewport: PixelRect,
    clip: PixelRect,
}

/// The renderer. Create it once with [`Renderer::new`] and call [`Renderer::paint`] every frame.
#[derive(Debug)]
pub struct Renderer {
    stamp: StampProgram,
    fill: FillProgram,
    /// The triangles of a filled region, with the vertex array that reads them.
    fill_vao: glow::VertexArray,
    fill_buffer: glow::Buffer,
    composite: CompositeProgram,
    blit: BlitProgram,
    vao: glow::VertexArray,
    quad: glow::Buffer,
    instances: glow::Buffer,
    paper: glow::Texture,
    /// The stamp textures, in the order of [`MaskKind::TEXTURED`].
    masks: [Option<glow::Texture>; 6],
    targets: Option<Targets>,
    committed_key: Option<CommittedKey>,
    has_committed: bool,
    /// Texture coordinates per pixel for screen-space textures.
    screen_scale: f32,
}

fn shader_header(gl: &glow::Context) -> Result<&'static str, GpuError> {
    let v = gl.version();
    if v.is_embedded && v.major >= 3 {
        // Integers too: a uniform both stages declare must have the same precision in each, and ES gives the vertex
        // stage `highp` and the fragment stage `mediump` for them unless told otherwise.
        Ok(
            "#version 300 es\nprecision highp float;\nprecision highp int;\nprecision highp sampler2D;\n",
        )
    } else if !v.is_embedded && (v.major > 3 || (v.major == 3 && v.minor >= 3)) {
        Ok("#version 330 core\n")
    } else {
        Err(GpuError::Unsupported(format!(
            "{}.{}{}",
            v.major,
            v.minor,
            if v.is_embedded { " ES" } else { "" }
        )))
    }
}

fn link(
    gl: &glow::Context,
    header: &str,
    vertex: &str,
    fragment: &str,
) -> Result<glow::Program, GpuError> {
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| GpuError::Create("program", e))?;
        let mut shaders = Vec::new();
        for (kind, stage, source) in [
            (glow::VERTEX_SHADER, "vertex", vertex),
            (glow::FRAGMENT_SHADER, "fragment", fragment),
        ] {
            let shader = gl
                .create_shader(kind)
                .map_err(|e| GpuError::Create("shader", e))?;
            gl.shader_source(shader, &format!("{header}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                for done in shaders {
                    gl.delete_shader(done);
                }
                gl.delete_program(program);
                return Err(GpuError::Shader { stage, log });
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        let linked = gl.get_program_link_status(program);
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        if !linked {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(GpuError::Link(log));
        }
        Ok(program)
    }
}

fn uniform(gl: &glow::Context, program: glow::Program, name: &str) -> Uniform {
    unsafe { gl.get_uniform_location(program, name) }
}

fn new_texture(
    gl: &glow::Context,
    width: i32,
    height: i32,
    internal: u32,
    format: u32,
    filter: i32,
    pixels: Option<&[u8]>,
) -> Result<glow::Texture, GpuError> {
    unsafe {
        let texture = gl
            .create_texture()
            .map_err(|e| GpuError::Create("texture", e))?;
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal as i32,
            width,
            height,
            0,
            format,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(pixels),
        );
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        Ok(texture)
    }
}

fn new_target(
    gl: &glow::Context,
    width: i32,
    height: i32,
    internal: u32,
    format: u32,
) -> Result<Target, GpuError> {
    unsafe {
        let texture = new_texture(
            gl,
            width,
            height,
            internal,
            format,
            glow::NEAREST as i32,
            None,
        )?;
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        let framebuffer = match gl.create_framebuffer() {
            Ok(f) => f,
            Err(e) => {
                gl.delete_texture(texture);
                return Err(GpuError::Create("framebuffer", e));
            }
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return Err(GpuError::Incomplete(status));
        }
        Ok(Target {
            framebuffer,
            texture,
        })
    }
}

fn delete_target(gl: &glow::Context, target: &Target) {
    unsafe {
        gl.delete_framebuffer(target.framebuffer);
        gl.delete_texture(target.texture);
    }
}

/// The pixels a stroke can touch, as a rectangle of a `width` by `height` target.
fn stroke_area(bounds: &[f32; 4], view: View, width: i32, height: i32) -> Option<PixelRect> {
    let px = |v: f32| v.clamp(-COORD_LIMIT, COORD_LIMIT);
    let [min_x, min_y, max_x, max_y] = *bounds;
    // The paper may be turned, so the box is the one round all four corners.
    let corners = [
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ]
    .map(|(x, y)| view.pixel(x, y));
    let low = |pick: fn(&(f32, f32)) -> f32| corners.iter().map(pick).fold(f32::INFINITY, f32::min);
    let high =
        |pick: fn(&(f32, f32)) -> f32| corners.iter().map(pick).fold(f32::NEG_INFINITY, f32::max);
    let left = px(low(|c| c.0)).floor() as i32 - EDGE_MARGIN;
    let right = px(high(|c| c.0)).ceil() as i32 + EDGE_MARGIN;
    let top = px(low(|c| c.1)).floor() as i32 - EDGE_MARGIN;
    let bottom = px(high(|c| c.1)).ceil() as i32 + EDGE_MARGIN;
    PixelRect {
        x: left,
        y: height - bottom,
        width: right - left,
        height: bottom - top,
    }
    .intersect(PixelRect {
        x: 0,
        y: 0,
        width,
        height,
    })
}

fn set_scissor(gl: &glow::Context, r: PixelRect) {
    unsafe { gl.scissor(r.x, r.y, r.width, r.height) }
}

fn set_viewport(gl: &glow::Context, r: PixelRect) {
    unsafe { gl.viewport(r.x, r.y, r.width, r.height) }
}

impl Renderer {
    /// Compiles the shaders and creates the buffers and the paper texture.
    ///
    /// # Errors
    /// Fails when the OpenGL version is too old or a shader does not compile; callers should fall back to
    /// another renderer.
    #[allow(
        clippy::too_many_lines,
        reason = "creating the programs, buffers and uniforms is one straight list"
    )]
    pub fn new(gl: &glow::Context) -> Result<Self, GpuError> {
        let header = shader_header(gl)?;
        let stamp = link(gl, header, shaders::STAMP_VERT, shaders::STAMP_FRAG)?;
        let fill = link(gl, header, shaders::FILL_VERT, shaders::FILL_FRAG)?;
        let composite = link(
            gl,
            header,
            shaders::FULLSCREEN_VERT,
            shaders::COMPOSITE_FRAG,
        )?;
        let blit = link(gl, header, shaders::FULLSCREEN_VERT, shaders::BLIT_FRAG)?;
        let grain = ink_brush::grain::paper_grain();
        let size = ink_brush::grain::SIZE as i32;
        let paper = Self::paper_texture(gl, size, &grain)?;
        unsafe {
            let vao = gl
                .create_vertex_array()
                .map_err(|e| GpuError::Create("vertex array", e))?;
            let quad = gl
                .create_buffer()
                .map_err(|e| GpuError::Create("buffer", e))?;
            let instances = gl
                .create_buffer()
                .map_err(|e| GpuError::Create("buffer", e))?;
            let corners: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&corners),
                glow::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instances));
            gl.enable_vertex_attrib_array(1);
            let stride = (STAMP_STRIDE * 4) as i32;
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, stride, 0);
            gl.vertex_attrib_divisor(1, 1);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 16);
            gl.vertex_attrib_divisor(2, 1);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            let fill_vao = gl
                .create_vertex_array()
                .map_err(|e| GpuError::Create("vertex array", e))?;
            let fill_buffer = gl
                .create_buffer()
                .map_err(|e| GpuError::Create("buffer", e))?;
            gl.bind_vertex_array(Some(fill_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(fill_buffer));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            gl.use_program(Some(stamp));
            gl.uniform_1_i32(uniform(gl, stamp, "u_stamp_mask").as_ref(), 0);
            gl.use_program(Some(composite));
            gl.uniform_1_i32(uniform(gl, composite, "u_mask").as_ref(), 0);
            gl.uniform_1_i32(uniform(gl, composite, "u_paper").as_ref(), 1);
            gl.uniform_1_i32(uniform(gl, composite, "u_screen").as_ref(), 2);
            gl.use_program(Some(blit));
            gl.uniform_1_i32(uniform(gl, blit, "u_tex").as_ref(), 0);
            gl.use_program(None);

            Ok(Self {
                fill: FillProgram {
                    program: fill,
                    view: uniform(gl, fill, "u_view"),
                    size: uniform(gl, fill, "u_size"),
                },
                fill_vao,
                fill_buffer,
                stamp: StampProgram {
                    program: stamp,
                    view: uniform(gl, stamp, "u_view"),
                    size: uniform(gl, stamp, "u_size"),
                    hardness: uniform(gl, stamp, "u_hardness"),
                    mask_mode: uniform(gl, stamp, "u_mask_mode"),
                    force_alpha: uniform(gl, stamp, "u_force_alpha"),
                },
                composite: CompositeProgram {
                    program: composite,
                    size: uniform(gl, composite, "u_size"),
                    origin: uniform(gl, composite, "u_target_origin"),
                    view: uniform(gl, composite, "u_view"),
                    color: uniform(gl, composite, "u_color"),
                    grain: uniform(gl, composite, "u_grain"),
                    reveal: uniform(gl, composite, "u_reveal"),
                    gain: uniform(gl, composite, "u_gain"),
                    screen_mode: uniform(gl, composite, "u_screen_mode"),
                    screen_scale: uniform(gl, composite, "u_screen_scale"),
                },
                blit: BlitProgram {
                    program: blit,
                    size: uniform(gl, blit, "u_size"),
                    origin: uniform(gl, blit, "u_target_origin"),
                },
                vao,
                quad,
                instances,
                paper,
                masks: [None; 6],
                screen_scale: 0.0,
                targets: None,
                committed_key: None,
                has_committed: false,
            })
        }
    }

    fn paper_texture(
        gl: &glow::Context,
        size: i32,
        pixels: &[u8],
    ) -> Result<glow::Texture, GpuError> {
        let texture = new_texture(
            gl,
            size,
            size,
            glow::R8,
            glow::RED,
            glow::LINEAR_MIPMAP_LINEAR as i32,
            Some(pixels),
        )?;
        unsafe {
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
            gl.generate_mipmap(glow::TEXTURE_2D);
        }
        Ok(texture)
    }

    /// Replaces the paper grain with a square single-channel texture, `size` pixels on a side.
    ///
    /// # Errors
    /// Fails if the texture cannot be created.
    pub fn set_grain(
        &mut self,
        gl: &glow::Context,
        size: i32,
        pixels: &[u8],
    ) -> Result<(), GpuError> {
        let texture = Self::paper_texture(gl, size, pixels)?;
        unsafe { gl.delete_texture(self.paper) };
        self.paper = texture;
        self.committed_key = None;
        Ok(())
    }

    fn mask_texture(&self, kind: MaskKind) -> Option<glow::Texture> {
        MaskKind::TEXTURED
            .iter()
            .position(|k| *k == kind)
            .and_then(|i| self.masks[i])
    }

    /// Sets the texture that shapes the stamps of `kind`: a square single-channel image, `size` pixels on a side.
    /// Pens whose texture was not set draw plain discs.
    ///
    /// # Errors
    /// Fails if the texture cannot be created.
    pub fn set_mask(
        &mut self,
        gl: &glow::Context,
        kind: MaskKind,
        size: i32,
        pixels: &[u8],
    ) -> Result<(), GpuError> {
        let Some(slot) = MaskKind::TEXTURED.iter().position(|k| *k == kind) else {
            return Ok(());
        };
        let texture = Self::paper_texture(gl, size, pixels)?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            let wrap = if kind == MaskKind::Marker {
                glow::REPEAT
            } else {
                glow::CLAMP_TO_EDGE
            };
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap as i32);
            if let Some(old) = self.masks[slot].replace(texture) {
                gl.delete_texture(old);
            }
        }
        self.committed_key = None;
        Ok(())
    }

    /// Frees every OpenGL object. The renderer must not be used afterwards.
    pub fn destroy(&mut self, gl: &glow::Context) {
        if let Some(t) = self.targets.take() {
            delete_target(gl, &t.mask);
            delete_target(gl, &t.committed);
        }
        unsafe {
            gl.delete_program(self.stamp.program);
            gl.delete_program(self.fill.program);
            gl.delete_vertex_array(self.fill_vao);
            gl.delete_buffer(self.fill_buffer);
            gl.delete_program(self.composite.program);
            gl.delete_program(self.blit.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.quad);
            gl.delete_buffer(self.instances);
            gl.delete_texture(self.paper);
            for texture in self.masks.into_iter().flatten() {
                gl.delete_texture(texture);
            }
        }
    }

    fn ensure_targets(
        &mut self,
        gl: &glow::Context,
        width: i32,
        height: i32,
    ) -> Result<(), GpuError> {
        if self
            .targets
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height)
        {
            return Ok(());
        }
        if let Some(old) = self.targets.take() {
            delete_target(gl, &old.mask);
            delete_target(gl, &old.committed);
        }
        let mask = new_target(gl, width, height, glow::RG8, glow::RG)?;
        let committed = match new_target(gl, width, height, glow::RGBA8, glow::RGBA) {
            Ok(t) => t,
            Err(e) => {
                delete_target(gl, &mask);
                return Err(e);
            }
        };
        self.targets = Some(Targets {
            width,
            height,
            mask,
            committed,
        });
        self.committed_key = None;
        Ok(())
    }

    /// Draws one frame into the host's framebuffer.
    ///
    /// # Errors
    /// Fails if the offscreen targets cannot be created.
    pub fn paint(&mut self, gl: &glow::Context, frame: &Frame<'_>) -> Result<(), GpuError> {
        let screen = frame.screen;
        let (width, height) = (screen.viewport.width, screen.viewport.height);
        if width <= 0 || height <= 0 {
            return Ok(());
        }
        self.ensure_targets(gl, width, height)?;
        self.screen_scale = 1.0 / (SCREEN_TILE_POINTS * frame.pixels_per_point.max(1e-3));
        unsafe {
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::SCISSOR_TEST);
            gl.enable(glow::BLEND);
            gl.color_mask(true, true, true, true);
            gl.bind_vertex_array(Some(self.vao));
        }

        let key = CommittedKey {
            revision: frame.revision,
            view: [
                frame.view.zoom.to_bits(),
                frame.view.origin[0].to_bits(),
                frame.view.origin[1].to_bits(),
                frame.view.angle.to_bits(),
            ],
            size: [width, height],
        };
        if self.committed_key != Some(key) {
            self.render_committed(gl, frame);
            self.committed_key = Some(key);
            self.has_committed = !frame.committed.is_empty();
        }

        let dest = Dest {
            framebuffer: screen.framebuffer,
            viewport: screen.viewport,
            clip: screen.clip,
        };
        if self.has_committed {
            self.blit_committed(gl, &dest);
        }
        for live in frame.live {
            self.draw_stroke(gl, *live, frame.view, &dest);
        }

        unsafe {
            gl.bind_vertex_array(None);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_framebuffer(glow::FRAMEBUFFER, screen.framebuffer);
        }
        set_viewport(gl, screen.viewport);
        set_scissor(gl, screen.clip);
        Ok(())
    }

    fn render_committed(&self, gl: &glow::Context, frame: &Frame<'_>) {
        let Some(targets) = &self.targets else { return };
        let area = PixelRect {
            x: 0,
            y: 0,
            width: targets.width,
            height: targets.height,
        };
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(targets.committed.framebuffer));
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
        }
        set_viewport(gl, area);
        set_scissor(gl, area);
        unsafe { gl.clear(glow::COLOR_BUFFER_BIT) };
        let dest = Dest {
            framebuffer: Some(targets.committed.framebuffer),
            viewport: area,
            clip: area,
        };
        for placed in frame.committed {
            self.draw_stroke(gl, *placed, frame.view, &dest);
        }
    }

    fn blit_committed(&self, gl: &glow::Context, dest: &Dest) {
        let Some(targets) = &self.targets else { return };
        let Some(visible) = dest.viewport.intersect(dest.clip) else {
            return;
        };
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, dest.framebuffer);
        }
        set_viewport(gl, dest.viewport);
        set_scissor(gl, visible);
        unsafe {
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.blit.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(targets.committed.texture));
            gl.uniform_2_f32(
                self.blit.size.as_ref(),
                targets.width as f32,
                targets.height as f32,
            );
            gl.uniform_2_f32(
                self.blit.origin.as_ref(),
                dest.viewport.x as f32,
                dest.viewport.y as f32,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }

    /// Stamps one stroke into the mask, then composites the mask onto `dest`.
    fn draw_stroke(&self, gl: &glow::Context, placed: Placed<'_>, view: View, dest: &Dest) {
        let Some(targets) = &self.targets else { return };
        let batch = placed.batch;
        let Some(area) = stroke_area(&batch.bounds, view, targets.width, targets.height) else {
            return;
        };
        if batch.is_empty() {
            return;
        }
        self.stamp_pass(gl, batch, view, targets, area);
        let Some(visible) = area
            .moved(dest.viewport.x, dest.viewport.y)
            .intersect(dest.clip)
        else {
            return;
        };
        self.composite_pass(gl, placed, view, dest, targets, visible);
    }

    /// Stage one: the stamps, accumulated into the mask. Red holds the stroke; for a pen with a reveal texture,
    /// green holds the union of the reveal stamps.
    fn stamp_pass(
        &self,
        gl: &glow::Context,
        batch: &StrokeBatch,
        view: View,
        targets: &Targets,
        area: PixelRect,
    ) {
        let style = batch.style;
        let (width, height) = (targets.width as f32, targets.height as f32);
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(targets.mask.framebuffer));
            gl.viewport(0, 0, targets.width, targets.height);
        }
        set_scissor(gl, area);
        unsafe {
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
        // The inside of a region goes in first, and the stamps along its edge over it.
        self.draw_fill(gl, batch, view, (width, height));
        unsafe {
            gl.use_program(Some(self.stamp.program));
            gl.uniform_4_f32(
                self.stamp.view.as_ref(),
                view.zoom,
                view.origin[0],
                view.origin[1],
                view.angle,
            );
            gl.uniform_2_f32(self.stamp.size.as_ref(), width, height);
            gl.uniform_1_f32(self.stamp.hardness.as_ref(), style.hardness);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instances));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&batch.stamps),
                glow::STREAM_DRAW,
            );
        }
        let stamp = self.mask_texture(style.mask);
        self.draw_stamps(
            gl,
            batch,
            &Stamps {
                texture: stamp,
                atlas: style.mask.is_atlas(),
                accumulate: style.accumulate,
                channel: [true, false],
                force_alpha: -1.0,
            },
        );
        if let (Some(_), Some(reveal)) = (stamp, self.mask_texture(style.reveal)) {
            self.draw_stamps(
                gl,
                batch,
                &Stamps {
                    texture: Some(reveal),
                    atlas: false,
                    accumulate: Accumulate::Max,
                    channel: [false, true],
                    force_alpha: 1.0,
                },
            );
        }
        unsafe { gl.color_mask(true, true, true, true) };
    }

    /// Covers the inside of a region, in the red channel of the mask, with the triangles of `batch`.
    fn draw_fill(&self, gl: &glow::Context, batch: &StrokeBatch, view: View, size: (f32, f32)) {
        if batch.fill.is_empty() {
            return;
        }
        unsafe {
            gl.color_mask(true, false, false, false);
            gl.blend_equation(glow::MAX);
            gl.blend_func(glow::ONE, glow::ONE);
            gl.use_program(Some(self.fill.program));
            gl.uniform_4_f32(
                self.fill.view.as_ref(),
                view.zoom,
                view.origin[0],
                view.origin[1],
                view.angle,
            );
            gl.uniform_2_f32(self.fill.size.as_ref(), size.0, size.1);
            gl.bind_vertex_array(Some(self.fill_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.fill_buffer));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&batch.fill),
                glow::STREAM_DRAW,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, (batch.fill.len() / 2) as i32);
            gl.bind_vertex_array(Some(self.vao));
        }
    }

    fn draw_stamps(&self, gl: &glow::Context, batch: &StrokeBatch, stamps: &Stamps) {
        unsafe {
            gl.color_mask(stamps.channel[0], stamps.channel[1], false, false);
            match stamps.accumulate {
                Accumulate::Max => {
                    gl.blend_equation(glow::MAX);
                    gl.blend_func(glow::ONE, glow::ONE);
                }
                Accumulate::Over => {
                    gl.blend_equation(glow::FUNC_ADD);
                    gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                }
            }
            let mode = match (stamps.texture, stamps.atlas) {
                (None, _) => 0,
                (Some(_), false) => 1,
                (Some(_), true) => 2,
            };
            gl.uniform_1_i32(self.stamp.mask_mode.as_ref(), mode);
            gl.uniform_1_f32(self.stamp.force_alpha.as_ref(), stamps.force_alpha);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, stamps.texture);
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, batch.len() as i32);
        }
    }

    /// Stage two: the mask, through the grain, in the stroke's color.
    fn composite_pass(
        &self,
        gl: &glow::Context,
        placed: Placed<'_>,
        view: View,
        dest: &Dest,
        targets: &Targets,
        visible: PixelRect,
    ) {
        let style = placed.batch.style;
        let (width, height) = (targets.width as f32, targets.height as f32);
        let [r, g, b, a] = placed.batch.color;
        let reveal =
            self.mask_texture(style.mask).is_some() && self.mask_texture(style.reveal).is_some();
        // A stroke with a screen-space texture but no such texture loaded gets the texture's average instead.
        let screen = self.mask_texture(style.screen_texture);
        let gain = if style.screen_texture != MaskKind::None && screen.is_none() {
            style.mask_gain * SCREEN_TEXTURE_AVERAGE
        } else {
            style.mask_gain
        };
        unsafe { gl.bind_framebuffer(glow::FRAMEBUFFER, dest.framebuffer) };
        set_viewport(gl, dest.viewport);
        set_scissor(gl, visible);
        unsafe {
            gl.blend_equation(glow::FUNC_ADD);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.composite.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(targets.mask.texture));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.paper));
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, screen);
            gl.active_texture(glow::TEXTURE0);
            gl.uniform_2_f32(self.composite.size.as_ref(), width, height);
            gl.uniform_2_f32(
                self.composite.origin.as_ref(),
                dest.viewport.x as f32,
                dest.viewport.y as f32,
            );
            gl.uniform_4_f32(
                self.composite.view.as_ref(),
                view.zoom,
                view.origin[0],
                view.origin[1],
                view.angle,
            );
            gl.uniform_4_f32(
                self.composite.color.as_ref(),
                r,
                g,
                b,
                (a * style.opacity * placed.opacity).clamp(0.0, 1.0),
            );
            gl.uniform_2_f32(self.composite.grain.as_ref(), style.grain, GRAIN_PERIOD);
            gl.uniform_1_i32(self.composite.reveal.as_ref(), i32::from(reveal));
            gl.uniform_1_f32(self.composite.gain.as_ref(), gain);
            gl.uniform_1_i32(
                self.composite.screen_mode.as_ref(),
                i32::from(screen.is_some()),
            );
            gl.uniform_1_f32(self.composite.screen_scale.as_ref(), self.screen_scale);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: View = View {
        zoom: 2.0,
        origin: [100.0, 100.0],
        angle: 0.0,
    };

    #[test]
    fn a_turned_paper_puts_a_stroke_where_the_turn_takes_it_and_the_area_covers_all_four_corners() {
        // A quarter turn counterclockwise about (100, 100): canvas right is up on the screen, canvas up is left.
        let turned = View {
            angle: std::f32::consts::FRAC_PI_2,
            ..VIEW
        };
        let (x, y) = turned.pixel(10.0, 0.0);
        assert!(
            (x - 100.0).abs() < 1e-3 && (y - 80.0).abs() < 1e-3,
            "{x} {y}"
        );
        let (x, y) = turned.pixel(0.0, 10.0);
        assert!(
            (x - 80.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3,
            "{x} {y}"
        );
        // The box [0, 10] by [0, 10] lands on x 80..100 and y 80..100.
        let area = stroke_area(&[0.0, 0.0, 10.0, 10.0], turned, 400, 200).expect("visible");
        assert_eq!((area.x, area.width), (78, 24));
        assert_eq!((area.y, area.height), (98, 24));
        // At an angle in between, the area is the box round the turned square, so it is bigger than the upright one.
        let slant = View {
            angle: std::f32::consts::FRAC_PI_4,
            ..VIEW
        };
        let upright = stroke_area(&[0.0, 0.0, 10.0, 10.0], VIEW, 400, 200).expect("visible");
        let askew = stroke_area(&[0.0, 0.0, 10.0, 10.0], slant, 400, 200).expect("visible");
        assert!(askew.width > upright.width && askew.height > upright.height);
    }

    #[test]
    fn a_stroke_area_covers_the_bounds_plus_a_margin_with_y_counted_from_the_bottom() {
        let area = stroke_area(&[0.0, 0.0, 10.0, 10.0], VIEW, 400, 200).expect("visible");
        // x: 100..120 plus 2 on each side; the canvas y range 0..10 is pixel rows 80..100 from the top
        assert_eq!((area.x, area.width), (98, 24));
        assert_eq!((area.y, area.height), (98, 24));
    }

    #[test]
    fn a_stroke_outside_the_target_has_no_area() {
        assert_eq!(
            stroke_area(&[1000.0, 0.0, 1010.0, 10.0], VIEW, 400, 200),
            None
        );
    }

    #[test]
    fn the_area_is_clipped_to_the_target() {
        let area =
            stroke_area(&[-1000.0, -1000.0, 1000.0, 1000.0], VIEW, 400, 200).expect("visible");
        assert_eq!(
            area,
            PixelRect {
                x: 0,
                y: 0,
                width: 400,
                height: 200
            }
        );
    }

    #[test]
    fn absurd_or_non_finite_bounds_do_not_overflow() {
        let _ = stroke_area(&[f32::MIN, f32::MIN, f32::MAX, f32::MAX], VIEW, 400, 200);
        let _ = stroke_area(&[f32::NAN; 4], VIEW, 400, 200);
    }

    #[test]
    fn rectangles_intersect_only_when_they_overlap() {
        let a = PixelRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        let b = a.moved(5, 5);
        assert_eq!(
            a.intersect(b),
            Some(PixelRect {
                x: 5,
                y: 5,
                width: 5,
                height: 5
            })
        );
        assert_eq!(a.intersect(a.moved(10, 0)), None);
    }
}
