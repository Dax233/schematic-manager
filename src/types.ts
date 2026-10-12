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

export interface AnalysisResult {
  blocks: BlockCount[];
  entities: EntityCount[];
  tileEntities: EntityCount[];
  nonAirTotal: number;
  airTotal: number;
  distinctBlockTypes: number;
  distinctStates: number;
}
