# blocks.dat 二进制格式（分析中）

本文件记录从 `legacy-cpp/src/blockdb.cpp` 的读取实现中反推出的布局，
供 Rust 侧重写方块库时对照。**当前尚未完成，末尾列出待确认的部分。**

原始数据文件：`legacy-cpp/res/blocks.dat.z`（zlib 压缩，解压后约 5.5MB）。

## 总体结构

顺序为：文件头 → 各数据段。每个数据段之前**重复一个 u32 段内计数**，
与文件头中的对应计数一致（读取实现里用 `u32 sec = r.u32v();` 读掉）。

所有整数为小端序。

```
偏移   类型        含义
------------------------------------------------------------------
0      char[8]     魔数 "PMBLKDB\0"（校验前 7 字节）
8      u32         格式版本 fmt
12     u32         versionCount   版本数
16     u32         textureCount   贴图数
20     u32         modelCount     模型数
24     u32         langCount      语言表组数
------------------------------------------------------------------
```

### 合理性上限（读取时用来拒绝异常文件）

| 字段 | 上限 |
|---|---|
| versionCount | 100000 |
| textureCount | 2000000 |
| modelCount | 5000000 |
| langCount | 100000 |
| strCount | 20000000 |

## 字符串表

```
u32 strCount
strCount × 字符串（长度编码方式待确认）
```

字符串表被后续所有段以**下标**引用，是去重的结果。

## 语言表

```
u32 sec                 // 等于 langCount
langCount × {
    u32 n                                   // 该组条目数
    n × { u32 blockNameIdx, u32 enIdx, u32 zhIdx }   // 均为字符串表下标
}
```

下标越界时读取实现会保留空串，而不是报错。

## 贴图表

```
u32 sec                 // 等于 textureCount
textureCount × {
    u16 w, u16 h        // 贴图尺寸
    u32 offset          // 在解压后像素池中的字节偏移
    u32 len             // 该贴图的字节长度
    u8  hasAlpha        // 是否为非 0 表示含透明通道
}
u32 rawSize             // 像素池解压后总字节数
u32 compSize            // 压缩数据长度
u8[compSize]            // zlib 压缩的像素池
```

像素格式为 RGBA8888，单张贴图的像素位于 `池基址 + offset`。
`hasAlpha` 参与渲染时的透明面判定。

## 待确认的部分

以下内容尚未从原实现中读全，动手移植前需要补齐：

1. **字符串的长度编码**：u16 长度前缀，还是 u32，还是其它形式。
2. **模型段**：`Model` / `ModelElement` / `ModelFace` 的字段与顺序，
   以及 `faceMask` 的位含义（哪一位对应哪个朝向）。
3. **变体段**：`VariantEntry`（方块状态字符串 → 模型下标）的编码。
4. **方块定义段**：`BlockDef` 的字段，以及属性列表的表达方式。
5. **版本段**：`VersionDef` 的字段、`dataVersion`、语言表下标、
   `isOverlay` 标志、以及该版本的贴图路径到贴图池下标的映射表。
6. **模组叠加包**如何与同 `dataVersion` 的基础版本合并。

## 参考实现

- 读取：`legacy-cpp/src/blockdb.cpp`（`BlockDB::Load`，约第 113 行起）
- 写出：`legacy-cpp/src/packbuild.cpp`（约 32KB，是格式的权威来源）
- 数据结构：`legacy-cpp/src/blockdb.h`
