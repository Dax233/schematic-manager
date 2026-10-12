#include "app_internal.h"
#include <ctime>
#include <shellapi.h>
#include "packbuild.h"
#include "packmgr.h"

namespace app {

// ------------------------------------------------------------------ 工具
std::wstring FmtTime(i64 ms) {
    if (ms <= 0) return L"-";
    time_t t = (time_t)(ms / 1000);
    tm lt;
    localtime_s(&lt, &t);
    wchar_t buf[64];
    swprintf(buf, 64, L"%04d-%02d-%02d %02d:%02d:%02d",
             lt.tm_year + 1900, lt.tm_mon + 1, lt.tm_mday, lt.tm_hour, lt.tm_min, lt.tm_sec);
    return buf;
}

ui::Rect ModalRect(int w, int h) {
    State& st = Get();
    if (w > st.width - 40) w = st.width - 40;
    if (h > st.height - 40) h = st.height - 40;
    return { (st.width - w) / 2, (st.height - h) / 2, w, h };
}

static void DimBackground(ui::Ctx& c) {
    State& st = Get();
    // 用实心混色而不是 AlphaBlend：某些环境下 AlphaBlend 不生效，
    // 会导致弹窗失去遮罩、看起来"被下面的内容盖住"。
    c.p->Fill({ 0, 0, st.width, st.height }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 42));
}

void ModalFrame(ui::Ctx& c, const ui::Rect& r, const wchar_t* title) {
    if (!ModalHosted()) {
        DimBackground(c);
        for (int i = 4; i >= 1; --i)
            c.p->Frame({ r.x - i + 2, r.y - i + 2, r.w + i * 2, r.h + i * 2 },
                       ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 30 + i * 6), 1);
        c.p->Fill(r, Theme::BgPanel);
        c.p->Frame(r, ui::Mix(Theme::TextDim, RGB(0, 0, 0), 35), 2);
    }
    ui::Rect tb = { r.x, r.y, r.w, 36 };
    c.p->Fill(tb, Theme::BgHeader);
    c.p->HLine(tb.x, tb.Bottom() - 1, tb.w, Theme::Separator);
    c.p->Text(title, { tb.x + 14, tb.y, tb.w - 60, tb.h }, Theme::Text, c.f->uiBold,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    // 注意：这里必须用一个独立的控件 id。若与弹窗底部「关闭」按钮共用 CMD_MODAL_CANCEL，
    // 标题栏按钮会先绘制并在鼠标松开那一帧清掉 activeId，导致底部按钮永远收不到点击。
    if (ui::IconButton(c, ui::Id("modalx", 1), { tb.Right() - 34, tb.y + 6, 26, 24 }, L"\u2715"))
        ExecuteCommand(CMD_MODAL_CANCEL);
}

// ------------------------------------------------------------------ 原生 EDIT 控件
ui::Rect g_editPos[EDIT_SLOT_COUNT];
bool     g_editWant[EDIT_SLOT_COUNT];
int      g_editFocusReq = -1;

static HWND     g_edits[EDIT_SLOT_COUNT] = {};
static WNDPROC  g_oldProc[EDIT_SLOT_COUNT] = {};
static bool     g_editInit[EDIT_SLOT_COUNT] = {};
static ui::Rect g_editLast[EDIT_SLOT_COUNT] = {};

static LRESULT CALLBACK EditProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    int slot = (int)GetPropW(h, L"pmslot");
    if (m == WM_KEYDOWN) {
        if (w == VK_RETURN) { PostMessageW(GetParent(h), WM_APP + 2, (WPARAM)slot, 0); return 0; }
        if (w == VK_ESCAPE) { PostMessageW(GetParent(h), WM_APP + 3, (WPARAM)slot, 0); return 0; }
    }
    if (slot >= 0 && slot < EDIT_SLOT_COUNT && g_oldProc[slot])
        return CallWindowProcW(g_oldProc[slot], h, m, w, l);
    return DefWindowProcW(h, m, w, l);
}

static HWND g_editParent = nullptr;

static HWND MakeEdit(HWND parent, HFONT f, int slot) {
    // 备注框用多行 + 纵向滚动，实现"自动换行"。
    // 关键：不加 ES_WANTRETURN —— 软换行只是显示效果，GetWindowText 取回的仍是
    // 不含换行符的原始文本，所以不会污染投影文件里的 Description。
    DWORD style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
    if (slot == EDIT_INFO_NOTE) style |= ES_MULTILINE | WS_VSCROLL | ES_LEFT;
    else                        style |= ES_AUTOHSCROLL;
    HWND h = CreateWindowExW(0, L"EDIT", L"", style,
        0, 0, 10, 10, parent, nullptr, nullptr, nullptr);
    if (!h) return nullptr;
    SendMessageW(h, WM_SETFONT, (WPARAM)f, TRUE);
    SetPropW(h, L"pmslot", (HANDLE)(INT_PTR)slot);
    g_oldProc[slot] = (WNDPROC)SetWindowLongPtrW(h, GWLP_WNDPROC, (LONG_PTR)EditProc);
    g_editInit[slot] = false;
    return h;
}

static void SetEditTextNow(HWND h, const std::wstring& s) {
    if (!h) return;
    wchar_t buf[4096];
    GetWindowTextW(h, buf, 4096);
    if (std::wstring(buf) != s) SetWindowTextW(h, s.c_str());
}

void ResetEditInit() { for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editInit[i] = false; }

void DestroyEdits() {
    for (int i = 0; i < EDIT_SLOT_COUNT; ++i) {
        if (g_edits[i]) { DestroyWindow(g_edits[i]); g_edits[i] = nullptr; }
        g_oldProc[i] = nullptr;
        g_editInit[i] = false;
        g_editLast[i] = { 0, 0, 0, 0 };
    }
    g_editFocusReq = -1;
}

void CommitInfoEdit(int slot) {
    State& st = Get();
    if (!st.cur || slot < 0 || slot >= EDIT_SLOT_COUNT || !g_edits[slot]) { st.infoEdit = -1; return; }
    wchar_t buf[4096] = L"";
    GetWindowTextW(g_edits[slot], buf, 4096);
    std::wstring v = buf;
    st.infoEdit = -1;

    std::string err;
    bool changed = false;
    if (slot == EDIT_INFO_NAME) {
        st.cur->SetMetaName(WideToUtf8(v)); changed = true;
    } else if (slot == EDIT_INFO_AUTHOR) {
        st.cur->SetMetaAuthor(WideToUtf8(v)); changed = true;
    } else if (slot == EDIT_INFO_DESC) {
        st.cur->SetMetaDescription(WideToUtf8(v));
        st.notePath.clear();          // 让备注框下一帧重新载入
        changed = true;
    } else if (slot == EDIT_INFO_NOTE) {
        st.cur->SetMetaDescription(WideToUtf8(v));
        st.noteOrig = v;
        st.noteDirty = false;
        SaveCurrentMetaOnce(i18n::T("log.op.meta"));
        InvalidateRect(st.hwnd, nullptr, FALSE);
        return;
    } else if (slot == EDIT_NOTE) {
        SetNoteFor(st.listSel, v);
        InvalidateRect(st.hwnd, nullptr, FALSE);
        return;
    } else if (slot == EDIT_INFO_FILE) {
        std::wstring base = TrimW(v);
        size_t dot = base.rfind(L'.');
        if (dot != std::wstring::npos && EqualsNoCase(base.substr(dot), L".litematic"))
            base = base.substr(0, dot);
        if (!RenameCurrentFile(base, err)) {
            SetStatus(L"\u91CD\u547D\u540D\u5931\u8D25: " + Utf8ToWide(err));
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        }
        SetStatus(L"\u5DF2\u91CD\u547D\u540D");
        InvalidateRect(st.hwnd, nullptr, FALSE);
        return;
    }
    if (changed) SaveCurrentWithBackup(i18n::T("log.op.meta"));
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void EnsureEditsFor(HWND parent) { g_editParent = parent; EnsureEdits(parent); }

void EnsureEdits(HWND parent) {
    State& st = Get();
    if (!parent) parent = st.hwnd;
    for (int i = 0; i < EDIT_SLOT_COUNT; ++i) {
        // 输入框必须属于"当前正在绘制的那个窗口"。父窗口变了就重建，
        // 否则会留下挂在已销毁弹窗上的孤儿控件，导致点击无反应。
        if (g_edits[i] && GetParent(g_edits[i]) != parent) {
            DestroyWindow(g_edits[i]);
            g_edits[i] = nullptr;
            g_oldProc[i] = nullptr;
            g_editInit[i] = false;
            g_editLast[i] = { 0, 0, 0, 0 };
        }
        if (g_editWant[i]) {
            if (!g_edits[i]) g_edits[i] = MakeEdit(parent,
                (i == EDIT_INFO_NOTE) ? st.fonts.title : st.fonts.ui, i);
            if (!g_edits[i]) continue;
            const ui::Rect& p = g_editPos[i];
            if (p.w <= 4) { ShowWindow(g_edits[i], SW_HIDE); continue; }
            ShowWindow(g_edits[i], SW_SHOW);
            if (g_editLast[i].x != p.x || g_editLast[i].y != p.y ||
                g_editLast[i].w != p.w || g_editLast[i].h != p.h) {
                SetWindowPos(g_edits[i], HWND_TOP, p.x, p.y, p.w, p.h, SWP_NOACTIVATE);
                g_editLast[i] = p;
            }
            if (!g_editInit[i]) {
                // 首次出现时灌入初值
                if (i == EDIT_SEARCHBOX) SetEditTextNow(g_edits[i], st.searchText);
                else if (i == EDIT_REP_SEARCH) SetEditTextNow(g_edits[i],
                        st.modal == MODAL_CONVERT ? st.cvSearch
                        : st.modal == MODAL_BLOCKPICK ? st.bpSearch
                        : (st.modal == MODAL_PACKS ? L"" : (st.blockPickOpen ? st.blockPickSearch : st.repSearch)));
                else if (i == EDIT_META_NAME) SetEditTextNow(g_edits[i], st.metaName);
                else if (i == EDIT_META_AUTHOR) SetEditTextNow(g_edits[i], st.metaAuthor);
                else if (i == EDIT_META_DESC) SetEditTextNow(g_edits[i], st.metaDesc);
                else if (i == EDIT_NOTE) SetEditTextNow(g_edits[i], GetNoteFor(st.listSel));
                else if (i >= EDIT_INFO_FILE && st.cur) {
                    ltm::Summary sm = st.cur->GetSummary();
                    if (i == EDIT_INFO_FILE) SetEditTextNow(g_edits[i], sm.fileName);
                    else if (i == EDIT_INFO_NAME) SetEditTextNow(g_edits[i], Utf8ToWide(sm.name));
                    else if (i == EDIT_INFO_AUTHOR) SetEditTextNow(g_edits[i], Utf8ToWide(sm.author));
                    else if (i == EDIT_INFO_DESC) SetEditTextNow(g_edits[i], Utf8ToWide(sm.description));
                }
                g_editInit[i] = true;
            }
        } else if (g_edits[i]) {
            // 不再需要时销毁（但正在编辑的信息字段要保留到提交）
            if (i >= EDIT_INFO_FILE && st.infoEdit == i) continue;
            DestroyWindow(g_edits[i]);
            g_edits[i] = nullptr;
            g_oldProc[i] = nullptr;
            g_editInit[i] = false;
            g_editLast[i] = { 0, 0, 0, 0 };
        }
    }

    // ---- 备注框：文件切换时自动重载，内容变化时标记为待保存
    if (g_edits[EDIT_INFO_NOTE] && st.cur) {
        if (st.noteGen != st.curGen) {                      // 换了文件 -> 重新载入描述
            st.noteGen = st.curGen;
            ltm::Summary smn = st.cur->GetSummary();
            st.noteOrig = Utf8ToWide(smn.description);
            st.noteText = st.noteOrig;
            st.noteDirty = false;
            SetEditTextNow(g_edits[EDIT_INFO_NOTE], st.noteOrig);
            HideCaret(g_edits[EDIT_INFO_NOTE]);
        } else {
            wchar_t nb[4096] = L"";
            GetWindowTextW(g_edits[EDIT_INFO_NOTE], nb, 4096);
            st.noteText = nb;
            bool wasDirty = st.noteDirty;
            st.noteDirty = (st.noteText != st.noteOrig);
            if (st.noteDirty != wasDirty) InvalidateRect(st.hwnd, nullptr, FALSE);
        }
        // 点到备注框内却没拿到焦点时，主动把焦点给它（否则没有输入光标）
        if (st.input.mousePressed &&
            g_editPos[EDIT_INFO_NOTE].Contains(st.input.mouseX, st.input.mouseY) &&
            GetFocus() != g_edits[EDIT_INFO_NOTE])
            g_editFocusReq = EDIT_INFO_NOTE;
    }

    // 回读：搜索框
    if (g_edits[EDIT_SEARCHBOX]) {
        wchar_t buf[512] = L"";
        GetWindowTextW(g_edits[EDIT_SEARCHBOX], buf, 512);
        if (st.searchText != buf) { st.searchText = buf; UpdateSearch(); }
    }
    // 回读：方块筛选框
    if (g_edits[EDIT_REP_SEARCH]) {
        wchar_t buf[512] = L"";
        GetWindowTextW(g_edits[EDIT_REP_SEARCH], buf, 512);
        if (st.modal == MODAL_REPLACE) st.repSearch = buf;
        else if (st.modal == MODAL_BLOCKPICK) st.bpSearch = buf;   // 方块选择弹窗的搜索
        else if (st.modal == MODAL_CONVERT) st.cvSearch = buf;     // 版本转换弹窗的搜索

        else if (st.blockPickOpen) st.blockPickSearch = buf;
    }
    // 回读：元数据弹窗
    if (st.modal == MODAL_META) {
        wchar_t buf[4096] = L"";
        if (g_edits[EDIT_META_NAME]) { GetWindowTextW(g_edits[EDIT_META_NAME], buf, 4096); st.metaName = buf; }
        if (g_edits[EDIT_META_AUTHOR]) { GetWindowTextW(g_edits[EDIT_META_AUTHOR], buf, 4096); st.metaAuthor = buf; }
        if (g_edits[EDIT_META_DESC]) { GetWindowTextW(g_edits[EDIT_META_DESC], buf, 4096); st.metaDesc = buf; }
    }

    // 备注：鼠标点到框外时自动保存
    if (st.noteDirty && g_edits[EDIT_INFO_NOTE] && st.input.mousePressed &&
        !g_editPos[EDIT_INFO_NOTE].Contains(st.input.mouseX, st.input.mouseY) &&
        !g_editPos[EDIT_INFO_NOTE].Contains(st.input.mouseX, st.input.mouseY))
        CommitInfoEdit(EDIT_INFO_NOTE);

    if (g_editFocusReq >= 0 && g_editFocusReq < EDIT_SLOT_COUNT && g_edits[g_editFocusReq]) {
        HWND h = g_edits[g_editFocusReq];
        SetFocus(h);
        SendMessageW(h, EM_SETSEL, 0, -1);
        g_editFocusReq = -1;
    }
}

// ------------------------------------------------------------------ 命令执行
static void OpenModal(int m) {
    State& st = Get();
    st.modal = m;
    for (int i = 0; i < 4; ++i) g_editPos[i] = { 0, 0, 0, 0 };
    if (m == MODAL_META && st.cur) {
        ltm::Summary s = st.cur->GetSummary();
        st.metaName = Utf8ToWide(s.name);
        st.metaAuthor = Utf8ToWide(s.author);
        st.metaDesc = Utf8ToWide(s.description);
        for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editInit[i] = false;
    } else if (m == MODAL_BACKUP) {
        st.backups = backup::List();
        st.bkSel = -1;
        st.bkScroll = 0;
    } else if (m == MODAL_LOG) {
        st.logLines = backup::ReadLog(400);
        st.logScroll = 0;
    } else if (m == MODAL_DUP) {
        RunDuplicateScan();
    } else if (m == MODAL_REPLACE) {
        st.repActiveField = 0;
        st.repSearch.clear();
        st.repScrollFrom = st.repScrollTo = 0;
        st.repFromSel = st.repToSel = -1;
        for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editInit[i] = false;
    }
    if (!OpenModalHost(m)) { st.modal = MODAL_NONE; return; }
}

static void CloseModal() { CloseModalHost(); }

void ExecuteCommand(int cmd) {
    State& st = Get();
    switch (cmd) {
        case CMD_MIN: ShowWindow(st.hwnd, SW_MINIMIZE); return;
        case CMD_MAX: ShowWindow(st.hwnd, IsZoomed(st.hwnd) ? SW_RESTORE : SW_MAXIMIZE); return;
        case CMD_CLOSE: PostMessageW(st.hwnd, WM_CLOSE, 0, 0); return;

        case CMD_ADD_DIR: HandleWindowCommand(cmd); return;
        case CMD_RESCAN: RescanAsync(); return;
        case CMD_EXIT: PostMessageW(st.hwnd, WM_CLOSE, 0, 0); return;
        case CMD_OPEN_DIR:
            if (st.listSel >= 0 && (size_t)st.listSel < st.files.size()) {
                std::wstring d = st.files[(size_t)st.listSel].sum.dirName;
                ShellExecuteW(nullptr, L"open", d.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
            }
            return;
        case CMD_EXPORT: ExportPreviews(); return;
        case CMD_EXPORT_SIZE:
            st.exportMenuOpen = !st.exportMenuOpen;
            st.moreOpen = false; st.fmtPickOpen = false;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;

        case CMD_EDIT_META:
            if (!st.cur) { SetStatus(i18n::T("preview.none")); InvalidateRect(st.hwnd, nullptr, FALSE); return; }
            OpenModal(MODAL_META); return;
        case CMD_EDIT_VERSION:
            if (!st.cur) return;
            OpenModal(MODAL_VERSION); return;
        case CMD_EDIT_REPLACE: OpenModal(MODAL_REPLACE); return;
        case CMD_EDIT_PREVIEW: {
            if (!st.cur) return;
            if (st.preview.px.empty()) { SetStatus(i18n::T("preview.failed")); InvalidateRect(st.hwnd, nullptr, FALSE); return; }
            // 写入 PreviewImageData（ARGB int 数组）
            nbt::Value* md = st.cur->Root().find("Metadata");
            if (!md) { st.cur->Root().set("Metadata", nbt::Value::MakeCompound()); md = st.cur->Root().find("Metadata"); }
            std::vector<i32> arr;
            arr.reserve((size_t)st.preview.w * st.preview.h);
            for (u32 c : st.preview.px) arr.push_back((i32)c);
            md->set("PreviewImageData", nbt::Value::MakeIntArray(std::move(arr)));
            st.cur->MarkModified();
            SaveCurrentWithBackup(i18n::T("log.op.preview"));
            return;
        }
        case CMD_EDIT_REVERT:
            if (st.cur && st.listSel >= 0) { OpenFile(st.listSel); SetStatus(L"已放弃修改"); }
            return;

        case CMD_PACKS_OPEN: OpenModal(MODAL_PACKS); if (st.detected.empty()) StartPackDetect(false); return;
        case CMD_PACKS_ADDROOT: HandleWindowCommand(cmd); StartPackDetect(false); return;
        case CMD_PACKS_IMPORT_ALL: {
            std::set<i32> have;
            for (size_t i = 0; i < st.db.VersionCount(); ++i)
                if (!st.db.Version(i).isOverlay) have.insert(st.db.Version(i).dataVersion);
            int n = 0;
            for (const auto& ins : st.detected) {
                if (have.count(ins.dataVersion)) continue;
                std::string err; packbuild::Result inf;
                if (packmgr::ImportJar(ins.jarPath, 1, ins.dataVersion, ins.versionId, err, &inf)) ++n;
            }
            st.detectMsg = FmtW(L"\u5df2\u5bfc\u5165 %d \u4e2a\u7248\u672c", n);
            ReloadPacks();
            return;
        }
        // ---- 搜索
        case CMD_SEARCH_FOCUS:
            g_editFocusReq = EDIT_SEARCHBOX;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        case CMD_SEARCH_NEXT: SearchNext(+1); return;
        case CMD_SEARCH_PREV: SearchNext(-1); return;
        case CMD_SEARCH_CLEAR:
            st.searchText.clear();
            st.searchHits.clear();
            st.searchCur = -1;
            if (g_edits[EDIT_SEARCHBOX]) SetWindowTextW(g_edits[EDIT_SEARCHBOX], L"");
            SetStatus(i18n::T("status.ready"));
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;

        // ---- 方块就地替换
        case CMD_BLK_PICK:
            OpenBlockPickModal();
            return;
        case CMD_CONVERT:
            OpenConvertModal();
            return;
        case CMD_BLK_CLEAR:
            st.blockSel = -1;
            st.blockToName.clear();
            st.blockPickOpen = false;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        case CMD_BLK_DO: {
            if (!st.cur || st.blockFromName.empty() || st.blockToName.empty()) return;
            if (st.curVersionIdx >= 0) st.db.SetActiveVersion(st.curVersionIdx);
            ltm::ReplaceRule rule;
            rule.fromName = st.blockFromName;
            rule.toName = st.blockToName;
            rule.inheritProps = true;      // 楼梯等方向属性自动保留
            u64 n = st.cur->ReplaceBlocks(rule, st.dbLoaded ? &st.db : nullptr, 0);
            if (n) {
                SaveCurrentWithBackup(i18n::T("log.op.replace"));
                if (st.cur) { st.curAnalysis = st.cur->Analyze(); st.curAnalysisDone = true; }
                st.blockFromName = st.blockToName;    // 原方块已全部变成目标方块
                st.blockSel = -1;
                std::wstring info = LastSaveInfo();
                SetStatus(FmtW(L"\u5DF2\u66FF\u6362 %llu \u4E2A\u65B9\u5757", (unsigned long long)n)
                          + (info.empty() ? L"" : L"\u3000\u3000" + info));
            } else {
                SetStatus(L"\u6CA1\u6709\u627E\u5230\u53EF\u66FF\u6362\u7684\u65B9\u5757");
            }
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        }

        // ---- 目录
        case CMD_MANAGE_DIRS:
            st.dirDropOpen = true;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        case CMD_EDIT_RENAME:
            if (!st.cur) { SetStatus(i18n::T("preview.none")); InvalidateRect(st.hwnd, nullptr, FALSE); return; }
            st.tab = TAB_INFO;
            st.infoEdit = EDIT_INFO_FILE;
            g_editInit[EDIT_INFO_FILE] = false;
            g_editFocusReq = EDIT_INFO_FILE;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;

        // ---- 帮助
        case CMD_HELP_BILIBILI:
            ShellExecuteW(nullptr, L"open", L"https://space.bilibili.com/19428259",
                          nullptr, nullptr, SW_SHOWNORMAL);
            return;

        case CMD_SAVE:
            if (st.cur && st.cur->IsModified()) SaveCurrentWithBackup(L"manual");
            return;
        case CMD_REVERT:
            if (st.cur && st.listSel >= 0) { OpenFile(st.listSel); SetStatus(L"\u5df2\u64a4\u9500\u672a\u4fdd\u5b58\u7684\u4fee\u6539"); }
            return;
        case CMD_DELETE_SEL:    DeleteSelected(); return;
        case CMD_COPY_SEL:      CopySelectedToClipboard(); return;
        case CMD_MOVE_SEL:      MoveSelected(); return;
        case CMD_OPEN_SEL_DIR:  OpenSelectedFolder(); return;
        case CMD_MORE:          st.moreOpen = !st.moreOpen; st.fmtPickOpen = false;
                                InvalidateRect(st.hwnd, nullptr, FALSE); return;
        case CMD_VER_PICK:      st.tab = TAB_INFO; OpenModal(MODAL_VERSION); return;
        case CMD_FMT_PICK:      st.fmtPickOpen = !st.fmtPickOpen; st.moreOpen = false;
                                InvalidateRect(st.hwnd, nullptr, FALSE); return;
        case CMD_NOTE_EDIT:
            if (!st.cur) return;
            CommitInfoEdit(EDIT_INFO_NOTE);
            return;
        case CMD_DUP: {
            RunDuplicateScan();
            int n = (int)st.dupGroups.size();
            for (auto& f : st.files) f.checked = false;
            for (const auto& g : st.dupGroups)
                for (size_t k = 1; k < g.size(); ++k)
                    if ((size_t)g[k] < st.files.size()) st.files[(size_t)g[k]].checked = true;
            SetStatus(n ? FmtW(L"\u53d1\u73b0 %d \u7ec4\u91cd\u590d\uff0c\u5df2\u81ea\u52a8\u52fe\u9009\u591a\u4f59\u526f\u672c\uff0c\u53ef\u76f4\u63a5\u5220\u9664", n)
                        : std::wstring(L"\u6ca1\u6709\u53d1\u73b0\u91cd\u590d\u6587\u4ef6"));
            st.tab = st.tab;
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        }
        case CMD_DUP_KEEP:
            for (auto& f : st.files) f.checked = false;
            for (const auto& g : st.dupGroups)
                for (size_t k = 1; k < g.size(); ++k)
                    if ((size_t)g[k] < st.files.size()) st.files[(size_t)g[k]].checked = true;
            SetStatus(L"\u5df2\u52fe\u9009\u6bcf\u7ec4\u9664\u7b2c\u4e00\u4e2a\u4e4b\u5916\u7684\u526f\u672c");
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        case CMD_DUP_DELETE:
            DeleteSelected();
            return;

        case CMD_DUP_CLOSE:
            st.dupGroups.clear();
            for (auto& f : st.files) f.dupGroup = -1;
            SetStatus(i18n::T("status.ready"));
            InvalidateRect(st.hwnd, nullptr, FALSE);
            return;
        case CMD_DUP_MODE0: st.dupMode = 0; ExecuteCommand(CMD_DUP); return;
        case CMD_DUP_MODE1: st.dupMode = 1; ExecuteCommand(CMD_DUP); return;
        case CMD_BACKUP: OpenModal(MODAL_BACKUP); return;
        case CMD_LOG: OpenModal(MODAL_LOG); return;
        case CMD_ABOUT: case CMD_LIBINFO: OpenModal(MODAL_ABOUT); return;

        case CMD_VIEW_FRONT: SetViewPreset(render::VIEW_FRONT); return;
        case CMD_VIEW_LEFT:  SetViewPreset(render::VIEW_LEFT45); return;
        case CMD_VIEW_RIGHT: SetViewPreset(render::VIEW_RIGHT45); return;
        case CMD_VIEW_BACK:  SetViewPreset(render::VIEW_BACK); return;

        case CMD_CHECK_ALL: for (auto& f : st.files) f.checked = true; InvalidateRect(st.hwnd, nullptr, FALSE); return;
        case CMD_CHECK_NONE: for (auto& f : st.files) f.checked = false; InvalidateRect(st.hwnd, nullptr, FALSE); return;
        case CMD_CHECK_INVERT: for (auto& f : st.files) f.checked = !f.checked; InvalidateRect(st.hwnd, nullptr, FALSE); return;

        case CMD_MODAL_CANCEL: CloseModal(); return;
        default: break;
    }

    // 弹窗内部的按钮
    if (st.modal == MODAL_META && cmd == CMD_MODAL_OK) {
        st.cur->SetMetaName(WideToUtf8(st.metaName));
        st.cur->SetMetaAuthor(WideToUtf8(st.metaAuthor));
        st.cur->SetMetaDescription(WideToUtf8(st.metaDesc));
        CloseModal();
        SaveCurrentWithBackup(i18n::T("log.op.meta"));
        return;
    }
    if (st.modal == MODAL_VERSION && cmd == CMD_MODAL_OK) {
        if (st.cur) {
            i32 dv = -1;
            if (st.verUseCurrent) {
                // 设为当前游戏版本：取内置库里最新的 dataVersion
                if (st.dbLoaded && st.db.VersionCount() > 0) dv = st.db.Version(st.db.VersionCount() - 1).dataVersion;
            } else {
                const auto& known = ltm::KnownDataVersions();
                if (st.verSelected >= 0 && (size_t)st.verSelected < known.size())
                    dv = known[(size_t)st.verSelected].first;
            }
            if (dv > 0) {
                st.cur->SetDataVersion(dv);
                CloseModal();
                SaveCurrentWithBackup(i18n::T("log.op.version"));
            } else CloseModal();
        }
        return;
    }
    if (st.modal == MODAL_REPLACE && cmd == CMD_REP_DO) {
        if (st.repFromSel >= 0 && st.repToSel >= 0 && st.dbLoaded) {
            const auto& names = st.db.AllBlockNames();
            ltm::ReplaceRule rule;
            rule.fromName = names[(size_t)st.repFromSel];
            rule.toName = names[(size_t)st.repToSel];
            rule.inheritProps = st.repInherit;
            if (rule.fromName == rule.toName) { CloseModal(); return; }

            std::vector<int> targets;
            if (st.repScope == 1) { if (st.listSel >= 0) targets.push_back(st.listSel); }
            else for (size_t i = 0; i < st.files.size(); ++i) if (st.files[i].checked) targets.push_back((int)i);

            if (targets.empty()) {
                // 没有勾选就作用于当前文件
                if (st.listSel >= 0) targets.push_back(st.listSel);
            }
            CloseModal();
            u64 total = 0;
            int n = ApplyReplaceToFiles(targets, rule, total);
            SetStatus(FmtW(i18n::T("rep.done"), (unsigned long long)total));
            if (st.listSel >= 0) OpenFile(st.listSel);
            backup::Log(L"*", i18n::T("log.op.replace"),
                        FmtW(L"%d files, %llu blocks", n, (unsigned long long)total));
            InvalidateRect(st.hwnd, nullptr, FALSE);
        } else CloseModal();
        return;
    }
}

// ------------------------------------------------------------------ 弹窗绘制
static void DrawMetaModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("meta.title"));
    int x = r.x + 20, y = r.y + 52, w = r.w - 40;
    auto field = [&](const wchar_t* label, int slot) {
        c.p->Text(label, { x, y, 100, 24 }, Theme::TextDim, c.f->ui);
        g_editWant[slot] = true;
        g_editPos[slot] = { x + 104, y + 2, w - 104, 24 };
        c.p->Fill({ x + 104, y + 2, w - 104, 24 }, RGB(255, 255, 255));
        c.p->Frame({ x + 104, y + 2, w - 104, 24 }, Theme::Separator, 1);
        y += 34;
    };
    field(i18n::T("meta.name"), 0);
    field(i18n::T("meta.author"), 1);
    field(i18n::T("meta.desc"), 2);
    c.p->Text(i18n::T("meta.hint"), { x, y + 4, w, 22 }, Theme::TextDim, c.f->small);
    int by = r.Bottom() - 48;
    if (ui::Button(c, CMD_MODAL_OK, { r.Right() - 200, by, 88, 30 }, i18n::T("btn.save"), true, true))
        ExecuteCommand(CMD_MODAL_OK);
    if (ui::Button(c, CMD_MODAL_CANCEL, { r.Right() - 104, by, 88, 30 }, i18n::T("btn.cancel")))
        ExecuteCommand(CMD_MODAL_CANCEL);
}

static void DrawVersionModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("ver.title"));
    int x = r.x + 20, y = r.y + 50, w = r.w - 40;

    ltm::Summary s = st.cur ? st.cur->GetSummary() : ltm::Summary();
    c.p->Text(i18n::T("ver.current"), { x, y, 90, 22 }, Theme::TextDim, c.f->ui);
    c.p->Text(VersionLabel(s) + FmtW(L"   (dataVersion %d)", s.dataVersion),
              { x + 96, y, w - 96, 22 }, Theme::Accent, c.f->uiBold);
    y += 30;

    // 列表
    const auto& known = ltm::KnownDataVersions();
    ui::Rect list = { x, y, w, r.Bottom() - y - 175 };
    c.p->Fill(list, RGB(255, 255, 255));
    c.p->Frame(list, Theme::Separator, 1);
    ui::Rect inner = { list.x + 1, list.y + 1, list.w - 14, list.h - 2 };
    int rowH = 22;
    int total = (int)known.size() * rowH;
    int maxScroll = std::max(0, total - inner.h);
    if (st.verListScroll > maxScroll) st.verListScroll = maxScroll;
    c.p->PushClip(inner);
    int first = st.verListScroll / rowH;
    int last = std::min((int)known.size(), first + inner.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        const auto& kv = known[(size_t)i];
        ui::Rect row = { inner.x, inner.y + i * rowH - st.verListScroll, inner.w, rowH };
        bool sel = (!st.verUseCurrent && st.verSelected == i);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) c.p->Fill(row, Theme::BgListSel);
        else if (hov) c.p->Fill(row, Theme::BgHover);
        c.p->Text(Utf8ToWide(kv.second), { row.x + 10, row.y, 220, row.h }, Theme::Text, c.f->ui);
        c.p->Text(std::to_wstring(kv.first), { row.x + 240, row.y, 90, row.h }, Theme::TextDim, c.f->small);
        if (kv.first == s.dataVersion)
            c.p->Text(L"\u25C0 当前", { row.Right() - 90, row.y, 80, row.h }, Theme::Accent, c.f->small,
                      DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) { st.verSelected = i; st.verUseCurrent = false; }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    c.p->PopClip();
    ui::ScrollBar(c, CMD_NONE + 2100, { inner.Right() + 2, inner.y, 12, inner.h }, st.verListScroll, total, inner.h);

    y = list.Bottom() + 8;
    ui::Rect cb = { x, y, w, 22 };
    c.p->Text(L"", cb, Theme::Text, c.f->ui);
    if (ui::Button(c, CMD_NONE + 2101, { x, y, w, 24 },
                   i18n::T("ver.setcur"), true, st.verUseCurrent)) {
        st.verUseCurrent = true;
    }
    y += 30;
    c.p->Text(i18n::T("ver.warn"), { x, y, w, r.Bottom() - y - 52 }, Theme::Danger, c.f->small,
              DT_LEFT | DT_TOP | DT_WORDBREAK);

    if (ui::Button(c, CMD_MODAL_OK, { r.Right() - 200, r.Bottom() - 44, 88, 30 }, i18n::T("btn.apply"), true, true))
        ExecuteCommand(CMD_MODAL_OK);
    if (ui::Button(c, CMD_MODAL_CANCEL, { r.Right() - 104, r.Bottom() - 44, 88, 30 }, i18n::T("btn.cancel")))
        ExecuteCommand(CMD_MODAL_CANCEL);
}

static void DrawReplaceModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("rep.title"));
    int x = r.x + 20, y = r.y + 50, w = r.w - 40;
    static const std::vector<std::string> kNoNames;
    const std::vector<std::string>& names = st.dbLoaded ? st.db.AllBlockNames() : kNoNames;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);

    auto slotBox = [&](const wchar_t* label, int sel, int field, int yy) {
        c.p->Text(label, { x, yy, 80, 28 }, Theme::TextDim, c.f->ui);
        ui::Rect box = { x + 84, yy, w - 84, 28 };
        bool active = (st.repActiveField == field);
        c.p->Fill(box, RGB(255, 255, 255));
        c.p->Frame(box, active ? Theme::Accent : Theme::Separator, active ? 2 : 1);
        if (sel >= 0 && (size_t)sel < names.size()) {
            const std::string& bn = names[(size_t)sel];
            const render::Image* ic = GetBlockIcon(bn, 20);
            if (ic && !ic->px.empty()) c.p->Image(*ic, { box.x + 4, box.y + 4, 20, 20 }, true);
            else c.p->Fill({ box.x + 4, box.y + 4, 20, 20 }, RGB(0x18, 0x00, 0x18));
            c.p->Text(BlockDisplay(bn, chinese), { box.x + 30, box.y, box.w - 200, box.h }, Theme::Text, c.f->ui);
            c.p->Text(Utf8ToWide(bn), { box.Right() - 200, box.y, 190, box.h }, Theme::TextDim, c.f->small,
                      DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        } else {
            c.p->Text(L"—", { box.x + 8, box.y, box.w - 16, box.h }, Theme::TextDim, c.f->ui);
        }
        if (box.Contains(c.in->mouseX, c.in->mouseY) && c.in->mousePressed) {
            st.repActiveField = field;
            c.in->usedMouse = true;
        }
    };
    slotBox(i18n::T("rep.from"), st.repFromSel, 0, y);
    y += 34;
    slotBox(i18n::T("rep.to"), st.repToSel, 1, y);
    y += 36;

    c.p->Text(i18n::T("rep.search"), { x, y, 74, 24 }, Theme::TextDim, c.f->small);
    g_editPos[3] = { x + 78, y + 1, w - 78, 24 };
    if (st.editRepSearch) {
        wchar_t buf[512];
        GetWindowTextW(st.editRepSearch, buf, 512);
        if (st.repSearch != buf) { st.repSearch = buf; st.repScrollFrom = st.repScrollTo = 0; }
    }
    y += 30;

    // 过滤后的列表
    static std::vector<int> filtered;
    static std::wstring lastSearch;
    static int lastVer = -99;
    if (st.repSearch != lastSearch || st.curVersionIdx != lastVer) {
        filtered.clear();
        std::wstring q = ToLowerW(st.repSearch);
        for (size_t i = 0; i < names.size(); ++i) {
            std::wstring dn = BlockDisplay(names[i], true) + L" " + Utf8ToWide(names[i]);
            if (q.empty() || ToLowerW(dn).find(q) != std::wstring::npos) filtered.push_back((int)i);
        }
        lastSearch = st.repSearch;
        lastVer = st.curVersionIdx;
    }

    ui::Rect list = { x, y, w, r.Bottom() - y - 118 };
    c.p->Fill(list, RGB(255, 255, 255));
    c.p->Frame(list, Theme::Separator, 1);
    ui::Rect inner = { list.x + 1, list.y + 1, list.w - 15, list.h - 2 };
    int rowH = 24;
    int total = (int)filtered.size() * rowH;
    int& scroll = (st.repActiveField == 0) ? st.repScrollFrom : st.repScrollTo;
    int maxScroll = std::max(0, total - inner.h);
    if (scroll > maxScroll) scroll = maxScroll;
    c.p->PushClip(inner);
    int first = scroll / rowH;
    int last = std::min((int)filtered.size(), first + inner.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        int bi = filtered[(size_t)i];
        const std::string& bn = names[(size_t)bi];
        ui::Rect row = { inner.x, inner.y + i * rowH - scroll, inner.w, rowH };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        bool isFrom = (st.repFromSel == bi), isTo = (st.repToSel == bi);
        if (isFrom || isTo) c.p->Fill(row, Theme::BgListSel);
        else if (hov) c.p->Fill(row, Theme::BgHover);
        const render::Image* ic = GetBlockIcon(bn, 18);
        if (ic && !ic->px.empty()) c.p->Image(*ic, { row.x + 3, row.y + 3, 18, 18 }, true);
        else c.p->Fill({ row.x + 3, row.y + 3, 18, 18 }, RGB(0x18, 0x00, 0x18));
        c.p->Text(BlockDisplay(bn, chinese), { row.x + 26, row.y, 200, row.h }, Theme::Text, c.f->small);
        c.p->Text(Utf8ToWide(bn), { row.x + 226, row.y, row.w - 320, row.h }, Theme::TextDim, c.f->small,
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        bool mod = IsModBlock(bn);
        c.p->Text(mod ? i18n::T("rep.source.mod") : i18n::T("rep.source.vanilla"),
                  { row.Right() - 90, row.y, 84, row.h }, mod ? Theme::Danger : Theme::TextDim, c.f->small,
                  DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            if (st.repActiveField == 0) st.repFromSel = bi; else st.repToSel = bi;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    c.p->PopClip();
    ui::ScrollBar(c, CMD_NONE + 2200, { inner.Right() + 2, inner.y, 12, inner.h }, scroll, total, inner.h);

    y = list.Bottom() + 8;
    ui::Rect inh = { x, y, 300, 22 };
    ui::Checkbox(c, CMD_NONE + 2201, inh, st.repInherit);
    c.p->Text(i18n::T("rep.inherit"), { inh.x + 20, inh.y, inh.w - 20, inh.h }, Theme::Text, c.f->small);

    y += 26;
    c.p->Text(i18n::T("rep.scope"), { x, y, 70, 22 }, Theme::TextDim, c.f->small);
    ui::Rect r0 = { x + 74, y, 150, 22 }, r1 = { x + 234, y, 150, 22 };
    if (ui::Button(c, CMD_NONE + 2202, r0, i18n::T("rep.scope.sel"), true, st.repScope == 0)) st.repScope = 0;
    if (ui::Button(c, CMD_NONE + 2203, r1, i18n::T("rep.scope.cur"), true, st.repScope == 1)) st.repScope = 1;

    if (ui::Button(c, CMD_REP_DO, { r.Right() - 200, r.Bottom() - 44, 88, 30 }, i18n::T("btn.replace"), true, true))
        ExecuteCommand(CMD_REP_DO);
    if (ui::Button(c, CMD_MODAL_CANCEL, { r.Right() - 104, r.Bottom() - 44, 88, 30 }, i18n::T("btn.cancel")))
        ExecuteCommand(CMD_MODAL_CANCEL);
}

static void DrawBackupModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("bk.title"));
    int x = r.x + 20, y = r.y + 50, w = r.w - 40;

    ui::Rect list = { x, y, w, r.Bottom() - y - 60 };
    c.p->Fill(list, RGB(255, 255, 255));
    c.p->Frame(list, Theme::Separator, 1);
    ui::Rect inner = { list.x + 1, list.y + 1, list.w - 15, list.h - 2 };
    int rowH = 34;
    int total = (int)st.backups.size() * rowH;
    int maxScroll = std::max(0, total - inner.h);
    if (st.bkScroll > maxScroll) st.bkScroll = maxScroll;
    if (st.backups.empty()) {
        c.p->Text(i18n::T("bk.empty"), inner, Theme::TextDim, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
    c.p->PushClip(inner);
    int first = st.bkScroll / rowH;
    int last = std::min((int)st.backups.size(), first + inner.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        const backup::Entry& e = st.backups[(size_t)i];
        ui::Rect row = { inner.x, inner.y + i * rowH - st.bkScroll, inner.w, rowH };
        bool sel = (st.bkSel == i);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) c.p->Fill(row, Theme::BgListSel);
        else if (hov) c.p->Fill(row, Theme::BgHover);
        c.p->Text(e.origName, { row.x + 8, row.y + 2, 260, 16 }, Theme::Text, c.f->ui);
        c.p->Text(e.op, { row.x + 274, row.y + 2, 120, 16 }, Theme::TextDim, c.f->small);
        c.p->Text(FmtTime(e.timeMs), { row.x + 8, row.y + 17, 260, 16 }, Theme::TextDim, c.f->small);
        c.p->Text(FormatBytes(e.size), { row.Right() - 80, row.y + 2, 72, 16 }, Theme::TextDim, c.f->small,
                  DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) st.bkSel = i;
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    c.p->PopClip();
    ui::ScrollBar(c, CMD_NONE + 2300, { inner.Right() + 2, inner.y, 12, inner.h }, st.bkScroll, total, inner.h);

    int by = r.Bottom() - 46;
    bool has = (st.bkSel >= 0 && (size_t)st.bkSel < st.backups.size());
    if (ui::Button(c, CMD_BK_RESTORE, { r.Right() - 320, by, 96, 30 }, i18n::T("btn.restore"), has)) {
        if (has) {
            std::string err;
            if (backup::Restore(st.backups[(size_t)st.bkSel], err)) {
                backup::Log(st.backups[(size_t)st.bkSel].origPath, i18n::T("log.op.restore"), L"ok");
                SetStatus(L"已还原");
                RescanAsync();
                CloseModal();
            } else SetStatus(Utf8ToWide(err));
        }
    }
    if (ui::Button(c, CMD_BK_DELETE, { r.Right() - 218, by, 96, 30 }, i18n::T("btn.delete"), has)) {
        if (has) { std::string err; backup::DeleteBackup(st.backups[(size_t)st.bkSel], err); st.backups = backup::List(); st.bkSel = -1; }
    }
    if (ui::Button(c, CMD_BK_REFRESH, { r.Right() - 116, by, 96, 30 }, i18n::T("btn.refresh"))) {
        st.backups = backup::List(); st.bkSel = -1; st.bkScroll = 0;
    }
}

static void DrawDupModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("dup.title"));
    int x = r.x + 20, y = r.y + 50, w = r.w - 40;

    c.p->Text(i18n::T("dup.mode"), { x, y, 80, 24 }, Theme::TextDim, c.f->small);
    if (ui::Button(c, CMD_NONE + 2400, { x + 80, y, 180, 24 }, i18n::T("dup.mode.hash"), true, st.dupMode == 0)) {
        st.dupMode = 0; RunDuplicateScan();
    }
    if (ui::Button(c, CMD_NONE + 2401, { x + 266, y, 200, 24 }, i18n::T("dup.mode.meta"), true, st.dupMode == 1)) {
        st.dupMode = 1; RunDuplicateScan();
    }
    y += 32;

    ui::Rect list = { x, y, w, r.Bottom() - y - 60 };
    c.p->Fill(list, RGB(255, 255, 255));
    c.p->Frame(list, Theme::Separator, 1);
    if (st.dupGroups.empty()) {
        c.p->Text(i18n::T("dup.none"), list, Theme::TextDim, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    } else {
        ui::Rect inner = { list.x + 1, list.y + 1, list.w - 15, list.h - 2 };
        c.p->PushClip(inner);
        int y2 = inner.y - st.dupScroll;
        for (size_t g = 0; g < st.dupGroups.size(); ++g) {
            if (y2 > inner.Bottom()) break;
            ui::Rect gh = { inner.x, y2, inner.w, 22 };
            if (y2 + 22 > inner.y) {
                c.p->Fill(gh, Theme::BgHeader);
                c.p->Text(FmtW(L"\u7EC4 %zu  (%zu \u4E2A\u6587\u4EF6)", g + 1, st.dupGroups[g].size()),
                          { gh.x + 8, gh.y, gh.w - 8, gh.h }, Theme::Text, c.f->smallBold);
            }
            y2 += 22;
            for (int idx : st.dupGroups[g]) {
                if (y2 > inner.Bottom()) break;
                if (idx < 0 || (size_t)idx >= st.files.size()) continue;
                FileItem& fi = st.files[(size_t)idx];
                ui::Rect row = { inner.x, y2, inner.w, 22 };
                if (y2 + 22 > inner.y) {
                    bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
                    if (hov) c.p->Fill(row, Theme::BgHover);
                    ui::Rect cbr = { row.x + 20, row.y + 3, 16, 16 };
                    ui::Checkbox(c, ui::Id("dupchk", idx), cbr, fi.checked);
                    c.p->Text(fi.sum.fileName, { row.x + 42, row.y, 280, row.h }, Theme::Text, c.f->small);
                    c.p->Text(fi.sum.dirName, { row.x + 330, row.y, row.w - 340, row.h }, Theme::TextDim, c.f->small,
                              DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);
                    if (hov && c.in->mouseReleased && !c.in->usedMouse) st.dupSel = idx;
                }
                y2 += 22;
            }
            y2 += 6;
        }
        int totalH = (int)(y2 + st.dupScroll - inner.y);
        c.p->PopClip();
        ui::ScrollBar(c, CMD_NONE + 2500, { inner.Right() + 2, inner.y, 12, inner.h },
                      st.dupScroll, totalH, inner.h);
    }

    int by = r.Bottom() - 46;
    if (ui::Button(c, CMD_DUP_KEEP, { r.Right() - 330, by, 140, 30 }, i18n::T("dup.keepfirst"))) {
        for (size_t g = 0; g < st.dupGroups.size(); ++g)
            for (size_t k = 1; k < st.dupGroups[g].size(); ++k)
                st.files[(size_t)st.dupGroups[g][k]].checked = true;
        for (size_t g = 0; g < st.dupGroups.size(); ++g)
            if (!st.dupGroups[g].empty()) st.files[(size_t)st.dupGroups[g][0]].checked = false;
    }
    if (ui::Button(c, CMD_DUP_DELETE, { r.Right() - 184, by, 140, 30 }, i18n::T("dup.deletemarked"), true, true)) {
        int n = 0;
        for (size_t i = 0; i < st.files.size(); ++i) {
            if (!st.files[i].checked) continue;
            std::wstring p = st.files[i].sum.path;
            std::wstring bp; std::string err;
            backup::CreateBackup(p, i18n::T("log.op.delete"), bp, err);
            if (fsx::DeleteFileAt(p)) { ++n; backup::Log(p, i18n::T("log.op.delete"), L"ok"); }
        }
        SetStatus(FmtW(L"\u5DF2\u5220\u9664 %d \u4E2A\u6587\u4EF6", n));
        RescanAsync();
        CloseModal();
    }
}

static void DrawLogModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("log.title"));
    int x = r.x + 20, y = r.y + 50, w = r.w - 40;
    ui::Rect list = { x, y, w, r.Bottom() - y - 20 };
    c.p->Fill(list, RGB(255, 255, 255));
    c.p->Frame(list, Theme::Separator, 1);
    ui::Rect inner = { list.x + 8, list.y + 6, list.w - 20, list.h - 12 };
    int rowH = 20;
    int total = (int)st.logLines.size() * rowH;
    int maxScroll = std::max(0, total - inner.h);
    if (st.logScroll > maxScroll) st.logScroll = maxScroll;
    if (st.logScroll < 0) st.logScroll = 0;
    c.p->PushClip(inner);
    int first = st.logScroll / rowH;
    int last = std::min((int)st.logLines.size(), first + inner.h / rowH + 2);
    for (int i = first; i < last; ++i) {
        ui::Rect row = { inner.x, inner.y + i * rowH - st.logScroll, inner.w, rowH };
        c.p->Text(ReplaceAllW(st.logLines[(size_t)i], L"\t", L"   "), row, Theme::Text, c.f->small);
    }
    c.p->PopClip();
    ui::ScrollBar(c, CMD_NONE + 2600, { inner.Right() + 2, inner.y, 12, inner.h }, st.logScroll, total, inner.h);
    if (c.in->wheel) {
        st.logScroll = std::max(0, std::min(maxScroll, st.logScroll - c.in->wheel * 60));
    }
}

static void DrawAboutModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ModalFrame(c, r, i18n::T("about.title"));
    int x = r.x + 26, y = r.y + 60, w = r.w - 52;

    c.p->Text(i18n::T("app.title"), { x, y, w, 30 }, Theme::Text, c.f->title,
              DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    y += 32;
    c.p->Text(L"\u7248\u672C\u53F7 1.0.0", { x, y, w, 22 }, Theme::TextDim, c.f->ui);
    y += 26;
    c.p->Text(L"\u4F5C\u8005\uFF1A\u9ED1\u5C71\u5927\u53D4", { x, y, w, 24 }, Theme::Text, c.f->uiBold);
    y += 30;
    c.p->Text(L"\u8BBF\u95EE\u9875\u9762\uFF1A", { x, y, 90, 26 }, Theme::TextDim, c.f->ui);
    if (ui::Button(c, CMD_HELP_BILIBILI, { x + 92, y, 300, 26 },
                   L"https://space.bilibili.com/19428259")) {
        ShellExecuteW(nullptr, L"open", L"https://space.bilibili.com/19428259", nullptr, nullptr, SW_SHOWNORMAL);
    }
    y += 36;
    {
        ui::Rect box = { x, y, w, 52 };
        c.p->Fill(box, RGB(0xFF, 0xF4, 0xE5));
        c.p->Frame(box, RGB(0xE0, 0xA0, 0x40), 1);
        c.p->Text(L"\u672C\u7A0B\u5E8F\u4EC5\u9650\u4E8E\u514D\u8D39\u5206\u4EAB\uFF0C\u7981\u6B62\u4EFB\u4F55\u9500\u552E\u884C\u4E3A\u3002",
                  { box.x + 12, box.y, box.w - 24, box.h }, RGB(0xB0, 0x50, 0x00), c.f->uiBold,
                  DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    y += 62;
    c.p->HLine(x, y, w, Theme::Separator);
    y += 12;

    c.p->Text(i18n::T("about.lib"), { x, y, w, 22 }, Theme::Text, c.f->uiBold);
    y += 24;
    if (st.dbLoaded) {
        size_t blocks = 0, overlays = 0;
        for (const auto& v : st.db.Versions()) {
            blocks += v.blocks.size();
            if (v.isOverlay) ++overlays;
        }
        c.p->Text(FmtW(L"\u5171 %zu \u4E2A\u6E38\u620F\u7248\u672C\uFF0C%zu \u6761\u65B9\u5757\u5B9A\u4E49\uFF0C%zu \u4E2A\u6A21\u7EC4\u53E0\u52A0\u5305\u3002\u5B8C\u5168\u79BB\u7EBF\uFF0C\u65E0\u9700\u5B89\u88C5 Minecraft\u3002",
                       st.db.VersionCount() - overlays, blocks, overlays),
                  { x, y, w, 40 }, Theme::TextDim, c.f->small, DT_LEFT | DT_TOP | DT_WORDBREAK);
        y += 44;
        std::wstring vers;
        for (const auto& v : st.db.Versions()) {
            if (v.isOverlay) continue;
            if (!vers.empty()) vers += L", ";
            vers += Utf8ToWide(v.id);
        }
        c.p->Text(vers, { x, y, w, r.Bottom() - y - 58 }, Theme::TextDim, c.f->small,
                  DT_LEFT | DT_TOP | DT_WORDBREAK);
    } else {
        c.p->Text(i18n::T("err.noaassets"), { x, y, w, 40 }, Theme::Danger, c.f->ui,
                  DT_LEFT | DT_TOP | DT_WORDBREAK);
    }

    if (ui::Button(c, CMD_MODAL_CANCEL, { r.Right() - 118, r.Bottom() - 44, 96, 30 },
                   i18n::T("btn.close")))
        ExecuteCommand(CMD_MODAL_CANCEL);
    (void)st;
}

void DrawModalIn(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editPos[i] = { 0, 0, 0, 0 };
    switch (st.modal) {
        case MODAL_META:    DrawMetaModal(c, r); break;
        case MODAL_VERSION: DrawVersionModal(c, r); break;
        case MODAL_REPLACE: DrawReplaceModal(c, r); break;
        case MODAL_BACKUP:  DrawBackupModal(c, r); break;
        case MODAL_LOG:     DrawLogModal(c, r); break;
        case MODAL_ABOUT:   DrawAboutModal(c, r); break;
        case MODAL_PACKS:   DrawPackModal(c, r); break;
        case MODAL_BLOCKPICK: DrawBlockPickModal(c, r); break;
        case MODAL_CONVERT:   DrawConvertModal(c, r); break;
        default: break;
    }
}

// ------------------------------------------------------------------ 输入
void OnMouseMove(int x, int y) {
    State& st = Get();
    st.input.mouseX = x;
    st.input.mouseY = y;
    st.mouseInWindow = true;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void OnMouseLeave() {
    State& st = Get();
    st.mouseInWindow = false;
    st.input.mouseX = -10000;
    st.input.mouseY = -10000;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void OnLButtonDown(int x, int y, bool dbl) {
    State& st = Get();
    st.input.mouseX = x; st.input.mouseY = y;
    st.input.mouseDown = true;
    st.input.mousePressed = true;
    if (dbl) st.input.mouseDoubleClick = true;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void OnLButtonUp(int x, int y) {
    State& st = Get();
    st.input.mouseX = x; st.input.mouseY = y;
    st.input.mouseDown = false;
    st.input.mouseReleased = true;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

void OnRButtonDown(int x, int y) {
    State& st = Get();
    st.input.mouseX = x; st.input.mouseY = y;
    // 右键：在文件列表上弹出快捷操作
    Layout L = ComputeLayout();
    if (L.left.Contains(x, y) && st.listSel >= 0) ExecuteCommand(CMD_OPEN_DIR);
}

void OnMouseWheel(int delta, int x, int y) {
    State& st = Get();
    st.input.mouseX = x; st.input.mouseY = y;
    st.input.wheel = delta / WHEEL_DELTA;
    Layout L = ComputeLayout();
    if (st.modal != MODAL_NONE) { InvalidateRect(st.hwnd, nullptr, FALSE); return; }
    if (L.left.Contains(x, y)) {
        int bodyH = L.left.h - HEADER_H;
        int total = (int)st.files.size() * ROW_H;
        st.listScroll -= st.input.wheel * 60;
        if (st.listScroll < 0) st.listScroll = 0;
        if (st.listScroll > std::max(0, total - bodyH)) st.listScroll = std::max(0, total - bodyH);
        st.input.wheel = 0;
    } else if (L.right.Contains(x, y)) {
        if (st.tab == TAB_BLOCKS) {
            st.blockScroll -= st.input.wheel * 60;
            if (st.blockScroll < 0) st.blockScroll = 0;
        } else if (st.tab == TAB_INFO) {
            st.infoScroll -= st.input.wheel * 40;
            if (st.infoScroll < 0) st.infoScroll = 0;
        }
        st.input.wheel = 0;
    }
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

bool OnKeyDown(UINT vk, bool ctrl, bool shift) {
    State& st = Get();
    if (vk == VK_ESCAPE) {
        if (st.modal != MODAL_NONE) { CloseModal(); return true; }
        if (g_openMenu >= 0) { g_openMenu = -1; InvalidateRect(st.hwnd, nullptr, FALSE); return true; }
        return false;
    }
    if (vk == VK_F5) { RescanAsync(); return true; }
    if (vk == VK_F3) { SearchNext(shift ? -1 : +1); return true; }
    if (ctrl && vk == 'F') { ExecuteCommand(CMD_SEARCH_FOCUS); return true; }
    if (st.infoEdit != -1 && vk == VK_ESCAPE) { st.infoEdit = -1; InvalidateRect(st.hwnd, nullptr, FALSE); return true; }
    if (st.blockPickOpen && vk == VK_ESCAPE) { st.blockPickOpen = false; InvalidateRect(st.hwnd, nullptr, FALSE); return true; }
    if (st.dirDropOpen && vk == VK_ESCAPE) { st.dirDropOpen = false; InvalidateRect(st.hwnd, nullptr, FALSE); return true; }
    if (ctrl && vk == 'A') { ExecuteCommand(CMD_CHECK_ALL); return true; }
    if (ctrl && vk == 'O') { ExecuteCommand(CMD_ADD_DIR); return true; }
    if (ctrl && vk == 'S') { if (st.cur && st.cur->IsModified()) SaveCurrentWithBackup(L"manual"); return true; }
    if (st.modal == MODAL_NONE) {
        if (vk == VK_DOWN && st.listSel + 1 < (int)st.files.size()) { st.listSel++; OpenFile(st.listSel); return true; }
        if (vk == VK_UP && st.listSel > 0) { st.listSel--; OpenFile(st.listSel); return true; }
        if (vk == VK_SPACE && st.listSel >= 0) { st.files[(size_t)st.listSel].checked = !st.files[(size_t)st.listSel].checked; return true; }
    }
    return false;
}

void OnChar(wchar_t ch) { (void)ch; }

// ------------------------------------------------------------------ 绘制入口
void Paint(HDC dc) {
    State& st = Get();
    if (st.width <= 0 || st.height <= 0) return;
    for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editWant[i] = false;
    st.painter.Begin(dc, st.width, st.height);

    ui::Ctx c{ &st.painter, &st.fonts, &st.input, true, st.dpi };

    // ---- 关键：弹窗打开时冻结主界面输入，否则主界面控件会在同一帧里抢走点击，
    //      表现为"弹窗按钮点不动、点下去操作的是主页面"。
    const bool modalOpen = (st.modal != MODAL_NONE);
    ui::Input frozen;
    if (modalOpen) {
        frozen = st.input;
        frozen.mousePressed = false;
        frozen.mouseReleased = false;
        frozen.mouseDown = false;
        frozen.clickedId = 0;
        frozen.doubleClickedId = 0;
        frozen.wheel = 0;
        frozen.activeId = 0;
        frozen.usedMouse = true;         // 主界面所有控件都不再响应
    }
    c.in = modalOpen ? &frozen : &st.input;

    st.painter.Fill({ 0, 0, st.width, st.height }, Theme::BgWindow);
    Layout L = ComputeLayout();

    DrawTitleBar(c, L);
    DrawMenuBar(c, L);
    DrawToolbar(c, L);
    DrawList(c, L);
    DrawDetail(c, L);
    DrawStatusBar(c, L);
    HandleLayoutDrag(c, L);
    if (st.dirDropOpen) DrawDirDropdown(c, L);

    // ---- 弹窗已经是独立窗口，这里不再绘制
    c.in = &st.input;
    // 注意：这里刻意不做"点击外部关闭"的判定。
    // 弹层的关闭只由显式操作触发（✕ / Esc / 选中方块 / 再点"替换为"框），
    // 避免任何基于鼠标位置或时序的隐式判定把弹层误关掉。
    if (st.blockPickOpen && !modalOpen) DrawBlockPickPopup(c, L);
    if (st.moreOpen && !modalOpen) DrawMoreMenu(c, L);
    if (st.fmtPickOpen && !modalOpen) DrawFormatPopup(c, L);
    if (st.exportMenuOpen && !modalOpen) DrawExportMenu(c, L);

    st.painter.Frame({ 0, 0, st.width, st.height }, Theme::Border, 2);
    st.painter.End();

    if (!modalOpen) EnsureEdits(st.hwnd);
    st.blockPickJustOpened = false;

    // 点击别处 = 提交内联编辑
    if (st.infoEdit != -1 && st.input.mousePressed && !st.input.usedMouse)
        CommitInfoEdit(st.infoEdit);

    st.input.NewFrame();
}

void OnSize(int w, int h) {
    State& st = Get();
    st.width = w;
    st.height = h;
    for (int i = 0; i < 4; ++i) g_editPos[i] = { 0, 0, 0, 0 };
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

RECT TitleBarRect() {
    State& st = Get();
    RECT r = { 0, 0, st.width, TITLE_H };
    return r;
}

bool HitTestNonClient(int x, int y, LRESULT& result) {
    State& st = Get();
    const int B = 6;   // 缩放边框热区
    if (x < B && y < B) { result = HTTOPLEFT; return true; }
    if (x >= st.width - B && y < B) { result = HTTOPRIGHT; return true; }
    if (x < B && y >= st.height - B) { result = HTBOTTOMLEFT; return true; }
    if (x >= st.width - B && y >= st.height - B) { result = HTBOTTOMRIGHT; return true; }
    if (x < B) { result = HTLEFT; return true; }
    if (x >= st.width - B) { result = HTRIGHT; return true; }
    if (y < B) { result = HTTOP; return true; }
    if (y >= st.height - B) { result = HTBOTTOM; return true; }
    // 标题栏（排除窗口按钮）
    if (y < TITLE_H && x < st.width - 46 * 3) { result = HTCAPTION; return true; }
    return false;
}

} // namespace app


namespace app {

// ---------------------------------------------------------------- 更多菜单
void DrawMoreMenu(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    ui::Painter& p = *c.p;
    struct It { const wchar_t* label; int cmd; const char* accel; };
    static const It items[] = {
        { L"\u6dfb\u52a0\u626b\u63cf\u76ee\u5f55\u2026", CMD_ADD_DIR,     "Ctrl+O" },
        { L"\u91cd\u65b0\u626b\u63cf",                  CMD_RESCAN,      "F5" },
        { L"\u91cd\u590d\u6587\u4ef6\u7b5b\u67e5\u2026", CMD_DUP,        "" },
        { L"\u5907\u4efd\u7ba1\u7406\u2026",            CMD_BACKUP,      "" },
        { L"\u64cd\u4f5c\u65e5\u5fd7\u2026",            CMD_LOG,         "" },
        { L"\u5bfc\u51fa\u9884\u89c8\u56fe\u2026",      CMD_EXPORT,      "" },
        { L"\u5173\u4e8e / \u5e2e\u52a9",              CMD_ABOUT,       "" },
        { L"\u9000\u51fa",                              CMD_EXIT,        "" },
    };
    int iw = 240, ih = 8 + (int)(sizeof(items) / sizeof(items[0])) * 28;
    ui::Rect dd = { L.toolbar.Right() - iw - 10, L.toolbar.Bottom() + 2, iw, ih };
    if (dd.x < 4) dd.x = 4;
    p.Fill({ dd.x + 3, dd.y + 3, dd.w, dd.h }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 18));
    p.Fill(dd, Theme::BgPanel);
    p.Frame(dd, Theme::TextDim, 1);
    int y = dd.y + 4;
    for (const It& it : items) {
        ui::Rect ir = { dd.x + 2, y, iw - 4, 28 };
        bool hov = ir.Contains(c.in->mouseX, c.in->mouseY);
        if (hov) p.Fill(ir, Theme::BgHover);
        p.Text(it.label, { ir.x + 12, ir.y, ir.w - 80, ir.h }, Theme::Text, c.f->ui,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        if (it.accel[0])
            p.Text(Utf8ToWide(it.accel), { ir.Right() - 76, ir.y, 68, ir.h }, Theme::TextDim,
                   c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased) { st.moreOpen = false; ExecuteCommand(it.cmd); return; }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
        y += 28;
    }
    if (c.in->mousePressed && !dd.Contains(c.in->mouseX, c.in->mouseY) &&
        !L.toolbar.Contains(c.in->mouseX, c.in->mouseY))
        st.moreOpen = false;
}

// ---------------------------------------------------------------- 投影格式选择
void DrawFormatPopup(ui::Ctx& c, const Layout& L) {
    State& st = Get();
    if (!st.cur) { st.fmtPickOpen = false; return; }
    ui::Painter& p = *c.p;
    struct F { int ver, sub; const wchar_t* desc; };
    static const F opts[] = {
        { 7, 1, L"MC 1.16 \u53CA\u4EE5\u540E\uFF08Litematica 0.2x\uFF0C\u63A8\u8350\uFF09" },
        { 7, 0, L"MC 1.16 \u53CA\u4EE5\u540E\uFF08\u65e9\u671f v7\uFF09" },
        { 6, 0, L"MC 1.14 ~ 1.15" },
        { 5, 0, L"MC 1.13.2" },
        { 4, 0, L"MC 1.12 \u53CA\u66F4\u65e9" },
    };
    int iw = 420, ih = 36 + (int)(sizeof(opts) / sizeof(opts[0])) * 30;
    ui::Rect dd = { L.right.x + 60, L.right.y + 260, iw, ih };
    if (dd.Bottom() > L.status.y - 6) dd.y = L.status.y - 6 - ih;
    p.Fill({ dd.x + 3, dd.y + 3, dd.w, dd.h }, ui::Mix(Theme::BgWindow, RGB(0, 0, 0), 20));
    p.Fill(dd, Theme::BgPanel);
    p.Frame(dd, Theme::Accent, 1);
    p.Fill({ dd.x, dd.y, dd.w, 32 }, Theme::BgHeader);
    p.Text(L"\u9009\u62E9\u6295\u5F71\u683C\u5F0F\uFF08\u5BF9\u5E94\u6E38\u620F\u7248\u672C\uFF09",
           { dd.x + 12, dd.y, dd.w - 24, 32 }, Theme::Text, c.f->uiBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    ltm::Summary sm = st.cur->GetSummary();
    int y = dd.y + 36;
    for (const F& o : opts) {
        ui::Rect row = { dd.x + 4, y, iw - 8, 30 };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        bool cur = (sm.formatVersion == o.ver && sm.formatSubVersion == o.sub);
        if (cur) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        p.Text(FmtW(L"v%d.%d", o.ver, o.sub), { row.x + 10, row.y, 60, row.h }, Theme::Accent, c.f->uiBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        p.Text(o.desc, { row.x + 76, row.y, row.w - 86, row.h }, Theme::Text, c.f->ui,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.cur->SetFormatVersion(o.ver, o.sub);
            st.fmtPickOpen = false;
            SaveCurrentWithBackup(i18n::T("log.op.version"));
            return;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
        y += 30;
    }
    if (c.in->mousePressed && !dd.Contains(c.in->mouseX, c.in->mouseY))
        st.fmtPickOpen = false;
}

} // namespace app
