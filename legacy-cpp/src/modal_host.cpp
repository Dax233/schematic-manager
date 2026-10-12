// 弹窗宿主：每个弹窗都是一个独立的顶层窗口（受主窗口 owner 约束）
// 这样 z 序、输入隔离都由 Windows 保证，不再依赖自绘覆盖层。
#include "app_internal.h"
#include "i18n.h"

namespace app {

struct ModalHost {
    HWND        hwnd = nullptr;
    HWND        owner = nullptr;
    ui::Painter painter;
    ui::Input   input;
    int         w = 0, h = 0;
    bool        tracking = false;
    bool        closing = false;
};
static ModalHost g_mh;
static const wchar_t* kModalClass = L"PM_ModalHost_Wnd";

// 窗口已建好后由 app_modal.cpp 实现：按 st.modal 绘制到矩形内
void DrawModalIn(ui::Ctx& c, const ui::Rect& r);
void EnsureEditsFor(HWND parent);
static void DoCloseModal();

bool ModalHosted() { return g_mh.hwnd != nullptr; }
HWND ModalHwnd() { return g_mh.hwnd; }

static void ModalSize(int kind, int& w, int& h) {
    switch (kind) {
        case MODAL_META:    w = 560; h = 320; break;
        case MODAL_VERSION: w = 620; h = 700; break;
        case MODAL_REPLACE: w = 880; h = 700; break;
        case MODAL_BACKUP:  w = 740; h = 580; break;
        case MODAL_LOG:     w = 780; h = 580; break;
        case MODAL_ABOUT:   w = 640; h = 560; break;
        case MODAL_PACKS:   w = 1300; h = 740; break;
        case MODAL_BLOCKPICK: w = 940; h = 660; break;
        case MODAL_CONVERT:   w = 1120; h = 700; break;
        default:            w = 600; h = 420; break;
    }
}

static void ModalPaint(HWND hwnd) {
    State& st = Get();
    // 与主窗口一致：先清空需求表，只由本次绘制决定要哪些输入框。
    // 漏掉这一步会把主窗口的输入框（备注、搜索）误建到弹窗下，
    // 弹窗销毁后它们随之消失，表现为"退出后所有输入框都点不动"。
    for (int i = 0; i < EDIT_SLOT_COUNT; ++i) g_editWant[i] = false;
    PAINTSTRUCT ps;
    HDC dc = BeginPaint(hwnd, &ps);
    if (g_mh.w > 0 && g_mh.h > 0) {
        g_mh.painter.Begin(dc, g_mh.w, g_mh.h);
        ui::Ctx c{ &g_mh.painter, &st.fonts, &g_mh.input, true, st.dpi };
        g_mh.painter.Fill({ 0, 0, g_mh.w, g_mh.h }, Theme::BgPanel);
        DrawModalIn(c, { 0, 0, g_mh.w, g_mh.h });
        g_mh.painter.Frame({ 0, 0, g_mh.w, g_mh.h }, Theme::Border, 2);
        g_mh.painter.End();
        EnsureEditsFor(hwnd);       // 弹窗自己的输入框
    }
    EndPaint(hwnd, &ps);
    // 关键：每帧结束后清掉 pressed/released，否则一次点击会被无限重复触发
    g_mh.input.NewFrame();
}

static LRESULT CALLBACK ModalProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
        case WM_NCCALCSIZE:
            if (wp == TRUE) {
                if (IsZoomed(hwnd)) {
                    // 最大化时留出任务栏
                }
                return 0;
            }
            break;

        case WM_NCHITTEST: {
            POINT pt = { GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            ScreenToClient(hwnd, &pt);
            // 右上角关闭按钮必须走客户区，否则会被当成标题栏拖动，按钮收不到点击
            if (pt.y >= 0 && pt.y < 36 && pt.x >= g_mh.w - 48 && pt.x < g_mh.w)
                return HTCLIENT;
            if (pt.y >= 0 && pt.y < 36) return HTCAPTION;   // 其余标题栏可拖动
            return HTCLIENT;
        }
        case WM_GETMINMAXINFO: {
            MINMAXINFO* mmi = (MINMAXINFO*)lp;
            mmi->ptMinTrackSize.x = g_mh.w;
            mmi->ptMinTrackSize.y = g_mh.h;
            mmi->ptMaxTrackSize.x = g_mh.w;
            mmi->ptMaxTrackSize.y = g_mh.h;
            return 0;
        }
        case WM_NCACTIVATE:
            return TRUE;
        case WM_ERASEBKGND:
            return 1;
        case WM_PAINT:
            ModalPaint(hwnd);
            return 0;

        case WM_MOUSEMOVE:
            if (!g_mh.tracking) {
                TRACKMOUSEEVENT tme = { sizeof(tme), TME_LEAVE, hwnd, 0 };
                TrackMouseEvent(&tme);
                g_mh.tracking = true;
            }
            g_mh.input.mouseX = GET_X_LPARAM(lp);
            g_mh.input.mouseY = GET_Y_LPARAM(lp);
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case WM_MOUSELEAVE:
            g_mh.tracking = false;
            g_mh.input.mouseX = -10000;
            g_mh.input.mouseY = -10000;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case WM_LBUTTONDOWN:
            SetFocus(hwnd);
            g_mh.input.mouseX = GET_X_LPARAM(lp);
            g_mh.input.mouseY = GET_Y_LPARAM(lp);
            g_mh.input.mouseDown = true;
            g_mh.input.mousePressed = true;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case WM_LBUTTONUP:
            g_mh.input.mouseX = GET_X_LPARAM(lp);
            g_mh.input.mouseY = GET_Y_LPARAM(lp);
            g_mh.input.mouseDown = false;
            g_mh.input.mouseReleased = true;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case WM_MOUSEWHEEL: {
            POINT pt = { GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            ScreenToClient(hwnd, &pt);
            g_mh.input.mouseX = pt.x;
            g_mh.input.mouseY = pt.y;
            g_mh.input.wheel = GET_WHEEL_DELTA_WPARAM(wp) / WHEEL_DELTA;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        }
        case WM_COMMAND:
            if (HIWORD(wp) == EN_CHANGE) { InvalidateRect(hwnd, nullptr, FALSE); return 0; }
            break;
        case WM_KEYDOWN:
            if (wp == VK_ESCAPE) { PostMessageW(hwnd, WM_CLOSE, 0, 0); return 0; }
            break;
        case WM_CLOSE:
            DoCloseModal();
            return 0;
        case WM_DESTROY:
            return 0;
        default: break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

static bool EnsureClass(HINSTANCE hi) {
    static bool done = false;
    if (done) return true;
    WNDCLASSEXW wc = {};
    wc.cbSize = sizeof(wc);
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
    wc.lpfnWndProc = ModalProc;
    wc.hInstance = hi;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hbrBackground = nullptr;
    wc.lpszClassName = kModalClass;
    wc.hIcon = (HICON)LoadImageW(hi, MAKEINTRESOURCEW(1), IMAGE_ICON, 32, 32, 0);
    done = RegisterClassExW(&wc) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS;
    return done;
}

bool OpenModalHost(int kind) {
    State& st = Get();
    if (g_mh.hwnd) return true;
    HINSTANCE hi = (HINSTANCE)GetWindowLongPtrW(st.hwnd, GWLP_HINSTANCE);
    if (!EnsureClass(hi)) return false;

    int w, h;
    ModalSize(kind, w, h);
    if (w > st.width - 60) w = st.width - 60;
    if (h > st.height - 80) h = st.height - 80;

    RECT orc;
    GetWindowRect(st.hwnd, &orc);
    int x = orc.left + (orc.right - orc.left - w) / 2;
    int y = orc.top + (orc.bottom - orc.top - h) / 2;

    g_mh.w = w; g_mh.h = h;
    g_mh.input = ui::Input();
    g_mh.tracking = false;
    g_mh.closing = false;
    g_mh.owner = st.hwnd;

    // 注意：不能用 WS_EX_DLGMODALFRAME —— 那个扩展样式会让系统画一圈立体边框，
    // 与主窗口的平面 2px 灰边不统一。边框完全由 ModalPaint 自己绘制。
    g_mh.hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        kModalClass, i18n::T("app.title"),
        WS_POPUP | WS_CLIPCHILDREN,
        x, y, w, h, st.hwnd, nullptr, hi, nullptr);
    if (!g_mh.hwnd) return false;

    // 经典模态：禁用主窗口，输入不可能跑到主界面
    EnableWindow(st.hwnd, FALSE);
    ShowWindow(g_mh.hwnd, SW_SHOW);
    SetWindowPos(g_mh.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
    SetForegroundWindow(g_mh.hwnd);
    SetFocus(g_mh.hwnd);
    InvalidateRect(g_mh.hwnd, nullptr, FALSE);
    return true;
}

// 真正销毁（只从 WM_CLOSE 调用，保证不在 WM_PAINT 里销毁自己）
static void DoCloseModal() {
    State& st = Get();
    if (!g_mh.hwnd || g_mh.closing) return;
    g_mh.closing = true;
    DestroyEdits();
    HWND h = g_mh.hwnd;
    g_mh.hwnd = nullptr;
    DestroyWindow(h);
    EnableWindow(st.hwnd, TRUE);
    SetForegroundWindow(st.hwnd);
    SetFocus(st.hwnd);
    st.modal = MODAL_NONE;
    InvalidateRect(st.hwnd, nullptr, FALSE);
}

// 外部请求关闭：投递 WM_CLOSE，避免在绘制过程中销毁窗口
void CloseModalHost() {
    if (!g_mh.hwnd || g_mh.closing) return;
    PostMessageW(g_mh.hwnd, WM_CLOSE, 0, 0);
}

} // namespace app
