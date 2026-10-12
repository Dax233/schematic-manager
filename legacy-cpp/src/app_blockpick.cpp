// 方块选择弹窗：带游戏版本选择 + 中英文搜索
#include "app_internal.h"
#include "i18n.h"
#include "str.h"
#include <algorithm>

namespace app {

// 打开弹窗（由 CMD_BLK_PICK 调用）
void OpenBlockPickModal() {
    State& st = Get();
    if (!st.dbLoaded) { SetStatus(i18n::T("err.noaassets")); return; }
    if (!st.cur) { SetStatus(i18n::T("preview.none")); return; }
    st.bpSel = -1;
    st.bpScroll = 0;
    st.bpVerScroll = 0;
    st.bpSearch.clear();
    // 默认选中投影自身的游戏版本
    st.bpVerIdx = -1;
    if (st.curVersionIdx >= 0) {
        st.bpVerIdx = st.curVersionIdx;
    } else if (st.cur) {
        int i = st.db.FindBaseVersion(st.cur->GetSummary().dataVersion);
        st.bpVerIdx = (i >= 0) ? i : 0;
    }
    if (st.bpVerIdx < 0) st.bpVerIdx = 0;
    ResetEditInit();
    st.modal = MODAL_BLOCKPICK;
    if (!OpenModalHost(MODAL_BLOCKPICK)) { st.modal = MODAL_NONE; return; }
}

// 版本列表：只列基础版本（跳过模组叠加包），前面加一个"全部"
static void CollectVersions(State& st, std::vector<int>& idx) {
    idx.clear();
    const auto& vs = st.db.Versions();
    for (size_t i = 0; i < vs.size(); ++i)
        if (!vs[i].isOverlay) idx.push_back((int)i);
}

void DrawBlockPickModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ui::Painter& p = *c.p;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);
    ModalFrame(c, r, L"\u9009\u62e9\u8981\u66ff\u6362\u6210\u7684\u65b9\u5757");

    const int pad = 14;
    const int top = r.y + 48;

    // ---- 搜索框（支持中文名 / 英文名 / 命名空间 id）
    p.Text(L"\u641C\u7D22", { r.x + pad, top - 18, 60, 16 }, Theme::TextDim, c.f->small);
    ui::Rect sb = { r.x + pad, top, r.w - pad * 2 - 96, 26 };
    p.Fill(sb, RGB(255, 255, 255));
    p.Frame(sb, Theme::Separator, 1);
    g_editWant[EDIT_REP_SEARCH] = true;
    g_editPos[EDIT_REP_SEARCH] = { sb.x + 3, sb.y + 3, sb.w - 6, sb.h - 6 };
    if (ui::Button(c, ui::Id("bpsearch", 1), { sb.Right() + 8, top, 88, 26 },
                   L"\u641C\u7D22\u65B9\u5757"))
        InvalidateRect(ModalHwnd(), nullptr, FALSE);
    if (st.bpSearch.empty())
        p.Text(L"\u8F93\u5165\u4E2D\u6587\u540D\u3001\u82F1\u6587\u540D\u6216 minecraft:xxx \u8FDB\u884C\u7B5B\u9009",
               { sb.x + 8, sb.y, sb.w - 16, sb.h }, Theme::TextDim, c.f->small);

    // ---- 左：游戏版本列表
    const int labTop = top + 32;               // 标题行，独立于搜索框
    const int listTop = top + 52;
    const int botH = 52;
    const int listH = r.Bottom() - listTop - botH - 8;
    ui::Rect vbox = { r.x + pad, listTop, 168, listH };
    p.Fill(vbox, Theme::BgListRow);
    p.Frame(vbox, Theme::Separator, 1);
    p.Text(L"\u6E38\u620F\u7248\u672C", { vbox.x, labTop, vbox.w, 18 }, Theme::Text, c.f->smallBold);

    std::vector<int> vlist;
    CollectVersions(st, vlist);
    int vTotal = (int)vlist.size();          // 不再提供"全部版本"
    int vRowH = 22;
    p.PushClip({ vbox.x + 1, vbox.y + 1, vbox.w - 2, vbox.h - 2 });
    for (int i = 0; i < vTotal; ++i) {
        ui::Rect row = { vbox.x + 1, vbox.y + 1 + i * vRowH - st.bpVerScroll, vbox.w - 2, vRowH };
        if (row.Bottom() < vbox.y || row.y > vbox.Bottom()) continue;
        bool sel = (st.bpVerIdx == vlist[(size_t)i]);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        std::wstring label = Utf8ToWide(st.db.Versions()[(size_t)vlist[(size_t)i]].id);
        p.Text(label, { row.x + 8, row.y, row.w - 12, row.h }, sel ? Theme::Accent : Theme::Text,
               c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.bpVerIdx = vlist[(size_t)i];
            st.bpSel = -1;
            st.bpScroll = 0;
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, ui::Id("bpver", 1), { vbox.Right() - 10, vbox.y + 1, 9, vbox.h - 2 },
                  st.bpVerScroll, vTotal * vRowH, vbox.h - 2);

    // ---- 右：方块列表
    ui::Rect bbox = { vbox.Right() + 10, listTop, r.Right() - pad - (vbox.Right() + 10), listH };
    p.Fill(bbox, Theme::BgPanel);
    p.Frame(bbox, Theme::Separator, 1);
    p.Text(chinese ? L"\u65B9\u5757\u5217\u8868" : L"Blocks",
           { bbox.x, labTop, bbox.w, 18 }, Theme::Text, c.f->smallBold);

    // 收集候选：按版本或全部，再按搜索词筛选
    const std::vector<std::string>& all = st.db.AllBlockNames();
    static std::vector<std::string> names;
    names.clear();
    if (st.bpVerIdx < 0 || (size_t)st.bpVerIdx >= st.db.Versions().size()) {
        names = all;
        st.bpVerIdx = -1;
    } else {
        for (const auto& b : st.db.Versions()[(size_t)st.bpVerIdx].blocks) names.push_back(b.name);
    }
    std::wstring q = ToLowerW(st.bpSearch);
    static std::vector<int> filt;
    filt.clear();
    for (size_t i = 0; i < names.size(); ++i) {
        if (q.empty()) { filt.push_back((int)i); continue; }
        std::wstring dn = ToLowerW(BlockDisplay(names[i], true) + L" " + Utf8ToWide(names[i]));
        if (dn.find(q) != std::wstring::npos) filt.push_back((int)i);
    }

    int rowH = 26;
    int bTotal = (int)filt.size() * rowH;
    int maxS = std::max(0, bTotal - (bbox.h - 2));
    if (st.bpScroll > maxS) st.bpScroll = maxS;
    if (st.bpScroll < 0) st.bpScroll = 0;
    p.PushClip({ bbox.x + 1, bbox.y + 1, bbox.w - 2, bbox.h - 2 });
    int first = st.bpScroll / rowH;
    int last = std::min((int)filt.size(), first + (bbox.h - 2) / rowH + 2);
    for (int i = first; i < last; ++i) {
        const std::string& bn = names[(size_t)filt[(size_t)i]];
        ui::Rect row = { bbox.x + 1, bbox.y + 1 + i * rowH - st.bpScroll, bbox.w - 2, rowH };
        if (row.Bottom() < bbox.y || row.y > bbox.Bottom()) continue;
        bool sel = (st.bpSel == filt[(size_t)i]);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        else if (i % 2) p.Fill(row, Theme::BgListRowAlt);

        const render::Image* ic = GetBlockIcon(bn, 20);
        if (ic && !ic->px.empty()) p.Image(*ic, { row.x + 4, row.y + 3, 20, 20 }, true);
        else p.Fill({ row.x + 4, row.y + 3, 20, 20 }, RGB(0x18, 0x00, 0x18));

        p.Text(BlockDisplay(bn, chinese), { row.x + 30, row.y, 210, row.h }, Theme::Text, c.f->ui);
        p.Text(Utf8ToWide(bn), { row.x + 246, row.y, row.w - 320, row.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (IsModBlock(bn))
            p.Text(i18n::T("rep.source.mod"), { row.Right() - 60, row.y, 54, row.h }, Theme::Danger,
                   c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            st.bpSel = filt[(size_t)i];
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, ui::Id("bpblk", 1), { bbox.Right() - 12, bbox.y + 1, 11, bbox.h - 2 },
                  st.bpScroll, bTotal, bbox.h - 2);

    if (filt.empty())
        p.Text(L"\u6CA1\u6709\u5339\u914D\u7684\u65B9\u5757", bbox, Theme::TextDim, c.f->ui,
               DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    // ---- 底部：已选 + 确认/取消
    ui::Rect bar = { r.x + pad, r.Bottom() - botH, r.w - pad * 2, botH - 10 };
    p.Fill(bar, Theme::BgHeader);
    p.Frame(bar, Theme::Separator, 1);
    if (st.bpSel >= 0 && st.bpSel < (int)names.size()) {
        const std::string& bn = names[(size_t)st.bpSel];
        const render::Image* ic = GetBlockIcon(bn, 22);
        if (ic && !ic->px.empty()) p.Image(*ic, { bar.x + 8, bar.y + 8, 22, 22 }, true);
        p.Text(BlockDisplay(bn, chinese), { bar.x + 36, bar.y, 200, bar.h }, Theme::Text, c.f->uiBold,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(Utf8ToWide(bn), { bar.x + 240, bar.y, bar.w - 500, bar.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        std::wstring ver = (st.bpVerIdx >= 0) ? Utf8ToWide(st.db.Versions()[(size_t)st.bpVerIdx].id)
                                              : std::wstring(L"\u5168\u90E8\u7248\u672C");
        p.Text(L"\u6765\u6E90\uFF1A" + ver, { bar.Right() - 300, bar.y, 180, bar.h }, Theme::TextDim,
               c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    } else {
        p.Text(L"\u5728\u4E0A\u9762\u70B9\u9009\u4E00\u4E2A\u65B9\u5757", { bar.x + 12, bar.y, bar.w - 220, bar.h },
               Theme::TextDim, c.f->ui, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    }
    bool can = (st.bpSel >= 0 && st.bpSel < (int)names.size());
    if (ui::Button(c, ui::Id("bpok", 1), { bar.Right() - 214, bar.y + 8, 100, bar.h - 16 },
                   L"\u786E\u8BA4\u66FF\u6362", can, true)) {
        const std::string bn = names[(size_t)st.bpSel];
        std::string ver = (st.bpVerIdx >= 0) ? st.db.Versions()[(size_t)st.bpVerIdx].id : std::string();
        // 版本不一致 -> 二次确认
        ltm::Summary sm = st.cur->GetSummary();
        if (!ver.empty() && !sm.mcVersionName.empty() && ver != sm.mcVersionName) {
            std::wstring msg = FmtW(
                L"\u6240\u9009\u65B9\u5757\u6765\u81EA\u6E38\u620F\u7248\u672C %s\uFF0C\n"
                L"\u4E0E\u5F53\u524D\u6295\u5F71\u7684\u6E38\u620F\u7248\u672C %s \u4E0D\u4E00\u81F4\u3002\n\n"
                L"\u66FF\u6362\u540E\u8BE5\u65B9\u5757\u53EF\u80FD\u65E0\u6CD5\u88AB\u6E38\u620F\u8BC6\u522B\uFF0C"
                L"\u6216\u663E\u793A\u4E3A\u9519\u8BEF\u65B9\u5757\u3002\n\n\u786E\u5B9A\u8981\u7EE7\u7EED\u5417\uFF1F",
                Utf8ToWide(ver).c_str(), Utf8ToWide(sm.mcVersionName).c_str());
            if (MessageBoxW(ModalHwnd(), msg.c_str(), L"\u7248\u672C\u4E0D\u4E00\u81F4",
                            MB_YESNO | MB_ICONWARNING) != IDYES)
                return;
        }
        st.blockToName = bn;
        st.blockToVersion = ver;
        CloseModalHost();
        // 关闭弹窗后由主窗口执行替换（内含自动备份 + 操作日志）
        PostMessageW(st.hwnd, WM_APP + 1, (WPARAM)CMD_BLK_DO, 0);
        return;
    }
    if (ui::Button(c, ui::Id("bpcancel", 1), { bar.Right() - 108, bar.y + 8, 96, bar.h - 16 },
                   i18n::T("btn.cancel")))
        CloseModalHost();
}

} // namespace app
