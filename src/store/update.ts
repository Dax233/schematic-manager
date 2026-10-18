/**
 * 更新域：「更新」窗口的版本信息与检测动作。
 *
 * 窗口打开时先取当前版本（这是本地信息，一定能拿到），检测更新要用户点按钮
 * 才发起——启动时自动联网会让离线环境凭空多一次失败，也会平白拖慢启动。
 *
 * 检测结果分三种，界面上必须能分辨：
 *   1. 有更新（hasUpdate 为真）
 *   2. 已是最新（查到了发布，但没有比当前更高的）
 *   3. 没查到发布（latest 为空）——这不是「最新」，是仓库里没有可解析的发布
 * 检测失败是第四种，走 error 单独展示，不落进上面任何一种。
 *
 * 与其他域无协作：本域不读写别处的状态，也不需要别的域提供数据。
 */
import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { t } from "../i18n";
import { appVersion, checkUpdate, openUrl } from "../api";
import { copyText } from "../utils/clipboard";
import type { UpdateInfo } from "../types";

/** 作者主页：与作者名一样属于固定信息，放在这里供窗口与动作共用 */
export const AUTHOR_PAGE = "https://space.bilibili.com/19428259";

/** 发布页：检测不到具体发布时的兜底地址，永远可以打开 */
export const RELEASES_PAGE = "https://github.com/heiheids/schematic-manager/releases";

export function createUpdateSlice() {
  const updateOpen = ref(false);
  /** 当前可执行文件版本；取不到时为空串 */
  const currentVersion = ref("");
  /** 上一次检测的结果；从未检测过为 null */
  const updateInfo = ref<UpdateInfo | null>(null);
  const updateChecking = ref(false);
  /** 检测失败的原因；为空表示没出错 */
  const updateError = ref("");

  /** 界面上展示与打开的下载地址。
   *
   * 查到了具体发布就用它，否则给发布列表页——两处都要用这个值，
   * 各写一次 `|| RELEASES_PAGE` 迟早会有一处漏掉。
   */
  const releaseUrl = computed(() => updateInfo.value?.releaseUrl || RELEASES_PAGE);

  /** 打开窗口：读一次版本号，并清掉上一次的检测结果。
   *
   * 结果不复用是有意的：用户关掉窗口又打开，多半就是想重新检测一次，
   * 留着几分钟前的结论反而会让人以为是刚刚查的。
   */
  async function openUpdate() {
    updateOpen.value = true;
    updateInfo.value = null;
    updateError.value = "";
    await loadVersion();
  }

  /** 读取当前版本。失败不弹消息：这只是窗口里的一行文字，
   * 失败时显示「未知」即可，不值得用一条气泡打断用户。 */
  async function loadVersion() {
    try {
      currentVersion.value = await appVersion();
    } catch {
      currentVersion.value = "";
    }
  }

  /** 检测更新。失败原因留在窗口里，不弹瞬时消息——
   * 用户需要照着原因判断是网络问题还是仓库问题。 */
  async function runUpdateCheck() {
    updateChecking.value = true;
    updateError.value = "";
    try {
      const info = await checkUpdate();
      updateInfo.value = info;
      // 界面以 result.current 为准：它是检测时后端读到的版本，
      // 与这里单独取的那次可能因升级而不同
      currentVersion.value = info.current;
    } catch (e) {
      updateError.value = String(e);
      updateInfo.value = null;
    } finally {
      updateChecking.value = false;
    }
  }

  /** 复制下载地址。
   *
   * 成功与失败都要明说：剪贴板可能被权限或环境拒绝，静默失败会让用户
   * 以为地址已经复制好了，去粘贴时才发现是空的。
   */
  async function copyReleaseUrl() {
    try {
      await copyText(releaseUrl.value);
      Message.success(t("update.copiedUrl"));
    } catch (e) {
      Message.error(t("update.copyFailed", { error: String(e) }));
    }
  }

  /** 用系统浏览器打开一个外部链接。
   *
   * 失败必须报出来：点了没反应会让人以为程序坏了，而原因（后端不肯放行、
   * 系统没有 xdg-open 之类）恰恰是用户能处理的。
   */
  async function openExternal(url: string) {
    try {
      await openUrl(url);
    } catch (e) {
      Message.error(t("update.openFailed", { error: String(e) }));
    }
  }

  /** 打开下载地址 */
  async function openReleaseUrl() {
    await openExternal(releaseUrl.value);
  }

  /** 打开作者主页 */
  async function openAuthorPage() {
    await openExternal(AUTHOR_PAGE);
  }

  return {
    updateOpen,
    currentVersion,
    updateInfo,
    updateChecking,
    updateError,
    releaseUrl,
    authorPage: AUTHOR_PAGE,
    openUpdate,
    runUpdateCheck,
    copyReleaseUrl,
    openReleaseUrl,
    openAuthorPage,
  };
}

export type UpdateSlice = ReturnType<typeof createUpdateSlice>;
