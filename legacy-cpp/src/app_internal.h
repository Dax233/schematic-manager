#pragma once
#include "app.h"
#include "i18n.h"
#include "str.h"
#include "fs.h"
#include <windowsx.h>

// 应用内部共享定义
namespace app {

enum Cmd {
    CMD_NONE = 0,
    // 文件
    CMD_ADD_DIR = 100, CMD_RESCAN, CMD_EXPORT, CMD_EXIT, CMD_MANAGE_DIRS, CMD_DIR_DROP,
    // 编辑
    CMD_EDIT_META = 200, CMD_EDIT_VERSION, CMD_EDIT_REPLACE, CMD_EDIT_PREVIEW,
    CMD_EDIT_REVERT, CMD_EDIT_RENAME,
    // 工具
    CMD_DUP = 300, CMD_BACKUP, CMD_LOG, CMD_OPEN_DIR,
    // 搜索
    CMD_SEARCH_FOCUS = 350, CMD_SEARCH_NEXT, CMD_SEARCH_PREV, CMD_SEARCH_CLEAR,
    // 帮助
    CMD_ABOUT = 400, CMD_LIBINFO, CMD_HELP_BILIBILI,
    // 窗口
    CMD_MIN = 500, CMD_MAX, CMD_CLOSE,
    CMD_MODAL_OK = 600, CMD_MODAL_CANCEL, CMD_MODAL_APPLY,
    // 视角 / 标签页
    CMD_VIEW_FRONT = 700, CMD_VIEW_LEFT, CMD_VIEW_RIGHT, CMD_VIEW_BACK,
    CMD_TAB_INFO = 720, CMD_TAB_BLOCKS, CMD_TAB_ENTITIES,
    CMD_CHECK_ALL = 740, CMD_CHECK_NONE, CMD_CHECK_INVERT,
    CMD_DUP_RUN = 760, CMD_DUP_KEEP, CMD_DUP_DELETE, CMD_DUP_CLOSE, CMD_DUP_MODE0, CMD_DUP_MODE1,
    CMD_BK_RESTORE = 780, CMD_BK_DELETE, CMD_BK_REFRESH,
    CMD_REP_DO = 800, CMD_REP_PICK_FROM, CMD_REP_PICK_TO,
    // 资源包
    CMD_PACKS_IMPORT_MC = 820, CMD_PACKS_IMPORT_MOD, CMD_PACKS_DETECT,
    CMD_PACKS_ADDROOT, CMD_PACKS_AUTOIMPORT, CMD_PACKS_TOGGLE, CMD_PACKS_DELETE,
    CMD_PACKS_IMPORT_ALL, CMD_PACKS_RELOAD, CMD_PACKS_OPEN = 840,
    // 方块页签就地替换
    CMD_BLK_PICK = 900, CMD_BLK_DO, CMD_BLK_CLEAR,
    // 信息页内联编辑
    CMD_INFO_EDIT = 950,
    // 列表操作
    CMD_SAVE = 960, CMD_REVERT, CMD_DELETE_SEL, CMD_COPY_SEL, CMD_MOVE_SEL,
    CMD_OPEN_SEL_DIR, CMD_MORE, CMD_NOTE_EDIT, CMD_FMT_PICK, CMD_VER_PICK,
    CMD_CONVERT = 980, CMD_EXPORT_SIZE,

    CMD_MENU_BAR  = 1000,      // 菜单栏项 id 基址
    CMD_MENU_ITEM = 1200       // 下拉项 id 基址（+ 命令号）
};

const int TITLE_H   = 32;
const int MENU_H    = 0;      // 菜单行已移除
const int TOOLBAR_H = 72;   // 两行
const int STATUS_H  = 24;
const int ROW_H     = 24;
const int HEADER_H  = 26;

// 内联编辑框槽位
enum EditSlot {
    EDIT_META_NAME = 0, EDIT_META_AUTHOR, EDIT_META_DESC,
    EDIT_REP_SEARCH, EDIT_SEARCHBOX,
    EDIT_INFO_FILE, EDIT_INFO_NAME, EDIT_INFO_AUTHOR, EDIT_INFO_DESC,
    EDIT_NOTE, EDIT_CVSEARCH, EDIT_INFO_NOTE,
    EDIT_SLOT_COUNT
};
extern ui::Rect g_editPos[EDIT_SLOT_COUNT];
extern bool     g_editWant[EDIT_SLOT_COUNT];
extern int      g_editFocusReq;

std::wstring FmtW(const wchar_t* fmt, ...);
std::wstring FmtTime(i64 ms);
bool IsScanning();

struct MenuEntry { const char* label; int cmd; const char* accel; };

void DrawModalIn(ui::Ctx& c, const ui::Rect& r);
bool ModalHosted();
HWND ModalHwnd();
bool OpenModalHost(int kind);
void CloseModalHost();
void EnsureEditsFor(HWND parent);
void DrawPackModal(ui::Ctx& c, const ui::Rect& r);
void HandleWindowCommand(int cmd);
void ExecuteCommand(int cmd);
std::wstring VersionLabel(const ltm::Summary& s);

// 布局
struct Layout {
    ui::Rect title, menu, toolbar, status, content, left, splitter, right;
    int leftW = 560;
};
Layout ComputeLayout();
void HandleLayoutDrag(ui::Ctx& c, const Layout& L);

// 各区域绘制
void DrawTitleBar(ui::Ctx& c, const Layout& L);
void DrawMenuBar(ui::Ctx& c, const Layout& L);
void DrawToolbar(ui::Ctx& c, const Layout& L);
void DrawList(ui::Ctx& c, const Layout& L);
void DrawDetail(ui::Ctx& c, const Layout& L);
void DrawStatusBar(ui::Ctx& c, const Layout& L);
void DrawDirDropdown(ui::Ctx& c, const Layout& L);
void DrawBlockPickPopup(ui::Ctx& c, const Layout& L);
void CloseBlockPickIfOutside(ui::Ctx& c);
void OpenBlockPickModal();
void DrawBlockPickModal(ui::Ctx& c, const ui::Rect& r);
void ResetEditInit();
void RunBrightTest(const std::wstring& path);
void OpenConvertModal();
void DrawConvertModal(ui::Ctx& c, const ui::Rect& r);
void AnalyzeConversion(int targetVerIdx);
void DrawMoreMenu(ui::Ctx& c, const Layout& L);
void DrawFormatPopup(ui::Ctx& c, const Layout& L);
void DrawExportMenu(ui::Ctx& c, const Layout& L);

// 弹窗辅助
ui::Rect ModalRect(int w, int h);
void ModalFrame(ui::Ctx& c, const ui::Rect& r, const wchar_t* title);
void CommitInfoEdit(int slot);
int  SplitterHot();
void EnsureEdits(HWND parent = nullptr);
void DestroyEdits();

extern int g_openMenu;

} // namespace app
