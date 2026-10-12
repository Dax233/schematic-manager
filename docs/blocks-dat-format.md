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

## 从写出实现确认的完整布局

以下取自 `legacy-cpp/src/packbuild.cpp` 的写出序列（约 431~524 行），
它是格式的权威来源。**每个 pack 文件只含一个版本**，
程序运行时会加载多个 pack（基础版 + 模组叠加包）再合并。

### 文件头

```
u32 formatVersion      // 当前为 3
u32 versionCount       // 该 pack 内的版本数，实际为 1
u32 textureCount
u32 modelCount
u32 langTableCount     // 实际为 1
```

### 字符串表

字符串以 **NUL 结尾（C 风格）**，不是长度前缀：

```
u32 strCount
strCount × <字节序列> 0x00
```

### 语言表

```
u32 langTableCount            // 段内重复计数，= 1
langTableCount × {
    u32 n
    n × { u32 blockNameIdx, u32 enIdx, u32 zhIdx }
}
```

### 贴图表

```
u32 textureCount
textureCount × { u16 w, u16 h, u32 offset, u32 len, u8 hasAlpha }
u32 pixelsRawSize
u32 pixelsCompSize
u8[pixelsCompSize]            // zlib 压缩的 RGBA8888 像素池
```

### 模型表

```
u32 modelCount
modelCount × {
    u8  translucent
    u16 elementCount
    elementCount × {
        <元素边界，6 个浮点>            // 待确认具体类型与顺序
        u8  hasRot
        u8  rotKind
        u8  axis
        u8  rescale
        <旋转相关的角度/原点，待确认>
        u8  faceMask                    // 自低位起，置位顺序对应面
        popcount(faceMask) × {
            u32 texIdx                  // 贴图池下标，0xFFFFFFFF 表示缺失
            u8  cullface                // 0xFF 表示无
            u16 rotation                // 归一化到 0~359
            <该面的 UV，4 个值，待确认>
        }
    }
}
```

### 版本段

```
u32 sectionCount                // 段内重复计数，= 1
sectionCount × {
    u32 versionId               // 版本标识（dataVersion 或字符串表下标）
    u32 langTableIdx
    u32 texPathCount
    texPathCount × { u32 texPathStrIdx, u32 texturePoolIdx }   // 路径 → 池下标
    u32 blockCount
    blockCount × {
        u32 blockNameStrIdx
        u16 variantCount
        variantCount × {
            u32 whenStrIdx       // 方块状态条件字符串
            u32 modelIdx
            u8  rotXDiv90
            u8  rotYDiv90
            u8  uvlock
        }
        u16 multipartCount
        multipartCount × <与 variant 相同的五个字段>
    }
}
```

## 字段级完整布局（已全部确认）

### 模型元素

```
u8  translucment            // 模型整体是否为半透明
u16 elementCount
elementCount × {
    f32[3]  from                    // 元素起点
    f32[3]  to                      // 元素终点
    u8      hasRot
    if hasRot {
        u8      rotKind
        f32[3]  origin
        u8      axis
        f32     angle
        u8      rescale
        f32[3]  rx, ry, rz
    }
    u8      faceMask                // 自低位起，位 0~5 依次对应六个面
    popcount(faceMask) × {
        u32   texIdx                // 贴图池下标，0xFFFFFFFF 表示缺失
        f32[4] uv
        u8    cullface              // 0xFF 表示无
        i16   tintindex             // 有符号
        u16   rotation              // 归一化到 0~359
    }
}
```

面的遍历顺序为「按位从低到高」，即先写最低置位的面。

### 版本段

```
u32 sectionCount            // 段内重复计数，= 1
sectionCount × {
    u32  versionNameStrIdx  // 版本名字符串表下标
    i32  dataVersion        // 有符号
    u32  langTableIdx
    u32  texPathCount
    texPathCount × { u32 texPathStrIdx, u32 texturePoolIdx }
    u32  blockCount
    blockCount × {
        u32 blockNameStrIdx
        u16 variantCount
        variantCount × { u32 whenStrIdx, u32 modelIdx, u8 rotXDiv90, u8 rotYDiv90, u8 uvlock }
        u16 multipartCount
        multipartCount × { u32 whenStrIdx, u32 modelIdx, u8 rotXDiv90, u8 rotYDiv90, u8 uvlock }
    }
}
```

## 实现时需要注意的点

- **所有整数小端序**，浮点为 IEEE754 单精度小端。
- 字符串为 NUL 结尾；解析时应同时做长度保护，避免损坏文件导致无界扫描。
- `dataVersion` 是 **i32 有符号**。叠加包是否以负值标记，需在实现时
  对照读取实现中的 `isOverlay` 判定逻辑确认，不可想当然。
- `texIdx` 的 `0xFFFFFFFF` 是「缺失贴图」的哨兵值，不是有效下标。
- 各段前的重复计数与文件头计数应当一致；不一致说明文件损坏，
  应显式报错而不是以文件头为准继续解析。
- 读取实现中另设了数量上限（版本 10 万、贴图 200 万、模型 500 万、
  语言表 10 万、字符串 2000 万），用于拒绝异常文件。Rust 侧应保留同类保护。
