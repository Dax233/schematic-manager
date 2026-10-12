// 测试构建的桩：app::HandleWindowCommand 只在 src/main.cpp（被 build.ps1 -Test 排除）中定义，
// 而 app_modal.cpp 的 ExecuteCommand 会引用它。为了 tests.exe 能链接通过，这里补一个空实现。
// 注意：测试不会真正调用菜单/命令路径，因此空实现不影响任何断言。
#include "../src/app_internal.h"

namespace app {

void HandleWindowCommand(int cmd) {
    (void)cmd;   // 测试环境下不处理窗口命令
}

} // namespace app
