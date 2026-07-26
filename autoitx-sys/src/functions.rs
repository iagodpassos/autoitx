//! The `AU3_*` function list — the single source of truth.
//!
//! Declared once here and consumed by two very different code generators:
//!
//! - [`crate::api`] turns it into the [`Au3`](crate::Au3) vtable and loader.
//! - `xtask-mock-dll` turns it into a stand-in DLL that records its arguments,
//!   which is how the marshalling layer gets tested without Windows.
//!
//! Keeping one list means the mock cannot drift from the real bindings: a
//! signature fixed in one place is fixed in both, and the mock exercising a
//! function proves that *this* declaration is callable, not a copy of it.
//!
//! This is the "x-macro" pattern: the list is a macro that hands itself to
//! whichever generator you name.

/// Passes the full `AU3_*` declaration list to the macro named by `$callback`.
///
/// The callback must accept a sequence of
/// `$(#[meta])* fn NAME(arg: Type, ...) $(-> Ret)?;` items.
///
/// ```ignore
/// macro_rules! count_them { ($(fn $n:ident($($a:ident:$t:ty),*$(,)?) $(-> $r:ty)?;)+)
///     => { const N: usize = [$(stringify!($n)),+].len(); } }
/// au3_functions!(count_them);   // N == 117
/// ```
#[macro_export]
macro_rules! au3_functions {
    ($callback:ident) => {
        $callback! {
        // ---- Initialisation and error state -----------------------------------

        /// Establishes AutoItX's default option table. Called once at load.
        fn AU3_Init();

        /// The error flag set by the *previous* AU3 call on this thread.
        ///
        /// Thread-global and overwritten by every call, so it must be read
        /// immediately after the call it describes, under the same lock.
        fn AU3_error() -> i32;

        // ---- Options ----------------------------------------------------------

        /// Sets an option; returns the previous value.
        ///
        /// Passing `AU3_INTDEFAULT` as `value` reads the current value without
        /// changing it — which is how the safe layer's defaults-parity test works.
        fn AU3_AutoItSetOption(option: PCWSTR, value: i32) -> i32;
        /// Alias of [`AU3_AutoItSetOption`](Au3::AU3_AutoItSetOption).
        fn AU3_Opt(option: PCWSTR, value: i32) -> i32;

        // ---- Clipboard --------------------------------------------------------

        /// Reads the clipboard as text. Sets the error flag if it holds no text.
        fn AU3_ClipGet(clip: PWSTR, buf_size: i32);
        /// Replaces the clipboard contents with text.
        fn AU3_ClipPut(clip: PCWSTR);

        // ---- Controls (by title/text/control, and by handle) ------------------

        fn AU3_ControlClick(
            title: PCWSTR, text: PCWSTR, control: PCWSTR,
            button: PCWSTR, clicks: i32, x: i32, y: i32,
        ) -> i32;
        fn AU3_ControlClickByHandle(
            hwnd: HWND, hctrl: HWND, button: PCWSTR, clicks: i32, x: i32, y: i32,
        ) -> i32;
        fn AU3_ControlCommand(
            title: PCWSTR, text: PCWSTR, control: PCWSTR,
            command: PCWSTR, extra: PCWSTR, result: PWSTR, buf_size: i32,
        );
        fn AU3_ControlCommandByHandle(
            hwnd: HWND, hctrl: HWND, command: PCWSTR, extra: PCWSTR,
            result: PWSTR, buf_size: i32,
        );
        fn AU3_ControlListView(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, command: PCWSTR,
            extra1: PCWSTR, extra2: PCWSTR, result: PWSTR, buf_size: i32,
        );
        fn AU3_ControlListViewByHandle(
            hwnd: HWND, hctrl: HWND, command: PCWSTR,
            extra1: PCWSTR, extra2: PCWSTR, result: PWSTR, buf_size: i32,
        );
        fn AU3_ControlDisable(title: PCWSTR, text: PCWSTR, control: PCWSTR) -> i32;
        fn AU3_ControlDisableByHandle(hwnd: HWND, hctrl: HWND) -> i32;
        fn AU3_ControlEnable(title: PCWSTR, text: PCWSTR, control: PCWSTR) -> i32;
        fn AU3_ControlEnableByHandle(hwnd: HWND, hctrl: HWND) -> i32;
        fn AU3_ControlFocus(title: PCWSTR, text: PCWSTR, control: PCWSTR) -> i32;
        fn AU3_ControlFocusByHandle(hwnd: HWND, hctrl: HWND) -> i32;
        fn AU3_ControlGetFocus(
            title: PCWSTR, text: PCWSTR, control_with_focus: PWSTR, buf_size: i32,
        );
        fn AU3_ControlGetFocusByHandle(hwnd: HWND, control_with_focus: PWSTR, buf_size: i32);
        fn AU3_ControlGetHandle(hwnd: HWND, control: PCWSTR) -> HWND;
        fn AU3_ControlGetHandleAsText(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, ret_text: PWSTR, buf_size: i32,
        );
        fn AU3_ControlGetPos(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, rect: *mut RECT,
        ) -> i32;
        fn AU3_ControlGetPosByHandle(hwnd: HWND, hctrl: HWND, rect: *mut RECT) -> i32;
        fn AU3_ControlGetText(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, control_text: PWSTR, buf_size: i32,
        );
        fn AU3_ControlGetTextByHandle(
            hwnd: HWND, hctrl: HWND, control_text: PWSTR, buf_size: i32,
        );
        fn AU3_ControlHide(title: PCWSTR, text: PCWSTR, control: PCWSTR) -> i32;
        fn AU3_ControlHideByHandle(hwnd: HWND, hctrl: HWND) -> i32;
        fn AU3_ControlMove(
            title: PCWSTR, text: PCWSTR, control: PCWSTR,
            x: i32, y: i32, width: i32, height: i32,
        ) -> i32;
        fn AU3_ControlMoveByHandle(
            hwnd: HWND, hctrl: HWND, x: i32, y: i32, width: i32, height: i32,
        ) -> i32;
        fn AU3_ControlSend(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, send_text: PCWSTR, mode: i32,
        ) -> i32;
        fn AU3_ControlSendByHandle(
            hwnd: HWND, hctrl: HWND, send_text: PCWSTR, mode: i32,
        ) -> i32;
        fn AU3_ControlSetText(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, control_text: PCWSTR,
        ) -> i32;
        fn AU3_ControlSetTextByHandle(hwnd: HWND, hctrl: HWND, control_text: PCWSTR) -> i32;
        fn AU3_ControlShow(title: PCWSTR, text: PCWSTR, control: PCWSTR) -> i32;
        fn AU3_ControlShowByHandle(hwnd: HWND, hctrl: HWND) -> i32;
        fn AU3_ControlTreeView(
            title: PCWSTR, text: PCWSTR, control: PCWSTR, command: PCWSTR,
            extra1: PCWSTR, extra2: PCWSTR, result: PWSTR, buf_size: i32,
        );
        fn AU3_ControlTreeViewByHandle(
            hwnd: HWND, hctrl: HWND, command: PCWSTR,
            extra1: PCWSTR, extra2: PCWSTR, result: PWSTR, buf_size: i32,
        );

        // ---- Mapped network drives (Windows-only concept) ---------------------

        fn AU3_DriveMapAdd(
            device: PCWSTR, share: PCWSTR, flags: i32,
            user: PCWSTR, pwd: PCWSTR, result: PWSTR, buf_size: i32,
        );
        fn AU3_DriveMapDel(device: PCWSTR) -> i32;
        fn AU3_DriveMapGet(device: PCWSTR, mapping: PWSTR, buf_size: i32);

        // ---- Privileges -------------------------------------------------------

        /// Non-zero if the current process is running elevated.
        fn AU3_IsAdmin() -> i32;

        // ---- Mouse ------------------------------------------------------------

        /// Clicks at a coordinate. `speed` is 0 (instant) to 100; AutoIt's default
        /// is 10.
        fn AU3_MouseClick(button: PCWSTR, x: i32, y: i32, clicks: i32, speed: i32) -> i32;
        fn AU3_MouseClickDrag(
            button: PCWSTR, x1: i32, y1: i32, x2: i32, y2: i32, speed: i32,
        ) -> i32;
        fn AU3_MouseDown(button: PCWSTR);
        /// The system cursor shape. 2 is the arrow and 5 the I-beam; both mean the
        /// target application is idle.
        fn AU3_MouseGetCursor() -> i32;
        fn AU3_MouseGetPos(point: *mut POINT);
        fn AU3_MouseMove(x: i32, y: i32, speed: i32) -> i32;
        fn AU3_MouseUp(button: PCWSTR);
        fn AU3_MouseWheel(direction: PCWSTR, clicks: i32);

        // ---- Pixels -----------------------------------------------------------

        fn AU3_PixelChecksum(rect: *mut RECT, step: i32) -> u32;
        fn AU3_PixelGetColor(x: i32, y: i32) -> i32;
        fn AU3_PixelSearch(
            rect: *mut RECT, colour: i32, variation: i32, step: i32, result: *mut POINT,
        );

        // ---- Processes --------------------------------------------------------

        fn AU3_ProcessClose(process: PCWSTR) -> i32;
        fn AU3_ProcessExists(process: PCWSTR) -> i32;
        fn AU3_ProcessSetPriority(process: PCWSTR, priority: i32) -> i32;
        fn AU3_ProcessWait(process: PCWSTR, timeout: i32) -> i32;
        fn AU3_ProcessWaitClose(process: PCWSTR, timeout: i32) -> i32;

        // ---- Launching --------------------------------------------------------

        fn AU3_Run(program: PCWSTR, dir: PCWSTR, show_flag: i32) -> i32;
        fn AU3_RunWait(program: PCWSTR, dir: PCWSTR, show_flag: i32) -> i32;
        fn AU3_RunAs(
            user: PCWSTR, domain: PCWSTR, password: PCWSTR, logon_flag: i32,
            program: PCWSTR, dir: PCWSTR, show_flag: i32,
        ) -> i32;
        fn AU3_RunAsWait(
            user: PCWSTR, domain: PCWSTR, password: PCWSTR, logon_flag: i32,
            program: PCWSTR, dir: PCWSTR, show_flag: i32,
        ) -> i32;

        // ---- Keyboard and misc ------------------------------------------------

        /// Sends keystrokes. `mode` 0 interprets `{}!+^#`; mode 1 sends them
        /// literally.
        fn AU3_Send(send_text: PCWSTR, mode: i32);
        fn AU3_Shutdown(flags: i32) -> i32;
        fn AU3_Sleep(milliseconds: i32);
        fn AU3_ToolTip(tip: PCWSTR, x: i32, y: i32);

        // ---- Status bars ------------------------------------------------------

        fn AU3_StatusbarGetText(
            title: PCWSTR, text: PCWSTR, part: i32, status_text: PWSTR, buf_size: i32,
        ) -> i32;
        fn AU3_StatusbarGetTextByHandle(
            hwnd: HWND, part: i32, status_text: PWSTR, buf_size: i32,
        ) -> i32;

        // ---- Windows ----------------------------------------------------------

        fn AU3_WinActivate(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinActivateByHandle(hwnd: HWND) -> i32;
        fn AU3_WinActive(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinActiveByHandle(hwnd: HWND) -> i32;
        fn AU3_WinClose(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinCloseByHandle(hwnd: HWND) -> i32;
        fn AU3_WinExists(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinExistsByHandle(hwnd: HWND) -> i32;
        fn AU3_WinGetCaretPos(point: *mut POINT) -> i32;
        /// Returns the window's class names, separated by `\n`.
        fn AU3_WinGetClassList(title: PCWSTR, text: PCWSTR, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinGetClassListByHandle(hwnd: HWND, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinGetClientSize(title: PCWSTR, text: PCWSTR, rect: *mut RECT) -> i32;
        fn AU3_WinGetClientSizeByHandle(hwnd: HWND, rect: *mut RECT) -> i32;
        fn AU3_WinGetHandle(title: PCWSTR, text: PCWSTR) -> HWND;
        fn AU3_WinGetHandleAsText(
            title: PCWSTR, text: PCWSTR, ret_text: PWSTR, buf_size: i32,
        );
        /// Fills `rect` as a Win32 `RECT` (left/top/right/bottom), despite AutoIt
        /// documenting `WinGetPos` as x/y/width/height.
        fn AU3_WinGetPos(title: PCWSTR, text: PCWSTR, rect: *mut RECT) -> i32;
        fn AU3_WinGetPosByHandle(hwnd: HWND, rect: *mut RECT) -> i32;
        fn AU3_WinGetProcess(title: PCWSTR, text: PCWSTR) -> DWORD;
        fn AU3_WinGetProcessByHandle(hwnd: HWND) -> DWORD;
        fn AU3_WinGetState(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinGetStateByHandle(hwnd: HWND) -> i32;
        fn AU3_WinGetText(title: PCWSTR, text: PCWSTR, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinGetTextByHandle(hwnd: HWND, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinGetTitle(title: PCWSTR, text: PCWSTR, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinGetTitleByHandle(hwnd: HWND, ret_text: PWSTR, buf_size: i32);
        fn AU3_WinKill(title: PCWSTR, text: PCWSTR) -> i32;
        fn AU3_WinKillByHandle(hwnd: HWND) -> i32;
        fn AU3_WinMenuSelectItem(
            title: PCWSTR, text: PCWSTR,
            item1: PCWSTR, item2: PCWSTR, item3: PCWSTR, item4: PCWSTR,
            item5: PCWSTR, item6: PCWSTR, item7: PCWSTR, item8: PCWSTR,
        ) -> i32;
        fn AU3_WinMenuSelectItemByHandle(
            hwnd: HWND,
            item1: PCWSTR, item2: PCWSTR, item3: PCWSTR, item4: PCWSTR,
            item5: PCWSTR, item6: PCWSTR, item7: PCWSTR, item8: PCWSTR,
        ) -> i32;
        fn AU3_WinMinimizeAll();
        fn AU3_WinMinimizeAllUndo();
        fn AU3_WinMove(
            title: PCWSTR, text: PCWSTR, x: i32, y: i32, width: i32, height: i32,
        ) -> i32;
        fn AU3_WinMoveByHandle(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> i32;
        fn AU3_WinSetOnTop(title: PCWSTR, text: PCWSTR, flag: i32) -> i32;
        fn AU3_WinSetOnTopByHandle(hwnd: HWND, flag: i32) -> i32;
        fn AU3_WinSetState(title: PCWSTR, text: PCWSTR, flags: i32) -> i32;
        fn AU3_WinSetStateByHandle(hwnd: HWND, flags: i32) -> i32;
        fn AU3_WinSetTitle(title: PCWSTR, text: PCWSTR, new_title: PCWSTR) -> i32;
        fn AU3_WinSetTitleByHandle(hwnd: HWND, new_title: PCWSTR) -> i32;
        fn AU3_WinSetTrans(title: PCWSTR, text: PCWSTR, trans: i32) -> i32;
        fn AU3_WinSetTransByHandle(hwnd: HWND, trans: i32) -> i32;
        fn AU3_WinWait(title: PCWSTR, text: PCWSTR, timeout: i32) -> i32;
        fn AU3_WinWaitByHandle(hwnd: HWND, timeout: i32) -> i32;
        fn AU3_WinWaitActive(title: PCWSTR, text: PCWSTR, timeout: i32) -> i32;
        fn AU3_WinWaitActiveByHandle(hwnd: HWND, timeout: i32) -> i32;
        fn AU3_WinWaitClose(title: PCWSTR, text: PCWSTR, timeout: i32) -> i32;
        fn AU3_WinWaitCloseByHandle(hwnd: HWND, timeout: i32) -> i32;
        fn AU3_WinWaitNotActive(title: PCWSTR, text: PCWSTR, timeout: i32) -> i32;
        fn AU3_WinWaitNotActiveByHandle(hwnd: HWND, timeout: i32) -> i32;
            }
    };
}
