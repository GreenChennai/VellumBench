//! vello GPU 离屏渲染 + 读回(ADR-0047 路线 (a)/(b) 的上屏半程)。
//!
//! wgpu 类型一律经 `vello::wgpu` re-export(vello 0.10 锁 wgpu 29.x,与
//! sable-paint 同一实例 —— sable-paint/src/gpu.rs 的版本对齐纪律照搬)。
//! 设备创建复用 sable-paint 的 `create_instance`/`create_device`
//! (后端选择:SABLE_GPU_BACKEND > 编译期 feature > 平台默认)。
//!
//! 已核实的硬边界(ADR-0046 证据):gpui 0.2.2 Windows 平台是 DirectX
//! 渲染器(`platform/windows/directx_renderer.rs`),`Window::paint_image`
//! 只接受 CPU 内存 `RenderImage`(`assets.rs`),**无外部纹理导入 API**
//! —— 故「零拷贝 NT handle 上屏」在 0.2.2 不可行,离屏纹理必须
//! `copy_texture_to_buffer` 读回;读回成本正是本模块要量的数。

use std::sync::mpsc;
use std::time::{Duration, Instant};

use sable::paint::gpu::{create_device, create_instance};
use vello::kurbo::Affine;
use vello::peniko::Color;
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// vello 目标纹理 + 读回缓冲的缓存(尺寸不变则跨帧复用)。
struct Targets {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

/// 独立 wgpu 设备 + vello 渲染器(与 gpui 的 DirectX 各自独立)。
pub struct GpuFrame {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    targets: Option<Targets>,
}

impl GpuFrame {
    /// 阻塞式设备初始化(启动期一次;pollster block_on,窗口打开前)。
    pub fn new() -> Result<Self, String> {
        pollster::block_on(async {
            let instance = create_instance();
            let (adapter, device, queue) = create_device(&instance)
                .await
                .map_err(|e| format!("wgpu 设备创建失败:{e}"))?;
            let renderer = Renderer::new(
                &device,
                RendererOptions {
                    use_cpu: false,
                    antialiasing_support: AaSupport::area_only(),
                    ..RendererOptions::default()
                },
            )
            .map_err(|e| format!("vello Renderer 创建失败:{e}"))?;
            Ok(Self {
                adapter,
                device,
                queue,
                renderer,
                targets: None,
            })
        })
    }

    /// adapter 信息(启动日志:排查用户机器问题的第一现场,sable 同口径)。
    pub fn adapter_info(&self) -> String {
        let i = self.adapter.get_info();
        format!("{} / {:?} / {}", i.name, i.backend, i.driver)
    }

    /// 渲染一帧并读回 RGBA8(不透明底,预乘 == 直 alpha)。
    /// 返回(像素,渲染+读回耗时)。
    pub fn render_to_rgba(
        &mut self,
        scene: &Scene,
        width: u32,
        height: u32,
    ) -> Result<(Vec<u8>, Duration), String> {
        let t0 = Instant::now();
        self.ensure_targets(width, height)?;
        // 字段级析构:texture/view/readback 与 renderer/device/queue 的借用互不相交。
        let Targets {
            texture,
            view,
            readback,
            bytes_per_row,
            ..
        } = self.targets.as_ref().expect("ensure_targets 刚建立");
        let params = RenderParams {
            base_color: Color::new([0.08, 0.09, 0.10, 1.0]),
            width,
            height,
            antialiasing_method: AaConfig::Area,
        };
        self.renderer
            .render_to_texture(&self.device, &self.queue, scene, view, &params)
            .map_err(|e| format!("render_to_texture 失败:{e}"))?;

        // 纹理 → 行对齐缓冲(256B 对齐)→ map 读回(wgpu 29:map_async +
        // poll(Wait) 驱动回调;channel 等待回调完成)。
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(*bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        // poll(Wait) 驱动 map 回调;PollError(超时/设备丢失)由回调侧
        // Result 传达,此处忽略即可。
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        receiver
            .recv()
            .map_err(|e| format!("map 回调通道断开:{e}"))?
            .map_err(|e| format!("buffer map 失败:{e}"))?;

        let row_len = (width * 4) as usize;
        let stride = *bytes_per_row as usize;
        let data = readback.slice(..).get_mapped_range();
        let mut rgba = Vec::with_capacity(row_len * height as usize);
        if stride == row_len {
            rgba.extend_from_slice(&data);
        } else {
            for row in 0..height as usize {
                let start = row * stride;
                rgba.extend_from_slice(&data[start..start + row_len]);
            }
        }
        drop(data);
        readback.unmap();
        Ok((rgba, t0.elapsed()))
    }

    fn ensure_targets(&mut self, width: u32, height: u32) -> Result<(), String> {
        if let Some(t) = &self.targets {
            if t.width == width && t.height == height {
                return Ok(());
            }
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas-spike offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes_per_row = width.div_ceil(64) * 256; // 4B/px → 64px 对齐即 256B
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("canvas-spike readback"),
            size: bytes_per_row as u64 * height as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.targets = Some(Targets {
            texture,
            view,
            readback,
            width,
            height,
            bytes_per_row,
        });
        Ok(())
    }
}

/// 视口仿射:世界(画板本地)→ 目标像素。`zoom` 缩放 + 平移使画板中心
/// 对准目标中心(4x 时看画板中部,0.25x 时整板可见)。
pub fn viewport_transform(world: [f64; 2], target: [f64; 2], zoom: f64) -> Affine {
    let pan_x = (target[0] - world[0] * zoom) / 2.0;
    let pan_y = (target[1] - world[1] * zoom) / 2.0;
    Affine::new([zoom, 0.0, 0.0, zoom, pan_x, pan_y])
}
