// Rust 命令的类型化封装，避免各组件直接拼命令名与参数。
import { invoke } from "@tauri-apps/api/core";
import type { AnalysisResult, SaveOutcome, SchematicSummary } from "./types";

/** 只读扫描单个投影文件（不解码方块） */
export function peekSchematic(path: string): Promise<SchematicSummary> {
  return invoke<SchematicSummary>("peek_schematic", { path });
}

/** 递归扫描目录下全部 .litematic */
export function scanLitematics(dir: string, maxDepth = 6): Promise<SchematicSummary[]> {
  return invoke<SchematicSummary[]>("scan_litematics", { dir, maxDepth });
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

/** dataVersion 对应的游戏版本名 */
export function mcVersionName(dataVersion: number): Promise<string> {
  return invoke<string>("mc_version_name", { dataVersion });
}
