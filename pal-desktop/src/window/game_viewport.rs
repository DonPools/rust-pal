//! Integer-scaled game presentation with optional space reserved beside the framebuffer.

use pixels::wgpu;

pub(super) const BATTLE_ASSIST_LOGICAL_WIDTH: f64 = 300.0;

const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 4.0 / 255.0,
    g: 8.0 / 255.0,
    b: 13.0 / 255.0,
    a: 1.0,
};

const SHADER: &str = r#"
@group(0) @binding(0)
var game: texture_2d<f32>;

@group(0) @binding(1)
var game_sampler: sampler;

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
    let color = textureSample(game, game_sampler, input.uv);
    return vec4<f32>(color.rgb, 1.0);
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SurfaceRect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GameSurfaceLayout {
    pub(super) game: SurfaceRect,
    pub(super) battle_assist: Option<SurfaceRect>,
}

impl GameSurfaceLayout {
    pub(super) fn new(
        surface_width: u32,
        surface_height: u32,
        game_width: u32,
        game_height: u32,
        scale_factor: f64,
        battle_assist_requested: bool,
        reserved_game_scale: Option<u32>,
    ) -> Self {
        let full_scale = integer_scale(surface_width, surface_height, game_width, game_height);
        let panel_width = physical(BATTLE_ASSIST_LOGICAL_WIDTH, scale_factor);
        let game_area_width = surface_width.saturating_sub(panel_width);
        let game_scale = reserved_game_scale.unwrap_or(full_scale);
        let can_reserve_panel = battle_assist_requested
            && panel_width < surface_width
            && game_area_width >= game_width.saturating_mul(game_scale)
            && surface_height >= game_height.saturating_mul(game_scale);
        if can_reserve_panel {
            Self {
                game: centered_game_rect(
                    game_area_width,
                    surface_height,
                    game_width,
                    game_height,
                    game_scale,
                ),
                battle_assist: Some(SurfaceRect {
                    x: game_area_width,
                    y: 0,
                    width: panel_width,
                    height: surface_height,
                }),
            }
        } else {
            Self {
                game: centered_game_rect(
                    surface_width,
                    surface_height,
                    game_width,
                    game_height,
                    full_scale,
                ),
                battle_assist: None,
            }
        }
    }
}

fn integer_scale(
    available_width: u32,
    available_height: u32,
    game_width: u32,
    game_height: u32,
) -> u32 {
    (available_width / game_width.max(1))
        .min(available_height / game_height.max(1))
        .max(1)
}

fn centered_game_rect(
    available_width: u32,
    available_height: u32,
    game_width: u32,
    game_height: u32,
    scale: u32,
) -> SurfaceRect {
    let width = game_width.saturating_mul(scale).min(available_width);
    let height = game_height.saturating_mul(scale).min(available_height);
    SurfaceRect {
        x: available_width.saturating_sub(width) / 2,
        y: available_height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn physical(logical: f64, scale_factor: f64) -> u32 {
    (logical * scale_factor.clamp(1.0, 3.0)).round().max(1.0) as u32
}

pub(super) struct GameViewportRenderer {
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl GameViewportRenderer {
    pub(super) fn new(
        device: &wgpu::Device,
        source: &wgpu::Texture,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("game_viewport_bind_group_layout"),
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
        let view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("game_viewport_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("game_viewport_bind_group"),
            layout: &bind_group_layout,
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("game_viewport_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("game_viewport_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("game_viewport_pipeline"),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        Self {
            bind_group,
            pipeline,
        }
    }

    pub(super) fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        rect: SurfaceRect,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("game_viewport_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(BACKGROUND),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_viewport(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_panel_does_not_change_the_default_game_rect() {
        let layout = GameSurfaceLayout::new(940, 400, 320, 200, 1.0, true, Some(2));

        assert_eq!(
            layout.game,
            SurfaceRect {
                x: 0,
                y: 0,
                width: 640,
                height: 400
            }
        );
        assert_eq!(
            layout.battle_assist,
            Some(SurfaceRect {
                x: 640,
                y: 0,
                width: 300,
                height: 400
            })
        );
    }

    #[test]
    fn panel_is_omitted_when_it_would_reduce_game_scale() {
        let layout = GameSurfaceLayout::new(640, 400, 320, 200, 1.0, true, Some(2));

        assert_eq!(
            layout.game,
            SurfaceRect {
                x: 0,
                y: 0,
                width: 640,
                height: 400
            }
        );
        assert_eq!(layout.battle_assist, None);
    }

    #[test]
    fn high_dpi_panel_uses_physical_width_without_moving_the_game() {
        let layout = GameSurfaceLayout::new(1880, 800, 320, 200, 2.0, true, Some(4));

        assert_eq!(
            layout.game,
            SurfaceRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 800
            }
        );
        assert_eq!(
            layout.battle_assist,
            Some(SurfaceRect {
                x: 1280,
                y: 0,
                width: 600,
                height: 800
            })
        );
    }

    #[test]
    fn unreserved_panel_only_uses_existing_space() {
        let without_space = GameSurfaceLayout::new(960, 600, 320, 200, 1.0, true, None);
        let with_space = GameSurfaceLayout::new(1260, 600, 320, 200, 1.0, true, None);

        assert_eq!(without_space.battle_assist, None);
        assert_eq!(with_space.game.width, 960);
        assert!(with_space.battle_assist.is_some());
    }
}
