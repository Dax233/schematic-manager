// Rust 命令的类型化封装，避免各组件直接拼命令名与参数。
import { invoke } from "@tauri-apps/api/core";
import type {
  AnalysisResult,
  SaveOutcome,
  ScanResult,
  SceneData,
  SchematicSummary,
} from "./types";

/** 只读扫描单个投影文件（不解码方块） */
export function peekSchematic(path: string): Promise<SchematicSummary> {
  return invoke<SchematicSummary>("peek_schematic", { path });
}

/** 递归扫描目录下全部 .litematic。返回值含被跳过的目录数，便于界面提示。 */
export function scanLitematics(dir: string, maxDepth = 6): Promise<ScanResult> {
  return invoke<ScanResult>("scan_litematics", { dir, maxDepth });
}

/** 完整解码并统计方块与实体 */
export function analyzeSchematic(path: string): Promise<AnalysisResult> {
  return invoke<AnalysisResult>("analyze_schematic", { path });
}

/**
 * 保存元数据。未提供的字段保持不变。
 * 保存前会由后端自动备份原件，备份失败则不写入。
 */
export function saveMetadata(
  path: string,
  fields: { name?: string; author?: string; description?: string },
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("save_metadata", { path, ...fields });
}

/**
 * 组装前端渲染所需的场景。
 * dbPath 为方块库文件（blocks.dat 的 zlib 压缩形式）的路径。
 */
export function buildScene(schematicPath: string, dbPath: string): Promise<SceneData> {
  return invoke<SceneData>("build_scene", { schematicPath, dbPath });
}

/** dataVersion 对应的游戏版本名 */
export function mcVersionName(dataVersion: number): Promise<string> {
  return invoke<string>("mc_version_name", { dataVersion });
}
