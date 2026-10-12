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

export interface BlockCount {
  name: string;
  state: string;
  count: number;
}

export interface EntityCount {
  id: string;
  count: number;
}

/** 目录扫描结果 */
export interface ScanResult {
  items: SchematicSummary[];
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
  /** 目标不支持的属性是否保留 */
  keepUnknownProps: boolean;
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

/** 方块库中的版本选项 */
export interface VersionOption {
  index: number;
  name: string;
  dataVersion: number;
  isOverlay: boolean;
  blockCount: number;
}

/** 一条备份记录 */
export interface BackupEntry {
  path: string;
  stem: string;
  op: string;
  /** 纳秒时间戳的十进制字符串；超出 JS 安全整数范围，故不用 number */
  createdMs: string;
  size: number;
}

/** 场景批次：一个批次对应一张贴图 */
export interface SceneBatch {
  texture: number;
  positions: number[];
  uvs: number[];
  shades: number[];
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
  nonAirTotal: number;
  airTotal: number;
  distinctBlockTypes: number;
  distinctStates: number;
}
