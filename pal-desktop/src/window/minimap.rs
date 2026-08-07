//! High-resolution translucent navigation overlay for exploration scenes.

use pal_core::game::CollisionMap;
use pal_core::role::{Direction, Role};
use pal_core::scene::SceneObject;
use pixels::wgpu;

const MIN_PANEL_LOGICAL_WIDTH: f32 = 96.0;
const MAX_PANEL_LOGICAL_WIDTH: f32 = 384.0;
const PANEL_SURFACE_WIDTH_RATIO: f32 = 0.30;
const PANEL_SURFACE_HEIGHT_RATIO: f32 = 0.45;
const BASE_CONTENT_WIDTH: f32 = 204.0;
const BASE_CONTENT_HEIGHT: f32 = 124.0;
const WORLD_VIEW_WIDTH: f32 = 816.0;
const WORLD_VIEW_HEIGHT: f32 = 496.0;
const SURFACE_MARGIN: f32 = 14.0;
const EDGE_SAMPLE_WORLD_UNITS: i32 = 4;
const MAX_RASTER_SCALE: f64 = 1.0;

const PANEL_TOP_COLOR: [u8; 4] = [21, 28, 31, 154];
const PANEL_BOTTOM_COLOR: [u8; 4] = [10, 16, 22, 174];
const PANEL_BORDER_COLOR: [u8; 4] = [198, 169, 104, 190];
const PANEL_SHADOW_COLOR: [u8; 4] = [0, 0, 0, 82];
const WALKABLE_COLOR: [u8; 4] = [41, 104, 82, 0];
const WALKABLE_EDGE_COLOR: [u8; 4] = [71, 139, 111, 112];
const BLOCKED_COLOR: [u8; 4] = [4, 11, 16, 0];
const BLOCKED_EDGE_COLOR: [u8; 4] = [132, 190, 157, 210];
const OBJECT_GLOW_COLOR: [u8; 4] = [242, 139, 55, 58];
const OBJECT_COLOR: [u8; 4] = [242, 139, 55, 246];
const PLAYER_GLOW_COLOR: [u8; 4] = [244, 195, 59, 56];
const PLAYER_OUTLINE_COLOR: [u8; 4] = [255, 246, 206, 244];
const PLAYER_COLOR: [u8; 4] = [244, 195, 59, 255];

const SHADER: &str = r#"
@group(0) @binding(0)
var overlay: texture_2d<f32>;

@group(0) @binding(1)
var overlay_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));

    var output: VertexOutput;
    output.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(overlay, overlay_sampler, input.uv);
}
"#;

#[derive(Clone, Copy)]
struct MiniMapLayout {
    panel: Rect,
    content: Rect,
    panel_radius: f32,
    shadow_padding: f32,
    marker_scale: f32,
}

impl MiniMapLayout {
    fn for_surface(scale_factor: f64, surface_width: u32, surface_height: u32) -> Self {
        let logical_width = surface_width as f64 / scale_factor;
        let logical_height = surface_height as f64 / scale_factor;
        let panel_width = ((logical_width as f32 * PANEL_SURFACE_WIDTH_RATIO)
            .min(logical_height as f32 * PANEL_SURFACE_HEIGHT_RATIO))
        .clamp(MIN_PANEL_LOGICAL_WIDTH, MAX_PANEL_LOGICAL_WIDTH);
        let content_padding = (panel_width * 0.045).clamp(6.0, 14.0);
        let content_width = panel_width - content_padding * 2.0;
        let content_height = content_width * BASE_CONTENT_HEIGHT / BASE_CONTENT_WIDTH;
        let panel_height = content_height + content_padding * 2.0;
        let shadow_padding = (panel_width * 0.03).clamp(5.0, 10.0);
        Self {
            panel: Rect {
                x: shadow_padding,
                y: shadow_padding,
                width: panel_width,
                height: panel_height,
            },
            content: Rect {
                x: shadow_padding + content_padding,
                y: shadow_padding + content_padding,
                width: content_width,
                height: content_height,
            },
            panel_radius: (panel_width * 0.058).clamp(7.0, 18.0),
            shadow_padding,
            marker_scale: (panel_width / 224.0).sqrt().clamp(0.70, 1.35),
        }
    }

    fn texture_extent(self, scale_factor: f64) -> (u32, u32) {
        (
            physical(self.panel.width + self.shadow_padding * 2.0, scale_factor),
            physical(self.panel.height + self.shadow_padding * 2.0, scale_factor),
        )
    }

    fn world_units_per_logical_x(self) -> f32 {
        WORLD_VIEW_WIDTH / self.content.width
    }

    fn world_units_per_logical_y(self) -> f32 {
        WORLD_VIEW_HEIGHT / self.content.height
    }
}

pub(super) struct MiniMapFrame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
    display_scale: f64,
    revision: Option<u64>,
}

#[derive(Clone, Copy)]
struct MiniMapGeometry {
    layout: MiniMapLayout,
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
    display_scale: f64,
    raster_scale: f64,
}

impl MiniMapGeometry {
    fn new(scale_factor: f64, surface_width: u32, surface_height: u32) -> Self {
        let display_scale = normalized_scale(scale_factor);
        let raster_scale = display_scale.min(MAX_RASTER_SCALE);
        let layout = MiniMapLayout::for_surface(display_scale, surface_width, surface_height);
        let (width, height) = layout.texture_extent(raster_scale);
        let (display_width, display_height) = layout.texture_extent(display_scale);
        Self {
            layout,
            width,
            height,
            display_width,
            display_height,
            display_scale,
            raster_scale,
        }
    }

    fn blank_pixels(self) -> Vec<u8> {
        vec![0; self.width as usize * self.height as usize * 4]
    }
}

impl MiniMapFrame {
    #[cfg(test)]
    pub(super) fn new<M: CollisionMap + ?Sized>(
        map: &M,
        player: &Role,
        scene_objects: &[SceneObject],
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let geometry = MiniMapGeometry::new(scale_factor, surface_width, surface_height);
        let mut pixels = geometry.blank_pixels();
        rasterize_panel(
            &mut pixels,
            geometry.width,
            geometry.height,
            geometry.raster_scale,
            geometry.layout,
        );
        rasterize_terrain(
            &mut pixels,
            geometry.width,
            geometry.height,
            geometry.raster_scale,
            geometry.layout,
            map,
            player,
        );
        rasterize_markers(&mut pixels, geometry, player, scene_objects);
        Self::from_pixels(pixels, geometry, None)
    }

    fn from_pixels(pixels: Vec<u8>, geometry: MiniMapGeometry, revision: Option<u64>) -> Self {
        Self {
            pixels,
            width: geometry.width,
            height: geometry.height,
            display_width: geometry.display_width,
            display_height: geometry.display_height,
            display_scale: geometry.display_scale,
            revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MiniMapKey {
    scene_number: u16,
    player_x: i32,
    player_y: i32,
    player_direction: Direction,
    markers: Vec<(u16, i32, i32)>,
    scale_bits: u64,
    surface_width: u32,
    surface_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniMapPanelKey {
    scale_bits: u64,
    surface_width: u32,
    surface_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MiniMapTerrainKey {
    panel: MiniMapPanelKey,
    scene_number: u16,
    player_x: i32,
    player_y: i32,
}

impl MiniMapPanelKey {
    fn new(scale_factor: f64, surface_width: u32, surface_height: u32) -> Self {
        Self {
            scale_bits: normalized_scale(scale_factor).to_bits(),
            surface_width,
            surface_height,
        }
    }
}

impl MiniMapKey {
    fn new(
        scene_number: u16,
        player: &Role,
        scene_objects: &[SceneObject],
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let markers = scene_objects
            .iter()
            .filter(|object| object.is_visible() && (object.can_search() || object.can_touch()))
            .map(|object| (object.id, object.world_x, object.world_y))
            .collect();
        Self {
            scene_number,
            player_x: player.world_x,
            player_y: player.world_y,
            player_direction: player.direction,
            markers,
            scale_bits: normalized_scale(scale_factor).to_bits(),
            surface_width,
            surface_height,
        }
    }
}

#[derive(Default)]
pub(super) struct MiniMapCache {
    cached: Option<(MiniMapKey, MiniMapFrame)>,
    panel: Option<(MiniMapPanelKey, Vec<u8>)>,
    terrain: Option<(MiniMapTerrainKey, Vec<u8>)>,
    revision: u64,
}

impl MiniMapCache {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn frame<M: CollisionMap + ?Sized>(
        &mut self,
        scene_number: u16,
        map: &M,
        player: &Role,
        scene_objects: &[SceneObject],
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> &MiniMapFrame {
        let key = MiniMapKey::new(
            scene_number,
            player,
            scene_objects,
            scale_factor,
            surface_width,
            surface_height,
        );
        if self
            .cached
            .as_ref()
            .is_none_or(|(cached_key, _)| cached_key != &key)
        {
            let panel_key = MiniMapPanelKey::new(scale_factor, surface_width, surface_height);
            let geometry = MiniMapGeometry::new(scale_factor, surface_width, surface_height);
            if self
                .panel
                .as_ref()
                .is_none_or(|(cached_key, _)| cached_key != &panel_key)
            {
                let mut pixels = geometry.blank_pixels();
                rasterize_panel(
                    &mut pixels,
                    geometry.width,
                    geometry.height,
                    geometry.raster_scale,
                    geometry.layout,
                );
                self.panel = Some((panel_key, pixels));
            }
            let terrain_key = MiniMapTerrainKey {
                panel: panel_key,
                scene_number,
                player_x: player.world_x,
                player_y: player.world_y,
            };
            if self
                .terrain
                .as_ref()
                .is_none_or(|(cached_key, _)| cached_key != &terrain_key)
            {
                let mut pixels = self
                    .panel
                    .as_ref()
                    .expect("minimap panel was cached")
                    .1
                    .clone();
                rasterize_terrain(
                    &mut pixels,
                    geometry.width,
                    geometry.height,
                    geometry.raster_scale,
                    geometry.layout,
                    map,
                    player,
                );
                self.terrain = Some((terrain_key, pixels));
            }
            let mut pixels = self
                .terrain
                .as_ref()
                .expect("minimap terrain was cached")
                .1
                .clone();
            rasterize_markers(&mut pixels, geometry, player, scene_objects);
            self.revision = self.revision.wrapping_add(1).max(1);
            let frame = MiniMapFrame::from_pixels(pixels, geometry, Some(self.revision));
            self.cached = Some((key, frame));
        }
        &self.cached.as_ref().expect("minimap frame was cached").1
    }
}

pub(super) struct MiniMapOverlay {
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
    scale_factor: f64,
    uploaded_revision: Option<u64>,
}

impl MiniMapOverlay {
    pub(super) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        scale_factor: f64,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("minimap_overlay_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("minimap_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("minimap_overlay_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("minimap_overlay_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        let scale_factor = normalized_scale(scale_factor);
        let (width, height) = (1, 1);
        let (texture, bind_group) =
            create_overlay_texture(device, &bind_group_layout, width, height);

        Self {
            bind_group_layout,
            bind_group,
            pipeline,
            texture,
            width,
            height,
            display_width: width,
            display_height: height,
            scale_factor,
            uploaded_revision: None,
        }
    }

    pub(super) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &MiniMapFrame,
    ) {
        let resized = self.width != frame.width || self.height != frame.height;
        if resized {
            let (texture, bind_group) =
                create_overlay_texture(device, &self.bind_group_layout, frame.width, frame.height);
            self.texture = texture;
            self.bind_group = bind_group;
            self.width = frame.width;
            self.height = frame.height;
            self.uploaded_revision = None;
        }
        self.display_width = frame.display_width;
        self.display_height = frame.display_height;
        self.scale_factor = frame.display_scale;
        if !resized && frame.revision.is_some() && self.uploaded_revision == frame.revision {
            return;
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        self.uploaded_revision = frame.revision;
    }

    pub(super) fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        surface_width: u32,
        surface_height: u32,
    ) {
        let margin = physical(SURFACE_MARGIN, self.scale_factor);
        let max_width = surface_width.saturating_sub(margin.saturating_mul(2));
        let max_height = surface_height.saturating_sub(margin.saturating_mul(2));
        if max_width == 0 || max_height == 0 {
            return;
        }
        let fit = (max_width as f32 / self.display_width as f32)
            .min(max_height as f32 / self.display_height as f32)
            .min(1.0);
        let width = (self.display_width as f32 * fit).round().max(1.0) as u32;
        let height = (self.display_height as f32 * fit).round().max(1.0) as u32;
        let origin_x = surface_width.saturating_sub(width).saturating_sub(margin);
        let origin_y = margin;

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("minimap_overlay_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_viewport(
            origin_x as f32,
            origin_y as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(origin_x, origin_y, width, height);
        pass.draw(0..4, 0..1);
    }
}

fn create_overlay_texture(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("minimap_overlay_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("minimap_overlay_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("minimap_overlay_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (texture, bind_group)
}

fn rasterize_markers(
    pixels: &mut [u8],
    geometry: MiniMapGeometry,
    player: &Role,
    scene_objects: &[SceneObject],
) {
    let MiniMapGeometry {
        layout,
        width,
        height,
        raster_scale,
        ..
    } = geometry;
    rasterize_objects(
        pixels,
        width,
        height,
        raster_scale,
        layout,
        player,
        scene_objects,
    );
    rasterize_player(
        pixels,
        width,
        height,
        raster_scale,
        layout,
        player.direction,
    );
}

fn rasterize_panel(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale_factor: f64,
    layout: MiniMapLayout,
) {
    let scale = scale_factor as f32;
    for y in 0..height {
        for x in 0..width {
            let logical_x = (x as f32 + 0.5) / scale;
            let logical_y = (y as f32 + 0.5) / scale;
            let distance =
                rounded_rect_distance(logical_x, logical_y, layout.panel, layout.panel_radius);
            if distance > 0.0 {
                let shadow = (1.0 - distance / layout.shadow_padding)
                    .clamp(0.0, 1.0)
                    .powi(2);
                blend_pixel(
                    pixels,
                    width,
                    height,
                    x as i32,
                    y as i32,
                    scaled_alpha(PANEL_SHADOW_COLOR, shadow),
                );
            }

            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            if coverage == 0.0 {
                continue;
            }
            let gradient = ((logical_y - layout.panel.y) / layout.panel.height).clamp(0.0, 1.0);
            let background = lerp_color(PANEL_TOP_COLOR, PANEL_BOTTOM_COLOR, gradient);
            blend_pixel(
                pixels,
                width,
                height,
                x as i32,
                y as i32,
                scaled_alpha(background, coverage),
            );

            let border = ((1.5 + distance) / 1.5).clamp(0.0, 1.0) * coverage;
            if border > 0.0 {
                blend_pixel(
                    pixels,
                    width,
                    height,
                    x as i32,
                    y as i32,
                    scaled_alpha(PANEL_BORDER_COLOR, border),
                );
            }
        }
    }
}

fn rasterize_terrain<M: CollisionMap + ?Sized>(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale_factor: f64,
    layout: MiniMapLayout,
    map: &M,
    player: &Role,
) {
    let scale = scale_factor as f32;
    let world_size = map.world_size();
    let start_x = (layout.content.x * scale).floor().max(0.0) as u32;
    let start_y = (layout.content.y * scale).floor().max(0.0) as u32;
    let end_x = ((layout.content.x + layout.content.width) * scale)
        .ceil()
        .min(width as f32) as u32;
    let end_y = ((layout.content.y + layout.content.height) * scale)
        .ceil()
        .min(height as f32) as u32;
    let sample_width = (end_x - start_x) as usize;
    let sample_height = (end_y - start_y) as usize;
    let mut blocked_samples = vec![None; sample_width.saturating_mul(sample_height)];

    for y in start_y..end_y {
        for x in start_x..end_x {
            let logical_x = (x as f32 + 0.5) / scale;
            let logical_y = (y as f32 + 0.5) / scale;
            let world_x = offset_world(
                player.world_x,
                (logical_x - layout.content.center_x()) * layout.world_units_per_logical_x(),
            );
            let world_y = offset_world(
                player.world_y,
                (logical_y - layout.content.center_y()) * layout.world_units_per_logical_y(),
            );
            blocked_samples[(y - start_y) as usize * sample_width + (x - start_x) as usize] =
                collision_at(map, world_size, world_x, world_y);
        }
    }

    let edge_offset_x = edge_sample_offset(layout.world_units_per_logical_x(), scale);
    let edge_offset_y = edge_sample_offset(layout.world_units_per_logical_y(), scale);
    for y in start_y..end_y {
        for x in start_x..end_x {
            let sample_x = (x - start_x) as usize;
            let sample_y = (y - start_y) as usize;
            let Some(blocked) = collision_sample(
                &blocked_samples,
                sample_width,
                sample_height,
                sample_x,
                sample_y,
            ) else {
                continue;
            };
            let edge = [
                sample_x.checked_sub(edge_offset_x).map(|x| (x, sample_y)),
                sample_x.checked_add(edge_offset_x).map(|x| (x, sample_y)),
                sample_y.checked_sub(edge_offset_y).map(|y| (sample_x, y)),
                sample_y.checked_add(edge_offset_y).map(|y| (sample_x, y)),
            ]
            .into_iter()
            .flatten()
            .filter_map(|(x, y)| {
                collision_sample(&blocked_samples, sample_width, sample_height, x, y)
            })
            .any(|neighbor| neighbor != blocked);
            let color = match (blocked, edge) {
                (true, true) => BLOCKED_EDGE_COLOR,
                (true, false) => BLOCKED_COLOR,
                (false, true) => WALKABLE_EDGE_COLOR,
                (false, false) => WALKABLE_COLOR,
            };
            let logical_x = (x as f32 + 0.5) / scale;
            let logical_y = (y as f32 + 0.5) / scale;
            let coverage = content_edge_fade(layout.content, logical_x, logical_y);
            blend_pixel(
                pixels,
                width,
                height,
                x as i32,
                y as i32,
                scaled_alpha(color, coverage),
            );
        }
    }
}

fn edge_sample_offset(world_units_per_logical_pixel: f32, raster_scale: f32) -> usize {
    (EDGE_SAMPLE_WORLD_UNITS as f32 * raster_scale / world_units_per_logical_pixel)
        .round()
        .max(1.0) as usize
}

fn collision_sample(
    samples: &[Option<bool>],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> Option<bool> {
    if x >= width || y >= height {
        return None;
    }
    samples[y.checked_mul(width)?.checked_add(x)?]
}

fn rasterize_objects(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale_factor: f64,
    layout: MiniMapLayout,
    player: &Role,
    scene_objects: &[SceneObject],
) {
    let scale = scale_factor as f32;
    let clip = layout.content.scaled(scale);
    let center_x = layout.content.center_x() * scale;
    let center_y = layout.content.center_y() * scale;
    let marker_scale = layout.marker_scale;
    for object in scene_objects
        .iter()
        .filter(|object| object.is_visible() && (object.can_search() || object.can_touch()))
    {
        let x = center_x
            + object.world_x.saturating_sub(player.world_x) as f32
                / layout.world_units_per_logical_x()
                * scale;
        let y = center_y
            + object.world_y.saturating_sub(player.world_y) as f32
                / layout.world_units_per_logical_y()
                * scale;
        if !clip.contains(x, y) {
            continue;
        }
        draw_circle_aa(
            pixels,
            width,
            height,
            x,
            y,
            4.2 * marker_scale * scale,
            OBJECT_GLOW_COLOR,
            Some(clip),
        );
        draw_circle_aa(
            pixels,
            width,
            height,
            x,
            y,
            2.1 * marker_scale * scale,
            OBJECT_COLOR,
            Some(clip),
        );
    }
}

fn rasterize_player(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale_factor: f64,
    layout: MiniMapLayout,
    direction: Direction,
) {
    let scale = scale_factor as f32;
    let clip = layout.content.scaled(scale);
    let center_x = layout.content.center_x() * scale;
    let center_y = layout.content.center_y() * scale;
    let marker_scale = layout.marker_scale;
    let (dx, dy) = direction_vector(direction);
    let tip_x = center_x + dx * 10.0 * marker_scale * scale;
    let tip_y = center_y + dy * 10.0 * marker_scale * scale;

    draw_circle_aa(
        pixels,
        width,
        height,
        center_x,
        center_y,
        7.0 * marker_scale * scale,
        PLAYER_GLOW_COLOR,
        Some(clip),
    );
    draw_line_aa(
        pixels,
        width,
        height,
        center_x,
        center_y,
        tip_x,
        tip_y,
        1.7 * marker_scale * scale,
        PLAYER_OUTLINE_COLOR,
        clip,
    );
    draw_circle_aa(
        pixels,
        width,
        height,
        center_x,
        center_y,
        4.4 * marker_scale * scale,
        PLAYER_OUTLINE_COLOR,
        Some(clip),
    );
    draw_circle_aa(
        pixels,
        width,
        height,
        center_x,
        center_y,
        2.8 * marker_scale * scale,
        PLAYER_COLOR,
        Some(clip),
    );
    draw_circle_aa(
        pixels,
        width,
        height,
        tip_x,
        tip_y,
        1.8 * marker_scale * scale,
        PLAYER_OUTLINE_COLOR,
        Some(clip),
    );
}

fn collision_at<M: CollisionMap + ?Sized>(
    map: &M,
    world_size: (i32, i32),
    world_x: i32,
    world_y: i32,
) -> Option<bool> {
    (world_x >= 0 && world_y >= 0 && world_x < world_size.0 && world_y < world_size.1)
        .then(|| map.is_world_blocked(world_x, world_y))
}

fn offset_world(base: i32, offset: f32) -> i32 {
    let offset = offset.round() as i64;
    (i64::from(base) + offset).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn direction_vector(direction: Direction) -> (f32, f32) {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    match direction {
        Direction::South => (-diagonal, diagonal),
        Direction::West => (-diagonal, -diagonal),
        Direction::North => (diagonal, -diagonal),
        Direction::East => (diagonal, diagonal),
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    fn center_y(self) -> f32 {
        self.y + self.height / 2.0
    }

    fn scaled(self, scale: f32) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
            width: self.width * scale,
            height: self.height * scale,
        }
    }
}

fn content_edge_fade(content: Rect, x: f32, y: f32) -> f32 {
    let distance = (x - content.x)
        .min(content.x + content.width - x)
        .min(y - content.y)
        .min(content.y + content.height - y);
    (distance / 6.0).clamp(0.0, 1.0)
}

fn rounded_rect_distance(x: f32, y: f32, rect: Rect, radius: f32) -> f32 {
    let center_x = rect.x + rect.width / 2.0;
    let center_y = rect.y + rect.height / 2.0;
    let qx = (x - center_x).abs() - (rect.width / 2.0 - radius);
    let qy = (y - center_y).abs() - (rect.height / 2.0 - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

#[allow(clippy::too_many_arguments)]
fn draw_circle_aa(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    center_x: f32,
    center_y: f32,
    radius: f32,
    color: [u8; 4],
    clip: Option<Rect>,
) {
    let start_x = (center_x - radius - 1.0).floor() as i32;
    let end_x = (center_x + radius + 1.0).ceil() as i32;
    let start_y = (center_y - radius - 1.0).floor() as i32;
    let end_y = (center_y + radius + 1.0).ceil() as i32;
    for y in start_y..=end_y {
        for x in start_x..=end_x {
            let pixel_x = x as f32 + 0.5;
            let pixel_y = y as f32 + 0.5;
            if clip.is_some_and(|clip| !clip.contains(pixel_x, pixel_y)) {
                continue;
            }
            let distance = (pixel_x - center_x).hypot(pixel_y - center_y);
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(pixels, width, height, x, y, scaled_alpha(color, coverage));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_line_aa(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    half_width: f32,
    color: [u8; 4],
    clip: Rect,
) {
    let min_x = start_x.min(end_x) - half_width - 1.0;
    let max_x = start_x.max(end_x) + half_width + 1.0;
    let min_y = start_y.min(end_y) - half_width - 1.0;
    let max_y = start_y.max(end_y) + half_width + 1.0;
    for y in min_y.floor() as i32..=max_y.ceil() as i32 {
        for x in min_x.floor() as i32..=max_x.ceil() as i32 {
            let pixel_x = x as f32 + 0.5;
            let pixel_y = y as f32 + 0.5;
            if !clip.contains(pixel_x, pixel_y) {
                continue;
            }
            let distance = distance_to_segment(pixel_x, pixel_y, start_x, start_y, end_x, end_y);
            let coverage = (half_width + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(pixels, width, height, x, y, scaled_alpha(color, coverage));
            }
        }
    }
}

fn distance_to_segment(
    point_x: f32,
    point_y: f32,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
) -> f32 {
    let segment_x = end_x - start_x;
    let segment_y = end_y - start_y;
    let length_squared = segment_x * segment_x + segment_y * segment_y;
    if length_squared <= f32::EPSILON {
        return (point_x - start_x).hypot(point_y - start_y);
    }
    let projection = (((point_x - start_x) * segment_x + (point_y - start_y) * segment_y)
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest_x = start_x + projection * segment_x;
    let nearest_y = start_y + projection * segment_y;
    (point_x - nearest_x).hypot(point_y - nearest_y)
}

fn blend_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, source: [u8; 4]) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= width || y >= height || source[3] == 0 {
        return;
    }
    let index = (y as usize * width as usize + x as usize) * 4;
    let destination = &mut pixels[index..index + 4];
    let source_alpha = u32::from(source[3]);
    let destination_alpha = u32::from(destination[3]);
    let inverse_source_alpha = 255 - source_alpha;
    let output_alpha_numerator = source_alpha * 255 + destination_alpha * inverse_source_alpha;
    for (output, source_channel) in destination[..3].iter_mut().zip(source[..3].iter().copied()) {
        let numerator = u32::from(source_channel) * source_alpha * 255
            + u32::from(*output) * destination_alpha * inverse_source_alpha;
        *output = ((numerator + output_alpha_numerator / 2) / output_alpha_numerator) as u8;
    }
    destination[3] = ((output_alpha_numerator + 127) / 255) as u8;
}

fn scaled_alpha(mut color: [u8; 4], scale: f32) -> [u8; 4] {
    color[3] = (f32::from(color[3]) * scale.clamp(0.0, 1.0)).round() as u8;
    color
}

fn lerp_color(start: [u8; 4], end: [u8; 4], amount: f32) -> [u8; 4] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|index| {
        (f32::from(start[index]) + (f32::from(end[index]) - f32::from(start[index])) * amount)
            .round() as u8
    })
}

fn normalized_scale(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() {
        scale_factor.clamp(0.5, 4.0)
    } else {
        1.0
    }
}

fn physical(logical: f32, scale_factor: f64) -> u32 {
    (f64::from(logical) * scale_factor).round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct TestMap;

    impl CollisionMap for TestMap {
        fn is_world_blocked(&self, world_x: i32, _world_y: i32) -> bool {
            world_x >= 800
        }

        fn world_size(&self) -> (i32, i32) {
            (2_000, 2_000)
        }
    }

    struct CountingMap(Cell<usize>);

    impl CollisionMap for CountingMap {
        fn is_world_blocked(&self, world_x: i32, _world_y: i32) -> bool {
            self.0.set(self.0.get() + 1);
            world_x >= 800
        }

        fn world_size(&self) -> (i32, i32) {
            (2_000, 2_000)
        }
    }

    fn player(direction: Direction) -> Role {
        Role {
            sprite_index: 0,
            world_x: 800,
            world_y: 800,
            direction,
            anim_frame: 0,
            frames_per_direction: 1,
        }
    }

    fn interactive_object() -> SceneObject {
        SceneObject {
            id: 1,
            world_x: 840,
            world_y: 800,
            layer: 0,
            trigger_script: 10,
            auto_script: 0,
            state: 1,
            trigger_mode: 1,
            sprite_index: Some(1),
            frames_per_direction: 1,
            sprite_frame_count: 4,
            direction: Direction::South,
            current_frame: 0,
            vanish_time: 0,
            auto_script_idle_frame: 0,
        }
    }

    fn pixel(frame: &MiniMapFrame, x: f32, y: f32) -> [u8; 4] {
        let raster_scale = frame.display_scale.min(MAX_RASTER_SCALE);
        let x = physical(x, raster_scale).min(frame.width - 1);
        let y = physical(y, raster_scale).min(frame.height - 1);
        let index = (y as usize * frame.width as usize + x as usize) * 4;
        frame.pixels[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn panel_size_tracks_small_default_and_full_screen_surfaces() {
        let small = MiniMapLayout::for_surface(1.0, 320, 200);
        let default = MiniMapLayout::for_surface(1.0, 640, 400);
        let full_screen = MiniMapLayout::for_surface(1.0, 1_920, 1_080);

        assert_eq!(small.panel.width, 96.0);
        assert_eq!(default.panel.width, 180.0);
        assert_eq!(full_screen.panel.width, 384.0);
        assert_eq!(small.texture_extent(1.0), (106, 73));
        assert_eq!(default.texture_extent(1.0), (191, 127));
        assert_eq!(full_screen.texture_extent(1.0), (404, 264));
        assert_eq!(
            MiniMapLayout::for_surface(2.0, 1_280, 800).texture_extent(2.0),
            (382, 253)
        );
        assert_eq!(normalized_scale(f64::NAN), 1.0);
    }

    #[test]
    fn rounded_panel_keeps_transparent_corners_and_translucent_center() {
        let layout = MiniMapLayout::for_surface(1.0, 640, 400);
        let frame = MiniMapFrame::new(&TestMap, &player(Direction::North), &[], 1.0, 640, 400);
        assert_eq!(pixel(&frame, 0.0, 0.0), [0, 0, 0, 0]);
        let center = pixel(&frame, layout.panel.center_x(), layout.panel.y + 4.0);
        assert!(center[3] > 100 && center[3] < 255);
        assert!(frame
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    }

    #[test]
    fn high_dpi_display_does_not_multiply_cpu_raster_work() {
        let layout = MiniMapLayout::for_surface(2.0, 1_280, 800);
        let frame = MiniMapFrame::new(&TestMap, &player(Direction::North), &[], 2.0, 1_280, 800);

        assert_eq!((frame.width, frame.height), layout.texture_extent(1.0));
        assert_eq!(
            (frame.display_width, frame.display_height),
            layout.texture_extent(2.0)
        );
        assert_eq!(frame.display_scale.min(MAX_RASTER_SCALE), 1.0);
        assert_eq!(frame.display_scale, 2.0);
    }

    #[test]
    fn cache_reuses_a_frame_until_visible_map_state_changes() {
        let mut cache = MiniMapCache::default();
        let map = CountingMap(Cell::new(0));
        let player = player(Direction::North);
        let objects = [interactive_object()];

        let first_revision = cache
            .frame(1, &map, &player, &objects, 2.0, 1_280, 800)
            .revision;
        assert!(map.0.get() > 0);
        assert!(
            map.0.get()
                <= cache.cached.as_ref().unwrap().1.width as usize
                    * cache.cached.as_ref().unwrap().1.height as usize
        );
        map.0.set(0);
        let cached_revision = cache
            .frame(1, &map, &player, &objects, 2.0, 1_280, 800)
            .revision;
        assert_eq!(map.0.get(), 0);
        assert_eq!(cached_revision, first_revision);

        let mut turned = player.clone();
        turned.direction = Direction::South;
        let turned_revision = cache
            .frame(1, &map, &turned, &objects, 2.0, 1_280, 800)
            .revision;
        assert_eq!(map.0.get(), 0);
        assert_ne!(turned_revision, first_revision);

        let mut moved = turned;
        moved.world_x += 40;
        cache.frame(1, &map, &moved, &objects, 2.0, 1_280, 800);
        assert!(map.0.get() > 0);
    }

    #[test]
    fn frame_draws_distinct_terrain_player_and_interaction_markers() {
        let layout = MiniMapLayout::for_surface(2.0, 1_280, 800);
        let frame = MiniMapFrame::new(
            &TestMap,
            &player(Direction::North),
            &[interactive_object()],
            2.0,
            1_280,
            800,
        );
        let player_pixel = pixel(&frame, layout.content.center_x(), layout.content.center_y());
        assert!(player_pixel[0] > player_pixel[1] && player_pixel[1] > player_pixel[2]);

        let object_pixel = pixel(
            &frame,
            layout.content.center_x() + 40.0 / layout.world_units_per_logical_x(),
            layout.content.center_y(),
        );
        assert!(object_pixel[0] > object_pixel[1] && object_pixel[1] > object_pixel[2]);

        let walkable = pixel(
            &frame,
            layout.content.center_x() - 0.5,
            layout.content.center_y() + 20.0,
        );
        let blocked = pixel(
            &frame,
            layout.content.center_x() + 0.5,
            layout.content.center_y() + 20.0,
        );
        assert_ne!(walkable, blocked);
    }

    #[test]
    fn hidden_and_inert_objects_do_not_change_the_overlay() {
        let baseline = MiniMapFrame::new(&TestMap, &player(Direction::South), &[], 1.0, 640, 400);
        let mut hidden = interactive_object();
        hidden.state = 0;
        let mut inert = interactive_object();
        inert.trigger_script = 0;
        let with_objects = MiniMapFrame::new(
            &TestMap,
            &player(Direction::South),
            &[hidden, inert],
            1.0,
            640,
            400,
        );
        assert_eq!(baseline.pixels, with_objects.pixels);
    }

    #[test]
    fn direction_vectors_match_pal_isometric_movement() {
        let south = direction_vector(Direction::South);
        let west = direction_vector(Direction::West);
        let north = direction_vector(Direction::North);
        let east = direction_vector(Direction::East);
        assert!(south.0 < 0.0 && south.1 > 0.0);
        assert!(west.0 < 0.0 && west.1 < 0.0);
        assert!(north.0 > 0.0 && north.1 < 0.0);
        assert!(east.0 > 0.0 && east.1 > 0.0);
    }
}
