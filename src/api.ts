// Rust 命令的类型化封装，避免各组件直接拼命令名与参数。
import { invoke } from "@tauri-apps/api/core";
import type {
  AnalysisResult,
  BackupEntry,
  BlockHit,
  SaveOutcome,
  ScanResult,
  ConvertItem,
  ConvertOutcome,
  ConvertTarget,
  DuplicateScan,
  ReplaceOutcome,
  ReplaceRule,
  ImportOutcome,
  PackList,
  SceneData,
  SchematicSummary,
  VersionOption,
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
 * 内置方块库的路径由后端解析，前端只提供投影文件路径。
 */
export function buildScene(schematicPath: string): Promise<SceneData> {
  return invoke<SceneData>("build_scene", { schematicPath });
}

/** 跨版本检索方块 */
export function searchBlocks(query: string, limit = 200): Promise<BlockHit[]> {
  return invoke<BlockHit[]>("search_blocks", { query, limit });
}

/** 列出方块库中的全部版本 */
export function listBlockVersions(): Promise<VersionOption[]> {
  return invoke<VersionOption[]>("list_block_versions");
}

/** 预计会被替换的方块数量，不写入文件 */
export function previewReplace(schematicPath: string, rule: ReplaceRule): Promise<ReplaceOutcome> {
  return invoke<ReplaceOutcome>("preview_replace", { schematicPath, rule });
}

/** 执行替换并写回文件；后端会先备份 */
export function applyReplace(schematicPath: string, rule: ReplaceRule): Promise<ReplaceOutcome> {
  return invoke<ReplaceOutcome>("apply_replace", { schematicPath, rule });
}

/** 列出可作为转换目标的版本 */
export function convertTargets(schematicPath: string): Promise<ConvertTarget[]> {
  return invoke<ConvertTarget[]>("convert_targets", { schematicPath });
}

/** 分析投影中用到但目标版本没有的方块 */
export function analyzeConversion(
  schematicPath: string,
  targetIndex: number,
): Promise<ConvertItem[]> {
  return invoke<ConvertItem[]>("analyze_conversion", { schematicPath, targetIndex });
}

/** 执行转换，结果另存为新文件，原文件不动 */
export function applyConversion(
  schematicPath: string,
  targetIndex: number,
  items: ConvertItem[],
): Promise<ConvertOutcome> {
  return invoke<ConvertOutcome>("apply_conversion", { schematicPath, targetIndex, items });
}

/** 在一个目录下筛查内容重复的投影文件（只探测该目录，不递归） */
export function scanDuplicates(dir: string): Promise<DuplicateScan> {
  return invoke<DuplicateScan>("scan_duplicates", { dir });
}

/** 列出备份；传 stem 时只返回该文件的备份 */
export function listBackups(stem?: string): Promise<BackupEntry[]> {
  return invoke<BackupEntry[]>("list_backups", { stem: stem ?? null });
}

/** 把备份还原到目标文件。后端会校验二者同源。 */
export function restoreBackup(backupPath: string, targetPath: string): Promise<void> {
  return invoke<void>("restore_backup", { backupPath, targetPath });
}

/** 删除一条备份 */
export function deleteBackup(backupPath: string): Promise<void> {
  return invoke<void>("delete_backup", { backupPath });
}

/** 读取操作日志，由新到旧 */
export function readOperationLog(maxLines = 200): Promise<string[]> {
  return invoke<string[]>("read_operation_log", { maxLines });
}

/** dataVersion 对应的游戏版本名 */
export function mcVersionName(dataVersion: number): Promise<string> {
  return invoke<string>("mc_version_name", { dataVersion });
}

// ---------------------------------------------------------------- 资源包

/** 列出已导入的资源包 */
export function listPacks(): Promise<PackList> {
  return invoke<PackList>("list_packs");
}

/**
 * 从 jar 导入资源包。
 * kind：1 = MC 客户端版本包（数据版本取自 jar 内 version.json），
 * 2 = 模组包（叠加到 dataVersion 指定的版本）。
 * name 为空时取 jar 文件名主干。
 */
export function importJar(
  jarPath: string,
  kind: number,
  dataVersion: number,
  name?: string,
): Promise<ImportOutcome> {
  return invoke<ImportOutcome>("import_jar", {
    jarPath,
    kind,
    dataVersion,
    name: name ?? null,
  });
}

/** 启用或停用某个资源包 */
export function setPackEnabled(file: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_pack_enabled", { file, enabled });
}

/** 删除某个资源包：同时移除清单记录与磁盘文件 */
export function deletePack(file: string): Promise<void> {
  return invoke<void>("delete_pack", { file });
}
