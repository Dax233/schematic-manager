/** 把字节数格式化为易读文本 */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/**
 * 把方块总数换算成「堆叠」形式。
 * Minecraft 一组为 64 个，剩余不足一组的单独列出，便于估算材料用量。
 */
export function formatStacks(count: number): string {
  const stacks = Math.floor(count / 64);
  const rest = count % 64;
  if (stacks === 0) return `${rest}`;
  return rest === 0 ? `${stacks}组` : `${stacks}组+${rest}`;
}

/** 尺寸以 X×Y×Z 形式展示 */
export function formatDims(d: [number, number, number]): string {
  return `${d[0]} × ${d[1]} × ${d[2]}`;
}
