#include "ui.h"
#include "str.h"

namespace ui {

// ------------------------------------------------------------------ Fonts
static HFONT MakeFont(int dpi, int pt, bool bold) {
    LOGFONTW lf = {};
    lf.lfHeight = -MulDiv(pt, dpi, 72);
    lf.lfWeight = bold ? FW_SEMIBOLD : FW_NORMAL;
    lf.lfCharSet = DEFAULT_CHARSET;
    lf.lfQuality = CLEARTYPE_QUALITY;
    lf.lfOutPrecision = OUT_TT_PRECIS;
    wcscpy(lf.lfFaceName, L"Microsoft YaHei UI");
    HFONT f = CreateFontIndirectW(&lf);
    if (!f) { wcscpy(lf.lfFaceName, L"Microsoft YaHei"); f = CreateFontIndirectW(&lf); }
    if (!f) { wcscpy(lf.lfFaceName, L"SimSun"); f = CreateFontIndirectW(&lf); }
    return f;
}

void Fonts::Create(int dpi) {
    Destroy();
    ui = MakeFont(dpi, 10, false);
    uiBold = MakeFont(dpi, 10, true);
    small = MakeFont(dpi, 9, false);
    smallBold = MakeFont(dpi, 9, true);
    title = MakeFont(dpi, 11, true);
}

void Fonts::Destroy() {
    if (ui) DeleteObject(ui);
    if (uiBold) DeleteObject(uiBold);
    if (small) DeleteObject(small);
    if (smallBold) DeleteObject(smallBold);
    if (title) DeleteObject(title);
    ui = uiBold = small = smallBold = title = nullptr;
}

// ------------------------------------------------------------------ Painter
void Painter::Begin(HDC dc, int w, int h) {
    dc_ = dc; w_ = w; h_ = h;
    memdc_ = CreateCompatibleDC(dc);
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    bmp_ = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (bits) memset(bits, 0xFF, (size_t)w * h * 4);
    oldbmp_ = (HBITMAP)SelectObject(memdc_, bmp_);
    clips_.clear();
    SetBkMode(memdc_, TRANSPARENT);
    SelectClipRgn(memdc_, nullptr);
}

void Painter::End() {
    if (memdc_) {
        SelectClipRgn(memdc_, nullptr);
        for (HRGN r : clips_) DeleteObject(r);
        clips_.clear();
        BitBlt(dc_, 0, 0, w_, h_, memdc_, 0, 0, SRCCOPY);
        SelectObject(memdc_, oldbmp_);
        DeleteObject(bmp_);
        DeleteDC(memdc_);
        memdc_ = nullptr; bmp_ = nullptr;
    }
    dc_ = nullptr;
}

void Painter::PushClip(const Rect& r) {
    HRGN nr = CreateRectRgn(r.x, r.y, r.Right(), r.Bottom());
    HRGN base = CreateRectRgn(0, 0, 0, 0);
    if (clips_.empty()) {
        // 当前的剪裁区（可能是窗口边界）
        if (GetClipRgn(memdc_, base) <= 0) SetRectRgn(base, 0, 0, w_, h_);
    } else {
        CombineRgn(base, clips_.back(), clips_.back(), RGN_COPY);
    }
    HRGN res = CreateRectRgn(0, 0, 0, 0);
    CombineRgn(res, base, nr, RGN_AND);
    SelectClipRgn(memdc_, res);
    clips_.push_back(res);
    DeleteObject(nr);
    DeleteObject(base);
}

void Painter::PopClip() {
    if (clips_.empty()) return;
    HRGN r = clips_.back();
    clips_.pop_back();
    if (clips_.empty()) SelectClipRgn(memdc_, nullptr);
    else SelectClipRgn(memdc_, clips_.back());
    DeleteObject(r);
}

void Painter::ResetClip() {
    for (HRGN r : clips_) DeleteObject(r);
    clips_.clear();
    SelectClipRgn(memdc_, nullptr);
}

void Painter::Fill(const Rect& r, COLORREF c) {
    if (r.Empty()) return;
    RECT rc = { r.x, r.y, r.Right(), r.Bottom() };
    HBRUSH br = CreateSolidBrush(c);
    FillRect(memdc_, &rc, br);
    DeleteObject(br);
}

void Painter::FillA(const Rect& r, u32 argb) {
    u32 a = (argb >> 24) & 0xFF;
    if (a == 0 || r.Empty()) return;
    COLORREF c = RGB((argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF);
    if (a == 255) { Fill(r, c); return; }
    HDC tmp = CreateCompatibleDC(memdc_);
    HBITMAP tb = CreateCompatibleBitmap(memdc_, r.w, r.h);
    HBITMAP ob = (HBITMAP)SelectObject(tmp, tb);
    RECT tr = { 0, 0, r.w, r.h };
    HBRUSH br = CreateSolidBrush(c);
    FillRect(tmp, &tr, br);
    DeleteObject(br);
    BLENDFUNCTION bf = { AC_SRC_OVER, 0, (BYTE)a, 0 };
    AlphaBlend(memdc_, r.x, r.y, r.w, r.h, tmp, 0, 0, r.w, r.h, bf);
    SelectObject(tmp, ob);
    DeleteObject(tb);
    DeleteDC(tmp);
}

void Painter::Frame(const Rect& r, COLORREF c, int t) {
    if (r.Empty()) return;
    for (int i = 0; i < t; ++i) {
        HLine(r.x, r.y + i, r.w, c);
        HLine(r.x, r.Bottom() - 1 - i, r.w, c);
        VLine(r.x + i, r.y, r.h, c);
        VLine(r.Right() - 1 - i, r.y, r.h, c);
    }
}

void Painter::HLine(int x, int y, int w, COLORREF c) {
    if (w <= 0) return;
    RECT rc = { x, y, x + w, y + 1 };
    HBRUSH br = CreateSolidBrush(c);
    FillRect(memdc_, &rc, br);
    DeleteObject(br);
}

void Painter::VLine(int x, int y, int h, COLORREF c) {
    if (h <= 0) return;
    RECT rc = { x, y, x + 1, y + h };
    HBRUSH br = CreateSolidBrush(c);
    FillRect(memdc_, &rc, br);
    DeleteObject(br);
}

void Painter::RoundedFill(const Rect& r, int radius, COLORREF fill, COLORREF border) {
    HBRUSH br = CreateSolidBrush(fill);
    HPEN pen = CreatePen(PS_SOLID, 1, (border == CLR_INVALID) ? fill : border);
    HGDIOBJ ob = SelectObject(memdc_, br);
    HGDIOBJ op = SelectObject(memdc_, pen);
    ::RoundRect(memdc_, r.x, r.y, r.Right(), r.Bottom(), radius * 2, radius * 2);
    SelectObject(memdc_, ob);
    SelectObject(memdc_, op);
    DeleteObject(br);
    DeleteObject(pen);
}

void Painter::RoundedFrame(const Rect& r, int radius, COLORREF c) {
    HPEN pen = CreatePen(PS_SOLID, 1, c);
    HGDIOBJ op = SelectObject(memdc_, pen);
    HGDIOBJ ob = SelectObject(memdc_, GetStockObject(NULL_BRUSH));
    ::RoundRect(memdc_, r.x, r.y, r.Right(), r.Bottom(), radius * 2, radius * 2);
    SelectObject(memdc_, op);
    SelectObject(memdc_, ob);
    DeleteObject(pen);
}

void Painter::Text(const std::wstring& s, const Rect& r, COLORREF c, HFONT f, UINT flags) {
    if (s.empty() || r.Empty()) return;
    HFONT old = (HFONT)SelectObject(memdc_, f);
    SetTextColor(memdc_, c);
    RECT rc = { r.x, r.y, r.Right(), r.Bottom() };
    DrawTextW(memdc_, s.c_str(), (int)s.size(), &rc, flags);
    SelectObject(memdc_, old);
}

void Painter::Image(const render::Image& img, const Rect& dst, bool smooth) {
    if (img.w <= 0 || img.h <= 0 || dst.Empty() || img.px.empty()) return;
    BITMAPINFO bi = {};
    bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bi.bmiHeader.biWidth = img.w;
    bi.bmiHeader.biHeight = -img.h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HDC src = CreateCompatibleDC(memdc_);
    HBITMAP bmp = CreateDIBSection(src, &bi, DIB_RGB_COLORS, &bits, nullptr, 0);
    HBITMAP ob = (HBITMAP)SelectObject(src, bmp);

    bool hasAlpha = false;
    if (bits) {
        u32* d = (u32*)bits;
        for (size_t i = 0; i < img.px.size(); ++i) {
            u32 c = img.px[i];
            u32 a = (c >> 24) & 0xFF;
            if (a != 255) hasAlpha = true;
            // AlphaBlend 需要预乘 alpha
            u32 r = ((c >> 16) & 0xFF) * a / 255;
            u32 g = ((c >> 8) & 0xFF) * a / 255;
            u32 b = (c & 0xFF) * a / 255;
            d[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
    int oldMode = SetStretchBltMode(memdc_, smooth ? HALFTONE : COLORONCOLOR);
    if (hasAlpha) {
        BLENDFUNCTION bf = { AC_SRC_OVER, 0, 255, AC_SRC_ALPHA };
        AlphaBlend(memdc_, dst.x, dst.y, dst.w, dst.h, src, 0, 0, img.w, img.h, bf);
    } else {
        StretchBlt(memdc_, dst.x, dst.y, dst.w, dst.h, src, 0, 0, img.w, img.h, SRCCOPY);
    }
    SetStretchBltMode(memdc_, oldMode);
    SelectObject(src, ob);
    DeleteObject(bmp);
    DeleteDC(src);
}

HFONT FontOf(Ctx& c, HFONT f) { return f ? f : c.f->ui; }

// ------------------------------------------------------------------ 控件
void Card(Ctx& c, const Rect& r, COLORREF fill) {
    c.p->Fill(r, fill);
    c.p->Frame(r, Theme::Separator, 1);
}

bool Button(Ctx& c, int id, const Rect& r, const std::wstring& label, bool enabled, bool primary) {
    bool over, pressed;
    bool clicked = c.in->Interact(id, r, enabled, over, pressed);
    if (!enabled) over = pressed = false;

    COLORREF face = primary ? Theme::Accent : Theme::BtnFace;
    if (!enabled) face = Mix(Theme::BtnFace, Theme::BgWindow, 60);
    else if (pressed) face = primary ? Mix(Theme::Accent, RGB(0, 0, 0), 20) : Theme::BtnFaceDown;
    else if (over) face = primary ? Mix(Theme::Accent, RGB(255, 255, 255), 12) : Theme::BtnFaceHot;

    COLORREF border = primary ? Mix(Theme::Accent, RGB(0, 0, 0), 15) : Theme::Separator;
    c.p->Fill(r, face);
    c.p->Frame(r, border, 1);
    COLORREF tc = primary ? Theme::TextOnAccent : (enabled ? Theme::Text : Theme::TextDim);
    c.p->Text(label, r, tc, c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    return clicked;
}

bool IconButton(Ctx& c, int id, const Rect& r, const wchar_t* glyph, bool enabled,
                COLORREF hoverColor, COLORREF glyphColor) {
    bool over, pressed;
    bool clicked = c.in->Interact(id, r, enabled, over, pressed);
    if (!enabled) over = pressed = false;
    if (over || pressed) c.p->Fill(r, pressed ? Mix(hoverColor, RGB(0, 0, 0), 10) : hoverColor);

    HFONT old = (HFONT)SelectObject(c.p->Dc(), c.f->ui);
    SetTextColor(c.p->Dc(), enabled ? glyphColor : Theme::TextDim);
    RECT rc = { r.x, r.y, r.Right(), r.Bottom() };
    DrawTextW(c.p->Dc(), glyph, -1, &rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(c.p->Dc(), old);
    return clicked;
}

bool Checkbox(Ctx& c, int id, const Rect& r, bool& value, bool enabled) {
    // 整行可点击
    bool over, pressed;
    bool clicked = c.in->Interact(id, r, enabled, over, pressed);

    int s = 14;
    Rect box = { r.x + 2, r.y + (r.h - s) / 2, s, s };
    COLORREF bg = value ? Theme::Accent : (over ? Theme::BtnFaceHot : RGB(255, 255, 255));
    if (!enabled) bg = Mix(bg, Theme::BgWindow, 50);
    c.p->Fill(box, bg);
    c.p->Frame(box, value ? Mix(Theme::Accent, RGB(0, 0, 0), 20) : Theme::TextDim, 1);
    if (value) {
        HPEN pen = CreatePen(PS_SOLID, 2, Theme::TextOnAccent);
        HGDIOBJ op = SelectObject(c.p->Dc(), pen);
        MoveToEx(c.p->Dc(), box.x + 3, box.y + 7, nullptr);
        LineTo(c.p->Dc(), box.x + 6, box.y + 10);
        LineTo(c.p->Dc(), box.x + 11, box.y + 4);
        SelectObject(c.p->Dc(), op);
        DeleteObject(pen);
    }
    if (clicked) value = !value;
    return clicked;
}

void Label(Ctx& c, const Rect& r, const std::wstring& text, COLORREF color, HFONT font, UINT flags) {
    c.p->Text(text, r, color, FontOf(c, font), flags);
}

void Panel(Ctx& c, const Rect& r, COLORREF fill, COLORREF border) {
    c.p->Fill(r, fill);
    if (border != CLR_INVALID) c.p->Frame(r, border, 1);
}

void Separator(Ctx& c, int x, int y, int w) { c.p->HLine(x, y, w, Theme::Separator); }

bool Tab(Ctx& c, int id, const Rect& r, const std::wstring& label, bool selected) {
    bool over, pressed;
    bool clicked = c.in->Interact(id, r, true, over, pressed);
    if (selected) {
        c.p->Fill(r, Theme::BgPanel);
        c.p->HLine(r.x, r.y, r.w, Theme::Accent);
        c.p->HLine(r.x, r.y + 1, r.w, Theme::Accent);
    } else if (over) {
        c.p->Fill(r, Theme::BgHover);
    }
    c.p->Text(label, r, selected ? Theme::Accent : Theme::Text,
              selected ? c.f->uiBold : c.f->ui, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    return clicked;
}

bool ScrollBar(Ctx& c, int id, const Rect& r, int& offset, int contentSize, int viewSize) {
    if (contentSize <= viewSize || r.h <= 24) { offset = 0; return false; }
    c.p->Fill(r, Mix(Theme::BgHeader, RGB(255, 255, 255), 35));
    int maxOff = contentSize - viewSize;
    int thumbH = std::max(24, (int)((i64)r.h * viewSize / contentSize));
    thumbH = std::min(thumbH, r.h);
    int thumbY = (int)((i64)(r.h - thumbH) * offset / std::max(1, maxOff));
    Rect thumb = { r.x + 1, r.y + thumbY, r.w - 2, thumbH };

    bool over = r.Contains(c.in->mouseX, c.in->mouseY);
    bool changed = false;
    if (over && c.in->wheel) {
        offset = std::max(0, std::min(maxOff, offset - c.in->wheel * 60));
        changed = true;
    }
    if (over && c.in->mousePressed) {
        if (thumb.Contains(c.in->mouseX, c.in->mouseY)) {
            c.in->activeId = id;
            c.in->usedMouse = true;
        } else {
            offset += (c.in->mouseY < thumb.y) ? -viewSize : viewSize;
            offset = std::max(0, std::min(maxOff, offset));
            changed = true;
            c.in->usedMouse = true;
        }
    }
    if (c.in->activeId == id) {
        static std::map<int, std::pair<int, int>> drag;
        if (c.in->mousePressed && thumb.Contains(c.in->mouseX, c.in->mouseY))
            drag[id] = { c.in->mouseY, offset };
        if (drag.count(id)) {
            int dy = c.in->mouseY - drag[id].first;
            int no = drag[id].second + (int)((i64)dy * maxOff / std::max(1, r.h - thumbH));
            no = std::max(0, std::min(maxOff, no));
            if (no != offset) { offset = no; changed = true; }
        }
        if (!c.in->mouseDown) { c.in->activeId = 0; drag.erase(id); }
    }
    bool hover = over || c.in->activeId == id;
    c.p->RoundedFill(thumb, 3,
        hover ? Mix(Theme::TextDim, RGB(255, 255, 255), 25) : Mix(Theme::TextDim, RGB(255, 255, 255), 55),
        CLR_INVALID);
    return changed;
}

bool MenuBarItem(Ctx& c, int id, const Rect& r, const std::wstring& label, bool open) {
    bool over = r.Contains(c.in->mouseX, c.in->mouseY);
    if (over) c.in->hotId = id;
    if (open || over) c.p->Fill(r, open ? Theme::BgHover : Mix(Theme::BgHeader, RGB(255, 255, 255), 45));
    c.p->Text(label, r, open ? Theme::Accent : Theme::Text, c.f->ui,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    if (over && c.in->mouseReleased) { c.in->clickedId = id; c.in->usedMouse = true; return true; }
    return false;
}

void ProgressBar(Ctx& c, const Rect& r, float ratio, COLORREF color) {
    c.p->Fill(r, Mix(Theme::BgHeader, RGB(255, 255, 255), 60));
    c.p->Frame(r, Theme::Separator, 1);
    ratio = std::max(0.0f, std::min(1.0f, ratio));
    Rect fill = { r.x + 1, r.y + 1, (int)((r.w - 2) * ratio), r.h - 2 };
    if (fill.w > 0) c.p->Fill(fill, color);
}

} // namespace ui
