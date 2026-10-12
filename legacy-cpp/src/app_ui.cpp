#include "app_internal.h"
#include "packmgr.h"
#include "i18n.h"
#include <ctime>

namespace app {

int g_openMenu = -1;
// 弹层与"替换为"框的矩形，供统一的外部点击判定使用
static ui::Rect g_tgtRect, g_popRect;
static ui::Rect g_exportBtnRect;   // 「尺寸 N ▼」按钮的位置，菜单要贴着它展开
static int g_splitterHot = 0;
static int g_colHot = 0;

std::wstring FmtW(const wchar_t* fmt, ...) {
    wchar_t buf[2048];
    va_list ap;
    va_start(ap, fmt);
    _vsnwprintf(buf, 2047, fmt, ap);
    va_end(ap);
    buf[2047] = 0;
    return buf;
}

std::wstring VersionLabel(const ltm::Summary& s) {
    if (!s.mcVersionName.empty()) return Utf8ToWide(s.mcVersionName);
    if (s.dataVersion > 0) return L"dv" + std::to_wstring(s.dataVersion);
    return L"-";
}


// 路径中间省略：保留头尾，中间用 ... 代替（自己算，避免 DT_PATH_ELLIPSIS 的显示异常）
static std::wstring EllipPath(const std::wstring& s, int px) {
    int maxChars = px / 7;
    if (maxChars < 8) maxChars = 8;
    if ((int)s.size() <= maxChars) return s;
    int keepTail = (maxChars - 3) * 2 / 3;
    int keepHead = maxChars - 3 - keepTail;
    if (keepHead < 2) keepHead = 2;
    if (keepTail < 2) keepTail = 2;
    return s.substr(0, (size_t)keepHead) + L"..." + s.substr(s.size() - (size_t)keepTail);
}

// ---------------------------------------------------------------- 布局
Layout ComputeLayout() {
    State& st = Get();
    Layout L;
    L.title = { 0, 0, st.width, TITLE_H };
    L.menu = { 0, TITLE_H, st.width, MENU_H };
    L.toolbar = { 0, TITLE_H + MENU_H, st.width, TOOLBAR_H };
    L.status = { 0, st.height - STATUS_H, st.width, STATUS_H };
    int top = TITLE_H + MENU_H + TOOLBAR_H;
    L.content = { 0, top, st.width, st.height - top - STATUS_H };

    int minLeft = 360;
    int maxLeft = std::max(minLeft, st.width - 500);
    if (st.leftW < minLeft) st.leftW = minLeft;
    if (st.leftW > maxLeft) st.leftW = maxLeft;
    L.leftW = st.leftW;
    L.left = { L.content.x, L.content.y, st.leftW, L.content.h };
    L.splitter = { L.left.Right(), L.content.y, 5, L.content.h };
    L.right = { L.splitter.Right(), L.content.y, st.width - L.splitter.Right(), L.content.h };
    return L;
}

void HandleLayoutDrag(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Rect hot = { L.splitter.x - 3, L.splitter.y, L.splitter.w + 6, L.splitter.h };
    bool over = hot.Contains(c.in->mouseX, c.in->mouseY);
    if (c.in->mousePressed && over) {
        st.splitterDrag = true;
        c.in->usedMouse = true;
    }
    if (st.splitterDrag) {
        if (c.in->mouseDown) {
            st.leftW = c.in->mouseX;
            InvalidateRect(st.hwnd, nullptr, FALSE);
        } else st.splitterDrag = false;
        c.in->usedMouse = true;
    }
    g_splitterHot = (over || st.splitterDrag) ? 1 : 0;
}

// 分栏分隔线或列表列分隔线悬停时都显示左右调节光标
int SplitterHot() { return g_splitterHot || g_colHot; }

// ---------------------------------------------------------------- 标题栏
void DrawTitleBar(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    p.Fill(L.title, Theme::BgHeader);
    p.HLine(0, L.title.Bottom() - 1, st.width, Theme::Separator);

    ui::Rect icon = { 10, 7, 18, 18 };
    p.RoundedFill(icon, 4, Theme::Accent, CLR_INVALID);
    p.Text(L"\u25A6", icon, Theme::TextOnAccent, c.f->uiBold, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    p.Text(i18n::T("app.title"), { 34, 0, 400, TITLE_H }, Theme::Text, c.f->title,
           DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    int bw = 46;
    ui::Rect rc = { st.width - bw, 0, bw, TITLE_H };
    ui::Rect rm = { st.width - bw * 2, 0, bw, TITLE_H };
    ui::Rect rn = { st.width - bw * 3, 0, bw, TITLE_H };
    if (ui::IconButton(c, CMD_MIN, rn, L"\u2500", true, Theme::BtnFaceHot)) ExecuteCommand(CMD_MIN);
    if (ui::IconButton(c, CMD_MAX, rm, L"\u25A1", true, Theme::BtnFaceHot)) ExecuteCommand(CMD_MAX);
    {
        bool over = rc.Contains(c.in->mouseX, c.in->mouseY);
        if (over && c.in->mousePressed) c.in->activeId = CMD_CLOSE;
        if (over && c.in->mouseReleased && c.in->activeId == CMD_CLOSE) {
            c.in->activeId = 0;
            ExecuteCommand(CMD_CLOSE);
            return;
        }
        if (!c.in->mouseDown && c.in->activeId == CMD_CLOSE) c.in->activeId = 0;
        bool pressed = over && c.in->mouseDown;
        if (over) p.Fill(rc, pressed ? ui::Mix(Theme::CloseHot, RGB(0, 0, 0), 15) : Theme::CloseHot);
        HFONT old = (HFONT)SelectObject(p.Dc(), c.f->ui);
        SetTextColor(p.Dc(), over ? RGB(255, 255, 255) : Theme::Text);
        RECT rr = { rc.x, rc.y, rc.Right(), rc.Bottom() };
        DrawTextW(p.Dc(), L"\u2715", -1, &rr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(p.Dc(), old);
    }
}

// ---------------------------------------------------------------- 菜单栏（已移除）
void DrawMenuBar(ui::Ctx& c, const Layout& L) { (void)c; (void)L; }

// ---------------------------------------------------------------- 目录下拉
void DrawDirDropdown(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    if (!st.dirDropOpen) return;
    ui::Painter& p = *c.p;

    int w = 470;
    size_t recShow = std::min<size_t>(10, st.recentDirs.size());
    // 高度按实际内容算：标题(24) + 目录行 + 分隔/标题(30) + 历史行
    int dirRows = (int)st.dirs.size() + (st.dirs.empty() ? 1 : 0);
    int recRows = recShow ? (int)recShow : 1;
    int h = 5 + 24 + dirRows * 26 + 30 + recRows * 26 + 8;
    ui::Rect dd = { L.toolbar.x + 10, L.toolbar.Bottom() + 2, w, h };
    if (dd.Bottom() > L.status.y - 4) dd.h = L.status.y - 4 - dd.y;
    if (dd.Right() > st.width) dd.x = st.width - w - 10;

    p.Fill({ dd.x + 3, dd.y + 3, dd.w, dd.h }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 18));
    p.Fill(dd, Theme::BgPanel);
    p.Frame(dd, Theme::TextDim, 1);

    int y = dd.y + 5;
    p.Text(L"\u5f53\u524d\u626b\u63cf\u76ee\u5f55\uff08\u53ef\u591a\u9009\uff09", { dd.x + 12, y, w - 24, 20 },
           Theme::TextDim, c.f->smallBold);
    y += 24;
    for (size_t i = 0; i < st.dirs.size(); ++i) {
        if (y + 26 > dd.Bottom() - 4) break;
        ui::Rect row = { dd.x + 4, y, w - 8, 26 };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (hov) p.Fill(row, Theme::BgHover);
        bool en = st.dirs[i].enabled;
        if (ui::Checkbox(c, ui::Id("diren", (int)i), { row.x + 6, row.y + 5, 16, 16 }, en)) {
            st.dirs[i].enabled = en;
            RescanAsync();
        }
        p.Text(EllipPath(st.dirs[i].path, row.w - 66), { row.x + 30, row.y, row.w - 66, row.h },
               Theme::Text, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        if (ui::IconButton(c, ui::Id("dirdel", (int)i), { row.Right() - 26, row.y + 3, 22, 20 }, L"\u2715"))
            RemoveDir(st.dirs[i].path);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.scanDir = st.dirs[i].path;
            st.dirDropOpen = false;
            PushRecentDir(st.dirs[i].path);
        }
        y += 26;
    }
    if (st.dirs.empty()) {
        p.Text(L"\uff08\u5c1a\u672a\u6dfb\u52a0\u76ee\u5f55\uff09", { dd.x + 12, y, w - 24, 22 },
               Theme::TextDim, c.f->small);
        y += 26;
    }
    if (recShow && y + 50 < dd.Bottom()) {
        p.HLine(dd.x + 8, y + 2, w - 16, Theme::Separator);
        y += 8;
        p.Text(L"\u6700\u8fd1\u6d4f\u89c8\u5386\u53f2", { dd.x + 12, y, w - 24, 20 }, Theme::TextDim, c.f->smallBold);
        y += 22;
        for (size_t i = 0; i < recShow; ++i) {
            if (y + 26 > dd.Bottom() - 4) break;
            ui::Rect row = { dd.x + 4, y, w - 8, 26 };
            bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
            if (hov) p.Fill(row, Theme::BgHover);
            p.Text(EllipPath(st.recentDirs[i], row.w - 20), { row.x + 10, row.y, row.w - 20, row.h },
                   Theme::Text, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            if (hov && c.in->mouseReleased && !c.in->usedMouse) {
                AddDir(st.recentDirs[i], true);
                st.scanDir = st.recentDirs[i];
                st.dirDropOpen = false;
            }
            y += 26;
        }
    }

    if (c.in->mousePressed && !dd.Contains(c.in->mouseX, c.in->mouseY) &&
        !L.toolbar.Contains(c.in->mouseX, c.in->mouseY))
        st.dirDropOpen = false;
}

// ---------------------------------------------------------------- 工具栏（两行）
void DrawToolbar(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    p.Fill(L.toolbar, Theme::BgPanel);
    p.HLine(0, L.toolbar.Bottom() - 1, L.toolbar.w, Theme::Separator);
    p.HLine(0, L.toolbar.y + 36, L.toolbar.w, Theme::Separator);

    const int r1 = L.toolbar.y + 4;      // 第一行
    const int r2 = L.toolbar.y + 40;     // 第二行
    const int hh = 28;

    bool anyChecked = false, hasSel = !st.files.empty();
    for (const auto& f : st.files) if (f.checked) { anyChecked = true; break; }
    bool modified = (st.cur && st.cur->IsModified());

    // ================= 第一行：地址栏 + 目录/扫描/查找/帮助 + 搜索 =================
    int x = 10;
    ui::Rect dirBtn = { x, r1, 250, hh };
    p.Fill(dirBtn, RGB(255, 255, 255));
    p.Frame(dirBtn, st.dirDropOpen ? Theme::Accent : Theme::Separator, st.dirDropOpen ? 2 : 1);
    {
        std::wstring show = st.scanDir;
        if (show.empty() && !st.dirs.empty()) show = st.dirs[0].path;
        if (show.empty()) show = L"(\u672a\u9009\u62e9\u76ee\u5f55)";
        p.Text(EllipPath(show, dirBtn.w - 62), { dirBtn.x + 8, dirBtn.y, dirBtn.w - 58, dirBtn.h },
               Theme::Text, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        int on = 0;
        for (const auto& d : st.dirs) if (d.enabled) ++on;
        if (st.dirs.size() > 1)
            p.Text(FmtW(L"%d/%d", on, (int)st.dirs.size()), { dirBtn.Right() - 54, dirBtn.y, 28, dirBtn.h },
                   Theme::TextDim, c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        p.Text(L"\u25BC", { dirBtn.Right() - 22, dirBtn.y, 18, dirBtn.h }, Theme::TextDim, c.f->small,
               DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        if (dirBtn.Contains(c.in->mouseX, c.in->mouseY) && c.in->mousePressed) {
            st.dirDropOpen = !st.dirDropOpen;
            c.in->usedMouse = true;
        }
    }
    x = dirBtn.Right() + 6;
    auto b1 = [&](int cmd, int w, const wchar_t* label, bool en = true, bool primary = false) {
        bool r = ui::Button(c, cmd, { x, r1, w, hh }, label, en, primary);
        x += w + 5;
        return r;
    };
    if (b1(CMD_ADD_DIR, 72, L"\u6dfb\u52a0\u76ee\u5f55")) ExecuteCommand(CMD_ADD_DIR);
    if (b1(CMD_RESCAN, 72, L"\u91cd\u65b0\u626b\u63cf")) ExecuteCommand(CMD_RESCAN);
    if (b1(CMD_DUP, 118, L"\u91cd\u590d\u6587\u4ef6\u67e5\u627e")) ExecuteCommand(CMD_DUP);
    if (b1(CMD_PACKS_OPEN, 88, L"\u8d34\u56fe\u8d44\u6e90")) ExecuteCommand(CMD_PACKS_OPEN);
    if (b1(CMD_ABOUT, 56, L"\u5e2e\u52a9")) ExecuteCommand(CMD_ABOUT);

    // 第一行右侧：搜索
    int searchW = 250;
    int rightX = L.toolbar.Right() - searchW - 136;
    if (rightX < x + 10) rightX = x + 10;
    p.Text(L"\u641C\u7D22", { rightX - 40, r1, 36, hh }, Theme::Text, c.f->small,
           DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    ui::Rect sb = { rightX, r1, searchW, hh };
    p.Fill(sb, RGB(255, 255, 255));
    p.Frame(sb, Theme::Separator, 1);
    if (st.modal == MODAL_NONE && !st.blockPickOpen && !st.moreOpen && !st.fmtPickOpen) {
        g_editWant[EDIT_SEARCHBOX] = true;
        g_editPos[EDIT_SEARCHBOX] = { sb.x + 2, sb.y + 2, sb.w - 62, sb.h - 4 };
    }
    if (st.searchText.empty())
        p.Text(L"\u540d\u79f0/\u4f5c\u8005/\u6587\u4ef6\u540d\u2026", { sb.x + 8, sb.y, sb.w - 68, sb.h },
               Theme::TextDim, c.f->small);
    {
        wchar_t cnt[64] = L"";
        if (!st.searchHits.empty()) swprintf(cnt, 64, L"%d/%d", st.searchCur + 1, (int)st.searchHits.size());
        else if (!st.searchText.empty()) swprintf(cnt, 64, L"0/0");
        if (cnt[0])
            p.Text(cnt, { sb.Right() - 62, sb.y, 58, sb.h }, Theme::TextDim, c.f->small,
                   DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    }
    int bx = sb.Right() + 4;
    if (ui::IconButton(c, CMD_SEARCH_PREV, { bx, r1, 28, hh }, L"\u25B2")) ExecuteCommand(CMD_SEARCH_PREV);
    if (ui::IconButton(c, CMD_SEARCH_NEXT, { bx + 30, r1, 28, hh }, L"\u25BC")) ExecuteCommand(CMD_SEARCH_NEXT);
    if (ui::IconButton(c, CMD_SEARCH_CLEAR, { bx + 60, r1, 28, hh }, L"\u2715")) ExecuteCommand(CMD_SEARCH_CLEAR);

    // ================= 第二行：左=文件操作  右=详情/编辑操作 =================
    int x2 = 10;
    auto b2 = [&](int cmd, int w, const wchar_t* label, bool en = true, bool primary = false) {
        bool r = ui::Button(c, cmd, { x2, r2, w, hh }, label, en, primary);
        x2 += w + 5;
        return r;
    };
    ui::Rect recur = { x2, r2, 96, hh };
    ui::Checkbox(c, CMD_NONE + 2001, recur, st.recursive);
    p.Text(L"\u5B50\u76EE\u5F55", { recur.x + 20, recur.y, recur.w - 20, recur.h }, Theme::Text,
           c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    x2 += recur.w + 10;
    if (b2(CMD_DELETE_SEL, 60, L"\u5220\u9664", anyChecked)) ExecuteCommand(CMD_DELETE_SEL);
    if (b2(CMD_COPY_SEL, 60, L"\u590D\u5236", anyChecked)) ExecuteCommand(CMD_COPY_SEL);
    if (b2(CMD_MOVE_SEL, 78, L"\u79FB\u52A8\u5230", anyChecked)) ExecuteCommand(CMD_MOVE_SEL);
    if (b2(CMD_OPEN_SEL_DIR, 84, L"\u6253\u5f00\u76EE\u5F55", hasSel)) ExecuteCommand(CMD_OPEN_SEL_DIR);
    if (b2(CMD_CONVERT, 136, L"\u8f6c\u6362\u6295\u5f71\u6e38\u620f\u7248\u672c", hasSel)) ExecuteCommand(CMD_CONVERT);

    // 第二行右侧
    int xr = L.toolbar.Right() - 10;
    auto br = [&](int cmd, int w, const wchar_t* label, bool en = true, bool primary = false) {
        xr -= w;
        bool r = ui::Button(c, cmd, { xr, r2, w, hh }, label, en, primary);
        xr -= 5;
        return r;
    };
    if (br(CMD_BACKUP, 84, L"\u5907\u4EFD\u7BA1\u7406")) ExecuteCommand(CMD_BACKUP);
    if (br(CMD_LOG, 84, L"\u64CD\u4F5C\u65E5\u5FD7")) ExecuteCommand(CMD_LOG);
    if (br(CMD_REVERT, 60, L"\u64A4\u9500", modified)) ExecuteCommand(CMD_REVERT);
    if (br(CMD_SAVE, 60, L"\u4FDD\u5B58", modified, modified)) ExecuteCommand(CMD_SAVE);
    if (br(CMD_EXPORT, 104, L"\u5BFC\u51FA\u9884\u89C8\u56FE")) ExecuteCommand(CMD_EXPORT);
    {
        int bx2 = xr - 104;                 // br 内部会先减宽度再绘制，这里要提前对齐
        if (br(CMD_EXPORT_SIZE, 104, FmtW(L"\u5C3A\u5BF8 %d \u25BC", st.exportSize).c_str())) {
            st.exportMenuOpen = !st.exportMenuOpen;
            st.moreOpen = false; st.fmtPickOpen = false;
        }
        g_exportBtnRect = { bx2, r2, 104, hh };
    }
}

// ---------------------------------------------------------------- 文件列表
void DrawList(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    p.Fill(L.left, Theme::BgPanel);
    g_colHot = 0;                      // 每帧重置，否则鼠标移开后光标卡在双箭头

    // ---- 重复筛选条（就地处理，不弹窗）
    int dupBarH = 0;
    if (!st.dupGroups.empty()) {
        dupBarH = 30;
        ui::Rect bar = { L.left.x, L.left.y, L.left.w, dupBarH };
        p.Fill(bar, RGB(0xFF, 0xF3, 0xE0));
        p.HLine(bar.x, bar.Bottom() - 1, bar.w, RGB(0xE0, 0xB0, 0x70));
        p.Text(FmtW(L"\u91CD\u590D %d \u7EC4", (int)st.dupGroups.size()),
               { bar.x + 8, bar.y, 96, bar.h }, RGB(0xB0, 0x60, 0x00), c.f->smallBold);
        int bx = bar.x + 110;
        auto sb = [&](int cmd, int w, const wchar_t* label, bool en, bool primary) {
            bool r = ui::Button(c, cmd, { bx, bar.y + 3, w, bar.h - 6 }, label, en, primary);
            bx += w + 4;
            return r;
        };
        if (sb(CMD_DUP_MODE0, 68, L"\u5185\u5BB9\u76F8\u540C", true, st.dupMode == 0)) {
            st.dupMode = 0; ExecuteCommand(CMD_DUP);
        }
        if (sb(CMD_DUP_MODE1, 68, L"\u5143\u6570\u636E\u540C", true, st.dupMode == 1)) {
            st.dupMode = 1; ExecuteCommand(CMD_DUP);
        }
        if (sb(CMD_DUP_KEEP, 74, L"\u4FDD\u7559\u9996\u4E2A", true, false)) ExecuteCommand(CMD_DUP_KEEP);
        if (sb(CMD_DUP_DELETE, 56, L"\u5220\u9664", true, true)) ExecuteCommand(CMD_DUP_DELETE);
        if (ui::IconButton(c, CMD_DUP_CLOSE, { bar.Right() - 26, bar.y + 3, 22, bar.h - 6 }, L"\u2715"))
            ExecuteCommand(CMD_DUP_CLOSE);
    }

    ui::Rect head = { L.left.x, L.left.y + dupBarH, L.left.w, HEADER_H };
    p.Fill(head, Theme::BgHeader);
    p.HLine(head.x, head.Bottom() - 1, head.w, Theme::Separator);

    const int cbW = 30;
    int nameW = std::max(80, st.colNameW);
    int verW  = std::max(56, st.colVerW);
    int sizeW = std::max(70, st.colSizeW);
    int dirW  = L.left.w - cbW - nameW - verW - sizeW;
    if (dirW < 70) {                       // 空间不够时压缩名称列
        nameW += dirW - 70;
        if (nameW < 80) nameW = 80;
        dirW = L.left.w - cbW - nameW - verW - sizeW;
        if (dirW < 40) dirW = 40;
    }

    int cx = head.x + 6 + cbW;         // 表头不再放全选复选框，列与其对齐

    // 可排序表头 + 可拖拽分隔线
    auto sortable = [&](int x, int w, int col, const wchar_t* label) {
        ui::Rect cr = { x, head.y, w, head.h };
        bool hov = cr.Contains(c.in->mouseX, c.in->mouseY);
        if (st.sortCol == col) p.Fill(cr, Theme::BgListSel);
        else if (hov) p.Fill(cr, Theme::BgHover);
        p.Text(label, { cr.x + 4, cr.y, cr.w - 18, cr.h }, Theme::Text, c.f->smallBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        if (st.sortCol == col)
            p.Text(st.sortAsc ? L"\u25B2" : L"\u25BC", { cr.Right() - 16, cr.y, 14, cr.h },
                   Theme::Accent, c.f->small, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            if (st.sortCol == col) st.sortAsc = !st.sortAsc;
            else { st.sortCol = col; st.sortAsc = true; }
            ApplySort();
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    };
    sortable(cx, nameW, 0, i18n::T("col.name"));
    cx += nameW;
    sortable(cx, verW, 1, i18n::T("col.version"));
    cx += verW;
    sortable(cx, sizeW, 2, i18n::T("col.size"));
    cx += sizeW;
    sortable(cx, std::max(40, dirW), 3, i18n::T("col.dir"));

    // 分隔线拖拽
    {
        int dx = head.x + 6 + cbW;
        const int cw[3] = { nameW, verW, sizeW };
        for (int k = 0; k < 3; ++k) {
            dx += cw[k];
            ui::Rect hz = { dx - 7, head.y, 15, head.h };   // 加宽命中区，方便抓取
            bool hov = hz.Contains(c.in->mouseX, c.in->mouseY) || (st.colDrag == k);
            bool active = (st.colDrag == k);
            if (hov) g_colHot = 1;
            if (hov && c.in->mousePressed && st.colDrag < 0) {
                st.colDrag = k;
                st.colDragX = c.in->mouseX;
                st.colDragW = cw[k];
                c.in->usedMouse = true;
            }
            if (active && c.in->mouseDown) {
                int nw = st.colDragW + (c.in->mouseX - st.colDragX);
                int minW = (k == 0) ? 80 : (k == 1) ? 56 : 70;
                if (nw < minW) nw = minW;
                if (k == 0) st.colNameW = nw; else if (k == 1) st.colVerW = nw; else st.colSizeW = nw;
                InvalidateRect(st.hwnd, nullptr, FALSE);
                c.in->usedMouse = true;
            }
            if (active && !c.in->mouseDown) st.colDrag = -1;
            p.VLine(dx, head.y + 3, head.h - 6,
                    (hov || active) ? Theme::Accent : Theme::Separator);
        }
    }

    const int selBarH = 30;
    ui::Rect body = { L.left.x, head.Bottom(), L.left.w - 5,
                      L.left.h - HEADER_H - dupBarH - selBarH };
    int total = (int)st.files.size() * ROW_H;
    int maxScroll = std::max(0, total - body.h);
    // 把请求的行滚入可见区（用这里真实的 body.h，保证底部选择栏不被遮住）
    if (st.scrollToSel) {
        st.scrollToSel = false;
        int i = st.scrollToIdx;
        st.scrollToIdx = -1;
        if (i >= 0 && i < (int)st.files.size()) {
            int rowTop = i * ROW_H;
            if (rowTop < st.listScroll) st.listScroll = rowTop;
            else if (rowTop + ROW_H > st.listScroll + body.h)
                st.listScroll = rowTop + ROW_H - body.h;
        }
    }
    if (st.listScroll > maxScroll) st.listScroll = maxScroll;
    if (st.listScroll < 0) st.listScroll = 0;

    p.PushClip(body);
    int first = st.listScroll / ROW_H;
    int last = std::min((int)st.files.size(), first + body.h / ROW_H + 2);
    for (int i = first; i < last; ++i) {
        FileItem& fi = st.files[(size_t)i];
        ui::Rect row = { body.x, body.y + i * ROW_H - st.listScroll, body.w - 14, ROW_H };
        if (row.Bottom() < body.y || row.y > body.Bottom()) continue;

        bool selected = (i == st.listSel);
        bool hover = row.Contains(c.in->mouseX, c.in->mouseY);
        bool isHit = false, isCurHit = false;
        for (size_t hIdx = 0; hIdx < st.searchHits.size(); ++hIdx) {
            if (st.searchHits[hIdx] == i) {
                isHit = true;
                if ((int)hIdx == st.searchCur) isCurHit = true;
                break;
            }
        }
        if (isCurHit)      p.Fill(row, RGB(0xFF, 0xE0, 0x9A));
        else if (isHit)    p.Fill(row, RGB(0xFF, 0xF5, 0xD8));
        else if (selected) p.Fill(row, Theme::BgListSel);
        else if (hover)    p.Fill(row, Theme::BgHover);
        else if (i % 2)    p.Fill(row, Theme::BgListRowAlt);
        else               p.Fill(row, Theme::BgListRow);
        if (isHit) p.Fill({ row.x, row.y, 3, row.h }, RGB(0xE0, 0xA0, 0x20));

        int x = row.x + 6;
        ui::Checkbox(c, ui::Id("rowchk", i), { x + 1, row.y + 5, 16, 16 }, fi.checked);
        x += cbW;

        if (st.cur && st.cur->IsModified() && st.cur->Path() == fi.sum.path)
            p.Fill({ x, row.y + 4, 3, row.h - 8 }, Theme::Accent);
        if (fi.dupGroup >= 0) p.Fill({ x + 5, row.y + 9, 6, 6 }, Theme::Danger);

        std::wstring nm = fi.sum.name.empty() ? fsx::GetFileStem(fi.sum.fileName) : Utf8ToWide(fi.sum.name);
        p.Text(nm, { x + 14, row.y, nameW - 20, row.h }, fi.sum.valid ? Theme::Text : Theme::Danger, c.f->ui);

        x += nameW;
        p.Text(VersionLabel(fi.sum), { x + 4, row.y, verW - 8, row.h }, Theme::TextDim, c.f->small);

        x += verW;
        p.Text(FmtW(L"%d\u00D7%d\u00D7%d %lld", fi.sum.enclosing[0], fi.sum.enclosing[1],
                    fi.sum.enclosing[2], (long long)fi.sum.totalBlocks),
               { x + 4, row.y, sizeW - 8, row.h }, Theme::TextDim, c.f->small);

        x += sizeW;
        p.Text(EllipPath(fi.sum.dirName, dirW - 12), { x + 4, row.y, dirW - 8, row.h },
               Theme::TextDim, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

        if (hover && c.in->mouseReleased && !c.in->usedMouse) {
            st.listSel = i;
            OpenFile(i);
            c.in->usedMouse = true;
        }
        if (hover && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, CMD_NONE + 2020, { body.Right(), body.y, 14, body.h }, st.listScroll, total, body.h);

    // ---- 选择操作栏
    {
        ui::Rect sel = { L.left.x, body.Bottom(), L.left.w, selBarH };
        p.Fill(sel, Theme::BgHeader);
        p.HLine(sel.x, sel.y, sel.w, Theme::Separator);
        int nSel = 0;
        for (const auto& f : st.files) if (f.checked) ++nSel;
        p.Text(FmtW(L"\u5DF2\u9009 %d / %d", nSel, (int)st.files.size()),
               { sel.x + 10, sel.y, 150, sel.h }, Theme::Text, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        int bx = sel.Right() - 234;
        if (bx < sel.x + 160) bx = sel.x + 160;
        if (ui::Button(c, CMD_CHECK_ALL, { bx, sel.y + 3, 68, sel.h - 6 }, L"\u5168\u9009"))
            ExecuteCommand(CMD_CHECK_ALL);
        if (ui::Button(c, CMD_CHECK_INVERT, { bx + 72, sel.y + 3, 68, sel.h - 6 }, L"\u53CD\u9009"))
            ExecuteCommand(CMD_CHECK_INVERT);
        if (ui::Button(c, CMD_CHECK_NONE, { bx + 144, sel.y + 3, 68, sel.h - 6 }, L"\u6E05\u9664"))
            ExecuteCommand(CMD_CHECK_NONE);
    }

}

// ---------------------------------------------------------------- 预览 + 详情
static void DrawPreviewAndInfo(ui::Ctx& c, ui::Rect& area) {
    State& st = Get();
    ui::Painter& p = *c.p;

    int psz = st.previewSize;
    int cardH = psz + 96;          // 额外一行放亮度滑块
    ui::Rect card = { area.x + 8, area.y + 8, area.w - 16, cardH };
    ui::Card(c, card);

    ui::Rect img = { card.x + 10, card.y + 12, psz, psz };
    p.Fill(img, RGB(0xF0, 0xF0, 0xF0));
    if (!st.preview.px.empty()) p.Image(st.preview, img, false);
    else
        p.Text(st.previewFailed ? st.previewErr : i18n::T("preview.none"), img, Theme::TextDim,
               c.f->ui, DT_CENTER | DT_VCENTER | DT_WORDBREAK);
    p.Frame(img, Theme::Separator, 1);

    // ---- 在预览图上按住拖动即可 3D 旋转（拖动中降超采样保证跟手，松手后重渲染）
    if (img.Contains(c.in->mouseX, c.in->mouseY) && c.in->mousePressed) {
        st.rotating = true;
        st.dragX0 = c.in->mouseX; st.dragY0 = c.in->mouseY;
        st.dragYaw0 = st.yaw;     st.dragPitch0 = st.pitch;
        c.in->usedMouse = true;
    }
    if (st.rotating) {
        c.in->usedMouse = true;
        if (c.in->mouseDown) {
            float ny = st.dragYaw0   + (c.in->mouseX - st.dragX0) * 0.6f;
            float np = st.dragPitch0 - (c.in->mouseY - st.dragY0) * 0.5f;
            if (np >  89.0f) np =  89.0f;
            if (np < -89.0f) np = -89.0f;
            if (ny != st.yaw || np != st.pitch) {
                st.yaw = ny; st.pitch = np;
                st.previewView = -1;      // 已离开预设角度，取消按钮高亮
                RenderPreview();
            }
        } else {
            st.rotating = false;
            RenderPreview();              // 松手后恢复 2x 超采样
        }
    }

    p.Text(L"\u62d6\u52a8\u53ef\u65cb\u8f6c", { img.x + 6, img.Bottom() - 18, 140, 16 },
           Theme::TextDim, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    int bw = psz / 4;
    ui::Rect brow = { img.x, img.Bottom() + 6, psz, 24 };
    const char* vkeys[4] = { "view.front", "view.left45", "view.right45", "view.back" };
    const int vcmds[4] = { CMD_VIEW_FRONT, CMD_VIEW_LEFT, CMD_VIEW_RIGHT, CMD_VIEW_BACK };
    for (int i = 0; i < 4; ++i) {
        if (ui::Button(c, vcmds[i], { brow.x + i * bw, brow.y, bw - 2, brow.h },
                       i18n::T(vkeys[i]), true, st.previewView == i)) {
            SetViewPreset(i);          // 必须走这里：它会同时设置 yaw/pitch 并重渲染
        }
    }
    // ---- 亮度滑块（全局渲染亮度，实时生效）
    {
        ui::Rect srow = { img.x, brow.Bottom() + 8, psz, 24 };
        p.Text(L"\u4EAE\u5EA6", { srow.x, srow.y, 40, srow.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        ui::Rect hit = { srow.x + 44, srow.y, srow.w - 44 - 58, srow.h };
        ui::Rect track = { hit.x, srow.y + 9, hit.w, 6 };
        p.Fill(track, RGB(0xD4, 0xD8, 0xDE));
        float t = (st.brightness - 0.5f) / 1.5f;
        if (t < 0) t = 0; if (t > 1) t = 1;
        p.Fill({ track.x, track.y, (int)(t * track.w), track.h }, Theme::Accent);
        bool hov = hit.Contains(c.in->mouseX, c.in->mouseY);
        int kx = track.x + (int)(t * track.w);
        ui::Rect knob = { kx - 6, srow.y + 3, 12, 18 };
        p.Fill(knob, (hov || st.brightDrag) ? Theme::Accent : RGB(0x8C, 0x93, 0x9C));
        p.Frame(knob, Theme::Separator, 1);
        if (hov && c.in->mousePressed) { st.brightDrag = true; c.in->usedMouse = true; }
        if (st.brightDrag) {
            c.in->usedMouse = true;
            if (c.in->mouseDown) {
                float nt = (float)(c.in->mouseX - track.x) / (float)std::max(1, track.w);
                if (nt < 0) nt = 0; if (nt > 1) nt = 1;
                float nb = 0.5f + nt * 1.5f;
                if (nb != st.brightness) {
                    st.brightness = nb;
                    render::SetBrightness(nb);
                    RenderPreview();
                }
            } else {
                st.brightDrag = false;
                packmgr::SaveBrightness(st.brightness);
            }
        }
        p.Text(FmtW(L"%d%%", (int)(st.brightness * 100.0f + 0.5f)),
               { srow.Right() - 54, srow.y, 54, srow.h }, Theme::Text, c.f->small,
               DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    }

    int ix = img.Right() + 16;
    ui::Rect info = { ix, card.y + 12, std::max(60, card.Right() - ix - 12), card.h - 24 };
    if (st.cur) {
        ltm::Summary s = st.cur->GetSummary();
        int y = info.y;
        auto line = [&](const wchar_t* label, const std::wstring& value, COLORREF col = Theme::Text) {
            p.Text(label, { info.x, y, 96, 20 }, Theme::TextDim, c.f->small);
            p.Text(value, { info.x + 100, y, info.w - 100, 20 }, col, c.f->ui,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
            y += 21;
        };
        line(i18n::T("detail.name"), Utf8ToWide(s.name));
        line(i18n::T("detail.author"), Utf8ToWide(s.author));
        line(i18n::T("detail.mcver"), VersionLabel(s), Theme::Accent);
        line(i18n::T("detail.dims"), FmtW(L"%d \u00D7 %d \u00D7 %d", s.enclosing[0], s.enclosing[1], s.enclosing[2]));
        line(i18n::T("detail.blocks"), FmtW(L"%lld   (%s)", (long long)s.totalBlocks,
                                            FormatStackCount((u64)s.totalBlocks).c_str()));
        if (st.curAnalysisDone) line(i18n::T("detail.types"), std::to_wstring(st.curAnalysis.distinctBlockTypes));
        line(i18n::T("detail.entities"), FmtW(L"%lld", (long long)s.entityCount));
        if (st.cur->IsModified())
            p.Text(i18n::T("status.modified"), { info.x, y + 4, info.w, 20 }, Theme::Danger, c.f->uiBold);

        // ---- 备注栏：多行文本框（软换行），内容写入投影文件的 Metadata.Description
        int ny = y + 24;
        int boxTop = ny + 20;
        int boxBottom = card.Bottom() - 44;          // 底部留出按钮位置
        if (boxBottom - boxTop >= 56) {
            p.Text(L"\u5907\u6CE8", { info.x, ny, 60, 18 }, Theme::Text, c.f->smallBold,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            ui::Rect nbox = { info.x, boxTop, std::max(120, info.w - 8), boxBottom - boxTop };
            p.Fill(nbox, RGB(255, 255, 255));
            p.Frame(nbox, Theme::Accent, 1);
            g_editWant[EDIT_INFO_NOTE] = true;
            g_editPos[EDIT_INFO_NOTE] = { nbox.x + 3, nbox.y + 3, nbox.w - 6, nbox.h - 6 };
            if (st.noteText.empty() && st.noteOrig.empty())
                p.Text(L"\u5728\u6b64\u586b\u5199\u8fd9\u4e2a\u6295\u5f71\u7684\u7b80\u5355\u5907\u6CE8\u2026",
                       { nbox.x + 8, nbox.y + 4, nbox.w - 16, 24 }, Theme::TextDim, c.f->ui,
                       DT_LEFT | DT_TOP | DT_SINGLELINE);
            if (ui::Button(c, CMD_NOTE_EDIT, { nbox.Right() - 108, nbox.Bottom() + 6, 108, 28 },
                           L"\u4fdd\u5b58\u5907\u6CE8", st.noteDirty, st.noteDirty))
                ExecuteCommand(CMD_NOTE_EDIT);
        }
    } else {
        p.Text(i18n::T("preview.none"), info, Theme::TextDim, c.f->ui, DT_LEFT | DT_TOP | DT_WORDBREAK);
    }
    area.y = card.Bottom();
    area.h -= card.h;
}

static void DrawInfoTab(ui::Ctx& c, const ui::Rect& inner) {
    State& st = Get();
    ui::Painter& p = *c.p;
    ltm::Summary s = st.cur->GetSummary();

    struct F { const wchar_t* label; std::wstring value; int slot; bool editable; COLORREF col; };
    std::vector<F> rows = {
        { i18n::T("detail.file"),        s.fileName,                     EDIT_INFO_FILE,   true,  Theme::Text },
        { i18n::T("detail.name"),        Utf8ToWide(s.name),             EDIT_INFO_NAME,   true,  Theme::Text },
        { i18n::T("detail.author"),      Utf8ToWide(s.author),           EDIT_INFO_AUTHOR, true,  Theme::Text },
        { i18n::T("detail.desc"),        Utf8ToWide(s.description),      EDIT_INFO_DESC,   true,  Theme::Text },
        { i18n::T("detail.mcver"),       VersionLabel(s),                -1, false, Theme::Accent },
        { i18n::T("detail.dataversion"), std::to_wstring(s.dataVersion), -1, false, Theme::TextDim },
        { i18n::T("detail.format"),      FmtW(L"v%d.%d", s.formatVersion, s.formatSubVersion), -1, false, Theme::Text },
        { L"\u683c\u5f0f\u5bf9\u5e94\u7248\u672c", FormatVersionHint(s.formatVersion, s.formatSubVersion), -1, false, Theme::TextDim },
        { i18n::T("detail.regions"),     std::to_wstring(s.regionCount), -1, false, Theme::Text },
        { i18n::T("detail.created"),     FmtTime(s.timeCreated),         -1, false, Theme::TextDim },
        { i18n::T("detail.modified"),    FmtTime(s.timeModified),        -1, false, Theme::TextDim },
        { i18n::T("detail.filesize"),    FormatBytes(s.fileSize),        -1, false, Theme::TextDim },
        { i18n::T("detail.preview"),     s.hasPreview ? i18n::T("detail.yes") : i18n::T("detail.no"), -1, false, Theme::TextDim },
    };

    int rowH = 22;
    int contentH = (int)rows.size() * rowH;
    int viewH = inner.h - 24;
    int maxScroll = std::max(0, contentH - viewH);
    if (st.infoScroll > maxScroll) st.infoScroll = maxScroll;
    if (st.infoScroll < 0) st.infoScroll = 0;
    p.PushClip({ inner.x, inner.y, inner.w, viewH });
    int y = inner.y - st.infoScroll;
    for (size_t i = 0; i < rows.size(); ++i) {
        F& r = rows[i];
        ui::Rect row = { inner.x, y, inner.w, rowH };
        bool editing = (r.editable && st.infoEdit == r.slot);
        bool hov = r.editable && row.Contains(c.in->mouseX, c.in->mouseY);

        p.Text(r.label, { row.x, row.y, 108, row.h }, Theme::TextDim, c.f->small);
        ui::Rect vr = { row.x + 114, row.y, row.w - 160, row.h };
        if (editing) {
            p.Fill(vr, RGB(255, 255, 255));
            p.Frame(vr, Theme::Accent, 1);
            g_editWant[r.slot] = true;
            g_editPos[r.slot] = { vr.x + 3, vr.y + 1, vr.w - 6, vr.h - 2 };
        } else {
            if (hov) p.Fill(vr, Theme::BgHover);
            p.Text(r.value, vr, r.col, c.f->ui);
            if (r.editable)
                p.Text(L"\u270E", { row.Right() - 18, row.y, 16, row.h }, Theme::TextDim, c.f->small,
                       DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        if (hov && c.in->mousePressed) {
            if (st.infoEdit != -1 && st.infoEdit != r.slot) CommitInfoEdit(st.infoEdit);
            st.infoEdit = r.slot;
            g_editFocusReq = r.slot;
            c.in->usedMouse = true;
        }
        y += rowH;
    }
    p.PopClip();                       // 结束信息行的裁剪区
    p.Text(st.infoEdit != -1
               ? L"\u76f4\u63a5\u4fee\u6539\u4e2d\uff1a\u56de\u8f66\u6216\u70b9\u51fb\u522b\u5904\u786e\u8ba4"
               : L"\u70b9\u51fb\u5e26 \u270E \u7684\u5b57\u6bb5\u53ef\u76f4\u63a5\u4fee\u6539\uff08\u6587\u4ef6\u540d\u3001\u540d\u79f0\u3001\u4f5c\u8005\u3001\u63cf\u8ff0\uff09",
           { inner.x, y + 4, inner.w, 20 }, Theme::TextDim, c.f->small);
}

static void DrawBlocksTab(ui::Ctx& c, const ui::Rect& inner) {
    State& st = Get();
    ui::Painter& p = *c.p;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);

    std::vector<std::pair<std::string, u64>> items;
    for (const auto& kv : st.curAnalysis.blocksByName)
        if (kv.first != "minecraft:air") items.push_back(kv);
    std::sort(items.begin(), items.end(), [](auto& a, auto& b) { return a.second > b.second; });

    int barH = 46;
    const int hdrH = 22;
    ui::Rect hdr = { inner.x, inner.y, inner.w, hdrH };
    p.Fill(hdr, Theme::BgHeader);
    p.Frame(hdr, Theme::Separator, 1);
    p.Text(L"\u65b9\u5757", { hdr.x + 42, hdr.y, 210, hdr.h }, Theme::TextDim, c.f->smallBold,
           DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    p.Text(L"\u65b9\u5757 ID", { hdr.x + 258, hdr.y, hdr.w - 600, hdr.h }, Theme::TextDim, c.f->smallBold,
           DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    p.Text(L"\u603b\u6570", { hdr.Right() - 236, hdr.y, 84, hdr.h }, Theme::TextDim, c.f->smallBold,
           DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    p.Text(L"\u5806\u53e0", { hdr.Right() - 146, hdr.y, 142, hdr.h }, Theme::TextDim, c.f->smallBold,
           DT_RIGHT | DT_VCENTER | DT_SINGLELINE);

    ui::Rect list = { inner.x, inner.y + hdrH, inner.w, std::max(20, inner.h - barH - 8 - hdrH) };
    int rowH = st.blockRowH;
    int total = (int)items.size() * rowH;
    int maxScroll = std::max(0, total - list.h);
    if (st.blockScroll > maxScroll) st.blockScroll = maxScroll;
    if (st.blockScroll < 0) st.blockScroll = 0;
    if (st.blockSel >= (int)items.size()) { st.blockSel = -1; st.blockToName.clear(); }

    p.PushClip(list);
    int first = st.blockScroll / rowH;
    int last = std::min((int)items.size(), first + list.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        auto& it = items[(size_t)i];
        ui::Rect row = { list.x, list.y + i * rowH - st.blockScroll, list.w - 14, rowH };
        if (row.Bottom() < list.y || row.y > list.Bottom()) continue;
        bool sel = (st.blockSel == i);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        else if (i % 2) p.Fill(row, Theme::BgListRowAlt);

        const render::Image* ic = GetBlockIcon(it.first, 28);
        if (ic && !ic->px.empty()) p.Image(*ic, { row.x + 4, row.y + 3, 28, 28 }, true);
        else p.Fill({ row.x + 4, row.y + 3, 28, 28 }, RGB(0x18, 0x00, 0x18));

        p.Text(BlockDisplay(it.first, chinese), { row.x + 42, row.y, 210, row.h }, Theme::Text, c.f->uiBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(Utf8ToWide(it.first), { row.x + 258, row.y, row.w - 600, row.h }, Theme::TextDim, c.f->ui,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (IsModBlock(it.first))
            p.Text(i18n::T("rep.source.mod"), { row.Right() - 330, row.y, 60, row.h }, Theme::Danger,
                   c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        // 总数
        p.Text(FmtW(L"%llu", (unsigned long long)it.second), { row.Right() - 236, row.y, 84, row.h },
               Theme::Text, c.f->ui, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        // 堆叠（按 64\u00d799999+63 的最大长度预留宽度）
        p.Text(FormatStackCount(it.second), { row.Right() - 146, row.y, 142, row.h }, Theme::Accent, c.f->uiBold,
               DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.blockSel = i;
            st.blockFromName = it.first;
            st.blockPickOpen = false;
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, CMD_NONE + 2030, { list.Right() + 2, list.y, 12, list.h }, st.blockScroll, total, list.h);

    ui::Rect bar = { inner.x, list.Bottom() + 8, inner.w, barH };
    p.Fill(bar, Theme::BgHeader);
    p.Frame(bar, Theme::Separator, 1);
    if (st.blockSel < 0) {
        p.Text(L"\u5728\u4e0a\u9762\u70b9\u9009\u4e00\u4e2a\u65b9\u5757\uff0c\u7136\u540e\u70b9\u201c\u66ff\u6362\u65b9\u5757\u201d",
               { bar.x + 12, bar.y, bar.w - 150, bar.h }, Theme::TextDim, c.f->ui,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    } else {
        if (st.blockFromName.empty()) st.blockFromName = items[(size_t)st.blockSel].first;
        const render::Image* ic = GetBlockIcon(st.blockFromName, 22);
        if (ic && !ic->px.empty()) p.Image(*ic, { bar.x + 10, bar.y + 11, 22, 22 }, true);
        p.Text(BlockDisplay(st.blockFromName, chinese), { bar.x + 38, bar.y, 220, bar.h },
               Theme::Text, c.f->uiBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(Utf8ToWide(st.blockFromName), { bar.x + 264, bar.y, bar.w - 420, bar.h },
               Theme::TextDim, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
    if (ui::Button(c, CMD_BLK_PICK, { bar.Right() - 128, bar.y + 8, 116, bar.h - 16 },
                   L"\u66ff\u6362\u65b9\u5757", st.blockSel >= 0, st.blockSel >= 0))
        ExecuteCommand(CMD_BLK_PICK);
}

static void DrawEntitiesTab(ui::Ctx& c, const ui::Rect& inner) {
    State& st = Get();
    ui::Painter& p = *c.p;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);

    std::vector<std::pair<std::string, u64>> items(st.curAnalysis.entitiesById.begin(),
                                                   st.curAnalysis.entitiesById.end());
    std::sort(items.begin(), items.end(), [](auto& a, auto& b) { return a.second > b.second; });

    int rowH = 24, y = inner.y;
    if (items.empty() && st.curAnalysis.tileEntitiesById.empty()) {
        p.Text(L"\u8be5\u6295\u5f71\u4e0d\u5305\u542b\u4efb\u4f55\u5b9e\u4f53", inner, Theme::TextDim, c.f->ui,
               DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        return;
    }
    if (!items.empty()) {
        p.Fill({ inner.x, y, inner.w, rowH }, Theme::BgHeader);
        p.Text(L"\u4e2d\u6587\u540d", { inner.x + 4, y, 196, rowH }, Theme::TextDim, c.f->smallBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        p.Text(L"\u5b9e\u4f53 ID", { inner.x + 206, y, inner.w - 300, rowH }, Theme::TextDim, c.f->smallBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        p.Text(L"\u6570\u91cf", { inner.Right() - 80, y, 76, rowH }, Theme::TextDim, c.f->smallBold,
               DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        p.Text(L"\u751f\u7269 / \u5b9e\u4f53", { inner.x, y, inner.w, rowH }, Theme::TextDim, c.f->smallBold);
        y += rowH;
        for (const auto& it : items) {
            if (y + rowH > inner.Bottom()) break;
            p.Text(EntityDisplay(it.first, chinese), { inner.x, y, 200, rowH }, Theme::Text, c.f->ui);
            p.Text(Utf8ToWide(it.first), { inner.x + 206, y, inner.w - 300, rowH }, Theme::TextDim, c.f->small,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
            p.Text(std::to_wstring(it.second), { inner.Right() - 80, y, 80, rowH }, Theme::Accent,
                   c.f->uiBold, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
            y += rowH;
        }
    }
    if (!st.curAnalysis.tileEntitiesById.empty()) {
        y += 8;
        if (y + rowH <= inner.Bottom())
        p.Fill({ inner.x, y, inner.w, rowH }, Theme::BgHeader);
            p.Text(L"\u65b9\u5757\u5b9e\u4f53\uff08\u4e2d\u6587\u540d\uff09", { inner.x + 4, y, 196, rowH },
                   Theme::TextDim, c.f->smallBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            p.Text(L"\u65b9\u5757 ID", { inner.x + 206, y, inner.w - 300, rowH }, Theme::TextDim, c.f->smallBold,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            p.Text(L"\u6570\u91cf", { inner.Right() - 80, y, 76, rowH }, Theme::TextDim, c.f->smallBold,
                   DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        y += rowH;
        std::vector<std::pair<std::string, u64>> ts(st.curAnalysis.tileEntitiesById.begin(),
                                                    st.curAnalysis.tileEntitiesById.end());
        std::sort(ts.begin(), ts.end(), [](auto& a, auto& b) { return a.second > b.second; });
        for (const auto& it : ts) {
            if (y + rowH > inner.Bottom()) break;
            p.Text(BlockDisplay(it.first, chinese), { inner.x, y, 200, rowH }, Theme::TextDim, c.f->small);
            p.Text(Utf8ToWide(it.first), { inner.x + 206, y, inner.w - 300, rowH }, Theme::TextDim, c.f->small,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
            p.Text(std::to_wstring(it.second), { inner.Right() - 80, y, 80, rowH }, Theme::TextDim,
                   c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
            y += rowH;
        }
    }
}

void DrawDetail(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    p.Fill(L.right, Theme::BgWindow);

    ui::Rect area = { L.right.x, L.right.y, L.right.w, L.right.h };
    DrawPreviewAndInfo(c, area);

    if (area.h < 80) return;
    ui::Rect tabrow = { area.x + 8, area.y + 4, area.w - 16, 26 };
    int tw = 88;
    if (ui::Tab(c, CMD_TAB_INFO, { tabrow.x, tabrow.y, tw, tabrow.h }, i18n::T("tab.info"), st.tab == TAB_INFO)) st.tab = TAB_INFO;
    if (ui::Tab(c, CMD_TAB_BLOCKS, { tabrow.x + tw, tabrow.y, tw, tabrow.h }, i18n::T("tab.blocks"), st.tab == TAB_BLOCKS)) st.tab = TAB_BLOCKS;
    if (ui::Tab(c, CMD_TAB_ENTITIES, { tabrow.x + tw * 2, tabrow.y, tw, tabrow.h }, i18n::T("tab.entities"), st.tab == TAB_ENTITIES)) st.tab = TAB_ENTITIES;
    p.HLine(tabrow.x, tabrow.Bottom(), tabrow.w, Theme::Separator);

    ui::Rect body = { area.x + 8, tabrow.Bottom() + 1, area.w - 16, area.h - (tabrow.Bottom() - area.y) - 6 };
    if (body.h <= 10) return;
    p.Fill(body, Theme::BgPanel);
    p.Frame(body, Theme::Separator, 1);
    ui::Rect inner = { body.x + 10, body.y + 8, body.w - 32, body.h - 16 };

    if (!st.cur) {
        p.Text(i18n::T("preview.none"), inner, Theme::TextDim, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        return;
    }
    if (st.tab == TAB_INFO)        DrawInfoTab(c, inner);
    else if (st.tab == TAB_BLOCKS) DrawBlocksTab(c, inner);
    else                           DrawEntitiesTab(c, inner);
}

// ---------------------------------------------------------------- 方块选择弹层
void DrawBlockPickPopup(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    if (!st.blockPickOpen || !st.dbLoaded) return;
    ui::Painter& p = *c.p;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);

    int w = 640, h = 430;
    ui::Rect pop = { L.right.x + 30, L.right.Bottom() - h - 40, w, h };
    if (pop.x + w > st.width) pop.x = st.width - w - 12;
    if (pop.x < 8) pop.x = 8;
    if (pop.y < L.right.y + 30) pop.y = L.right.y + 30;
    if (pop.Bottom() > L.status.y - 6) pop.y = L.status.y - 6 - h;

    p.Fill({ pop.x + 3, pop.y + 3, pop.w, pop.h }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 22));
    p.Fill(pop, Theme::BgPanel);
    p.Frame(pop, Theme::Accent, 1);

    p.Fill({ pop.x, pop.y, pop.w, 34 }, Theme::BgHeader);
    p.Text(L"\u9009\u62e9\u76ee\u6807\u65b9\u5757", { pop.x + 12, pop.y, pop.w - 50, 34 }, Theme::Text,
           c.f->uiBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    if (ui::IconButton(c, ui::Id("blkpopx", 1), { pop.Right() - 34, pop.y + 5, 26, 24 }, L"\u2715"))
        st.blockPickOpen = false;
    p.HLine(pop.x, pop.y + 34, pop.w, Theme::Separator);

    g_popRect = pop;
    g_editWant[EDIT_REP_SEARCH] = true;
    g_editPos[EDIT_REP_SEARCH] = { pop.x + 10, pop.y + 42, pop.w - 20, 24 };
    p.Fill({ pop.x + 10, pop.y + 42, pop.w - 20, 24 }, RGB(255, 255, 255));
    p.Frame({ pop.x + 10, pop.y + 42, pop.w - 20, 24 }, Theme::Separator, 1);
    if (st.blockPickSearch.empty())
        p.Text(L"\u8f93\u5165\u5173\u952e\u5b57\u7b5b\u9009\u2026", { pop.x + 16, pop.y + 42, pop.w - 32, 24 },
               Theme::TextDim, c.f->small);

    const std::vector<std::string>& names = st.db.AllBlockNames();
    static std::vector<int> filt;
    static std::wstring lastQ;
    static size_t lastCount = (size_t)-1;
    static std::wstring lastQuerySentinel;
    // 注意：不能只比较搜索词。第一次打开时搜索词与 lastQ 都是空，条件不成立会导致列表永远为空。
    // 因此用 lastCount 作为"是否已构建过"的标记。
    if (lastCount != names.size() || st.blockPickSearch != lastQ) {
        filt.clear();
        std::wstring q = ToLowerW(st.blockPickSearch);
        for (size_t i = 0; i < names.size(); ++i) {
            std::wstring dn = BlockDisplay(names[i], true) + L" " + Utf8ToWide(names[i]);
            if (q.empty() || ToLowerW(dn).find(q) != std::wstring::npos) filt.push_back((int)i);
        }
        lastQ = st.blockPickSearch;
        lastCount = names.size();
    }

    ui::Rect list = { pop.x + 10, pop.y + 72, pop.w - 20, pop.h - 84 };
    int rowH = 24;
    int total = (int)filt.size() * rowH;
    int maxScroll = std::max(0, total - list.h);
    if (st.blockPickScroll > maxScroll) st.blockPickScroll = maxScroll;
    if (st.blockPickScroll < 0) st.blockPickScroll = 0;
    p.PushClip(list);
    int first = st.blockPickScroll / rowH;
    int last = std::min((int)filt.size(), first + list.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        const std::string& bn = names[(size_t)filt[(size_t)i]];
        ui::Rect row = { list.x, list.y + i * rowH - st.blockPickScroll, list.w - 14, rowH };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (bn == st.blockToName) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        const render::Image* ic = GetBlockIcon(bn, 18);
        if (ic && !ic->px.empty()) p.Image(*ic, { row.x + 3, row.y + 3, 18, 18 }, true);
        else p.Fill({ row.x + 3, row.y + 3, 18, 18 }, RGB(0x18, 0x00, 0x18));
        p.Text(BlockDisplay(bn, chinese), { row.x + 26, row.y, 190, row.h }, Theme::Text, c.f->small);
        p.Text(Utf8ToWide(bn), { row.x + 220, row.y, row.w - 306, row.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        bool mod = IsModBlock(bn);
        p.Text(mod ? i18n::T("rep.source.mod") : i18n::T("rep.source.vanilla"),
               { row.Right() - 80, row.y, 74, row.h }, mod ? Theme::Danger : Theme::TextDim, c.f->small,
               DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.blockToName = bn;
            st.blockPickOpen = false;
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, CMD_NONE + 3400, { list.Right() + 2, list.y, 12, list.h },
                  st.blockPickScroll, total, list.h);

}

// 外部点击关闭（统一判定：鼠标松开时既不在弹层内、也不在"替换为"框内）
void CloseBlockPickIfOutside(ui::Ctx& c) {
    State& st = Get();
    if (!st.blockPickOpen) return;
    if (!c.in->mouseReleased) return;
    if (g_popRect.Contains(c.in->mouseX, c.in->mouseY)) return;
    if (g_tgtRect.Contains(c.in->mouseX, c.in->mouseY)) return;
    st.blockPickOpen = false;
}

// ---------------------------------------------------------------- 状态栏
void DrawStatusBar(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    p.Fill(L.status, Theme::BgHeader);
    p.HLine(0, L.status.y, L.status.w, Theme::Separator);
    std::wstring msg = st.status;
    if (IsScanning()) msg = i18n::T("status.scanning");
    p.Text(msg, { 10, L.status.y, std::max(80, L.status.w - 560), L.status.h }, Theme::Text, c.f->ui);

    int n = (int)st.files.size(), sel = 0;
    for (const auto& f : st.files) if (f.checked) ++sel;
    std::wstring right = FmtW(i18n::T("status.found"), n);
    if (sel) right += L"   " + FmtW(i18n::T("status.selected"), sel);
    if (!st.searchHits.empty())
        right += FmtW(L"   \u641c\u7d22\u547d\u4e2d %d", (int)st.searchHits.size());
    if (st.dbLoaded) right += FmtW(L"   \u65b9\u5757\u5e93 %zu \u7248\u672c", st.db.VersionCount());
    p.Text(right, { std::max(10, L.status.w - 660), L.status.y, 650, L.status.h },
           Theme::TextDim, c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
}

// ---------------------------------------------------------------- 导出尺寸选择
void DrawExportMenu(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    if (!st.exportMenuOpen) return;
    ui::Painter& p = *c.p;
    static const int sizes[4] = { 512, 1024, 2048, 3072 };
    int iw = 230, ih = 34 + 4 * 32 + 10;
    ui::Rect dd = { g_exportBtnRect.x, g_exportBtnRect.Bottom() + 2, iw, ih };
    if (dd.Right() > L.toolbar.Right() - 6) dd.x = L.toolbar.Right() - 6 - iw;
    if (dd.x < 4) dd.x = 4;
    p.Fill({ dd.x + 3, dd.y + 3, dd.w, dd.h }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 18));
    p.Fill(dd, Theme::BgPanel);
    p.Frame(dd, Theme::TextDim, 1);
    p.Text(L"导出图边长", { dd.x + 12, dd.y + 4, iw - 24, 26 }, Theme::TextDim,
           c.f->smallBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    int y = dd.y + 34;
    for (int k = 0; k < 4; ++k) {
        ui::Rect row = { dd.x + 2, y, iw - 4, 32 };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        bool cur = (st.exportSize == sizes[k]);
        if (cur) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        p.Text(FmtW(L"%d \u00D7 %d", sizes[k], sizes[k]), { row.x + 12, row.y, 150, row.h },
               cur ? Theme::Accent : Theme::Text, c.f->ui, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.exportSize = sizes[k];
            packmgr::SaveExportSize(st.exportSize);
            st.exportMenuOpen = false;
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
        y += 32;
    }
    if (c.in->mousePressed && !dd.Contains(c.in->mouseX, c.in->mouseY) &&
        !L.toolbar.Contains(c.in->mouseX, c.in->mouseY))
        st.exportMenuOpen = false;
}

} // namespace app
