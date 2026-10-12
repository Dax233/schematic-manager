#pragma once
#include "common.h"
#include "render.h"

// 极简自绘扁平 UI 框架（GDI + 双缓冲位图）
namespace ui {

struct Rect {
    int x = 0, y = 0, w = 0, h = 0;
    int Right() const { return x + w; }
    int Bottom() const { return y + h; }
    bool Contains(int px, int py) const { return px >= x && py >= y && px < x + w && py < y + h; }
    Rect Inflate(int d) const { return { x - d, y - d, w + 2 * d, h + 2 * d }; }
    Rect Offset(int dx, int dy) const { return { x + dx, y + dy, w, h }; }
    bool Empty() const { return w <= 0 || h <= 0; }
};

inline COLORREF Mix(COLORREF a, COLORREF b, int pctB) {
    if (pctB < 0) pctB = 0;
    if (pctB > 100) pctB = 100;
    int r = (GetRValue(a) * (100 - pctB) + GetRValue(b) * pctB) / 100;
    int g = (GetGValue(a) * (100 - pctB) + GetGValue(b) * pctB) / 100;
    int bl = (GetBValue(a) * (100 - pctB) + GetBValue(b) * pctB) / 100;
    return RGB(r, g, bl);
}

struct Fonts {
    HFONT ui = nullptr, uiBold = nullptr, small = nullptr, smallBold = nullptr, title = nullptr;
    void Create(int dpi);
    void Destroy();
};

class Painter {
public:
    void Begin(HDC dc, int w, int h);
    void End();

    HDC Dc() const { return memdc_; }
    int Width() const { return w_; }
    int Height() const { return h_; }

    void Fill(const Rect& r, COLORREF c);
    void FillA(const Rect& r, u32 argb);
    void Frame(const Rect& r, COLORREF c, int t = 1);
    void HLine(int x, int y, int w, COLORREF c);
    void VLine(int x, int y, int h, COLORREF c);
    void RoundedFill(const Rect& r, int radius, COLORREF fill, COLORREF border);

    void Text(const std::wstring& s, const Rect& r, COLORREF c, HFONT f,
              UINT flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

    void PushClip(const Rect& r);
    void PopClip();
    void ResetClip();

    // 绘制 RGBA 图像（自动处理透明，缩放用 HALFTONE）
    void Image(const render::Image& img, const Rect& dst, bool smooth = false);
    // 纯色描边圆角
    void RoundedFrame(const Rect& r, int radius, COLORREF c);

private:
    HDC     dc_ = nullptr;
    HDC     memdc_ = nullptr;
    HBITMAP bmp_ = nullptr, oldbmp_ = nullptr;
    int     w_ = 0, h_ = 0;
    std::vector<HRGN> clips_;
};

struct Input {
    int  mouseX = 0, mouseY = 0;
    bool mouseDown = false;
    bool mousePressed = false;
    bool mouseReleased = false;
    bool mouseDoubleClick = false;
    int  wheel = 0;
    bool ctrl = false, shift = false;

    int  hotId = 0;
    int  activeId = 0;
    int  clickedId = 0;
    int  doubleClickedId = 0;
    bool usedMouse = false;      // 本帧是否已有控件消费了点击

    void NewFrame() {
        mousePressed = false; mouseReleased = false; mouseDoubleClick = false;
        wheel = 0; clickedId = 0; doubleClickedId = 0; usedMouse = false;
    }
    // 标准控件交互：返回是否被点击
    bool Interact(int id, const Rect& r, bool enabled, bool& over, bool& pressed) {
        over = enabled && r.Contains(mouseX, mouseY);
        if (over) hotId = id;
        if (over && mousePressed) { activeId = id; usedMouse = true; }
        pressed = (activeId == id) && over && mouseDown;
        bool clicked = false;
        if (mouseReleased) {
            if (activeId == id) {
                if (over) clicked = true;
                activeId = 0;
            }
        }
        return enabled && clicked;
    }
};

struct Ctx {
    Painter* p = nullptr;
    Fonts*   f = nullptr;
    Input*   in = nullptr;
    bool     enabled = true;
    int      dpi = 96;
};

inline int Id(const char* tag, int index) {
    u32 h = 2166136261u;
    for (const char* s = tag; *s; ++s) { h ^= (u8)*s; h *= 16777619u; }
    h ^= (u32)index; h *= 16777619u;
    return (int)(h & 0x7FFFFFFF);
}

// ---- 控件
bool Button(Ctx& c, int id, const Rect& r, const std::wstring& label,
            bool enabled = true, bool primary = false);
bool IconButton(Ctx& c, int id, const Rect& r, const wchar_t* glyph, bool enabled = true,
                COLORREF hoverColor = Theme::BtnFaceHot, COLORREF glyphColor = Theme::Text);
bool Checkbox(Ctx& c, int id, const Rect& r, bool& value, bool enabled = true);
void Label(Ctx& c, const Rect& r, const std::wstring& text, COLORREF color,
           HFONT font = nullptr, UINT flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
void Panel(Ctx& c, const Rect& r, COLORREF fill, COLORREF border = CLR_INVALID);
void Separator(Ctx& c, int x, int y, int w);
bool Tab(Ctx& c, int id, const Rect& r, const std::wstring& label, bool selected);
bool ScrollBar(Ctx& c, int id, const Rect& r, int& offset, int contentSize, int viewSize);
bool MenuBarItem(Ctx& c, int id, const Rect& r, const std::wstring& label, bool open);
void ProgressBar(Ctx& c, const Rect& r, float ratio, COLORREF color);
void Card(Ctx& c, const Rect& r, COLORREF fill = Theme::BgPanel);
HFONT FontOf(Ctx& c, HFONT f);

} // namespace ui
