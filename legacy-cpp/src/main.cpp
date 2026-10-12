// 投影文件管理器 —— Win32 外壳（无边框窗口 + 自绘标题栏）
#include "app.h"
#include "app_internal.h"
#include "i18n.h"
#include "str.h"
#include "fs.h"
#include <shlobj.h>
#include <windowsx.h>
#include "packmgr.h"


int RunSelfTest();
int RunPackTest(const wchar_t* jarArg);

static const wchar_t* kWndClass = L"PM_SchematicManager_Wnd";
static HWND g_hwnd = nullptr;
static bool g_trackingMouse = false;

// ------------------------------------------------------------------ 目录选择
namespace app {
void HandleWindowCommand(int cmd) {
    static bool s_picking = false;
    if (s_picking) return;                       // 防止重入导致无限弹窗
    s_picking = true;
    struct PickGuard { bool* f; ~PickGuard() { *f = false; } };
    PickGuard guard{ &s_picking };
    State& st = Get();
    // 弹窗打开时以弹窗为 owner，否则主窗口被禁用会导致选择框行为异常
    HWND owner = ModalHosted() ? ModalHwnd() : st.hwnd;
    if (cmd == CMD_ADD_DIR) {
        BROWSEINFOW bi = {};
        bi.hwndOwner = owner;
        bi.lpszTitle = i18n::T("menu.file.adddir");
        bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_USENEWUI;
        LPITEMIDLIST pidl = SHBrowseForFolderW(&bi);
        if (pidl) {
            wchar_t buf[MAX_PATH * 2] = L"";
            if (SHGetPathFromIDListW(pidl, buf)) SetScanDir(buf);
            CoTaskMemFree(pidl);
        }
    } else if (cmd == CMD_PACKS_ADDROOT) {
        BROWSEINFOW bi = {};
        bi.hwndOwner = owner;
        bi.lpszTitle = L"\u9009\u62e9 Minecraft \u5b89\u88c5\u76ee\u5f55\uff08\u4e0a\u5c42\u76ee\u5f55\u5373\u53ef\uff09";
        bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_USENEWUI;
        LPITEMIDLIST pidl = SHBrowseForFolderW(&bi);
        if (pidl) {
            wchar_t buf[MAX_PATH * 2] = L"";
            if (SHGetPathFromIDListW(pidl, buf)) {
                auto roots = packmgr::LoadScanRoots();
                bool dup = false;
                for (const auto& r : roots) if (_wcsicmp(r.c_str(), buf) == 0) dup = true;
                if (!dup) { roots.push_back(buf); packmgr::SaveScanRoots(roots); }
                st.detectMsg = std::wstring(L"\u5df2\u6dfb\u52a0\u626b\u63cf\u76ee\u5f55: ") + buf;
            }
            CoTaskMemFree(pidl);
        }
    }
}
} // namespace app

// ------------------------------------------------------------------ 窗口过程
static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    app::State& st = app::Get();

    switch (msg) {
        case WM_NCCALCSIZE:
            if (wp == TRUE) return 0;      // 去掉全部非客户区，自己画
            break;

        case WM_NCHITTEST: {
            POINT pt = { GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            ScreenToClient(hwnd, &pt);
            LRESULT r = 0;
            if (app::HitTestNonClient(pt.x, pt.y, r)) return r;
            return HTCLIENT;
        }

        case WM_GETMINMAXINFO: {
            MINMAXINFO* mmi = (MINMAXINFO*)lp;
            mmi->ptMinTrackSize.x = 1180;
            mmi->ptMinTrackSize.y = 620;
            HMONITOR mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            MONITORINFO mi = { sizeof(mi) };
            if (GetMonitorInfoW(mon, &mi)) {
                mmi->ptMaxPosition.x = mi.rcWork.left - mi.rcMonitor.left;
                mmi->ptMaxPosition.y = mi.rcWork.top - mi.rcMonitor.top;
                mmi->ptMaxSize.x = mi.rcWork.right - mi.rcWork.left;
                mmi->ptMaxSize.y = mi.rcWork.bottom - mi.rcWork.top;
            }
            return 0;
        }

        case WM_NCACTIVATE:
            return TRUE;      // 不画系统标题栏

        case WM_ERASEBKGND:
            return 1;

        case WM_PAINT: {
            PAINTSTRUCT ps;
            HDC dc = BeginPaint(hwnd, &ps);
            app::Paint(dc);
            EndPaint(hwnd, &ps);
            return 0;
        }

        case WM_COMMAND:
            // 输入框内容变化时刷新界面，否则"保存备注"按钮要等鼠标移动才更新
            if (HIWORD(wp) == EN_CHANGE) { InvalidateRect(hwnd, nullptr, FALSE); return 0; }
            break;

        case WM_SIZE:
            app::OnSize(LOWORD(lp), HIWORD(lp));
            return 0;

        case WM_MOUSEMOVE: {
            if (!g_trackingMouse) {
                TRACKMOUSEEVENT tme = { sizeof(tme), TME_LEAVE, hwnd, 0 };
                TrackMouseEvent(&tme);
                g_trackingMouse = true;
            }
            app::OnMouseMove(GET_X_LPARAM(lp), GET_Y_LPARAM(lp));
            return 0;
        }
        case WM_MOUSELEAVE:
            g_trackingMouse = false;
            app::OnMouseLeave();
            return 0;

        case WM_LBUTTONDOWN: {
            SetFocus(hwnd);
            app::OnLButtonDown(GET_X_LPARAM(lp), GET_Y_LPARAM(lp), false);
            return 0;
        }
        case WM_LBUTTONDBLCLK:
            app::OnLButtonDown(GET_X_LPARAM(lp), GET_Y_LPARAM(lp), true);
            return 0;
        case WM_LBUTTONUP:
            app::OnLButtonUp(GET_X_LPARAM(lp), GET_Y_LPARAM(lp));
            return 0;
        case WM_RBUTTONDOWN:
            app::OnRButtonDown(GET_X_LPARAM(lp), GET_Y_LPARAM(lp));
            return 0;

        case WM_MOUSEWHEEL: {
            POINT pt = { GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            ScreenToClient(hwnd, &pt);
            app::OnMouseWheel(GET_WHEEL_DELTA_WPARAM(wp), pt.x, pt.y);
            return 0;
        }

        case WM_KEYDOWN: {
            bool ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
            bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
            if (app::OnKeyDown((UINT)wp, ctrl, shift)) return 0;
            break;
        }
        case WM_CHAR:
            app::OnChar((wchar_t)wp);
            return 0;

        case WM_APP + 1:
            app::ExecuteCommand((int)wp);
            return 0;
        case WM_APP + 2:                 // 编辑框回车 -> 提交
            app::CommitInfoEdit((int)wp);
            return 0;
        case WM_APP + 3:                 // 编辑框 Esc -> 取消
            app::Get().infoEdit = -1;
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;

        case WM_SETCURSOR: {
            POINT pt;
            GetCursorPos(&pt);
            ScreenToClient(hwnd, &pt);
            if (app::SplitterHot()) { SetCursor(LoadCursor(nullptr, IDC_SIZEWE)); return TRUE; }
            (void)pt;
            break;
        }

        case WM_TIMER:
            if (wp == 1) app::Tick();
            return 0;

        case WM_SETFOCUS:
            InvalidateRect(hwnd, nullptr, FALSE);
            return 0;
        case WM_KILLFOCUS:
            return 0;

        case WM_DESTROY:
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            return 0;

        default: break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

// ------------------------------------------------------------------ 入口
int WINAPI wWinMain(HINSTANCE hInst, HINSTANCE, PWSTR, int nCmdShow) {
    // Win7 兼容：进程级 DPI 感知
    typedef BOOL (WINAPI *SetDpiFn)(void);
    if (HMODULE u = LoadLibraryW(L"user32.dll")) {
        SetDpiFn fn = (SetDpiFn)GetProcAddress(u, "SetProcessDPIAware");
        if (fn) fn();
    }

    i18n::DetectFromSystem();

    // 无界面自检模式
    if (wcsstr(GetCommandLineW(), L"--selftest")) return RunSelfTest();
    if (wcsstr(GetCommandLineW(), L"--packtest")) {
        const wchar_t* pt = wcsstr(GetCommandLineW(), L"--packtest");
        const wchar_t* sp = wcschr(pt, L' ');
        wchar_t jar[MAX_PATH * 2] = L"";
        if (sp) { while (*sp == L' ') ++sp; if (*sp == L'"') { ++sp; const wchar_t* q = wcschr(sp, L'"'); if (q) wcsncpy(jar, sp, (size_t)(q - sp)); } else wcsncpy(jar, sp, MAX_PATH*2-1); }
        return RunPackTest(jar);
    }

    WNDCLASSEXW wc = {};
    wc.cbSize = sizeof(wc);
    wc.style = CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS;
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
    wc.hbrBackground = nullptr;
    wc.lpszClassName = kWndClass;
    wc.hIcon = (HICON)LoadImageW(hInst, MAKEINTRESOURCEW(1), IMAGE_ICON, 32, 32, 0);
    wc.hIconSm = (HICON)LoadImageW(hInst, MAKEINTRESOURCEW(1), IMAGE_ICON, 16, 16, 0);
    if (!RegisterClassExW(&wc)) {
        MessageBoxW(nullptr, L"窗口类注册失败", i18n::T("app.title"), MB_ICONERROR);
        return 1;
    }

    // 居中 + 适合屏幕的默认尺寸
    int sw = GetSystemMetrics(SM_CXSCREEN);
    int sh = GetSystemMetrics(SM_CYSCREEN);
    int w = std::min(1560, sw - 60);
    int h = std::min(980, sh - 60);
    if (w < 1180) w = 1180;
    if (h < 700) h = 700;
    int x = (sw - w) / 2, y = (sh - h) / 2;

    g_hwnd = CreateWindowExW(
        WS_EX_APPWINDOW,
        kWndClass, i18n::T("app.title"),
        WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU | WS_CLIPCHILDREN,
        x, y, w, h, nullptr, nullptr, hInst, nullptr);
    if (!g_hwnd) {
        MessageBoxW(nullptr, L"窗口创建失败", i18n::T("app.title"), MB_ICONERROR);
        return 1;
    }

    app::Get().hwnd = g_hwnd;
    app::Get().width = w;
    app::Get().height = h;
    app::Init(g_hwnd);

    // 隐藏调试参数：启动时直接打开某个弹窗（便于验证界面）
    {
        const wchar_t* cl = GetCommandLineW();
        if (const wchar_t* bt = wcsstr(cl, L"--brighttest=")) {
        std::wstring f(bt + 13);
        while (!f.empty() && (f.front() == L'"' || f.front() == L' ')) f.erase(f.begin());
        while (!f.empty() && (f.back() == L'"' || f.back() == L' ')) f.pop_back();
        app::RunBrightTest(f);
        return 0;
    }
    const wchar_t* om = wcsstr(cl, L"--openmodal=");
        if (om) {
            int n = _wtoi(om + 12);
            if (n > 0 && n <= 8) {
                app::Get().cur.reset(new ltm::Schematic());
                std::string e2;
                if (!app::Get().files.empty())
                    app::Get().cur->Load(app::Get().files[0].sum.path, e2);
                const int cmds[10] = {0, app::CMD_EDIT_META, app::CMD_EDIT_VERSION, app::CMD_EDIT_REPLACE,
                                     app::CMD_EDIT_PREVIEW, app::CMD_BACKUP, app::CMD_DUP, app::CMD_LOG, app::CMD_BLK_PICK, app::CMD_PACKS_OPEN };
                PostMessageW(g_hwnd, WM_APP + 1, (WPARAM)cmds[n], 0);
            }
        }
    }

    SetTimer(g_hwnd, 1, 200, nullptr);
    ShowWindow(g_hwnd, nCmdShow);
    UpdateWindow(g_hwnd);

    // 首次自动扫描
    if (!app::Get().scanDir.empty()) app::RescanAsync();

    MSG msg;
    while (GetMessageW(&msg, nullptr, 0, 0) > 0) {
        // EDIT 控件需要消息预处理
        HWND h = GetFocus();
        wchar_t cls[32] = L"";
        if (h) GetClassNameW(h, cls, 32);
        if (wcscmp(cls, L"Edit") == 0) {
            if (IsDialogMessageW(g_hwnd, &msg)) continue;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    app::Shutdown();
    return 0;
}
