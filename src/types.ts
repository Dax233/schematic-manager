// 与 Rust 侧 src-tauri/src/dto.rs 一一对应。
// 字段名在两处必须保持一致，改动时请同步。

export interface SchematicSummary {
  path: string;
  fileName: string;
  dirName: string;
  fileSize: number;
  error: string;
  formatVersion: number;
  dataVersion: number;
  mcVersionName: string;
  name: string;
  author: string;
  description: string;
  timeCreated: number;
  timeModified: number;
  enclosing: [number, number, number];
  totalBlocks: number;
  totalVolume: number;
  regionCount: number;
  entityCount: number;
  tileEntityCount: number;
  pendingTicks: number;
  hasPreview: boolean;
}

/** 批量文件操作的结果（与后端 FileOpOutcome 对齐） */
export interface FileOpOutcome {
  /** 实际处理的文件名 */
  names: string[];
  /** 复制操作写入的目标路径；删除时为空 */
  written: string[];
  /** 目标目录（复制时） */
  destDir: string;
  /** 处理的文件总字节数 */
  totalBytes: number;
}

export interface BlockCount {
  name: string;
  state: string;
  count: number;
  /** 中文名，取自方块库语言表；查不到时为空串 */
  zhName: string;
  /** 英文名，取自方块库语言表；查不到时为空串 */
  enName: string;
}

export interface EntityCount {
  id: string;
  count: number;
  /** 中文名，取自方块库语言表；查不到时为空串 */
  zhName: string;
  /** 英文名，取自方块库语言表；查不到时为空串 */
  enName: string;
}

/** 一个实体实例：id 与投影内坐标（已含区域原点） */
export interface EntityInstance {
  id: string;
  x: number;
  y: number;
  z: number;
}

/** 实体贴图库的可用情况 */
export interface EntityTextureStatus {
  available: boolean;
  versions: number;
  textures: number;
  paths: number;
  /** 不可用时的原因；可用时为空串 */
  error: string;
}

/** 一张实体贴图的像素 */
export interface EntityTexture {
  path: string;
  width: number;
  height: number;
  hasAlpha: boolean;
  /** RGBA8888 像素，长度恒为 width * height * 4 */
  rgba: number[];
}

/** 目录扫描结果 */
export interface ScanResult {  items: SchematicSummary[];
  /** 因权限或 IO 错误被跳过的目录数 */
  skippedDirs: number;
  /** 被跳过的目录路径（最多前若干条） */
  skippedPaths: string[];
}

/** 方块替换规则 */
export interface ReplaceRule {
  fromName: string;
  fromProps: [string, string][];
  toName: string;
  toProps: [string, string][];
  /** 是否继承源方块中目标也支持的属性 */
  inheritProps: boolean;
}

/** 方块替换结果 */
export interface ReplaceOutcome {
  replaced: number;
  paletteChanged: number;
}

/** 方块所在的版本 */
export interface BlockVersion {
  index: number;
  dataVersion: number;
}

/** 方块检索结果 */
export interface BlockHit {
  name: string;
  zh: string;
  en: string;
  versions: BlockVersion[];
}

/** 重命名投影文件的结果 */
export interface RenameOutcome {
  /** 重命名后的完整路径 */
  path: string;
  /** 重命名后的文件名 */
  fileName: string;
  /** 被重新链接到新名字的备份分组数 */
  relinked: number;
  /**
   * 备份重新链接失败的原因。
   *
   * 与 relinked 分开而不是让整个命令失败：文件本身已经改名成功，
   * 若这时当成整体失败，界面显示的状态会与磁盘不符。
   */
  relinkError?: string;
}

/** 一个方块的渲染图 */
export interface BlockIcon {
  /** 方块状态键，与请求里的一致 */
  state: string;
  /** `data:image/png;base64,...`；渲染失败时为空串 */
  dataUrl: string;
  /** 渲染失败的原因；成功时为空串 */
  error: string;
}

/** 方块库中的版本选项 */
export interface VersionOption {
  index: number;
  name: string;
  dataVersion: number;
  isOverlay: boolean;
  blockCount: number;
}

/** 转换中需要处理的一项 */
export interface ConvertItem {
  name: string;
  count: number;
  suggested: string;
  chosen: string;
}

/** 可作为转换目标的版本 */
export interface ConvertTarget {
  index: number;
  name: string;
  dataVersion: number;
}

/** 转换结果 */
export interface ConvertOutcome {
  replacedKinds: number;
  replacedBlocks: number;
  skippedKinds: number;
  outputPath: string;
}

/** 一组内容相同的投影文件 */
export interface DuplicateGroup {
  size: number;
  paths: string[];
}

/** 未能读取的文件 */
export interface UnreadableFile {
  path: string;
  reason: string;
}

/** 重复文件筛查结果 */
export interface DuplicateScan {
  groups: DuplicateGroup[];
  /** 读取失败、未能参与判定的文件 */
  unreadable: UnreadableFile[];
}

/** 一条备份记录 */
export interface BackupEntry {
  path: string;
  stem: string;
  op: string;
  /** 纳秒时间戳的十进制字符串；超出 JS 安全整数范围，故不用 number */
  createdNs: string;
  size: number;
}

/** 场景批次：一个批次对应一张贴图 */
export interface SceneBatch {
  texture: number;
  positions: number[];
  uvs: number[];
  /** 每顶点颜色系数，RGB 三通道，已含明暗与染色 */
  colors: number[];
  /** 贴图透明类型。决定前端如何绘制该批次 */
  alphaMode: "opaque" | "cutout" | "translucent";
  indices: number[];
  textureWidth: number;
  textureHeight: number;
  /** RGBA8888 像素 */
  textureRgba: number[];
}

/** 前端渲染所需的完整场景 */
export interface SceneData {
  batches: SceneBatch[];
  totalVertices: number;
  totalTriangles: number;
  enclosing: [number, number, number];
  origin: [number, number, number];
  mcVersionName: string;
  dataVersion: number;
  /** 该版本方块库中查不到定义的方块种类数 */
  unresolvedBlocks: number;
}

/** 保存操作的结果 */
export interface SaveOutcome {
  path: string;
  /** 保存前生成的备份文件位置 */
  backupPath: string;
}

export interface AnalysisResult {
  blocks: BlockCount[];
  entities: EntityCount[];
  tileEntities: EntityCount[];
  /**
   * 逐个实体的 id 与投影内坐标。
   *
   * 名字只在 `entities` 里放一份：同一实体可能有上千个实例，
   * 逐条重复中英文名会让结果体积成倍增长。
   */
  entityInstances: EntityInstance[];
  nonAirTotal: number;
  airTotal: number;
  distinctBlockTypes: number;
  distinctStates: number;
}

/** 一个已导入的资源包 */
export interface PackEntry {
  /** 资源包文件名（在 packs 目录内） */
  file: string;
  name: string;
  /** 来源 jar 路径 */
  source: string;
  /**
   * 来源所在目录。从 mods 文件夹批量导入时同一个目录会带出多个包，
   * 界面据此把它们归成一组。
   */
  sourceDir: string;
  /** 类型：0 内置、1 MC 版本包、2 模组包 */
  kind: number;
  /** 类型的中文名 */
  kindLabel: string;
  dataVersion: number;
  mcVersion: string;
  enabled: boolean;
  size: number;
  /** 文件是否实际存在。清单可能与磁盘不一致（用户手工删过文件）。 */
  exists: boolean;
}

/** 资源包列表，含清单里被跳过的行 */
/** 一个已启用却未能载入的资源包 */
export interface PackLoadFailure {
  file: string;
  reason: string;
}

export interface PackList {
  packs: PackEntry[];
  /** 资源包存放目录，供界面直接打开去管理磁盘上的文件 */
  packsDir: string;
  /** 已启用但载入或合并失败的包 */
  loadFailures: PackLoadFailure[];
  /** 清单里解析失败而被跳过的行说明 */
  skipped: string[];
}

/** 导入 jar 的结果 */
export interface ImportOutcome {
  file: string;
  name: string;
  kind: number;
  kindLabel: string;
  dataVersion: number;
  mcVersion: string;
  blocks: number;
  models: number;
  textures: number;
  /** 解析失败而被跳过的 blockstate 数 */
  failedBlockstates: number;
  /** 因无法解析模型而被跳过的方块数 */
  failedBlocks: number;
  namespaces: number;
  size: number;
}

/** 从 mods 文件夹批量导入的结果 */
export interface ModsImport {
  /** 被扫描的目录 */
  dir: string;
  /** 成功导入的模组 jar 文件名 */
  imported: string[];
  /**
   * 失败项及原因。必须回报：某一个模组坏掉不该让其余都导不进来，
   * 但用户也需要知道少了哪些、为什么。
   */
  failed: { path: string; reason: string }[];
  /**
   * 有意跳过的条目及原因（非 jar、按客户端包跳过）。
   *
   * 与 failed 分开：它们不是错误，但也不能无声无息地少导入——
   * 名字里恰好含 client 的模组会被判成客户端包。
   */
  skipped: { path: string; reason: string }[];
}
