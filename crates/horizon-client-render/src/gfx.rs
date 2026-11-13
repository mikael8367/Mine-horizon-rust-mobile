//! GPU renderer for streamed terrain sections, sky, block outline and HUD.
//! Section origins are supplied relative to the camera each frame so large
//! world coordinates keep full precision.
use crate::{
    Result,
    block_entity_render::{StaticCacheFrameStats, StaticSectionRevision, StaticSectionSnapshot},
    environment::Sky,
    fast_hash::FxHashMap,
    graphics::GraphicsApi,
    hud::{HudBatch, HudRenderer},
    mesher::{Part as Quads, SectionMesh, Sides, TerrainVertex, Vertex, packed},
    section_mask::SectionMask,
    sprites::SpriteAnimation,
};
use glam::{DVec3, Mat4, Vec3, Vec4};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
use wgpu::util::DeviceExt;
use winit::window::Window;

// RenderPipelines.CRUMBLING: dst * src + src * dst, alpha replaced.
const CRUMBLING_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::Src,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent::REPLACE,
};
/// `TerrainVertex` for `vs_terrain`: position, UV, layer, light, colour.
const TERRAIN_ATTRIBUTES: [wgpu::VertexAttribute; 5] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Uint16x2,2=>Uint16,4=>Uint8x2,3=>Unorm8x4];

/// entity.wgsl after the OverlayTexture constants it reads.
fn entity_shader() -> String {
    crate::overlay::wgsl_constants() + include_str!("hdr_light.wgsl") + include_str!("entity.wgsl")
}

fn no_pack_hdr_gain(layered: bool, pack: bool, world: bool) -> bool {
    layered && !pack && world
}

fn light_marks(format: wgpu::TextureFormat, hdr: bool) -> bool {
    hdr && format != LINEAR_FRAME
}

fn terrain_shader() -> String {
    include_str!("hdr_light.wgsl").to_owned() + include_str!("terrain.wgsl")
}

fn border_shader() -> String {
    include_str!("hdr_light.wgsl").to_owned() + include_str!("border.wgsl")
}

/// The static block-entity vertex entry point shares entity shading but lives
/// in its own source file and pipeline.
fn static_entity_shader() -> String {
    entity_shader() + include_str!("static_entity.wgsl")
}

#[cfg(test)]
mod crumbling_tests;
mod entity_batch;
mod entity_outline;
mod fluid;
#[cfg(test)]
mod inter_stage_tests;
mod section_fade;
mod terrain_model;
#[cfg(test)]
pub(crate) mod test_gpu;
mod weather;
mod world_text;
use entity_batch::EntityBatch;
pub use entity_batch::{
    ADDITIVE_PARTICLE_OVERLAY, DISSOLVE_OVERLAY, DRAGON_RAYS_OVERLAY, ENERGY_SWIRL_OVERLAY,
    FLUID_OVERLAY, OIT_ENTITY, SHADOW_OVERLAY, TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY,
    TERRAIN_MODEL_CUTOUT_OVERLAY, TERRAIN_MODEL_SOLID_OVERLAY, TEXT_POLYGON_OFFSET_OVERLAY,
    TRANSLUCENT_GLINT_ITEM_OVERLAY, TRANSLUCENT_ITEM_OVERLAY, TRANSLUCENT_PARTICLE_OVERLAY,
    WATER_MASK_OVERLAY, WORLD_TEXT_OVERLAY, WORLD_TEXT_POLYGON_OFFSET_OVERLAY, is_native_terrain,
    mark_oit,
};
use external::{ShadowCull, shadow_box, shadow_casters, shadow_runs};
use section_fade::{
    CHUNK_VISIBILITY_THRESHOLD, FadeInputs, chunk_visibility, fade_millis, section_instance,
    section_visibility,
};
pub use view::{View, ViewSet};

/// Window.getAppropriateLineWidth, in framebuffer pixels.
pub fn outline_width(width: u32) -> f32 {
    2.5f32.max(width as f32 / 1920.0 * 2.5)
}

/// ScreenSize, stroke width and ProjectionType.PERSPECTIVE layering.
pub fn outline_params(width: u32, height: u32) -> [f32; 4] {
    [
        width as f32,
        height.max(1) as f32,
        outline_width(width),
        1.0 - 1.0 / 4096.0,
    ]
}

/// Camera-relative segment endpoints; no shape expansion in vanilla 26.3.
pub fn outline_vertices(edges: &[[f64; 6]], camera: glam::DVec3) -> Vec<[f32; 6]> {
    let mut out = Vec::with_capacity(edges.len());
    outline_vertices_into(edges, camera, &mut out);
    out
}

fn outline_vertices_into(edges: &[[f64; 6]], camera: glam::DVec3, out: &mut Vec<[f32; 6]>) {
    let camera = camera.to_array();
    out.clear();
    out.extend(
        edges
            .iter()
            .map(|e| std::array::from_fn(|i| (e[i] - camera[i % 3]) as f32)),
    );
}

/// Whether the renderer should initialize its occlusion culling pass (`cull`).
///
/// Software adapters spend more time running the culling pass than drawing
/// the sections it removes. Keep the pass available for explicit A/B checks,
/// while leaving hardware adapters on the existing default.
fn occlusion_culling_requested(info: &wgpu::AdapterInfo, env: Option<&str>) -> bool {
    env != Some("0") && (env.is_some() || info.device_type != wgpu::DeviceType::Cpu)
}

/// `DrawableGizmoPrimitives.getGroup`: a line whose ARGB alpha is 255 joins the
/// opaque group, drawn with `RenderTypes.lines` (depth written); the rest
/// with `RenderTypes.linesTranslucentNoDepthWrite`.
fn debug_line_opaque(line: &[f32; 8]) -> bool {
    line[6].to_bits() >> 24 == 0xFF
}

/// The depth-tested gizmo lines in their upload order: the opaque group first,
/// then the translucent one, each in submission order.
fn debug_lines_opaque_first(lines: &[[f32; 8]]) -> impl Iterator<Item = &[f32; 8]> {
    let opaque = lines.iter().filter(|l| debug_line_opaque(l));
    opaque.chain(lines.iter().filter(|l| !debug_line_opaque(l)))
}

/// Whether Translucent Batching is offered (`graphics::offer_translucent_batching`): where
/// indirect multi-draws (`multi_draw`) take its batches, but not OpenGL's, which loop over
/// their entries on the CPU (and without them every section is a call of its own anyway).
/// Frames drawn flat (Metal, Intel Gen11/Gen12's Vulkan driver) take it too: their
/// translucent calls then come one per page of its arena rather than about one per section.
fn translucent_batchable(info: &wgpu::AdapterInfo, multi_draw: bool) -> bool {
    multi_draw && info.backend != wgpu::Backend::Gl
}

/// The vertices flat draws bind for a call's page key: a page of the section arena, or of
/// the translucent arena up to its last range in use (`flat::TRANSLUCENT_KEYS`).
fn flat_vertices<'a>(
    arena: &'a arena::Arena,
    translucent: Option<&'a arena::Arena>,
) -> impl Fn(u32) -> wgpu::BufferSlice<'a> + Copy {
    move |key| match (translucent, key.checked_sub(flat::TRANSLUCENT_KEYS)) {
        (Some(translucent), Some(page)) => translucent.binding(page),
        _ => arena.buffer(key).slice(..),
    }
}

/// Bytes of `texture` with every mip level of every layer.
pub(crate) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let size = texture.size();
    let block = u64::from(texture.format().block_copy_size(None).unwrap_or(4));
    (0..texture.mip_level_count())
        .map(|level| {
            let side = |v: u32| u64::from((v >> level).max(1));
            side(size.width) * side(size.height) * block
        })
        .sum::<u64>()
        * u64::from(size.depth_or_array_layers)
}

/// Whether translucent quads have an arena of their own: with Translucent Batching
/// (`batching`), unless a shader pack's frames can draw terrain flat (as for `flat_frame`),
/// which reads them in the section arena through its tables: where flat draws exist and
/// are on (`flat_draws`), for a `shaders/` pack's terrain programs (`terrain`) or a shadow
/// pass (`shadow`: a `shaders/` pack's, or the `[shadow]` of one of Horizon's own).
fn translucent_split(batching: bool, flat_draws: bool, terrain: bool, shadow: bool) -> bool {
    batching && !(flat_draws && (terrain || shadow))
}

/// The compact layout of solid quads, unless `HORIZON_PACKED_QUADS=0` (A/B checks).
fn packed_wanted() -> bool {
    std::env::var("HORIZON_PACKED_QUADS").as_deref() != Ok("0")
}

pub(crate) mod animate;
mod arena;
pub mod capture;
mod census;
pub mod compositor;
pub(crate) mod core_lines;
pub(crate) mod core_sky;
pub(crate) mod core_text;
mod cull;
mod dx12_compiler;
mod end_sky;
pub mod entity_skin;
pub mod external;
mod flat;
mod frame_gen;
mod gpu_culling;
mod hdr;
mod hdr_icc;
mod hdr_start;
mod hdr_stats;
pub(crate) mod hud_coverage;
mod in_flight;
pub use hdr::{Calibration as HdrCalibration, Output as HdrOutput};
mod hold;
pub use hold::WorldHold;
pub use output::{DisplayReading, EffectiveOutput, FrameConfiguration, FrameRequest};
pub use screenshot_save::Saved as SavedScreenshot;
pub mod block_light;
mod custom_sky;
mod displace;
mod frame_policy;
pub mod lightning;
mod linear_blend;
#[cfg(test)]
use frame_policy::frame_format;
use frame_policy::{frame_configuration, horizon_tone_curve, overlay_format};
mod lod;
mod native_material;
mod native_pack;
mod native_post;
pub mod native_render;
mod native_shadow;
mod native_sky;
mod occlusion;
mod oit;
mod oit_features;
pub(crate) mod output;
mod output_apply;
mod pack_atlas;
mod pack_center_depth;
pub mod pack_draw;
mod pack_entities;
mod pack_hand;
mod pack_images;
mod pack_lines;
mod pack_post;
mod pack_shadow;
mod pack_sky;
mod pack_textures;
pub mod pack_timing;
pub mod pack_uniforms;
mod pack_weather;
mod pass_timing_api;
use pass_timing_api::{after_culling_pass, pixel_count, world_pass_after_culling};
mod page_runs;
pub mod panorama;
mod readback;
mod resident;
mod reuse;
mod screenshot_save;
mod stars;
mod surface_setup;
use surface_setup::{AdapterChoice, AdapterFailure, open_adapter, pick_adapter, set_up_surface};
mod off_page;
pub mod submit;
mod sunrise_fan;
mod timing;
pub mod translucency;
pub(crate) mod upload;
mod view;
mod visbuffer;
mod wgsl_prefix;
mod world_post;
pub mod world_primitives;
mod world_warp;
use off_page::move_off_page;

/// A section's instance: its camera-relative origin and, in `w`, the step of
/// its fixed-point UVs (`terrain.wgsl`'s `terrain_uv`).
/// The world pass again after occlusion culling's test: it loads both
/// targets (phase 2 draws over phase 1, reading the test's results).
/// `LevelRenderer.addSkyPass`: fluids and camera effects block the sky pass.
fn sky_open(frame: &Frame) -> bool {
    !frame.sky_blocked
}

/// The Overworld-style sky (disc, sunset, stars, void) draws this frame.
fn sky_visible(frame: &Frame) -> bool {
    !frame.title && frame.skybox && sky_open(frame)
}

/// The End during the dragon fight, whose world fog leaves out vanilla's sky: a shader pack's
/// `gbuffers_skybasic` still draws a fan around the horizon (`pack_sky`), as our probe packs read
/// it in Iris 1.11.7.
fn end_fog_sky(frame: &Frame) -> bool {
    !frame.title && frame.end_sky && frame.boss_world_fog
}

/// The world pass's part that draws a shader pack's solid terrain: the world's colour and
/// depth loaded, and its `surface` target, cleared on the first of these passes.
fn surface_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    surface: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPass<'e> {
    let keep = |load| wgpu::Operations {
        load,
        store: wgpu::StoreOp::Store,
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Horizon world, pack terrain"),
        color_attachments: &[
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: keep(wgpu::LoadOp::Load),
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: surface,
                resolve_target: None,
                depth_slice: None,
                ops: keep(if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                }),
            }),
        ],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}
/// The point a `shaders/` pack's terrain is drawn relative to: the camera with the fraction of
/// each coordinate rounded to the nearest 1/65536 block. Read back with a probe pack of ours,
/// Iris 1.11.7's terrain `gl_Vertex` is the block's position minus this, for cameras from 13
/// to 40000 blocks out (#8464); `cameraPosition` and the matrices keep the exact camera. Off by
/// up to 2^-17 blocks, a fragment on a texel boundary of the shadow map reads the other texel.
fn pack_terrain_camera(camera: DVec3) -> DVec3 {
    let whole = camera.floor();
    whole + ((camera - whole) * 65536.0).round() / 65536.0
}
/// A new value for `Renderer::block_entity_revision`.
fn next_block_entity_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_color: [f32; 4],
    misc: [f32; 4],
    inverse_view: [[f32; 4]; 4],
    sunrise: [f32; 4],
    sun: [f32; 4],
    /// x: sky fog end, y: cloud fog end, z: dark disc (camera below the horizon),
    /// w: the glint strength (`GlintAlpha`).
    fog_ends: [f32; 4],
    /// Atlas rects (u0, v0, width, height) of end_sky.png and end_portal.png.
    end_portal_sprites: [[f32; 4]; 2],
    /// x: vanilla's `GameTime` uniform; y: nether cardinal lighting; z: how many
    /// `dynamic_lights` are set; w: the glint scroll clock (`Glint::scroll_ticks`).
    time: [f32; 4],
    /// Atlas rect of a destroy stage (entity.wgsl takes only its size: every stage's is the same,
    /// and each crack carries its own stage's origin).
    crumbling_sprite: [f32; 4],
    /// Framebuffer size, selection stroke width, perspective layering scale.
    outline_params: [f32; 4],
    /// Horizon Extras' dynamic lights: camera-relative position and level (`dynamic_light`).
    dynamic_lights: [[f32; 4]; crate::dynamic_light::MAX],
    colored_lights: [crate::dynamic_light::Colored; crate::dynamic_light::MAX],
    colored_count: [u32; 4],
    emissive_layers: [[u32; 4]; crate::native_emissive::MAX],
    emissive_count: [u32; 4],
    /// The sunrise fan's triangles as the sky shader tests them (`sunrise_fan::normals`).
    sunrise_fan: [[f32; 4]; 2 * sunrise_fan::TRIANGLES],
    /// x: `Options.textureFiltering` (0 none, 1 RGSS, 2 anisotropic); y: the sprite arrays' width;
    /// z: the mip level at which a sprite is one texel, when the views reach it, else 0.
    filtering: [f32; 4],
    /// HDR output's tier windows, low to very bright: where the lift starts, where it is
    /// full (`hdr::Hdr::windows`); zero without HDR.
    hdr_windows: [[f32; 4]; 2],
    hdr_light: [f32; 4],
    /// The scene's peak, HDR lighting's gain, world/look and debug; then the tiers' gains
    /// (`Hdr::draw_params`).
    hdr_settings: [f32; 4],
    hdr_gains: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct StaticEntityUniform {
    camera_block: [i32; 4],
    camera_fraction: [f32; 4],
}
struct GpuStaticEntitySection {
    buffer: wgpu::Buffer,
    origin_buffer: wgpu::Buffer,
    origin: [i32; 3],
    capacity: u64,
    revision: u64,
    vertices: u32,
}
pub type SectionKey = (i32, i32, i32);
/// What a section's mesh brings with it besides its quads (`Renderer::upload`): placed with
/// the mesh, now or when a hold lets go of it.
struct MeshFacts<'a> {
    visibility: u64,
    block_entities: &'a [([i32; 3], u32)],
    lights: &'a [([i32; 3], u32)],
    bounds: Option<[glam::Vec3; 2]>,
    /// It has translucent quads.
    translucent: bool,
    /// It has no quads at all: that is its mesh.
    empty: bool,
}
impl<'a> MeshFacts<'a> {
    fn of(mesh: &'a SectionMesh) -> Self {
        Self {
            visibility: mesh.visibility,
            block_entities: &mesh.block_entities,
            lights: &mesh.lights,
            bounds: mesh.bounds,
            translucent: !mesh.translucent.is_empty(),
            empty: mesh.solid.is_empty() && mesh.translucent.is_empty(),
        }
    }
}
/// A section's new mesh waiting behind a hold (gfx/hold.rs): uploaded, with what goes with it.
struct HeldMesh {
    gpu: Option<SectionGpu>,
    visibility: u64,
    block_entities: Vec<([i32; 3], u32)>,
    lights: Vec<([i32; 3], u32)>,
    bounds: Option<[glam::Vec3; 2]>,
    translucent: bool,
    empty: bool,
}
impl HeldMesh {
    fn facts(&self) -> MeshFacts<'_> {
        MeshFacts {
            visibility: self.visibility,
            block_entities: &self.block_entities,
            lights: &self.lights,
            bounds: self.bounds,
            translucent: self.translucent,
            empty: self.empty,
        }
    }
}
/// The water mirror's targets: the world as a camera mirrored about the water plane sees
/// it, at half the world's resolution (the material stage's `reflection`).
struct MirrorTarget {
    size: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}
struct SectionGpu {
    /// Arena page and first vertex of its solid quads, and after them its
    /// translucent quads unless those have an arena of their own; (0, 0),
    /// unused, when that leaves it no quads in `arena`.
    page: u32,
    start: u64,
    /// The page and first vertex of its translucent quads: in
    /// `translucent_arena` when there is one, else after the solid quads.
    /// Unused without translucent quads.
    translucent_at: (u32, u64),
    solid: u32,
    /// Slots its solid quads take in `arena`: four vertices each, or in the compact layout
    /// (`packed`, `mesher::packed`) a record each and the vertices of those without one.
    solid_slots: u64,
    packed: bool,
    /// Its records are a far tile's (`lod::packed`), not a section's: the two
    /// layouts are told apart by the base vertex its draws use.
    packed_far: bool,
    /// Where the solid quads' buckets by facing side end (`mesher::Sides`).
    sides: Sides,
    translucent: u32,
    sort: translucency::SortState,
    sorted_indices: Option<(u32, u64)>,
    /// Its id for flat draws (`flat::Flat::add`), when they are on.
    flat_id: Option<u32>,
    /// Its record's id in GPU culling's table (`gpu_culling::Records`), while
    /// that is on and it has solid quads.
    gpu_id: Option<u32>,
    /// Its record is `FADING` (GPU culling on): the CPU draws its solid quads
    /// while its instance carries a fade (`drawn_faded`), and GPU culling's
    /// pass leaves it out.
    gpu_fading: bool,
    /// When it came in over the far terrain it replaces (drawn whole at once,
    /// `fade`): a section new where far tiles are drawn, which skips vanilla's
    /// fade-in. Meshed again, it stays as it was.
    fading: Option<std::time::Instant>,
    /// When it began to fade out, its column gone over far terrain that
    /// stays behind it (`Renderer::leave_section`); released once it has.
    leaving: Option<std::time::Instant>,
    /// Animated sprite layers present in this section.
    animated_layers: Box<[u16]>,
}
/// Extracts the animated sprite layers present in a section mesh. Meshing
/// records the distinct layers, so uploading does not rescan every vertex.
pub(crate) fn collect_animated_layers(
    mesh: &SectionMesh,
    is_animated: impl Fn(u16) -> bool,
) -> Box<[u16]> {
    mesh.sprite_layers
        .iter()
        .copied()
        .filter(|&layer| is_animated(layer))
        .collect()
}

impl SectionGpu {
    /// Its fade code this frame: 0, drawn whole, or `lod::HELD` (whole, in
    /// front of far terrain) while it leaves (`leaving`) or its column has left
    /// the render distance and far terrain is coming in under it (`retiring`).
    /// A near section never dithers over far terrain: the holes of its pattern
    /// showed its own inside (cave walls, the stone under the grass), nearer
    /// than the tile behind, an x-ray of each column coming in or going out.
    /// It comes in whole over the tile it replaces, which stays behind it until
    /// `fading` is `lod::FADE` old, and leaves whole after as long.
    fn fade(&self, retiring: bool) -> u32 {
        if self.leaving.is_some() || retiring {
            lod::HELD
        } else {
            0
        }
    }
    /// Its vertices in `arena`, and in `translucent_arena` when `split` (there
    /// is one): (page, first vertex, vertices) of each.
    fn vertex_ranges(&self, split: bool) -> (Option<(u32, u64, u64)>, Option<(u32, u64, u64)>) {
        if !split {
            let vertices = self.solid_slots + u64::from(self.translucent) * 4;
            return (Some((self.page, self.start, vertices)), None);
        }
        let (page, start) = self.translucent_at;
        (
            (self.solid > 0).then_some((self.page, self.start, self.solid_slots)),
            (self.translucent > 0).then(|| (page, start, u64::from(self.translucent) * 4)),
        )
    }
    /// Whether GPU culling keeps a record of it at `key` while it is on (`gpu_id`):
    /// it has solid quads, in one of the level's `sections`, which the pass's
    /// tables span.
    fn gpu_recorded(&self, key: SectionKey, sections: usize) -> bool {
        self.solid > 0 && (0..sections as i32).contains(&key.1)
    }
    /// The base vertex of its solid draws: quad `q`'s corners are `base + 4 * q` on (the
    /// quad indices). In a compact layout they name its record's slot past
    /// `PACKED_BASE`, or past `FAR_BASE` for a far tile's records, which is how
    /// terrain.wgsl's `section_corner` tells the two apart.
    fn solid_base(&self) -> u32 {
        if !self.packed {
            return self.start as u32;
        }
        let base = if self.packed_far {
            crate::lod::packed::FAR_BASE
        } else {
            packed::PACKED_BASE
        };
        base + 4 * self.start as u32
    }
    /// GPU culling's record of it at `key`, its quads within `bounds`
    /// (`SectionMesh::bounds`) and in their buckets by facing side (`sides`),
    /// `FADING` while it fades (`gpu_fading`).
    fn record(&self, key: SectionKey, bounds: Option<[Vec3; 2]>) -> gpu_culling::Record {
        let boxed = gpu_culling::rounded_box(bounds);
        gpu_culling::Record::new(
            key,
            self.page,
            u64::from(self.solid_base()),
            self.solid,
            boxed,
        )
        .facing(&self.sides)
        .fading(self.gpu_fading)
    }
    /// Whether GPU frames' walk lists it within the render distance
    /// (`Resident::walk`): its mark. Its translucent quads are the CPU's, or
    /// GPU culling can have no record of it, which holds whether GPU culling is
    /// on or not; or, while it is, its solid quads are the CPU's while it fades
    /// (`gpu_fading`).
    fn listed(&self, key: SectionKey, sections: usize) -> bool {
        self.translucent > 0 || !self.gpu_recorded(key, sections) || self.gpu_fading
    }
    /// Whether GPU culling draws its solid quads in a GPU frame, `near`: its
    /// column within the render distance.
    fn gpu_draws_solid(&self, near: bool) -> bool {
        near && self.gpu_id.is_some() && !self.gpu_fading
    }
    /// Whether frames draw it faded at `f.now`: its instance, as the instance
    /// loop makes it (`section_instance` with its `fade` and its vanilla
    /// visibility), is not a whole section's. As time goes on a fade only ends:
    /// once false, this stays false until the section is meshed again, leaves,
    /// is held (`lod_retiring`), or the chunk fade-in time changes.
    fn drawn_faded(&self, key: SectionKey, f: FadeInputs<'_>) -> bool {
        let fade = self.fade(f.retiring.contains(&key));
        let visibility = section_visibility(f.uploaded.get(&key), f.fade_ms, f.now);
        section_instance([0.0; 3], fade, visibility)[3].to_bits()
            != section_instance([0.0; 3], 0, 1.0)[3].to_bits()
    }
    /// When its fade is over at the latest (`drawn_faded`): the end of
    /// vanilla's fade-in (its first upload and `f.fade_ms`; one coming in over
    /// far terrain is whole at once, `fade`). None while it leaves or is held,
    /// which ends only when it goes or is meshed again, and without a fade-in.
    fn fade_end(&self, key: SectionKey, f: FadeInputs<'_>) -> Option<Instant> {
        if self.leaving.is_some() || f.retiring.contains(&key) {
            return None;
        }
        let vanilla = f.uploaded.get(&key).filter(|_| f.fade_ms > 0);
        vanilla.map(|&t| t + std::time::Duration::from_millis(f.fade_ms))
    }
}
/// Which of a mesh's ranges `move_off_page` moves: what it holds in the section
/// arena (`split`: its solid quads alone, `SectionGpu::vertex_ranges`), its
/// translucent quads in `translucent_arena`, or its sorted indices.
#[derive(Clone, Copy)]
enum Part {
    Section { split: bool },
    Translucent,
    Indices,
}
impl Part {
    /// (page, first vertex or index, count), as the release paths free it.
    fn of(self, s: &SectionGpu) -> Option<(u32, u64, u64)> {
        match self {
            Self::Section { split } => s.vertex_ranges(split).0,
            Self::Translucent => s.vertex_ranges(true).1,
            Self::Indices => s
                .sorted_indices
                .map(|(page, at)| (page, at, u64::from(s.translucent) * 6)),
        }
    }
    /// Points the mesh where its range moved.
    fn set(self, s: &mut SectionGpu, to: (u32, u64)) {
        match self {
            Self::Section { split } => {
                (s.page, s.start) = to;
                if !split {
                    // Its translucent quads came along, after the solid ones.
                    s.translucent_at = (to.0, to.1 + s.solid_slots);
                }
            }
            Self::Translucent => s.translucent_at = to,
            Self::Indices => s.sorted_indices = Some(to),
        }
    }
}
/// Starts moving a page of a large arena far larger than its ranges in use
/// (`Arena::oversized`) into a new page of the size they get (`Arena::open_page`),
/// for `Renderer::shrink_translucent`: the page closes and `move_off_page` empties
/// it. None: no page to shrink. False, and nothing changes: no video memory for
/// the new page, or a range there without its mesh in `meshes`, which would keep
/// the page from ever emptying.
fn begin_shrink<'a>(
    arena: &mut arena::Arena,
    device: &wgpu::Device,
    meshes: impl Iterator<Item = &'a mut SectionGpu>,
    part: Part,
) -> Option<bool> {
    let (page, capacity) = arena.oversized()?;
    let held: u64 = meshes
        .filter_map(|s| part.of(s).filter(|r| r.0 == page))
        .map(|(.., n)| arena::Arena::span(n))
        .sum();
    debug_assert_eq!(held, arena.used(page), "a range in use without its mesh");
    if held != arena.used(page) || !arena.open_page(device, capacity) {
        return Some(false);
    }
    arena.close(page);
    Some(true)
}
/// GPU culling's flag (`SectionGpu::gpu_fading`) for its recorded section `s`
/// at `key`, decided at `f.now`: whether it draws faded (`drawn_faded`), and if
/// time ends that, its end waited for in `fades` (`fade_end`).
fn decide_fade(
    s: &SectionGpu,
    key: SectionKey,
    fades: &mut gpu_culling::Fades,
    f: FadeInputs<'_>,
) -> bool {
    let fading = s.drawn_faded(key, f);
    if let Some(end) = s.fade_end(key, f).filter(|_| fading) {
        fades.push(end, key);
    }
    fading
}
/// GPU culling: the section at `key` began to fade out over far terrain
/// (`Renderer::leave_section`), or is held while far terrain comes in
/// (`Renderer::lod_retiring`). It draws faded until it goes or is meshed again,
/// so its record is flagged and the CPU draws it, with no end to wait for.
fn hold_fading(
    sections: &mut resident::Resident<SectionGpu>,
    records: &mut gpu_culling::Records,
    key: SectionKey,
) {
    let Some(s) = sections.find_mut(key).filter(|s| !s.gpu_fading) else {
        return;
    };
    let Some(id) = s.gpu_id else {
        return;
    };
    s.gpu_fading = true;
    records.set_fading(id, true);
    sections.set_marked(key, true);
}
/// GPU culling switched off (`Renderer::set_gpu_culling`): no section keeps a
/// record or a flag, and those it flagged get back the walk marks they have
/// while it is off (`SectionGpu::listed`). `rows`: the level's sections.
fn forget_records(sections: &mut resident::Resident<SectionGpu>, rows: usize) {
    let mut marks = Vec::new();
    for (key, s, _) in sections.iter_mut() {
        s.gpu_id = None;
        if std::mem::take(&mut s.gpu_fading) {
            marks.push((key, s.listed(key, rows)));
        }
    }
    for (key, marked) in marks {
        sections.set_marked(key, marked);
    }
}
/// GPU culling's fading sections brought up to `f.now`, before a frame's gate
/// and `prepare`: every recorded section anew when the flags were decided with
/// another chunk fade-in time (GPU culling just switched on, or Video Settings
/// changed it), else those whose fade was due to end, handed back to the GPU if
/// whole by then. A section's record (`FADING`) and its walk mark follow its
/// `gpu_fading`. `rows`: the level's sections.
fn mark_fades(
    sections: &mut resident::Resident<SectionGpu>,
    records: &mut gpu_culling::Records,
    fades: &mut gpu_culling::Fades,
    rows: usize,
    f: FadeInputs<'_>,
) {
    let mut marks = Vec::new();
    if fades.fade_ms != Some(f.fade_ms) {
        fades.clear();
        fades.fade_ms = Some(f.fade_ms);
        for (key, s, _) in sections.iter_mut() {
            let Some(id) = s.gpu_id else {
                continue;
            };
            let fading = decide_fade(s, key, fades, f);
            if fading != s.gpu_fading {
                s.gpu_fading = fading;
                records.set_fading(id, fading);
                marks.push((key, s.listed(key, rows)));
            }
        }
    } else {
        while let Some(key) = fades.pop_due(f.now) {
            // Gone since, or no longer flagged.
            let Some(s) = sections.find_mut(key).filter(|s| s.gpu_fading) else {
                continue;
            };
            // Still fading: meshed again since, with its own end waited for
            // (`decide_fade`), or leaving or held, with none.
            if s.drawn_faded(key, f) {
                continue;
            }
            s.gpu_fading = false;
            records.set_fading(s.gpu_id.expect("flagged sections have records"), false);
            marks.push((key, s.listed(key, rows)));
        }
    }
    for (key, marked) in marks {
        sections.set_marked(key, marked);
    }
}
/// Line segments the hit outline can hold.
const MAX_OUTLINE_EDGES: u64 = 256;
/// Staging buffer size for section meshes: a frame of uploads while
/// exploring fits in a few.
const SECTION_UPLOAD_CHUNK: u64 = 4 << 20;
/// Section copies are submitted early past this many staged bytes, which
/// bounds the staging memory frames that are not drawn can hold.
const SECTION_COPY_LIMIT: u64 = 64 << 20;
/// What compaction copies in a frame, `compact_arena` first and
/// `shrink_translucent` with what it left, and a range more where a call's
/// first is larger (`move_off_page`): a page empties over a few frames, with
/// no hitch.
const COMPACT_BUDGET: u64 = 32 << 20;
/// A large translucent page starts to shrink (`Renderer::shrink_translucent`)
/// this long after terrain last arrived, and `SHRINK_RETRY` after a try found
/// no room.
/// `Renderer::remove_columns` filters each store once rather than looking up every
/// section key from this many keys, or from as many as there are resident sections.
const BULK_REMOVAL_KEYS: usize = 256;
const SHRINK_SETTLE: std::time::Duration = std::time::Duration::from_secs(1);
const SHRINK_RETRY: std::time::Duration = std::time::Duration::from_secs(5);
/// Not before then may a large translucent page start to shrink.
#[derive(Default)]
struct ShrinkGate(Option<Instant>);
impl ShrinkGate {
    /// Not before `wait` from `now`, nor before an earlier wait is over.
    fn wait(&mut self, now: Instant, wait: std::time::Duration) {
        self.0 = self.0.max(Some(now + wait));
    }
    /// Whether one may start now: the waits are over, and `compact_arena` copied
    /// nothing this frame (it `left` the whole budget).
    fn may_start(&self, now: Instant, left: u64) -> bool {
        left == COMPACT_BUDGET && self.0.is_none_or(|at| now >= at)
    }
}
pub struct Frame<'a> {
    pub camera: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_degrees: f32,
    /// The camera entity's eye and view rotation (yaw, pitch): the camera's own except in a
    /// third-person view.
    pub eye: DVec3,
    pub eye_rotation: [f32; 2],
    /// The camera is the entity's eye: no third-person view.
    pub first_person: bool,
    /// The `GameRenderer.bobHurt`/`bobView` pose multiplied into the
    /// projection (`crate::camera::bob_matrix`).
    pub bob: Mat4,
    /// The spinning effect multiplied into the level's projection only
    /// (`crate::camera::spinning_effect_matrix`), not the hand's.
    pub spin: Mat4,
    pub sky: Sky,
    /// Block edits and chunk membership that can change the shader pack's shadow map.
    pub shadow_scene_revision: (u64, u64),
    /// Opt-in reuse of the shader pack's last shadow map when the key still matches
    /// (`horizon-graphics.txt` `shadowCache`; off by default).
    pub shadow_cache: bool,
    /// Render distance in blocks (fog end).
    pub distance: f32,
    /// Full-detail terrain distance in blocks; distant LODs bypass the cave graph.
    pub detail_distance: f32,
    /// `Camera.depthFar`: the far plane (`crate::camera::depth_far`).
    pub depth_far: f32,
    pub underwater: bool,
    /// The y of the horizontal water plane around the camera, in world blocks, while one
    /// of Horizon's own packs reads the water mirror (`Game::water_plane`,
    /// `Renderer::wants_water_plane`).
    pub water_plane: Option<f32>,
    /// Skip sections the camera cannot see through open faces (vanilla's
    /// occlusion graph; off in spectator-like views inside blocks).
    pub cave_culling: bool,
    /// Title screen: only the dedicated panorama cube map and the HUD.
    pub title: bool,
    /// Lava, powder snow, blindness, darkness or a boss bar's world fog leaves out the sky pass
    /// (`fog::sky_pass_skipped`).
    pub sky_blocked: bool,
    /// Dimensions without a skybox (the Nether) clear to the fog colour.
    pub skybox: bool,
    /// The End: clear to black and draw the resident sky box behind the world.
    pub end_sky: bool,
    /// End sky atlas bounds (u0, v0, u1, v1); absent when the sprite is missing.
    pub end_sky_sprite: Option<[f32; 4]>,
    /// A boss bar makes world fog (`BossHealthOverlay.shouldCreateWorldFog`: the Ender Dragon's).
    pub boss_world_fog: bool,
    /// Environmental fog start/end (Nether 10..96), if any.
    pub environment_fog: Option<(f32, f32)>,
    pub water_fog: [f32; 3],
    /// `FogRenderer.setupFog` result; replaces the underwater/environment fields when set.
    pub fog: Option<crate::fog::FogData>,
    /// Camera eye below the horizon height and not under water (`shouldRenderDarkDisc`).
    pub dark_disc: bool,
    /// The hit block outline as line segments (from, to) in world space.
    pub outline: Vec<[f64; 6]>,
    /// `BlockOutlineRenderState.isTranslucent`: draw after translucent terrain.
    pub outline_after_terrain: bool,
    /// `BlockOutlineRenderState.highContrast`: a thick black outline under a bright one.
    pub outline_high_contrast: bool,
    /// The PvP & HUD page's outline and hit colour (`pvp::Frame`; default: vanilla's).
    pub pvp: crate::pvp::Frame,
    /// `lines` with a colour and pixel width (`gizmos::camera_relative`):
    /// the fishing line and debug gizmos, depth tested, then gizmos drawn
    /// over everything.
    pub debug_lines: &'a [[f32; 8]],
    pub debug_lines_on_top: &'a [[f32; 8]],
    /// `DebugCrosshairRenderer`'s axes, camera relative like `debug_lines`:
    /// drawn after the hand at its projection, without the bob or fog.
    pub debug_crosshair: &'a [[f32; 8]],
    pub hud: &'a HudBatch,
    /// `GlintAlpha` (accessibility Glint Strength); the entity shader uses it
    /// for held/world items, independent of what the HUD batch last drew.
    pub glint_alpha: f32,
    /// The glint scroll clock (`Glint::scroll_ticks`): entity.wgsl's
    /// `ARMOR_ENTITY_GLINT_TEXTURING` phase.
    pub glint_ticks: f32,
    /// CardinalLighting.Type.NETHER, otherwise DEFAULT.
    pub nether_lighting: bool,
    /// Camera-relative entity geometry textured from the shared atlas.
    pub entities: &'a [Vertex],
    /// Per-section cacheable block-entity models. Positions are section-local.
    pub static_block_entities: &'a [StaticSectionSnapshot],
    /// Sections currently intersecting the coarse camera frustum.
    pub visible_static_block_entity_sections: &'a [[i32; 3]],
    /// Cache work for this frame, reported in bench summaries.
    pub static_block_entity_stats: StaticCacheFrameStats,
    /// Which entity `entities`' vertices belong to: (first vertex, entity id), each run up
    /// to the next; id 0 for none. Frame generation follows these entities' own motion.
    pub entity_motion: &'a [(u32, u64)],
    /// For each mark of `entity_motion`, the GPU-posed vertex count at it (`skin`'s
    /// `vertices`): where that entity's posed vertices start. A shader pack's `at_velocity`
    /// matches both parts of an entity with last frame's.
    pub entity_motion_posed: &'a [u32],
    /// The glowing entities' silhouettes in their team colours (`OutlineBufferSource`),
    /// outlined through walls after the hand.
    pub glowing: &'a [Vertex],
    /// Runs of `entities` by entity, while a shader pack draws them (`pack_draw::EntitySpan`).
    pub entity_spans: &'a [pack_draw::EntitySpan],
    /// Ordinary entity models posed on the GPU (`entity_skin`), drawn before `entities`'
    /// ordinary ones; only when `Renderer::skins_entities` said so.
    pub skin: Option<&'a entity_skin::SkinFrame>,
    pub weather: &'a [crate::weather::Vertex],
    pub weather_time: crate::weather::Time,
    /// Horizon Extras' dynamic lights, nearest first (`dynamic_light::select`); empty when off.
    pub dynamic_lights: &'a [[f32; 4]],
    pub colored_lights: &'a [crate::dynamic_light::Colored],
    pub emissive_layers: &'a [[u32; 4]],
    /// Cloud mesh inputs; geometry and GPU storage are retained by the renderer.
    pub clouds: Option<crate::clouds::Input<'a>>,
    /// Retained lightning bolts, drawn before the other additive entities.
    pub lightning: lightning::Input<'a>,
    /// Additive translucent geometry after lightning (dragon rays).
    pub additive: &'a [Vertex],
    /// Name tags over everything, without depth (`TEXT_SEE_THROUGH`).
    pub see_through: &'a [Vertex],
    /// Native mods' lines, boxes, beams and text (`native_world_primitives`), drawn at the end
    /// of the world pass, before the first-person hand.
    pub world_primitives: world_primitives::Batch<'a>,
    /// Opaque entity quads only a native pack's shadow pass draws (the local player in first
    /// person).
    pub shadow_only: &'a [Vertex],
    /// Camera-relative sun and moon, or the animated End flash (additive,
    /// behind everything and after the resident End sky box).
    pub celestial: &'a [Vertex],
    /// The stars, drawn from their static buffer after the sun and moon:
    /// the sky's rotation (camera-relative world from star space) and the
    /// star brightness.
    pub stars: Option<(Mat4, f32)>,
    /// First-person hand and held item, drawn after clearing depth.
    pub hand: &'a [Vertex],
    /// Where the arm and each held item start in `hand` (first vertex), with the
    /// `currentRenderedItemId` a shader pack's `gbuffers_hand` reads for them.
    pub hand_items: &'a [(u32, i32)],
    /// `CameraRenderState.hudFov`: the hand's own field of view
    /// (`GameRenderer.render3dHud`).
    pub hand_fov_degrees: f32,
    /// Full-screen camera effects drawn over the world and hand, under the HUD.
    pub screen: &'a [crate::screen_effects::ScreenQuad],
    /// What a shader pack's uniforms need from the game.
    pub pack: pack_uniforms::PackInputs,
    /// The world border wall, when near enough to be drawn.
    pub border: Option<BorderDraw>,
    pub game_time: f32,
    /// `GlobalSettingsUniform` GameTime: (game time % 24000 + partial tick) / 24000.
    pub shader_game_time: f32,
    /// Atlas rects (u0, v0, width, height) of end_sky.png and end_portal.png
    /// (`rendertype_end_portal`'s Sampler0 and Sampler1).
    pub end_portal_sprites: [[f32; 4]; 2],
    /// A destroy-stage sprite: projected UVs repeat inside a rect of this size, from the
    /// origin each crack vertex carries (`breaking::stage_light`).
    pub crumbling_sprite: [f32; 4],
    pub ticks: u64,
    /// The darkness effect on the lightmap: the brightness it takes off the
    /// gamma option and the pulsing scale subtracted from the light.
    pub darkness: (f32, f32),
    /// Night vision (or conduit water vision) intensity on the lightmap.
    pub night_vision: f32,
    /// Torch flicker added to the block light factor of 1.4.
    pub block_flicker: f32,
    /// `lightmap.fsh` BossOverlayWorldDarkeningFactor (a wither's bar).
    pub boss_darken: f32,
    /// `Screen.extractBlurredBackground`: blur the frame by this radius
    /// (`menuBackgroundBlurriness`) before the screen is drawn over it.
    pub menu_blur: Option<f32>,
    /// `Options.chunkSectionFadeInTime`, in seconds: how long a section takes to fade in once
    /// uploaded (0 draws it whole at once).
    pub chunk_fade: f32,
    /// `GameRenderer.requestedPostEffects`: the chains that run over the world.
    pub post_effects: crate::post_chain::Request<'a>,
}
/// One frame of the world border wall (`WorldBorderRenderer.render`).
#[derive(Clone, Debug)]
pub struct BorderDraw {
    /// The cached wall buffer; uploaded only when `rebuilt`.
    pub vertices: [crate::border_render::BorderVertex; 16],
    pub rebuilt: bool,
    pub transforms: crate::border_render::BorderTransforms,
    /// Walls to draw (indices of four-vertex quads), nearest first.
    pub sides: Vec<usize>,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BorderUniform {
    color: [f32; 4],
    offset: [f32; 4],
}
/// A frame to save as a PNG: F2's screenshot, an agent's `game_screenshot`,
/// or the icon the Select World list shows.
pub struct ScreenshotRequest {
    pub path: PathBuf,
    /// `GameRenderer.takeAutoScreenshot`: centre-crop the frame to a square
    /// and average it down to this size. `None` saves the frame as drawn.
    pub size: Option<[u32; 2]>,
    /// F2's: drawn at the game's resolution (Resolution Scale above 100%) times
    /// `Renderer::screenshot_quality`, not the window's.
    pub scaled: bool,
    /// Written on a thread of its own (F2's, the world icon's): its outcome comes back a
    /// frame or a few later in `Renderer::saved_screenshots`, not `screenshot_result`.
    pub background: bool,
}
/// A solid terrain pipeline without HDR output's lighting (terrain.wgsl's `HDR_LIGHTING`),
/// for the frames HDR is off, and with it where the display offers HDR output: the
/// lighting's code, even never run, slows the shader on Apple GPUs (~15% of the solid
/// pass at Retina sizes).
pub struct SolidPipeline {
    plain: wgpu::RenderPipeline,
    hdr: Option<wgpu::RenderPipeline>,
}

impl SolidPipeline {
    /// The pipeline for a frame with HDR output on (`hdr`) or off.
    pub fn get(&self, hdr: bool) -> &wgpu::RenderPipeline {
        match &self.hdr {
            Some(lit) if hdr => lit,
            _ => &self.plain,
        }
    }
}

pub struct Renderer {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// The window's swapchain; `None` when rendering offscreen (`--offscreen`). Shared
    /// with frame generation's present thread while it runs.
    surface: Option<Arc<wgpu::Surface<'static>>>,
    /// The surface `release_surface` set aside for the renderer that replaces
    /// this one on the same device: reused, not created again, as a new
    /// surface for the window can come back lost (Wayland on Mesa).
    spare_surface: std::sync::Mutex<Option<wgpu::Surface<'static>>>,
    /// The offscreen colour target, sized like `config`.
    offscreen: Option<wgpu::Texture>,
    /// Offscreen submissions still on the GPU: at most two frames run ahead.
    in_flight: std::collections::VecDeque<wgpu::SubmissionIndex>,
    /// Splitscreen: the caller waits for the GPU once every player has submitted
    /// (`wait_in_flight`), so one player's work overlaps the GPU drawing another's.
    pub paced_elsewhere: bool,
    /// Which steps of the first frame on this device still go to the log
    /// (see `startup_step`).
    first_frame: FirstFrame,
    gpu_timer: Option<timing::GpuTimer>,
    /// GPU frame times (ms) read back since the caller last drained them.
    pub gpu_times: Vec<f32>,
    /// The last lightmap written, RGBA rows of sky level by block level
    /// (F3+4 shows it).
    pub lightmap_pixels: [u8; 16 * 16 * 4],
    /// Time the last `render` spent blocked on the GPU or the display
    /// (acquiring, presenting, offscreen pacing, the submit thread holding the queue),
    /// excluded from CPU time.
    pub last_wait: std::time::Duration,
    /// CPU time of the last `render`, waits excluded.
    pub last_encode: std::time::Duration,
    /// Watched only while HDR is On or Auto, including Auto in SDR;
    /// before `window`, so it's dropped first.
    display: Option<DisplayWatch>,
    /// What this renderer's display last reported (`DisplayWatch`), its own: another
    /// renderer's watch never changes it.
    reading: output::DisplayReading,
    /// The watch was just made: the settings' Auto values take what it reported.
    display_fresh: bool,
    window: Arc<Window>,
    pub device: wgpu::Device,
    /// The shader modules, layouts and pipelines kept for the next renderer on
    /// this device ([`reuse`]).
    reuse: Arc<reuse::Reuse>,
    /// Frames are finished and submitted on its submit thread ([`submit`]).
    pub queue: submit::Queue,
    /// The last frame went to the submit thread; whether it had no window surface.
    /// The next frame takes it in ([`Self::land_frame`]).
    deferred: Option<bool>,
    /// Set by the device's lost callback. wgpu reports nothing else when an
    /// operation fails on a lost device (no error scope sees it).
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    pub config: wgpu::SurfaceConfiguration,
    solid: SolidPipeline,
    /// `solid` split in two passes for the world pass (terrain.wgsl's `opaque_quad`).
    solid_opaque: SolidPipeline,
    solid_cutout: SolidPipeline,
    translucent: wgpu::RenderPipeline,
    translucent_mark: wgpu::RenderPipeline,
    /// A native mod's world warp (`world_warp`): nothing made until a mod gives one.
    world_warps: world_warp::WorldWarps,
    /// Flat section draws (`flat`), where multi-draw is emulated.
    flat: Option<flat::Flat>,
    /// Sections upload their solid quads in the compact layout (`mesher::packed`): the solid
    /// passes read the arena themselves (`flat`) and no shader pack reads it as vertices.
    packed_quads: bool,
    /// The terrain's acceleration structures, with ray tracing on (`ray_scene`).
    ray_scene: Option<surface_setup::ray_query::ray_scene::RayScene>,
    /// The compact layout is allowed (`HORIZON_PACKED_QUADS`, `set_packed_layout`).
    packed_on: bool,
    /// The visibility buffer for flat frames' solid terrain (`visbuffer`,
    /// `HORIZON_VISBUFFER=1`).
    visbuf: Option<visbuffer::Visbuffer>,
    /// `flat` draws this frame's sections (off: the per-section draws).
    flat_on: bool,
    /// Occlusion culling of solid section draws, where the GPU's instance
    /// counts reach a multi-draw's entries (Vulkan, Metal).
    cull: Option<cull::Cull>,
    cull_on: bool,
    /// Solid quads facing away from the eye are left out (`mesher::Sides`).
    sides_on: bool,
    /// The near solid sections culled and drawn on the GPU (`gpu_culling`): its pass,
    /// records and fades while it is on, where it is offered (`gpu_culling_offer`, mirrored
    /// for the settings in `graphics::gpu_culling_offer`), and whether the last frame drew
    /// them so (`gpu_frame`).
    gpu_culling: Option<gpu_culling::GpuCulling>,
    gpu_culling_offer: crate::graphics::GpuCullingOffer,
    gpu_frame: bool,
    /// GPU frames since GPU culling was switched on: the first and every 64th after it
    /// check its records (`check_gpu_records`).
    #[cfg(debug_assertions)]
    gpu_checks: u32,
    sky: wgpu::RenderPipeline,
    /// The main sky without the sunrise fan, which `sunrise_pipeline` draws as
    /// geometry over it. The mirror keeps `sky`: its per-pixel fan is cheap at
    /// the reflection's size and needs no second projection.
    sky_base: wgpu::RenderPipeline,
    sunrise_pipeline: wgpu::RenderPipeline,
    sunrise_buffer: wgpu::Buffer,
    dark_disc: wgpu::RenderPipeline,
    /// Clears the world pass's depth to the far plane so the first-person hand draws in the
    /// same pass (`hand_in_world`), as the hand's own pass would clear it.
    depth_reset: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    lines_secondary: wgpu::RenderPipeline,
    /// A resource pack's `core/rendertype_lines`, which draws the block outline in their place.
    core_lines: Option<core_lines::CoreLines>,
    core_text: Option<core_text::CoreText>,
    /// The packs' `core/sky` and `core/stars`, drawn in place of the native sky and stars.
    core_sky: Option<core_sky::CoreSky>,
    /// Gizmo lines: opaque ones (`RenderTypes.lines`, which writes depth),
    /// translucent ones (`RenderTypes.linesTranslucentNoDepthWrite`) and the
    /// always-on-top ones.
    debug_lines_opaque: wgpu::RenderPipeline,
    debug_lines: wgpu::RenderPipeline,
    debug_lines_on_top: wgpu::RenderPipeline,
    debug_crosshair: Option<wgpu::RenderPipeline>,
    /// The debug crosshair and the first-person hand in `overlay_format`, where it isn't the
    /// frame's.
    overlay_crosshair: Option<wgpu::RenderPipeline>,
    overlay_hand: Option<wgpu::RenderPipeline>,
    debug_line_buffer: wgpu::Buffer,
    debug_line_capacity: u64,
    /// The terrain resources with the debug crosshair's projection
    /// (`crosshair_globals`).
    crosshair_group: wgpu::BindGroup,
    crosshair_globals: wgpu::Buffer,
    /// The water mirror: its solid terrain pipeline, its camera's globals and their group,
    /// and its targets once drawn.
    solid_mirror: wgpu::RenderPipeline,
    mirror_globals: wgpu::Buffer,
    mirror_group: wgpu::BindGroup,
    mirror: Option<MirrorTarget>,
    entity_pipeline: wgpu::RenderPipeline,
    static_entity_pipeline: wgpu::RenderPipeline,
    static_entity_group: wgpu::BindGroup,
    static_entity_transform: wgpu::Buffer,
    static_entity_camera: Option<DVec3>,
    static_entity_catalog_revision: u64,
    static_entity_frame_stats: StaticCacheFrameStats,
    hdr_mark_pipeline: wgpu::RenderPipeline,
    /// `hdr_mark_pipeline` at `entity_pipeline`'s depth bias, for what that drew.
    hdr_entity_mark_pipeline: wgpu::RenderPipeline,
    hdr_hand_unmark_pipeline: wgpu::RenderPipeline,
    hdr_mark_lightning_pipeline: wgpu::RenderPipeline,
    weather: weather::Weather,
    crumbling_pipeline: wgpu::RenderPipeline,
    shadow_blob_pipeline: wgpu::RenderPipeline,
    translucent_item_pipeline: wgpu::RenderPipeline,
    /// `RenderPipelines.WATER_MASK`: depth writes only, no colour.
    water_mask_pipeline: wgpu::RenderPipeline,
    cloud_pipeline: wgpu::RenderPipeline,
    hdr_cloud_mark_pipeline: wgpu::RenderPipeline,
    cloud_mesh: crate::clouds::Mesh,
    cloud_buffer: wgpu::Buffer,
    cloud_capacity: u64,
    cloud_uniform: wgpu::Buffer,
    cloud_group: wgpu::BindGroup,
    additive_pipeline: wgpu::RenderPipeline,
    dragon_rays_pipeline: wgpu::RenderPipeline,
    /// `RenderPipelines.ENERGY_SWIRL`: additive, unlit, the sprite repeated within itself.
    energy_swirl_pipeline: wgpu::RenderPipeline,
    lightning_pipeline: wgpu::RenderPipeline,
    lightning: lightning::Renderer,
    see_through_pipeline: wgpu::RenderPipeline,
    /// Native mods' world primitives (`Frame::world_primitives`).
    world_primitives: world_primitives::WorldPrimitives,
    text_polygon_offset_pipeline: wgpu::RenderPipeline,
    world_text_pipeline: wgpu::RenderPipeline,
    fluid_pipeline: wgpu::RenderPipeline,
    terrain_model_pipelines: [wgpu::RenderPipeline; 3],
    celestial_pipeline: wgpu::RenderPipeline,
    celestial_buffer: wgpu::Buffer,
    end_sky: end_sky::EndSky,
    stars: stars::Stars,
    /// Custom sky layers (`crate::custom_sky`), and the frame's from the game.
    custom_sky: custom_sky::CustomSky,
    custom_sky_draws: Vec<crate::custom_sky::Draw>,
    /// The glowing entities' outline (`LevelRenderer.entityOutlineTarget`).
    entity_outline: entity_outline::EntityOutline,
    panorama: panorama::Panorama,
    /// `RenderPipelines.WORLD_BORDER` and its buffers.
    border_pipeline: wgpu::RenderPipeline,
    border_group: wgpu::BindGroup,
    border_uniform: wgpu::Buffer,
    border_vertices: wgpu::Buffer,
    /// Screen effects: alpha-blended, then vignette (`dst * (1 - src)`)
    /// pipelines, each for every sampling (index `blend * 4 + sampling`).
    screen_pipelines: [wgpu::RenderPipeline; 16],
    screen_buffer: wgpu::Buffer,
    screen_capacity: u64,
    entity_group: wgpu::BindGroup,
    /// The layout of `entity_group`, for the native shadow pass.
    entity_bindings: wgpu::BindGroupLayout,
    /// The entity resources with the hand's projection (`hand_globals`).
    hand_group: wgpu::BindGroup,
    hand_globals: wgpu::Buffer,
    entity_buffer: wgpu::Buffer,
    entity_capacity: u64,
    static_entity_sections: HashMap<[i32; 3], GpuStaticEntitySection>,
    static_entity_indices: wgpu::Buffer,
    static_entity_index_quads: u32,
    entity_batch: EntityBatch,
    /// Ordinary entity models posed on the GPU; `None` without compute shaders.
    entity_skin: Option<entity_skin::EntitySkin>,
    group: wgpu::BindGroup,
    fluid_group: wgpu::BindGroup,
    fluid_sampler: wgpu::Sampler,
    terrain_model_group: wgpu::BindGroup,
    terrain_model_sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    /// Resource-pack normal/specular maps, split like the colour sprite arrays.
    pbr_maps: Option<Vec<[wgpu::Texture; 2]>>,
    lightmap: wgpu::Texture,
    /// The block sprites, `sprite_split` layers per array (the first is
    /// `sprites` in terrain.wgsl, the others `sprites_1`...).
    sprite_textures: Vec<wgpu::Texture>,
    /// Block sprite layers per array on this device ([`layer_split`]).
    sprite_split: u32,
    /// A 1-layer array bound where a pack needs fewer arrays.
    sprite_unused: wgpu::TextureView,
    /// The sprite arrays as the terrain samples them (`SPRITE_ARRAYS` views, `sprite_unused`
    /// past the ones in use, the Mipmap Levels option's levels), for native mods
    /// (`WorldDrawn::block_sprites`).
    sprite_views: Vec<wgpu::TextureView>,
    /// Nearest, repeating, nearest mip level: for drawing a sprite's texels sharp elsewhere.
    sprite_nearest: wgpu::Sampler,
    mip_levels: u32,
    /// The mipmap levels sampled (the Mipmap Levels option), at most `mip_levels - 1`.
    mipmaps: u32,
    terrain_layout: wgpu::BindGroupLayout,
    /// terrain.wgsl's `SPRITE_ARRAYS` and `SPRITE_SPLIT`, for a shader pack's terrain.
    terrain_sprites: [f64; 2],
    sprite_sampler: wgpu::Sampler,
    /// `Options.textureFiltering`: 0 none, 1 RGSS, 2 anisotropic.
    texture_filtering: u32,
    /// The anisotropic sampler's clamp (`Options.maxAnisotropyValue`); 1 unless anisotropic.
    max_anisotropy: u16,
    lightmap_view: wgpu::TextureView,
    lightmap_sampler: wgpu::Sampler,
    animations: animate::SpriteAnimator,
    material_animations: Vec<SpriteAnimation>,
    active_animated_layers: Vec<u64>,
    uploads: upload::Uploads,
    /// Section meshes are staged through their own belt into
    /// `section_copies`, which the next frame records its commands after:
    /// `Queue::write_buffer` would make a staging buffer per write.
    section_uploads: upload::Uploads,
    section_copies: Option<wgpu::CommandEncoder>,
    /// Bytes staged in `section_copies`.
    section_copy_bytes: u64,
    /// When a large translucent page may start to shrink (`shrink_translucent`).
    shrink_gate: ShrinkGate,
    /// Whether `lightmap` holds `lightmap_pixels` yet.
    lightmap_uploaded: bool,
    sprite_minimum_size: u32,
    /// Each sprite layer's own side, before the arrays scaled it up (`mipmap::Settings::native_size`).
    sprite_native: Vec<u32>,
    /// The Mipmap Levels option (`set_mipmap_levels`), which sets a shader pack atlas's padding.
    mipmap_option: u32,
    indices: wgpu::Buffer,
    index_quads: u32,
    instances: wgpu::Buffer,
    instance_capacity: u64,
    outline: wgpu::Buffer,
    depth: wgpu::TextureView,
    sections: resident::Resident<SectionGpu>,
    lods: FxHashMap<crate::lod::TileKey, lod::Tile>,
    /// Far tiles' occlusion culling slots (`lod::Tile::cull_slot`): those
    /// free, and the next never used.
    lod_cull_free: Vec<u32>,
    lod_cull_next: u32,
    /// The chunk columns the resident tiles leave out (`Lod::take_cutout`):
    /// their sections are drawn even past the render distance, so terrain
    /// leaving it never opens a hole before the tiles fill it in.
    lod_cutout: crate::fast_hash::FxHashSet<(i32, i32)>,
    /// Removed sections of `lod_cutout` columns, released once tiles cover them.
    lod_retiring: crate::fast_hash::FxHashSet<SectionKey>,
    /// Removed sections fading out over far terrain (`leave_section`).
    lod_leaving: Vec<(SectionKey, std::time::Instant)>,
    /// A mod's hold on the terrain's picture (`hold_world`, gfx/hold.rs) and the new meshes
    /// waiting behind it.
    hold: hold::Hold<HeldMesh>,
    /// Finished tile meshes `upload_lods` has yet to upload.
    lod_pending: Option<lod::Pending>,
    detail_columns: FxHashMap<(i32, i32), SectionMask>,
    /// Bumped whenever a column becomes or stops being complete.
    detail_revision: u64,
    arena: arena::Arena,
    /// Translucent quads' own arena, with large pages, while the layout is split
    /// (`translucent_split`): the back-to-front list then stays in one page (and
    /// one sorted-index page) and draws in a few multi-draws rather than one per
    /// few sections.
    translucent_arena: Option<arena::Arena>,
    /// Translucent Batching (`Graphics::translucent_batching`, read when a renderer
    /// is made), where it is offered (`graphics::offer_translucent_batching`).
    translucent_batching: bool,
    /// Every section and far tile was dropped to change the layout since the game
    /// last asked (`take_dropped_sections`).
    sections_dropped: bool,
    sorted_indices: arena::Arena,
    /// Buffers of released arena pages, freed a few frames later on a thread of their own.
    graveyard: arena::Graveyard<wgpu::Buffer>,
    translucent_sorter: translucency::Sorter,
    translucency_cursor: usize,
    /// `DrawIndexedIndirectArgs` for this frame's solid then translucent draws.
    indirect: wgpu::Buffer,
    indirect_capacity: u64,
    /// Indirect draws may set `first_instance` (the section origin).
    pub multi_draw: bool,
    /// Per-frame scratch, kept to avoid allocating every frame.
    visible: Vec<resident::Visible>,
    visible_lods: Vec<lod::Visible>,
    /// The sections a shader pack's shadow camera sees (`external::ShadowView`).
    shadow_visible: Vec<resident::Visible>,
    /// The external shader pipeline world frames call (`external`); `None` draws vanilla's way.
    external: Option<Box<dyn external::ExternalPipeline>>,
    translucent_draws: Vec<(u64, usize, [u32; 5], (u32, Option<u32>))>,
    origins: Vec<[f32; 4]>,
    /// `DrawIndexedIndirectArgs` words: index count, instance count, first
    /// index, base vertex, first instance.
    draws: Vec<[u32; 5]>,
    /// Vertex arena page and optional sorted index arena page of each draw:
    /// with sorted indices (a translucent draw), a page of `translucent_arena`
    /// when there is one.
    draw_pages: Vec<(u32, Option<u32>)>,
    /// Solid draws left to occlusion culling's phase 2, while the list is
    /// built, and their pages.
    phase2_draws: Vec<[u32; 5]>,
    phase2_pages: Vec<(u32, Option<u32>)>,
    /// Each section's solid draws, to group them by arena page.
    solid_runs: page_runs::PageRuns,
    phase2_runs: page_runs::PageRuns,
    /// The shadow camera's draws by page, in bands of distance from the light.
    shadow_runs: page_runs::PageRuns,
    /// Block-entity blocks per meshed section (world position, state id).
    block_entities: FxHashMap<SectionKey, Vec<([i32; 3], u32)>>,
    /// Changes on every mesh upload for that section; block-entity static
    /// geometry also depends on the section's packed light.
    block_entity_section_revisions: FxHashMap<SectionKey, u64>,
    /// Changes only when the block-entity positions/states in a section
    /// change. Empty-section tombstones remain until that section unloads so
    /// the static cache can remove deleted models without a global flush.
    block_entity_membership_revisions: FxHashMap<SectionKey, u64>,
    /// When each section was first uploaded (`RenderSection.uploadedTime`): kept across meshing
    /// it again, gone with its column. Sections that fade in over far terrain are not in it.
    section_uploaded: FxHashMap<SectionKey, std::time::Instant>,
    /// When `section_uploaded` last gained a section: once the fade time has passed since,
    /// every section is whole and frames skip the per-section lookups.
    newest_upload: Option<std::time::Instant>,
    /// When a section last began to fade in over far terrain (`SectionGpu::fading`), for the
    /// same shortcut.
    newest_lod_fade: Option<std::time::Instant>,
    /// Changes whenever `block_entities` does (unique across renderers).
    block_entity_revision: u64,
    /// The light sources of each section and their revision, for `block_light`.
    light_blocks: FxHashMap<SectionKey, Vec<([i32; 3], u32)>>,
    light_revision: u64,
    /// VisGraph face connectivity per meshed section (cave culling).
    occlusion: occlusion::Occlusion,
    /// The column `occlusion`, `sections` and `detail_columns` count in: the
    /// level's (`clear_sections`).
    layout: crate::chunk::ColumnLayout,
    /// Resolution scale, FXAA and TAA (Horizon settings).
    world_post: world_post::Post,
    /// Frame generation (Horizon setting).
    frame_gen: frame_gen::FrameGen,
    /// The entities' texture atlas (`entity_group`'s), for frame generation's motion.
    entity_atlas: wgpu::TextureView,
    /// HDR output (Horizon setting): `Some` while the swapchain is HDR.
    hdr: Option<hdr::Hdr>,
    /// Which pixels of each block sprite may shine in HDR output (terrain.wgsl's
    /// `emission_thresholds`), measured when HDR output turns on.
    emission_stats: hdr_stats::EmissionStats,
    /// What this renderer's destination offers and its device draws.
    output_caps: output::OutputCapabilities,
    /// An output that failed to apply, not tried again until something it needs changes.
    output_rejected: Option<output_apply::Rejected>,
    /// Since when Auto keeps HDR on through the display's loss of its report.
    auto_off_since: Option<std::time::Instant>,
    /// The output applied (`update_output`), with the frame every pipeline that draws it
    /// targets: the swapchain's 8-bit format, or `Rgba16Unorm` when the renderer was made with
    /// HDR output on and Smooth Gradients and the device draws and blends into it, or
    /// `LINEAR_FRAME` (`frame_format`). A 16-bit frame with HDR off since reaches the
