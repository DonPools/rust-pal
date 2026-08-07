# VOC 音效格式

`VOC.MKF` 的每个非空 chunk 是一个 Creative Voice File。文件头为 26 字节：20 字节
`Creative Voice File` 签名（以 `0x1A` 结尾）、数据偏移、版本和校验值。校验关系为：

```text
checksum = (!version + 0x1234) & 0xFFFF
```

数据由一个字节的 block 类型和三字节 little-endian 长度组成，类型 `0` 是无长度的
结束标记。当前解码器支持原版资源需要的单声道 8-bit unsigned PCM：

| 类型 | 内容 |
|---:|---|
| 1 | time constant、codec、PCM；codec 必须为 0 |
| 2 | 延续前一个 PCM block |
| 3 | 静音长度和 time constant |
| 4 | marker，忽略 |
| 5 | ASCII 文本，忽略 |
| 9 | 新格式 PCM 头；仅支持 8-bit、单声道、codec 0 |

旧格式采样率为 `1_000_000 / (256 - time_constant)`。同一 clip 中采样率改变、未知
block、压缩 codec、截断长度或缺少结束标记均解析失败，不得越界读取或 panic。

桌面层将 PCM 转为 16-bit 单声道样本。内部动态增益默认 100%；系统菜单按原版提供
“禁用 / 启用”二选框，禁用后忽略新的播放请求。真实资源检查会验证全部非空 chunk，
并确认数据覆盖多种采样率和样本长度。

## 参考

- Creative Voice File 1.20 格式
- SDLPAL `audio.c`：脚本音效编号到 `VOC.MKF` chunk 的映射
