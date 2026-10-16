/**
 * 复制文本到剪贴板。
 *
 * 优先用 navigator.clipboard：它能给出失败原因（权限被拒、不是安全上下文等）。
 * 但它在 WebView 里不保证可用，因此失败后回退到 execCommand 那条老路
 * （同步、返回布尔值）。两条都失败就抛出，由调用方如实提示——绝不能悄悄
 * 当成复制成功，那样用户会以为地址已经在剪贴板里了。
 */
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // 继续走兜底：权限被拒时 execCommand 仍可能成功
    }
  }
  if (fallbackCopy(text)) return;
  throw new Error("浏览器拒绝了剪贴板写入");
}

/** 兜底方案：临时文本框 + execCommand。 */
function fallbackCopy(text: string): boolean {
  const area = document.createElement("textarea");
  area.value = text;
  // 移到视口外：直接插进文档流会让页面滚动一下，
  // 用 fixed 定位则不影响滚动位置
  area.style.position = "fixed";
  area.style.top = "-1000px";
  area.setAttribute("readonly", "");
  document.body.appendChild(area);
  try {
    area.select();
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    document.body.removeChild(area);
  }
}
