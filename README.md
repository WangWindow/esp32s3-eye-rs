# ESP32-S3-EYE · Rust + esp-hal + Embassy

基于 **Espressif / 源地工作室（VCC-GND）ESP32-S3-EYE** 的 Rust 例程，复刻官方摄像头例程的核心链路：

```
OV2640 (DVP) ──LCD_CAM/GDMA──> QVGA YUV422 ──CPU 转换──> RGB565 ──SPI──> ST7789 240×240
```

同时加入官方 Demo 的非 AI 交互功能：屏幕模式切换、运动检测、肤色人脸候选框、暂停和状态栏。

> **关于“完整官方固件”**
>
> Espressif 出厂固件里的**人脸识别**和**语音唤醒/命令词**依赖 ESP-DL、ESP-SR 等闭源 C 组件和模型文件，本项目没有把它们移植进 Rust。`FACE` 模式使用 YCbCr 肤色连通域做“人脸候选框”演示，不是神经网络人脸检测，更不能识别人脸 ID。除此之外的摄像头预览、LCD、按键、运动检测、待机界面均已实现。

---

## 1. 已实现功能

| 功能 | 说明 |
| --- | --- |
| 摄像头实时预览 | OV2640 QVGA（320×240）YUV422，中心裁剪为 240×222，顶部 18 px 状态栏 |
| LCD 显示 | ST7789 240×240，SPI 40 MHz，RGB565 大端 |
| 帧缓冲 | 153.6 KiB 帧缓冲放在 8 MiB PSRAM，避免占用内部 SRAM |
| 模式切换 | `CAM` / `MOTION` / `FACE` / `STANDBY` |
| 运动检测 | 16×16 亮度网格帧差，检测到运动时左上角显示蓝色方块 |
| 人脸候选 | YCbCr 肤色阈值 + 4 邻域连通域，最大色块画蓝框（启发式，非 AI） |
| 按键 | UP / DOWN / PLAY 通过 ADC 电阻梯，BOOT 为 GPIO0 |
| 暂停 | PLAY 冻结当前画面，再按恢复 |
| 状态栏 | 当前模式、FPS |
| LED | 预览/检测时点亮，待机/暂停时熄灭 |
| 串口日志 | ESP32-S3 内置 USB Serial/JTAG，115200 8N1 |
| probe-rs | 支持 `idf` 格式更新应用镜像，可用于 GDB 调试 |

实测（OV2640 20 MHz XCLK、CPU 240 MHz、SPI 40 MHz）：约 **6–8 FPS**。每帧会停止 DMA、转换并刷新屏幕，因此帧率低于相机本身。

刷新策略：状态栏与摄像头预览分区显示；状态栏只在启动、模式切换时重绘，FPS 字段每秒仅在数值变化时覆盖写入；运动/人脸框直接合成到每帧扫描线中。这样避免了“先刷摄像头、再刷状态栏/框”的二次闪烁。

---

## 2. 硬件与引脚

板卡：ESP32-S3-EYE（ESP32-S3-WROOM-1-N8R8，8 MB Flash + 8 MB Octal PSRAM）。

### 摄像头 OV2640（DVP）

| 信号 | GPIO |
| --- | --- |
| XCLK / MCLK | 15 |
| PCLK | 13 |
| VSYNC | 6 |
| HREF / H_ENABLE | 7 |
| D0…D7 | 11, 9, 8, 10, 12, 18, 17, 16 |
| SIOD / SDA | 4 |
| SIOC / SCL | 5 |

### LCD ST7789

| 信号 | GPIO |
| --- | --- |
| SCLK | 21 |
| MOSI | 47 |
| DC | 43 |
| CS | 44 |
| RST | 未连接（使用软件复位） |
| BL | 48 |

### 按键 / LED

| 功能 | GPIO | 说明 |
| --- | --- | --- |
| UP | ADC1_CH0 (GPIO1) | 电阻梯约 2.41 V |
| PLAY | ADC1_CH0 (GPIO1) | 电阻梯约 1.98 V |
| DOWN | ADC1_CH0 (GPIO1) | 电阻梯约 0.82 V |
| BOOT | 0 | 低有效，内部上拉 |
| LED | 3 | 高电平点亮 |

---

## 3. 项目结构

```
.
├── Cargo.toml
├── build.rs                    # Xtensa 链接器脚本 / linkall.x
├── rust-toolchain.toml         # espup 的 esp 工具链
├── .cargo/
│   ├── config.toml             # target、runner、build-std
│   └── esp-config.toml         # esp-hal/esp-rtos 运行期配置
├── scripts/
│   ├── flash.sh                # 默认 runner：espflash 首次烧录 + monitor
│   └── probe-flash.sh          # probe-rs 更新应用镜像
└── src/
    ├── lib.rs                  # 板级驱动库
    ├── board.rs                # 引脚、分辨率、裁剪常量
    ├── buttons.rs              # ADC 电阻梯 + BOOT 按键消抖
    ├── camera.rs               # OV2640 初始化、DVP 捕获、运动/肤色分析
    ├── display.rs              # ST7789 驱动、YUV422→RGB565、OSD
    ├── graphics.rs             # 5×7 点阵字体
    ├── modes.rs                # 四种工作模式
    └── bin/main.rs             # Embassy 主任务 / 状态机
```

---

## 4. 开发环境

### 4.1 安装 Xtensa Rust 工具链

ESP32-S3 是 Xtensa 架构，需要 Espressif 的 Rust 分支：

```bash
cargo install espup --locked
espup install --targets esp32s3
source ~/export-esp.sh
```

`rust-toolchain.toml` 已固定使用 `esp` 工具链。

### 4.2 烧录工具

首次烧录需要 ESP-IDF 二级引导程序和分区表：

```bash
cargo install espflash --locked
```

`probe-rs` 用于后续应用镜像更新和调试：

```bash
cargo install probe-rs-tools --locked
```

### 4.3 编译

```bash
source ~/export-esp.sh
cargo build --release
```

---

## 5. 烧录与运行

### 5.1 默认方式（推荐，首次必须）

```bash
cargo run --release
```

脚本会优先使用 espflash；在终端中运行时等价于：

```bash
espflash flash --chip esp32s3 --port /dev/ttyACM0 \
  --after watchdog-reset --monitor \
  target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

如果串口不是 `/dev/ttyACM0`：

```bash
ESPFLASH_PORT=/dev/ttyACM1 cargo run --release
```

### 5.2 probe-rs 更新应用

引导程序已经烧录过一次后，可以只用 probe-rs 更新应用：

```bash
./scripts/probe-flash.sh target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

原理是：

```bash
probe-rs download --chip esp32s3 --binary-format idf \
  target/xtensa-esp32s3-none-elf/release/esp32s3-eye
probe-rs reset --chip esp32s3
```

`--binary-format idf` 会生成 ESP-IDF 兼容的应用镜像并写入 `0x10000`。

### 5.3 probe-rs GDB 调试

```bash
probe-rs gdb --chip esp32s3 target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

> 注意：使用 probe-rs 时不要同时用 `cat`、`minicom` 或 `espflash monitor` 占住 `/dev/ttyACM0`，USB Serial/JTAG 同一时间只能被一个工具打开。

---

## 6. 操作说明

| 按键 | 功能 |
| --- | --- |
| UP | 下一个模式：CAM → MOTION → FACE → STANDBY |
| DOWN | 上一个模式 |
| BOOT | 下一个模式（与 UP 相同） |
| PLAY | 暂停 / 恢复画面 |

模式说明：

- **CAM**：实时摄像头画面 + 模式/FPS 状态栏。
- **MOTION**：在 CAM 基础上做 16×16 网格帧差，检测到运动时左上角显示蓝色方块。
- **FACE**：在 CAM 基础上做肤色连通域，最大色块画蓝框。**这是演示用的启发式，不是人脸识别。**
- **STANDBY**：停止摄像头，显示标题和按键说明。

启动后默认进入 **CAM** 模式，方便直接确认摄像头和屏幕是否工作。

---

## 7. 实现要点

### 7.1 摄像头格式

OV2640 输出 **QVGA YUV422（YUYV）**：

```
[Y0, Cb, Y1, Cr] [Y0, Cb, Y1, Cr] ...
```

`esp32s3-cam-async` 负责 OV2640 SCCB 初始化和 LCD_CAM/DMA 捕获状态机；本项目负责：

- 等待 VSYNC 对齐的完整帧；
- 把 DMA 分块复制到 PSRAM 帧缓冲；
- 逐行把 YUYV 转成 RGB565；
- 中心裁剪 `x = 40..280`、`y = 9..231`，显示为 240×222；
- 顶部 18 px 留给状态栏，摄像头窗口从 y=18 开始，互不覆盖；
- 如果摄像头模组相对屏幕上下颠倒，`board.rs` 中的 `CAM_VFLIP = true` 会在扫描输出时翻转 Y 方向，同时人脸框坐标也会同步翻转。

### 7.2 为什么用 PSRAM

QVGA 一帧 YUV422 是：

```
320 × 240 × 2 = 153,600 字节
```

放在内部 SRAM 会挤占栈、DMA 描述符和 Embassy 运行时。ESP32-S3-EYE 板载 8 MB PSRAM，因此用 `esp_hal::psram::Psram` 把帧缓冲放到外部 RAM。DMA 仍然写内部 2 KiB 环形缓冲，再由 CPU 拷贝到 PSRAM。

### 7.3 显示

ST7789 使用：

- `COLMOD = 0x55`（RGB565）
- `MADCTL = 0x00`（不镜像、不交换 XY）
- `INVON`（该 1.3 寸屏默认需要反色）
- 背光 GPIO48 **低电平点亮**（板载背光电路为低有效，esp-bsp 也把 LEDC 占空比反相）

如果画面上下/左右颠倒，修改 `display.rs` 中 `MADCTL` 的值即可：

| 值 | 效果 |
| --- | --- |
| `0x00` | 默认 |
| `0x60` | 旋转 90° |
| `0xC0` | 旋转 180° |
| `0xA0` | 旋转 270° |

### 7.4 按键

三个正面按键共用一个 ADC 电阻梯（GPIO1 / ADC1_CH0），按下时分别产生约 2.41 V、1.98 V、0.82 V。代码把 12 位 ADC 原始值换算成 mV 后与这些中心值比较，并做两次扫描消抖。

BOOT 是独立的 GPIO0 低有效按键。

如果实测 UP / PLAY / DOWN 与预期不一致，交换 `buttons.rs` 中 `LADDER` 数组的按键映射即可（不同批次的丝印顺序可能不同）。

---

## 8. 常见问题

### 屏幕全黑

1. 确认背光 GPIO48 为低（低有效）；
2. 确认 LCD 的 CS=44、DC=43、SCLK=21、MOSI=47；
3. 尝试修改 `MADCTL` 的旋转值；
4. 确认使用的是 240×240 ST7789 子板；
5. 若背光极性因板卡版本不同而相反，把 `display.rs` 中 `init()` 和 `set_backlight()` 的高低电平对调。

### 摄像头画面上下颠倒

修改 `src/board.rs`：

```rust
pub const CAM_VFLIP: bool = true;   // 上下颠倒时设为 true
```

翻转在显示扫描阶段完成，不改变 OV2640 寄存器，人脸框坐标也会同步适配。

### 摄像头画面全黑

1. 撕掉摄像头镜头保护膜；
2. 确认环境有光；
3. 检查 24-pin FPC 是否插紧；
4. 串口日志出现 `OV2640 ready` 说明 SCCB 正常；
5. 如果出现 `camera DMA start failed` 或长时间没有 `frames=` 日志，尝试把 `camera.rs` 中的 `CAMERA_CLOCK` 从 `HIGH_FPS` 改为 `BALANCED`。

### `cargo run` 提示找不到 espflash

```bash
cargo install espflash --locked
```

或者使用：

```bash
./scripts/probe-flash.sh target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

### `probe-rs` 报 “Failed to open probe”

先关闭占用 `/dev/ttyACM0` 的串口工具：

```bash
sudo fuser -k /dev/ttyACM0
```

### 编译时报 Xtensa linker 找不到

```bash
source ~/export-esp.sh
```

每个新终端都需要执行一次。

---

## 9. 参考与致谢

- Espressif ESP32-S3-EYE 用户指南与原理图
- [esp-bsp / esp32_s3_eye](https://github.com/espressif/esp-bsp/tree/master/bsp/esp32_s3_eye)：引脚定义、LCD/按键配置
- [esp32s3-cam-async](https://github.com/cbruiz/esp32s3-cam-async)：OV2640 初始化表、LCD_CAM/DMA 捕获状态机（MIT OR Apache-2.0）
- [esp-hal](https://github.com/esp-rs/esp-hal)、[esp-rtos](https://github.com/esp-rs/esp-rtos)、[Embassy](https://embassy.dev/)
- Espressif ESP-WHO / 出厂 Demo：模式与交互逻辑参考（AI/语音部分未移植）

## 10. 许可证

MIT OR Apache-2.0。
