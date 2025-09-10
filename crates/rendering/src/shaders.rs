use crate::error::Result;
use wgpu::ShaderModule as WgpuShaderModule;

pub struct ShaderModule {
    pub vertex_shader: WgpuShaderModule,
    pub fragment_shader: WgpuShaderModule,
}

impl ShaderModule {
    pub fn new(device: &wgpu::Device) -> Result<Self> {
        let vertex_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Vertex Shader"),
            source: wgpu::ShaderSource::Wgsl(VERTEX_SHADER.into()),
        });

        let fragment_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Fragment Shader"),
            source: wgpu::ShaderSource::Wgsl(FRAGMENT_SHADER.into()),
        });

        Ok(Self {
            vertex_shader,
            fragment_shader,
        })
    }
}

const VERTEX_SHADER: &str = r#"
struct Uniforms {
    transform: mat4x4<f32>,
    window_center: f32,
    window_width: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    
    // Create a full-screen triangle
    let x = f32((vertex_index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vertex_index & 2u) * 2.0 - 1.0;
    
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coords = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    
    return out;
}
"#;

const FRAGMENT_SHADER: &str = r#"
struct Uniforms {
    transform: mat4x4<f32>,
    window_center: f32,
    window_width: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var volume_texture: texture_3d<f32>;

@group(0) @binding(2)
var volume_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the volume texture
    let sample_coord = vec3<f32>(in.tex_coords, 0.5);
    let sample_value = textureSample(volume_texture, volume_sampler, sample_coord).r;
    
    // Apply window/level
    let window_min = uniforms.window_center - uniforms.window_width * 0.5;
    let window_max = uniforms.window_center + uniforms.window_width * 0.5;
    
    let normalized = clamp((sample_value - window_min) / (window_max - window_min), 0.0, 1.0);
    
    return vec4<f32>(normalized, normalized, normalized, 1.0);
}
"#;
