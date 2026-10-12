#include "i18n.h"
#include "str.h"

namespace i18n {

static Lang g_lang = LANG_ZH;

struct Entry { const char* key; const wchar_t* zh; const wchar_t* en; };

static const Entry kTable[] = {
    // 窗口 / 通用
    { "app.title",           L"投影文件管理器", L"Schematic Manager" },
    { "app.author",          L"作者：bilibili黑山大叔", L"Author: bilibili HeiShanDaShu" },
    { "btn.ok",              L"确定", L"OK" },
    { "btn.cancel",          L"取消", L"Cancel" },
    { "btn.apply",           L"应用", L"Apply" },
    { "btn.close",           L"关闭", L"Close" },
    { "btn.browse",          L"浏览…", L"Browse..." },
    { "btn.refresh",         L"刷新", L"Refresh" },
    { "btn.save",            L"保存", L"Save" },
    { "btn.saveas",          L"另存为…", L"Save As..." },
    { "btn.restore",         L"还原", L"Restore" },
    { "btn.delete",          L"删除", L"Delete" },
    { "btn.replace",         L"替换", L"Replace" },
    { "btn.backupmgr",       L"备份管理", L"Backups" },
    { "btn.scan",            L"扫描", L"Scan" },
    { "btn.selectall",       L"全选", L"Select All" },
    { "btn.selectnone",      L"全不选", L"Select None" },
    { "btn.export",          L"导出预览图", L"Export Preview" },
    { "btn.rendering",       L"渲染中…", L"Rendering..." },

    // 菜单
    { "menu.file",           L"文件", L"File" },
    { "menu.file.adddir",    L"添加扫描目录…", L"Add Folder..." },
    { "menu.file.managedir", L"管理扫描目录…", L"Manage Folders..." },
    { "menu.file.rename",    L"重命名文件…", L"Rename File..." },
    { "menu.file.rescan",    L"重新扫描", L"Rescan" },
    { "menu.file.export",    L"导出预览图…", L"Export Preview..." },
    { "menu.file.exit",      L"退出", L"Exit" },
    { "menu.edit",           L"编辑", L"Edit" },
    { "menu.edit.meta",      L"编辑元数据…", L"Edit Metadata..." },
    { "menu.edit.version",   L"修改版本标记…", L"Change Version Tag..." },
    { "menu.edit.replace",   L"批量替换方块…", L"Replace Blocks..." },
    { "menu.edit.preview",   L"写入预览图", L"Embed Preview Image" },
    { "menu.edit.revert",    L"放弃未保存修改", L"Discard Unsaved Changes" },
    { "menu.tools",          L"工具", L"Tools" },
    { "menu.tools.packs",    L"贴图与资源包管理…", L"Textures && Packs..." },
    { "menu.tools.dup",      L"重复文件筛查…", L"Find Duplicates..." },
    { "menu.tools.backup",   L"备份管理…", L"Backup Manager..." },
    { "menu.tools.log",      L"操作日志…", L"Operation Log..." },
    { "menu.tools.opendir",  L"打开所在目录", L"Open Containing Folder" },
    { "menu.search",         L"搜索", L"Search" },
    { "menu.search.find",    L"查找投影…", L"Find Schematic..." },
    { "menu.search.next",    L"下一个匹配", L"Next Match" },
    { "menu.search.prev",    L"上一个匹配", L"Previous Match" },
    { "menu.search.clear",   L"清除搜索", L"Clear Search" },
    { "menu.help",           L"帮助", L"Help" },
    { "menu.help.bili",      L"访问我的 B 站主页", L"Visit Bilibili Page" },
    { "menu.help.about",     L"关于", L"About" },
    { "menu.help.blocks",    L"方块库信息", L"Block Library Info" },

    // 列表列
    { "col.name",            L"名称", L"Name" },
    { "col.version",         L"制作版本", L"MC Version" },
    { "col.size",            L"尺寸", L"Size" },
    { "col.blocks",          L"方块数", L"Blocks" },
    { "col.dir",             L"来源目录", L"Folder" },

    // 详情
    { "detail.title",        L"详细信息", L"Details" },
    { "detail.file",         L"文件名", L"File" },
    { "detail.name",         L"投影名称", L"Name" },
    { "detail.author",       L"作者", L"Author" },
    { "detail.desc",         L"描述", L"Description" },
    { "detail.mcver",        L"制作版本", L"MC Version" },
    { "detail.dataversion",  L"数据版本", L"Data Version" },
    { "detail.format",       L"投影格式", L"Format" },
    { "detail.dims",         L"尺寸 (X×Y×Z)", L"Size (X x Y x Z)" },
    { "detail.volume",       L"包围盒体积", L"Bounding Volume" },
    { "detail.blocks",       L"方块总数", L"Total Blocks" },
    { "detail.types",        L"方块种类", L"Block Types" },
    { "detail.entities",     L"实体数量", L"Entities" },
    { "detail.tiles",        L"方块实体", L"Block Entities" },
    { "detail.regions",      L"区域数量", L"Regions" },
    { "detail.created",      L"创建时间", L"Created" },
    { "detail.modified",     L"修改时间", L"Modified" },
    { "detail.preview",      L"内置预览图", L"Embedded Preview" },
    { "detail.filesize",     L"文件大小", L"File Size" },
    { "detail.yes",          L"有", L"Yes" },
    { "detail.no",           L"无", L"No" },

    // 标签页
    { "tab.info",            L"信息", L"Info" },
    { "tab.blocks",          L"方块", L"Blocks" },
    { "tab.entities",        L"实体", L"Entities" },

    // 预览
    { "view.front",          L"正面", L"Front" },
    { "view.left45",         L"左侧 45°", L"Left 45°" },
    { "view.right45",        L"右侧 45°", L"Right 45°" },
    { "view.back",           L"背面", L"Back" },
    { "preview.none",        L"选择一个投影文件以查看预览", L"Select a schematic to preview" },
    { "preview.failed",      L"渲染失败", L"Render failed" },

    // 状态栏
    { "status.ready",        L"就绪", L"Ready" },
    { "status.scanning",     L"正在扫描…", L"Scanning..." },
    { "status.found",        L"找到 %d 个投影文件", L"%d schematic file(s) found" },
    { "status.selected",     L"已选 %d 个", L"%d selected" },
    { "status.rendered",     L"渲染完成（%.0f 毫秒）", L"Rendered in %.0f ms" },
    { "status.modified",     L"有未保存的修改", L"Unsaved changes" },
    { "status.saved",        L"已保存", L"Saved" },
    { "status.backedup",     L"已备份到 %s", L"Backed up to %s" },

    // 元数据编辑
    { "meta.title",          L"编辑投影信息", L"Edit Schematic Info" },
    { "meta.name",           L"投影名称", L"Name" },
    { "meta.author",         L"作者", L"Author" },
    { "meta.desc",           L"描述", L"Description" },
    { "meta.hint",           L"修改前会自动备份原文件。", L"The original file is backed up automatically." },

    // 版本修改
    { "ver.title",           L"修改版本标记", L"Change Version Tag" },
    { "ver.current",         L"当前", L"Current" },
    { "ver.target",          L"目标版本", L"Target version" },
    { "ver.warn",            L"注意：调高版本号会让 Litematica 跳过官方数据修复器，若期间方块被重命名可能显示错误方块；调低版本号会触发真正的转换，可能破坏数据，请务必先备份。",
                              L"Warning: raising the version makes Litematica skip the vanilla DataFixer; if blocks were renamed in between, wrong blocks may appear. Lowering it triggers real conversion and may corrupt data. Always back up first." },
    { "ver.setself",         L"设为该投影自身版本", L"Keep schematic's own version" },
    { "ver.setcur",          L"设为当前游戏版本", L"Set to current game version" },

    // 方块替换
    { "rep.title",           L"批量替换方块", L"Replace Blocks" },
    { "rep.from",            L"源方块", L"From block" },
    { "rep.to",              L"目标方块", L"To block" },
    { "rep.search",          L"搜索方块…", L"Search blocks..." },
    { "rep.inherit",         L"尽量继承方块属性（推荐）", L"Inherit block properties (recommended)" },
    { "rep.scope",           L"作用范围", L"Apply to" },
    { "rep.scope.sel",       L"选中的文件", L"Selected files" },
    { "rep.scope.cur",       L"当前文件", L"Current file" },
    { "rep.source.vanilla",  L"原版", L"Vanilla" },
    { "rep.source.mod",      L"模组", L"Mod" },
    { "rep.missing",         L"（无贴图）", L"(no texture)" },
    { "rep.done",            L"替换完成：%llu 个方块", L"Replaced %llu blocks" },
    { "rep.confirm",         L"即将修改 %d 个文件，每个文件修改前都会自动备份。是否继续？",
                              L"%d file(s) will be modified. Each is backed up first. Continue?" },

    // 备份
    { "bk.title",            L"备份管理", L"Backup Manager" },
    { "bk.file",             L"原文件", L"Original" },
    { "bk.op",               L"操作", L"Operation" },
    { "bk.time",             L"时间", L"Time" },
    { "bk.size",             L"大小", L"Size" },
    { "bk.empty",            L"暂无备份", L"No backups" },
    { "bk.confirmdel",       L"确定删除选中的备份？", L"Delete the selected backup(s)?" },
    { "bk.confirmrestore",   L"确定用该备份覆盖原文件？当前内容会先被备份。",
                              L"Restore this backup over the original? The current content will be backed up first." },

    // 重复筛查
    { "dup.title",           L"重复文件筛查", L"Find Duplicates" },
    { "dup.mode",            L"判定方式", L"Match by" },
    { "dup.mode.hash",       L"内容完全相同", L"Identical content" },
    { "dup.mode.meta",       L"名称+尺寸+方块数相同", L"Same name, size and block count" },
    { "dup.groups",          L"发现 %d 组重复", L"%d duplicate group(s)" },
    { "dup.none",            L"没有发现重复文件", L"No duplicates found" },
    { "dup.keepfirst",       L"每组保留第一个", L"Keep first in each group" },
    { "dup.deletemarked",    L"删除勾选项", L"Delete checked" },
    { "dup.confirmdel",      L"确定删除 %d 个文件？此操作会先备份。",
                              L"Delete %d file(s)? They will be backed up first." },
    { "dup.scanning",        L"正在计算…", L"Computing..." },

    // 日志
    { "log.title",           L"操作日志", L"Operation Log" },
    { "log.op.replace",      L"替换方块", L"Replace blocks" },
    { "log.op.meta",         L"修改元数据", L"Edit metadata" },
    { "log.op.version",      L"修改版本标记", L"Change version tag" },
    { "log.op.preview",      L"写入预览图", L"Embed preview" },
    { "log.op.restore",      L"还原备份", L"Restore backup" },
    { "log.op.delete",       L"删除文件", L"Delete file" },

    // 关于
    { "about.title",         L"关于", L"About" },
    { "about.lib",           L"内置方块库", L"Embedded block library" },
    { "about.libdesc",       L"共 %d 个游戏版本，%d 个方块定义，%d 张贴图。完全离线，无需安装 Minecraft。",
                              L"%d game version(s), %d block definitions, %d textures. Fully offline, no Minecraft required." },

    // 错误
    { "err.notlitematic",    L"不是有效的投影文件", L"Not a valid schematic file" },
    { "err.noaassets",       L"内置方块库缺失或损坏，预览与方块名将不可用。",
                              L"Embedded block library missing or corrupt; previews and block names unavailable." },
};

void SetLang(Lang l) { g_lang = l; }
Lang GetLang() { return g_lang; }

bool IsChineseSystem() {
    LANGID id = GetUserDefaultUILanguage();
    switch (PRIMARYLANGID(id)) {
        case LANG_CHINESE:
        case LANG_JAPANESE:
        case LANG_KOREAN:
            return PRIMARYLANGID(id) == LANG_CHINESE;
        default:
            return false;
    }
}

void DetectFromSystem() {
    g_lang = IsChineseSystem() ? LANG_ZH : LANG_EN;
}

const wchar_t* T(const char* key) {
    for (const Entry& e : kTable) {
        if (strcmp(e.key, key) == 0) return (g_lang == LANG_ZH) ? e.zh : e.en;
    }
    // 回退：转成宽字符的 key
    static thread_local std::wstring buf;
    buf = Utf8ToWide(key);
    return buf.c_str();
}

} // namespace i18n
