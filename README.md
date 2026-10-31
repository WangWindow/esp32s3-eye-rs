# ESP32-S3-EYE · Rust + esp-hal + Embassy

ESP32-S3-EYE 摄像头 / LCD / SD 卡例程：

```
OV2640 → LCD_CAM + GDMA → RGB565 → ST7789 240×240
```

## 功能

- OV2640 QVGA RGB565，20 MHz XCLK，稳定 **12 FPS**
- ST7789 240×240，SPI 80 MHz，RGB565 逐行直拷
- `CAM` / `MOTION` / `FACE` / `SD` / `STANDBY` 五种模式
- 运动检测、肤色人脸候选框、5 点关键点（启发式）
- microSD 1-bit SDMMC：容量检测 + 非破坏性块读写自检
- UP / DOWN / PLAY / BOOT 按键，PLAY 暂停，LED 指示

> `FACE` 不是 ESP-DL 人脸识别，只是肤色 + 关键点演示。

## 引脚

| 功能 | GPIO |
| --- | --- |
| 摄像头 XCLK / PCLK / VSYNC / HREF | 15 / 13 / 6 / 7 |
| 摄像头 D0–D7 | 11, 9, 8, 10, 12, 18, 17, 16 |
| 摄像头 SCCB SDA / SCL | 4 / 5 |
| LCD SCLK / MOSI / DC / CS / BL | 21 / 47 / 43 / 44 / 48 |
| UP / PLAY / DOWN | ADC1_CH0 (GPIO1) 电阻梯 |
| BOOT / LED | 0 / 3 |
| microSD CLK / CMD / D0 | 39 / 38 / 40 |

板级引脚、几何尺寸、时钟参数和所有板载硬件接口集中放在 `crates/esp32s3-eye-board`。

## 构建

```bash
cargo install espup --locked
espup install --targets esp32s3
source ~/export-esp.sh
cargo install espflash --locked

cargo build --release
```

每个新终端都需要先 `source ~/export-esp.sh`。

## 烧录

```bash
cargo run --release
# 或指定串口
ESPFLASH_PORT=/dev/ttyACM1 cargo run --release
```

首次烧录由 espflash 写入 ESP-IDF 引导程序和分区表。之后可以只更新应用：

```bash
./scripts/probe-flash.sh target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

probe-rs 调试：

```bash
probe-rs gdb --chip esp32s3 target/xtensa-esp32s3-none-elf/release/esp32s3-eye
```

使用 probe-rs 时不要同时占用 `/dev/ttyACM0`。

## 操作

| 按键 | 功能 |
| --- | --- |
| UP / BOOT | 下一个模式 |
| DOWN | 上一个模式 |
| PLAY | 暂停 / 恢复 |

## 代码划分

- 通用驱动：`ov2640`、`esp32s3-lcd-cam`、`st7789`、`sd-card`
- 板级配置与板载按键：`esp32s3-eye-board`
- 应用、显示 UI、菜单、视觉算法：`esp32s3-eye-app`

许可证：MIT
