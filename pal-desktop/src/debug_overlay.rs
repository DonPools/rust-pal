//! Physical-resolution debug information rendered after the virtual framebuffer.

use pal_core::scene::{TriggerKind, TriggerRequest};
use pal_core::script::{ScriptDebugSnapshot, ScriptInstructionDebug, ScriptOpcode};
use pixels::wgpu;

const PANEL_LOGICAL_WIDTH: u32 = 320;
const PANEL_LOGICAL_HEIGHT: u32 = 190;
const PANEL_LOGICAL_MARGIN: f32 = 8.0;
const TEXT_LOGICAL_X: u32 = 8;
const TEXT_LOGICAL_Y: u32 = 8;
const TEXT_LOGICAL_SCALE: u32 = 1;
const LINE_LOGICAL_HEIGHT: u32 = 12;

const BACKGROUND: [u8; 4] = [12, 16, 20, 218];
const BORDER: [u8; 4] = [104, 120, 128, 255];
const TEXT: [u8; 4] = [232, 240, 236, 255];
const ACCENT: [u8; 4] = [80, 224, 144, 255];

const SHADER: &str = r#"
@group(0) @binding(0)
var panel: texture_2d<f32>;

@group(0) @binding(1)
var panel_sampler: sampler;

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
    return textureSample(panel, panel_sampler, input.uv);
}
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DebugSnapshot {
    pub scene_number: u16,
    pub object_count: usize,
    pub player_x: i32,
    pub player_y: i32,
    pub camera_x: i32,
    pub camera_y: i32,
    pub virtual_width: u32,
    pub virtual_height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub scale_factor: f64,
    pub focused_object: Option<DebugObjectSnapshot>,
    pub script: ScriptDebugSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DebugObjectSnapshot {
    pub id: u16,
    pub world_x: i32,
    pub world_y: i32,
    pub state: i16,
    pub layer: i16,
    pub trigger_mode: u16,
    pub trigger_script: u16,
    pub auto_script: u16,
    pub sprite_index: Option<usize>,
    pub frames_per_direction: u16,
    pub sprite_frame_count: usize,
    pub direction: u16,
    pub current_frame: u16,
    pub vanish_time: i16,
    pub visible: bool,
    pub blocker: bool,
    pub can_search: bool,
    pub can_touch: bool,
}

pub(crate) struct DebugOverlay {
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    scale_factor: f64,
}

impl DebugOverlay {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        scale_factor: f64,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug_overlay_bind_group_layout"),
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
            label: Some("debug_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("debug_overlay_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug_overlay_pipeline"),
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
        let (width, height) = panel_extent(scale_factor);
        let (texture, bind_group) = create_panel_texture(device, &bind_group_layout, width, height);

        Self {
            bind_group_layout,
            bind_group,
            pipeline,
            texture,
            pixels: vec![0; width as usize * height as usize * 4],
            width,
            height,
            scale_factor,
        }
    }

    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, snapshot: DebugSnapshot) {
        if (snapshot.scale_factor - self.scale_factor).abs() > f64::EPSILON {
            self.resize(device, snapshot.scale_factor);
        }

        rasterize_panel(&mut self.pixels, self.width, self.height, snapshot);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        surface_width: u32,
        surface_height: u32,
    ) {
        let origin = physical(PANEL_LOGICAL_MARGIN, self.scale_factor);
        let available_width = surface_width.saturating_sub(origin);
        let available_height = surface_height.saturating_sub(origin);
        let width = self.width.min(available_width);
        let height = self.height.min(available_height);
        if width == 0 || height == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("debug_overlay_render_pass"),
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
        let origin_x = surface_width.saturating_sub(width).saturating_sub(origin);
        pass.set_viewport(
            origin_x as f32,
            origin as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(origin_x, origin, width, height);
        pass.draw(0..4, 0..1);
    }

    fn resize(&mut self, device: &wgpu::Device, scale_factor: f64) {
        let (width, height) = panel_extent(scale_factor);
        let (texture, bind_group) =
            create_panel_texture(device, &self.bind_group_layout, width, height);
        self.texture = texture;
        self.bind_group = bind_group;
        self.pixels.resize(width as usize * height as usize * 4, 0);
        self.width = width;
        self.height = height;
        self.scale_factor = scale_factor;
    }
}

fn create_panel_texture(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("debug_overlay_texture"),
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
        label: Some("debug_overlay_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("debug_overlay_bind_group"),
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

fn panel_extent(scale_factor: f64) -> (u32, u32) {
    (
        physical(PANEL_LOGICAL_WIDTH as f32, scale_factor),
        physical(PANEL_LOGICAL_HEIGHT as f32, scale_factor),
    )
}

fn physical(logical: f32, scale_factor: f64) -> u32 {
    (f64::from(logical) * scale_factor).round().max(1.0) as u32
}

fn rasterize_panel(pixels: &mut [u8], width: u32, height: u32, snapshot: DebugSnapshot) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&BACKGROUND);
    }
    draw_border(pixels, width, height, BORDER);

    let lines = snapshot_lines(snapshot);
    let scale = physical(TEXT_LOGICAL_SCALE as f32, snapshot.scale_factor);
    let x = physical(TEXT_LOGICAL_X as f32, snapshot.scale_factor);
    let start_y = physical(TEXT_LOGICAL_Y as f32, snapshot.scale_factor);
    let line_height = physical(LINE_LOGICAL_HEIGHT as f32, snapshot.scale_factor);
    for (line_index, line) in lines.iter().enumerate() {
        let color = if line_index == 0 { ACCENT } else { TEXT };
        draw_text(
            pixels,
            width,
            height,
            x,
            start_y + line_index as u32 * line_height,
            scale,
            line,
            color,
        );
    }
}

fn snapshot_lines(snapshot: DebugSnapshot) -> Vec<String> {
    let mut lines = vec![
        format!(
            "SCENE {} OBJ {}",
            snapshot.scene_number, snapshot.object_count
        ),
        format!(
            "PLAYER {},{} CAM {},{}",
            snapshot.player_x, snapshot.player_y, snapshot.camera_x, snapshot.camera_y
        ),
        format!(
            "VIEW {}X{}",
            snapshot.virtual_width, snapshot.virtual_height
        ),
        script_status(snapshot.script),
        script_owner_line(snapshot.script),
        script_trigger(snapshot.script.trigger),
        instruction_line("LAST", snapshot.script.last_instruction),
        instruction_line("NEXT", snapshot.script.next_instruction),
        format!(
            "WAIT {} CALL {}",
            snapshot.script.wait_frames, snapshot.script.call_depth
        ),
    ];
    if let Some(object) = snapshot.focused_object {
        lines.extend([
            format!(
                "FOCUS #{} POS {},{}",
                object.id, object.world_x, object.world_y
            ),
            format!(
                "STATE {} LAYER {} MODE {}",
                object.state, object.layer, object.trigger_mode
            ),
            format!(
                "SPRITE {} FRAME {}/{} TOT {} DIR {}",
                object
                    .sprite_index
                    .map_or("-".to_owned(), |index| index.to_string()),
                object.current_frame,
                object.frames_per_direction,
                object.sprite_frame_count,
                direction_name(object.direction)
            ),
            format!(
                "TRIG {} AUTO {} VANISH {}",
                object.trigger_script, object.auto_script, object.vanish_time
            ),
            format!(
                "VIS {} BLOCK {} SEARCH {} TOUCH {}",
                flag(object.visible),
                flag(object.blocker),
                flag(object.can_search),
                flag(object.can_touch)
            ),
        ]);
    } else {
        lines.push("FOCUS NONE".to_owned());
    }
    lines
}

fn direction_name(direction: u16) -> &'static str {
    match direction {
        0 => "N",
        1 => "E",
        2 => "S",
        3 => "W",
        _ => "?",
    }
}

fn flag(value: bool) -> &'static str {
    if value {
        "Y"
    } else {
        "N"
    }
}

fn script_status(script: ScriptDebugSnapshot) -> String {
    if script.active {
        "SCRIPT ACTIVE".to_owned()
    } else if script.trigger.is_some() {
        "SCRIPT COMPLETE".to_owned()
    } else {
        "SCRIPT IDLE".to_owned()
    }
}

fn script_trigger(trigger: Option<TriggerRequest>) -> String {
    let Some(trigger) = trigger else {
        return "ROOT -".to_owned();
    };
    let kind = match trigger.kind {
        TriggerKind::Search => "SEARCH",
        TriggerKind::Touch => "TOUCH",
        TriggerKind::Item => "ITEM",
    };
    format!(
        "ROOT {kind} {} @{}",
        owner_name(trigger.object_id),
        trigger.script_entry
    )
}

fn script_owner_line(script: ScriptDebugSnapshot) -> String {
    let owner = script
        .next_instruction
        .or(script.last_instruction)
        .map(|instruction| instruction.object_id)
        .or_else(|| script.trigger.map(|trigger| trigger.object_id));
    format!("OWNER {}", owner.map_or("-".to_owned(), owner_name))
}

fn owner_name(object_id: u16) -> String {
    if object_id == 0xffff {
        "SYSTEM".to_owned()
    } else {
        format!("#{}", object_id)
    }
}

fn instruction_line(label: &str, instruction: Option<ScriptInstructionDebug>) -> String {
    instruction.map_or_else(
        || format!("{label} -"),
        |instruction| {
            let mnemonic = instruction
                .decoded_opcode()
                .map_or("UNKNOWN", ScriptOpcode::mnemonic);
            format!(
                "{label} @{} {:04X} {mnemonic} {:04X}/{:04X}/{:04X}",
                instruction.entry,
                instruction.opcode,
                instruction.operands[0],
                instruction.operands[1],
                instruction.operands[2]
            )
        },
    )
}

fn draw_border(pixels: &mut [u8], width: u32, height: u32, color: [u8; 4]) {
    for x in 0..width {
        put_rgba(pixels, width, height, x, 0, color);
        put_rgba(pixels, width, height, x, height - 1, color);
    }
    for y in 0..height {
        put_rgba(pixels, width, height, 0, y, color);
        put_rgba(pixels, width, height, width - 1, y, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
    color: [u8; 4],
) {
    for (index, character) in text.chars().enumerate() {
        for (row, bits) in glyph(character).iter().enumerate() {
            for column in 0..5 {
                if bits & (0b1_0000 >> column) == 0 {
                    continue;
                }
                let pixel_x = x + (index as u32 * 6 + column) * scale;
                let pixel_y = y + row as u32 * scale;
                fill_rect(pixels, width, height, pixel_x, pixel_y, scale, scale, color);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    rect_width: u32,
    rect_height: u32,
    color: [u8; 4],
) {
    for row in y..y.saturating_add(rect_height).min(height) {
        for column in x..x.saturating_add(rect_width).min(width) {
            put_rgba(pixels, width, height, column, row, color);
        }
    }
}

fn put_rgba(pixels: &mut [u8], width: u32, height: u32, x: u32, y: u32, color: [u8; 4]) {
    if x >= width || y >= height {
        return;
    }
    let index = (y as usize * width as usize + x as usize) * 4;
    pixels[index..index + 4].copy_from_slice(&color);
}

pub(crate) fn glyph(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '#' => [10, 31, 10, 10, 31, 10, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        ',' => [0, 0, 0, 0, 0, 4, 8],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        '@' => [14, 17, 23, 21, 23, 16, 14],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(scale_factor: f64) -> DebugSnapshot {
        DebugSnapshot {
            scene_number: 3,
            object_count: 12,
            player_x: 320,
            player_y: 200,
            camera_x: 160,
            camera_y: 100,
            virtual_width: 320,
            virtual_height: 200,
            surface_width: 1280,
            surface_height: 800,
            scale_factor,
            focused_object: Some(DebugObjectSnapshot {
                id: 7,
                world_x: 330,
                world_y: 205,
                state: 2,
                layer: 1,
                trigger_mode: 4,
                trigger_script: 100,
                auto_script: 200,
                sprite_index: Some(7),
                frames_per_direction: 3,
                sprite_frame_count: 12,
                direction: 2,
                current_frame: 1,
                vanish_time: 0,
                visible: true,
                blocker: true,
                can_search: true,
                can_touch: true,
            }),
            script: ScriptDebugSnapshot {
                active: false,
                trigger: None,
                last_instruction: None,
                next_instruction: None,
                call_depth: 0,
                wait_frames: 0,
            },
        }
    }

    #[test]
    fn panel_extent_tracks_physical_resolution() {
        assert_eq!(panel_extent(1.0), (320, 190));
        assert_eq!(panel_extent(2.0), (640, 380));
        assert_eq!(panel_extent(1.5), (480, 285));
    }

    #[test]
    fn panel_rasterization_uses_background_border_and_text() {
        let (width, height) = panel_extent(1.0);
        let mut pixels = vec![0; width as usize * height as usize * 4];
        rasterize_panel(&mut pixels, width, height, snapshot(1.0));

        assert_eq!(&pixels[..4], &BORDER);
        assert!(pixels.chunks_exact(4).any(|pixel| pixel == BACKGROUND));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel == ACCENT));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel == TEXT));
    }

    #[test]
    fn snapshot_lines_fit_inside_the_logical_panel() {
        let lines = snapshot_lines(snapshot(1.0));
        let text_width = |line: &str| line.chars().count() as u32 * 6 * TEXT_LOGICAL_SCALE;
        let text_height = 7 * TEXT_LOGICAL_SCALE;

        assert!(lines
            .iter()
            .all(|line| TEXT_LOGICAL_X + text_width(line) < PANEL_LOGICAL_WIDTH));
        assert!(
            TEXT_LOGICAL_Y + (lines.len() as u32 - 1) * LINE_LOGICAL_HEIGHT + text_height
                < PANEL_LOGICAL_HEIGHT
        );
    }
}
