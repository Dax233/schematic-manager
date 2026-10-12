#pragma once
#include "common.h"
#include "ui.h"
#include "litematic.h"
#include "blockdb.h"
#include "render.h"
#include "backup.h"
#include "packmgr.h"

// 应用状态与界面
namespace app {

struct FileItem {
    ltm::Summary sum;
    bool checked = false;
    int  dupGroup = -1;
    u64  contentHash = 0;
    std::wstring note;          // 自定义备注      // 文件原始字节的 FNV-1a，用于重复筛查
};

enum PanelTab { TAB_INFO = 0, TAB_BLOCKS, TAB_ENTITIES, TAB_COUNT };

enum ModalKind {
    MODAL_NONE = 0, MODAL_META, MODAL_VERSION, MODAL_REPLACE,
    MODAL_BACKUP, MODAL_DUP, MODAL_LOG, MODAL_ABOUT, MODAL_PACKS
, MODAL_BLOCKPICK , MODAL_CONVERT };

struct CvItem {                    // 版本转换：待处理方块
    std::string name;
    u64         count = 0;
    std::string repl;              // 替代方块（空 = 未指定，保持原样）
};

struct State {
    HWND hwnd = nullptr;
    int  dpi = 96;
    ui::Painter painter;
    ui::Fonts   fonts;
    ui::Input   input;
    int  width = 1200, height = 760;

    // 方块库
    mc::BlockDB db;
    bool dbLoaded = false;
    std::string dbError;

    // 多目录管理
    std::vector<packmgr::DirEntry> dirs;
    std::vector<std::wstring> recentDirs;
    bool dirDropOpen = false;

    // 文件列表
    std::vector<FileItem> files;
    std::wstring scanDir;
    bool recursive = true;
    int  leftW = 560;              // 左侧栏宽度（可拖拽）
    bool splitterDrag = false;

    // 搜索
    std::wstring searchText;
    std::vector<int> searchHits;
    int  searchCur = -1;

    // 方块页签：就地替换
    int          blockSel = -1;
    std::string  blockFromName, blockToName;
    bool         blockPickOpen = false;
    std::wstring blockPickSearch;
    int          blockPickScroll = 0;

    // 信息页内联编辑
    int infoEdit = -1;

    // 弹出层
    bool moreOpen = false;
    bool fmtPickOpen = false;
    bool blockPickJustOpened = false;
    int  blockRowH = 34;           // 方块列表行高（放大显示）
    bool noteEditing = false;
    int  infoScroll = 0;
    // 备注栏（写入投影文件的 Metadata.Description）
    std::wstring noteText, noteOrig, notePath;
    int          curGen = 0;         // 每次打开文件自增，用于判断"换文件了"
    int          noteGen = -1;       // 备注框当前载入的是哪一代文件
    bool         noteDirty = false;

    // 列表列宽与排序
    int  colNameW = 190, colVerW = 78, colSizeW = 120;
    int  colDrag = -1, colDragX = 0, colDragW = 0;
    int   exportSize = 512;        // 导出图边长（512/1024/2048/3072）
    bool  exportMenuOpen = false;
    // 自由视角（拖动旋转）；previewView 仅用于四个预设按钮的高亮
    float yaw   = -45.0f;
    float pitch =  30.0f;
    bool  rotating = false;
    float dragYaw0 = 0, dragPitch0 = 0;
    int   dragX0 = 0, dragY0 = 0;
    float brightness = 1.45f;      // 渲染亮度系数（默认 145%）
    bool  brightDrag = false;
    bool scrollToSel = false;      // 请求把指定行滚入可见区（由 DrawList 用真实几何执行）
    int  scrollToIdx = -1;
    int  sortCol = -1;             // -1 不排序(按文件名) / 0 名称 / 1 版本 / 2 尺寸 / 3 目录
    bool sortAsc = true;

    // 方块选择弹窗
    int          bpVerIdx = -1;        // -1 = 全部版本
    std::wstring bpSearch;
    int          bpSel = -1;
    int          bpScroll = 0;
    int          bpVerScroll = 0;

    // 版本转换
    int                  cvVerIdx = -1;
    bool                 cvAnalyzed = false;
    std::vector<CvItem>  cvItems;
    int                  cvSel = -1;
    int                  cvScroll = 0;
    int                  cvBlkScroll = 0;
    std::wstring         cvSearch;
    std::wstring         lastConvertInfo;
    std::string  blockToVersion;       // 目标方块所属游戏版本
    int  listSel = -1;
    int  listScroll = 0;
    int  listSortCol = -1;        // -1 无排序
    bool listSortAsc = true;

    // 当前打开的投影
    std::unique_ptr<ltm::Schematic> cur;
    ltm::Analysis curAnalysis;
    bool curAnalysisDone = false;
    int  curVersionIdx = 0;

    // 预览
    render::Image preview;
    int  previewView = render::VIEW_LEFT45;
    int  previewSize = 400;
    bool previewDirty = true;
    double previewMs = 0;
    bool previewFailed = false;
    std::wstring previewErr;

    // 面板
    int  tab = TAB_INFO;
    int  blockScroll = 0;
    int  entityScroll = 0;

    // 弹窗
    int  modal = MODAL_NONE;
    std::wstring status;

    // 元数据编辑缓冲
    std::wstring metaName, metaAuthor, metaDesc;
    HWND editName = nullptr, editAuthor = nullptr, editDesc = nullptr;

    // 版本弹窗
    int  verListScroll = 0;
    int  verSelected = -1;
    bool verUseCurrent = false;

    // 替换弹窗
    int  repFromSel = -1, repToSel = -1;
    int  repActiveField = 0;      // 0=源 1=目标
    std::wstring repSearch;
    int  repScrollFrom = 0, repScrollTo = 0;
    bool repInherit = true;
    int  repScope = 0;            // 0=选中文件 1=当前文件
    bool repPickFrom = true;      // 正在选择哪一侧
    HWND editRepSearch = nullptr;

    // 备份弹窗
    std::vector<backup::Entry> backups;
    int  bkScroll = 0, bkSel = -1;
    int  bkFilterMode = 0;        // 0=全部 1=仅当前文件

    // 重复筛查
    int  dupMode = 0;             // 0=内容相同 1=元数据相同
    bool dupScanning = false;
    std::vector<std::vector<int>> dupGroups;
    int  dupScroll = 0;
    int  dupSel = -1;

    // 日志
    std::vector<std::wstring> logLines;
    int  logScroll = 0;

    // 方块图标缓存
    std::map<std::string, render::Image> iconCache;

    // 资源包 / 贴图管理
    std::vector<packmgr::Install> detected;
    bool          detecting = false;
    std::wstring  detectMsg;
    int           packSel = -1, installSel = -1;
    int           packScroll = 0, installScroll = 0;
    int           importKind = 1;          // 1=MC版本包 2=模组包

    // 鼠标状态
    bool mouseInWindow = false;
    bool tracking = false;
};

State& Get();

// ---- 生命周期
bool Init(HWND hwnd);
void Shutdown();
void OnSize(int w, int h);
void Paint(HDC dc);
void Tick();

// ---- 输入
void OnMouseMove(int x, int y);
void OnMouseLeave();
void OnLButtonDown(int x, int y, bool dbl);
void OnLButtonUp(int x, int y);
void OnRButtonDown(int x, int y);
void OnMouseWheel(int delta, int x, int y);
bool OnKeyDown(UINT vk, bool ctrl, bool shift);
void OnChar(wchar_t ch);

// ---- 功能
void SetScanDir(const std::wstring& dir);
void RescanAsync();
void OpenFile(int index);
void SaveCurrentWithBackup(const std::wstring& opName, bool allowBackup = true);
std::wstring LastSaveInfo();
void SetLastSaveInfo(const std::wstring& s);
void RenderPreview();

// 资源包 / 贴图管理
void StartPackDetect(bool autoImport);
void TickPacks();
void ReloadPacks();
bool PackImportRunning();

// 搜索 / 目录 / 杂项
void RunSearch(bool jumpToFirst);
void SearchNext(int dir);
void AddDir(const std::wstring& dir, bool enabled);
void RemoveDir(const std::wstring& dir);
std::vector<std::wstring> EnabledDirs();
bool RenameCurrentFile(const std::wstring& newBaseName, std::string& err);
void PushRecentDir(const std::wstring& dir);
std::wstring FormatStackCount(u64 n);          // 129 -> "64×3+1"
std::wstring FormatVersionHint(i32 formatVersion, i32 subVersion);
std::wstring EntityDisplay(const std::string& entityId, bool chinese);
void UpdateSearch();
void ApplySort();
void SetViewPreset(int preset);
void SaveCurrentMetaOnce(const std::wstring& opName);
void AttachNotes();
void SetNoteFor(int fileIndex, const std::wstring& text);
std::wstring GetNoteFor(int fileIndex);
void RebaseNotePath(const std::wstring& oldPath, const std::wstring& newPath);
void DeleteSelected();
void CopySelectedToClipboard();
void MoveSelected();
void OpenSelectedFolder();
void ExportPreviews();
void LoadDirsFromSettings();
// 重复筛查 / 批量替换
void RunDuplicateScan();
int  ApplyReplaceToFiles(const std::vector<int>& indices, const ltm::ReplaceRule& rule,
                         u64& totalBlocks);

// 供 main.cpp 使用
RECT TitleBarRect();
bool HitTestNonClient(int x, int y, LRESULT& result);
void SetStatus(const std::wstring& s);
void ExecuteCommand(int cmd);

// 方块工具
std::wstring BlockDisplay(const std::string& blockName, bool chinese);
bool IsModBlock(const std::string& blockName);
const render::Image* GetBlockIcon(const std::string& blockName, int size);

} // namespace app
