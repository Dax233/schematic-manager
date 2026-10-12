// 投影游戏版本转换：检测版本差异 -> 逐个指定替代方块 -> 另存为新文件
#include "app_internal.h"
#include "i18n.h"
#include "str.h"
#include "fs.h"
#include <algorithm>
#include <set>

namespace app {

// 取命名空间的 path 部分
static std::string CvPath(const std::string& id) {
    size_t c = id.find(':');
    return (c == std::string::npos) ? id : id.substr(c + 1);
}
static bool CvIsVanilla(const std::string& id) {
    return id.compare(0, 10, "minecraft:") == 0;
}

// 自动推荐：优先同后缀（_stairs/_slab/_planks...），其次同前缀（deepslate_...）
static std::string SuggestBlock(const std::string& name, const std::set<std::string>& target) {
    std::string p = CvPath(name);
    std::vector<std::string> cand;
    for (const auto& t : target) cand.push_back(t);
    // 原版优先，其次字典序，保证结果稳定
    std::sort(cand.begin(), cand.end(), [](const std::string& a, const std::string& b) {
        bool va = CvIsVanilla(a), vb = CvIsVanilla(b);
        if (va != vb) return va;
        return a < b;
    });

    // 1) 同后缀：取最后一个下划线之后的部分
    size_t u = p.rfind('_');
    if (u != std::string::npos && u + 1 < p.size()) {
        std::string suf = p.substr(u);                 // 例如 "_stairs"
        for (const auto& t : cand) {
            std::string tp = CvPath(t);
            if (tp.size() > suf.size() &&
                tp.compare(tp.size() - suf.size(), suf.size(), suf) == 0)
                return t;
        }
    }
    // 2) 同前缀：第一个下划线及之前
    size_t u2 = p.find('_');
    if (u2 != std::string::npos) {
        std::string pre = p.substr(0, u2 + 1);         // 例如 "deepslate_"
        for (const auto& t : cand) {
            std::string tp = CvPath(t);
            if (tp.size() > pre.size() && tp.compare(0, pre.size(), pre) == 0)
                return t;
        }
    }
    return std::string();
}

// 分析：投影里用到、但目标版本不存在的方块
void AnalyzeConversion(int targetVerIdx) {
    State& st = Get();
    st.cvItems.clear();
    st.cvSel = -1;
    st.cvScroll = 0;
    if (!st.cur || !st.dbLoaded) return;
    if (targetVerIdx < 0 || (size_t)targetVerIdx >= st.db.Versions().size()) return;
    if (!st.curAnalysisDone) { st.curAnalysis = st.cur->Analyze(); st.curAnalysisDone = true; }

    std::set<std::string> target;
    for (const auto& b : st.db.Versions()[(size_t)targetVerIdx].blocks) target.insert(b.name);

    for (const auto& kv : st.curAnalysis.blocksByName) {
        if (kv.first == "minecraft:air" || kv.first.empty()) continue;
        if (target.count(kv.first)) continue;          // 目标版本有 -> 不用处理
        CvItem it;
        it.name = kv.first;
        it.count = kv.second;
        it.repl = SuggestBlock(kv.first, target);
        st.cvItems.push_back(it);
    }
    std::sort(st.cvItems.begin(), st.cvItems.end(),
              [](const CvItem& a, const CvItem& b) { return a.count > b.count; });
    st.cvAnalyzed = true;
}

void OpenConvertModal() {
    State& st = Get();
    if (!st.dbLoaded) { SetStatus(i18n::T("err.noaassets")); return; }
    if (!st.cur) { SetStatus(i18n::T("preview.none")); return; }
    st.cvVerIdx = -1;
    st.cvAnalyzed = false;
    st.cvItems.clear();
    st.cvSel = -1;
    st.cvScroll = 0;
    st.cvBlkScroll = 0;
    st.bpSearch.clear();
    ResetEditInit();
    st.modal = MODAL_CONVERT;
    if (!OpenModalHost(MODAL_CONVERT)) { st.modal = MODAL_NONE; return; }
}

// 执行转换：另存为新文件
static void ApplyConversion() {
    State& st = Get();
    if (!st.cur || st.cvVerIdx < 0) return;
    const auto& vd = st.db.Versions()[(size_t)st.cvVerIdx];
    std::wstring orig = st.cur->Path();
    if (orig.empty()) { SetStatus(L"\u65e0\u6CD5\u786E\u5B9A\u539F\u6587\u4EF6\u8DEF\u5F84"); return; }
    std::wstring stem = fsx::GetFileStem(orig);
    std::wstring dir = fsx::GetDirectoryName(orig);
    std::wstring out = fsx::JoinPath(dir, stem + L"_" + Utf8ToWide(vd.id) + L".litematic");

    // 替换：使用源版本的方块定义来判断属性继承
    if (st.curVersionIdx >= 0) st.db.SetActiveVersion(st.curVersionIdx);
    int done = 0;
    for (const auto& it : st.cvItems) {
        if (it.repl.empty()) continue;
        ltm::ReplaceRule rule;
        rule.fromName = it.name;
        rule.toName = it.repl;
        rule.inheritProps = true;
        if (st.cur->ReplaceBlocks(rule, &st.db, 0) > 0) ++done;
    }
    // 改制作版本号
    st.cur->SetDataVersion(vd.dataVersion);

    std::string err;
    if (!st.cur->SaveTo(out, err)) {
        st.lastConvertInfo = L"\u4FDD\u5B58\u5931\u8D25: " + Utf8ToWide(err);
        SetStatus(st.lastConvertInfo);
        return;
    }
    backup::Log(out, L"convert", Utf8ToWide(vd.id));
    int left = 0;
    for (const auto& it : st.cvItems) if (it.repl.empty()) ++left;
    st.lastConvertInfo = FmtW(L"\u5DF2\u8F6C\u6362\u4E3A %s\uFF1A\u66FF\u6362 %d \u79CD\u65B9\u5757\uFF0C%d \u79CD\u672A\u6307\u5B9A\uff5c%s",
                              Utf8ToWide(vd.id).c_str(), done, left, out);
    SetStatus(st.lastConvertInfo);
    CloseModalHost();
    RescanAsync();
}

void DrawConvertModal(ui::Ctx& c, const ui::Rect& r) {
    State& st = Get();
    ui::Painter& p = *c.p;
    bool chinese = (i18n::GetLang() == i18n::LANG_ZH);
    ModalFrame(c, r, L"\u8F6C\u6362\u6295\u5F71\u6E38\u620F\u7248\u672C");

    ltm::Summary sm = st.cur ? st.cur->GetSummary() : ltm::Summary();
    const int pad = 14;
    const int infoY = r.y + 40;          // 顶部信息行
    const int labY  = r.y + 68;          // 列标题行
    const int top   = r.y + 90;          // 列表起始
    p.Text(L"\u5F53\u524D\u6295\u5F71\u7248\u672C\uFF1A" + (sm.mcVersionName.empty() ? std::wstring(L"-")
                                                                    : Utf8ToWide(sm.mcVersionName)),
           { r.x + pad, infoY, 300, 20 }, Theme::Text, c.f->ui, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    // ---------- 第一步：选目标版本 ----------
    if (st.cvVerIdx < 0) {
        p.Text(L"\u9009\u62E9\u76EE\u6807\u6E38\u620F\u7248\u672C\uFF08\u8F6C\u6362\u540E\u53E6\u5B58\u4E3A\u65B0\u6587\u4EF6\uFF0C\u539F\u6587\u4EF6\u4E0D\u53D8\uFF09",
               { r.x + pad, labY, r.w - pad * 2, 18 }, Theme::TextDim, c.f->small);
        std::vector<int> vl;
        for (size_t i = 0; i < st.db.Versions().size(); ++i)
            if (!st.db.Versions()[i].isOverlay) vl.push_back((int)i);
        int rowH = 24;
        ui::Rect box = { r.x + pad, top, r.w - pad * 2, r.Bottom() - top - 56 };
        p.Fill(box, Theme::BgPanel);
        p.Frame(box, Theme::Separator, 1);
        int total = (int)vl.size() * rowH;
        int maxS = std::max(0, total - (box.h - 2));
        if (st.cvScroll > maxS) st.cvScroll = maxS;
        if (st.cvScroll < 0) st.cvScroll = 0;
        p.PushClip({ box.x + 1, box.y + 1, box.w - 2, box.h - 2 });
        int first = st.cvScroll / rowH;
        int last = std::min((int)vl.size(), first + (box.h - 2) / rowH + 2);
        for (int i = first; i < last; ++i) {
            const auto& v = st.db.Versions()[(size_t)vl[(size_t)i]];
            ui::Rect row = { box.x + 1, box.y + 1 + i * rowH - st.cvScroll, box.w - 2, rowH };
            bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
            if (hov) p.Fill(row, Theme::BgHover);
            p.Text(Utf8ToWide(v.id), { row.x + 12, row.y, 200, row.h }, Theme::Text, c.f->ui,
                   DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            p.Text(FmtW(L"dataVersion %d", v.dataVersion), { row.x + 220, row.y, 200, row.h },
                   Theme::TextDim, c.f->small, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
            p.Text(FmtW(L"%zu \u79CD\u65B9\u5757", v.blocks.size()), { row.Right() - 160, row.y, 150, row.h },
                   Theme::TextDim, c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
            if (hov && c.in->mouseReleased && !c.in->usedMouse) {
                st.cvVerIdx = vl[(size_t)i];
                AnalyzeConversion(st.cvVerIdx);
                c.in->usedMouse = true;
            }
            if (hov && c.in->mousePressed) c.in->usedMouse = true;
        }
        p.PopClip();
        ui::ScrollBar(c, ui::Id("cvver", 1), { box.Right() - 12, box.y + 1, 11, box.h - 2 },
                      st.cvScroll, total, box.h - 2);
        if (ui::Button(c, ui::Id("cvcancel0", 1), { r.Right() - pad - 96, r.Bottom() - 46, 96, 30 },
                       i18n::T("btn.cancel")))
            CloseModalHost();
        return;
    }

    // ---------- 第二步：处理不兼容方块 ----------
    // 防御：任何原因导致下标失效都退回第一步，绝不越界访问
    if (st.cvVerIdx < 0 || (size_t)st.cvVerIdx >= st.db.Versions().size()) {
        st.cvVerIdx = -1;
        st.cvAnalyzed = false;
        return;
    }
    std::string tgtId = st.db.Versions()[(size_t)st.cvVerIdx].id;
    p.Text(L"\u76EE\u6807\u7248\u672C\uFF1A" + Utf8ToWide(tgtId) +
           FmtW(L"\u3000\u9700\u5904\u7406 %zu \u79CD\u65B9\u5757",
                st.cvItems.size()),
           { r.x + 340, infoY, 480, 20 }, Theme::Accent, c.f->uiBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    if (ui::Button(c, ui::Id("cvback", 1), { r.Right() - pad - 96, infoY - 2, 96, 24 },
                   L"\u91CD\u9009\u7248\u672C")) {
        st.cvVerIdx = -1;
        st.cvAnalyzed = false;
        st.cvItems.clear();
        st.cvSel = -1;
        st.cvScroll = 0;
        st.cvBlkScroll = 0;
        // 必须立即返回：本帧后续代码仍会访问 Versions()[cvVerIdx]，
        // 此时 cvVerIdx 已是 -1，(size_t)-1 会越界导致崩溃。
        return;
    }

    int botH = 76;
    int midW = (r.w - pad * 2 - 10) * 52 / 100;
    ui::Rect lbox = { r.x + pad, top, midW, r.Bottom() - top - botH };
    ui::Rect rbox = { lbox.Right() + 10, top, r.w - pad - (lbox.Right() + 10), r.Bottom() - top - botH };
    p.Fill(lbox, Theme::BgPanel);
    p.Frame(lbox, Theme::Separator, 1);
    p.Text(L"\u9700\u8981\u66FF\u6362\u7684\u65B9\u5757", { lbox.x, labY, lbox.w, 18 },
           Theme::Text, c.f->smallBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    p.Text(L"\u76EE\u6807\u7248\u672C\u53EF\u7528\u65B9\u5757", { rbox.x, labY, rbox.w, 18 },
           Theme::Text, c.f->smallBold, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    // 左：问题方块（点一行选中，再从右侧点方块作为替代品）
    int rowH = 30;
    int lTotal = (int)st.cvItems.size() * rowH;
    int lMax = std::max(0, lTotal - (lbox.h - 2));
    if (st.cvScroll > lMax) st.cvScroll = lMax;
    if (st.cvScroll < 0) st.cvScroll = 0;
    p.PushClip({ lbox.x + 1, lbox.y + 1, lbox.w - 2, lbox.h - 2 });
    int lf = st.cvScroll / rowH;
    int ll = std::min((int)st.cvItems.size(), lf + (lbox.h - 2) / rowH + 2);
    for (int i = lf; i < ll; ++i) {
        CvItem& it = st.cvItems[(size_t)i];
        ui::Rect row = { lbox.x + 1, lbox.y + 1 + i * rowH - st.cvScroll, lbox.w - 2, rowH };
        bool sel = (st.cvSel == i);
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (sel) p.Fill(row, Theme::BgListSel);
        else if (hov) p.Fill(row, Theme::BgHover);
        else if (i % 2) p.Fill(row, Theme::BgListRowAlt);
        const render::Image* ic = GetBlockIcon(it.name, 20);
        if (ic && !ic->px.empty()) p.Image(*ic, { row.x + 4, row.y + 5, 20, 20 }, true);
        else p.Fill({ row.x + 4, row.y + 5, 20, 20 }, RGB(0x18, 0x00, 0x18));
        p.Text(BlockDisplay(it.name, chinese), { row.x + 30, row.y, 150, 15 }, Theme::Text, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(Utf8ToWide(CvPath(it.name)), { row.x + 30, row.y + 15, 150, 15 }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(FmtW(L"%llu", (unsigned long long)it.count), { row.x + 184, row.y, 60, row.h },
               Theme::Text, c.f->small, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
        std::wstring rl = it.repl.empty() ? std::wstring(L"\u2014 \u672A\u6307\u5B9A")
                                          : BlockDisplay(it.repl, chinese);
        p.Text(L"\u2192 " + rl, { row.x + 250, row.y, row.w - 256, row.h },
               it.repl.empty() ? Theme::Danger : Theme::Accent, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) { st.cvSel = i; c.in->usedMouse = true; }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, ui::Id("cvlst", 1), { lbox.Right() - 12, lbox.y + 1, 11, lbox.h - 2 },
                  st.cvScroll, lTotal, lbox.h - 2);
    if (st.cvItems.empty())
        p.Text(L"\u6CA1\u6709\u4E0D\u517C\u5BB9\u7684\u65B9\u5757\uFF0C\u53EF\u4EE5\u76F4\u63A5\u8F6C\u6362",
               lbox, Theme::Text, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    // 右：目标版本方块 + 搜索
    // 搜索框：用强调色描边 + 纯白底，避免与浅色面板糊在一起看不出来
    ui::Rect sbox = { rbox.x + 2, rbox.y + 2, rbox.w - 98, 24 };
    p.Fill(sbox, RGB(255, 255, 255));
    p.Frame(sbox, Theme::Accent, 2);
    g_editWant[EDIT_REP_SEARCH] = true;
    g_editPos[EDIT_REP_SEARCH] = { sbox.x + 5, sbox.y + 4, sbox.w - 10, sbox.h - 8 };
    if (st.cvSearch.empty())
        p.Text(L"\u8f93\u5165\u65b9\u5757\u4e2d\u6587\u540d / \u82f1\u6587\u540d / ID \u7b5b\u9009\u2026",
               { sbox.x + 8, sbox.y, sbox.w - 16, sbox.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    if (ui::Button(c, ui::Id("cvsearch", 1), { rbox.Right() - 90, rbox.y + 2, 88, 24 }, L"\u641C\u7D22\u65B9\u5757"))
        InvalidateRect(ModalHwnd(), nullptr, FALSE);

    static std::vector<std::string> names;
    names.clear();
    for (const auto& b : st.db.Versions()[(size_t)st.cvVerIdx].blocks) names.push_back(b.name);
    std::wstring q = ToLowerW(st.cvSearch);
    static std::vector<int> filt;
    filt.clear();
    for (size_t i = 0; i < names.size(); ++i) {
        if (q.empty()) { filt.push_back((int)i); continue; }
        std::wstring dn = ToLowerW(BlockDisplay(names[i], true) + L" " + Utf8ToWide(names[i]));
        if (dn.find(q) != std::wstring::npos) filt.push_back((int)i);
    }
    ui::Rect bl = { rbox.x + 2, rbox.y + 30, rbox.w - 2, rbox.h - 32 };
    int bRowH = 24;
    int bTotal = (int)filt.size() * bRowH;
    int bMax = std::max(0, bTotal - (bl.h - 2));
    if (st.cvBlkScroll > bMax) st.cvBlkScroll = bMax;
    if (st.cvBlkScroll < 0) st.cvBlkScroll = 0;
    p.PushClip({ bl.x, bl.y, bl.w - 12, bl.h });
    int bf = st.cvBlkScroll / bRowH;
    int bl2 = std::min((int)filt.size(), bf + (bl.h - 2) / bRowH + 2);
    for (int i = bf; i < bl2; ++i) {
        const std::string& bn = names[(size_t)filt[(size_t)i]];
        ui::Rect row = { bl.x, bl.y + i * bRowH - st.cvBlkScroll, bl.w - 12, bRowH };
        bool hov = row.Contains(c.in->mouseX, c.in->mouseY);
        if (hov) p.Fill(row, Theme::BgHover);
        else if (i % 2) p.Fill(row, Theme::BgListRowAlt);
        const render::Image* ic = GetBlockIcon(bn, 18);
        if (ic && !ic->px.empty()) p.Image(*ic, { row.x + 4, row.y + 3, 18, 18 }, true);
        else p.Fill({ row.x + 4, row.y + 3, 18, 18 }, RGB(0x18, 0x00, 0x18));
        p.Text(BlockDisplay(bn, chinese), { row.x + 28, row.y, 150, row.h }, Theme::Text, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        p.Text(Utf8ToWide(CvPath(bn)), { row.x + 184, row.y, row.w - 190, row.h }, Theme::TextDim, c.f->small,
               DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        if (hov && c.in->mouseReleased && !c.in->usedMouse) {
            if (st.cvSel >= 0 && st.cvSel < (int)st.cvItems.size())
                st.cvItems[(size_t)st.cvSel].repl = bn;      // 指定替代方块
            c.in->usedMouse = true;
        }
        if (hov && c.in->mousePressed) c.in->usedMouse = true;
    }
    p.PopClip();
    ui::ScrollBar(c, ui::Id("cvblk", 1), { bl.Right() - 11, bl.y, 10, bl.h }, st.cvBlkScroll, bTotal, bl.h);

    // 底部
    int unset = 0;
    for (const auto& it : st.cvItems) if (it.repl.empty()) ++unset;
    p.Text(unset ? FmtW(L"\u6709 %d \u79CD\u65B9\u5757\u672A\u6307\u5B9A\u66FF\u4EE3\u54C1\uFF0C\u5B83\u4EEC\u5C06\u4FDD\u6301\u539F\u6837\uFF08\u6E38\u620F\u5185\u663E\u793A\u4E3A\u672A\u77E5\u65B9\u5757\uFF09", unset)
                 : std::wstring(L"\u5168\u90E8\u65B9\u5757\u5DF2\u6709\u66FF\u4EE3\u54C1"),
           { r.x + pad, r.Bottom() - botH + 6, r.w - pad * 2 - 300, 22 },
           unset ? RGB(0xB0, 0x60, 0x00) : Theme::Text, c.f->small,
           DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    p.Text(L"\u70B9\u5DE6\u4FA7\u4E00\u884C\uFF0C\u518D\u70B9\u53F3\u4FA7\u65B9\u5757\u5373\u53EF\u6307\u5B9A\u66FF\u4EE3\u54C1\u3002\u7ED3\u679C\u53E6\u5B58\u4E3A\u65B0\u6587\u4EF6\u3002",
           { r.x + pad, r.Bottom() - botH + 30, r.w - pad * 2 - 300, 20 }, Theme::TextDim, c.f->small,
           DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    if (ui::Button(c, ui::Id("cvok", 1), { r.Right() - pad - 200, r.Bottom() - 46, 120, 30 },
                   L"\u5F00\u59CB\u8F6C\u6362", true, true))
        ApplyConversion();
    if (ui::Button(c, ui::Id("cvcancel", 1), { r.Right() - pad - 72, r.Bottom() - 46, 72, 30 },
                   i18n::T("btn.cancel")))
        CloseModalHost();
}

} // namespace app
