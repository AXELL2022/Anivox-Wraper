using System;
using System.Runtime.InteropServices;

namespace Anivox.WinUI;

/// <summary>
/// Notification-area icon with the same menu as the GTK and Tauri builds:
/// "Открыть", "Сворачивать в трей" (checkbox) and "Выход". Lives on the UI thread,
/// whose message loop also serves this hidden message-only window.
/// </summary>
internal sealed class TrayIcon : IDisposable
{
    private const uint WM_APP_TRAY = 0x8000 + 1;
    private const uint WM_LBUTTONUP = 0x0202;
    private const uint WM_RBUTTONUP = 0x0205;
    private const uint WM_CONTEXTMENU = 0x007B;
    private const uint WM_NULL = 0x0000;
    private const uint NIM_ADD = 0, NIM_MODIFY = 1, NIM_DELETE = 2;
    private const uint NIF_MESSAGE = 1, NIF_ICON = 2, NIF_TIP = 4;
    private const uint MF_STRING = 0, MF_CHECKED = 8, MF_SEPARATOR = 0x800;
    private const uint TPM_RETURNCMD = 0x100, TPM_RIGHTBUTTON = 2, TPM_BOTTOMALIGN = 0x20;
    private const int CmdOpen = 1, CmdMinimize = 2, CmdQuit = 3;
    private static readonly IntPtr HWND_MESSAGE = new(-3);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public int cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public uint uVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WndClassEx
    {
        public int cbSize;
        public uint style;
        public WndProc lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string? lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X, Y;
    }

    private delegate IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    private static extern bool Shell_NotifyIcon(uint message, ref NotifyIconData data);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern ushort RegisterClassEx(ref WndClassEx cls);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateWindowEx(uint exStyle, string cls, string title, uint style,
        int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterWindowMessage(string name);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr LoadImage(IntPtr instance, string name, uint type, int cx, int cy, uint flags);

    [DllImport("user32.dll")]
    private static extern bool DestroyIcon(IntPtr icon);

    [DllImport("user32.dll")]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool AppendMenu(IntPtr menu, uint flags, UIntPtr id, string? text);

    [DllImport("user32.dll")]
    private static extern int TrackPopupMenuEx(IntPtr menu, uint flags, int x, int y, IntPtr hwnd, IntPtr tpm);

    [DllImport("user32.dll")]
    private static extern bool DestroyMenu(IntPtr menu);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    // uxtheme by ordinal (Windows 10 1903+): lets Win32 menus follow the system dark theme.
    [DllImport("uxtheme.dll", EntryPoint = "#135")]
    private static extern int SetPreferredAppMode(int mode);

    [DllImport("uxtheme.dll", EntryPoint = "#136")]
    private static extern void FlushMenuThemes();

    [DllImport("uxtheme.dll", EntryPoint = "#133")]
    private static extern bool AllowDarkModeForWindow(IntPtr hwnd, bool allow);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? name);

    private readonly WndProc _proc; // kept alive while the window exists
    private readonly IntPtr _hwnd;
    private readonly IntPtr _icon;
    private readonly uint _taskbarCreated;
    private bool _added;

    public bool MinimizeToTray { get; set; }

    /// <summary>True while the icon is in the notification area (closing may hide the window).</summary>
    public bool IsShown => _added;

    public event Action? Toggle;
    public event Action? Open;
    public event Action? ToggleMinimize;
    public event Action? Quit;

    public TrayIcon(string iconPath, bool minimizeToTray)
    {
        MinimizeToTray = minimizeToTray;
        _proc = Proc;
        var instance = GetModuleHandle(null);
        var cls = new WndClassEx
        {
            cbSize = Marshal.SizeOf<WndClassEx>(),
            lpfnWndProc = _proc,
            hInstance = instance,
            lpszClassName = "AnivoxWinUITray",
        };
        RegisterClassEx(ref cls);
        _hwnd = CreateWindowEx(0, cls.lpszClassName, "Anivox", 0, 0, 0, 0, 0, HWND_MESSAGE,
            IntPtr.Zero, instance, IntPtr.Zero);
        UseSystemMenuTheme(_hwnd);
        // Explorer restarts drop every icon; this message asks to add them again.
        _taskbarCreated = RegisterWindowMessage("TaskbarCreated");
        const uint IMAGE_ICON = 1, LR_LOADFROMFILE = 0x10;
        _icon = LoadImage(IntPtr.Zero, iconPath, IMAGE_ICON, GetSystemMetrics(49), GetSystemMetrics(50),
            LR_LOADFROMFILE);
        Add();
    }

    /// <summary>
    /// The tray menu is a plain Win32 menu, light by default. Allowing dark mode for the app
    /// makes it follow the system theme, like Explorer's own menus.
    /// </summary>
    private static void UseSystemMenuTheme(IntPtr hwnd)
    {
        try
        {
            const int AllowDark = 1;
            SetPreferredAppMode(AllowDark);
            AllowDarkModeForWindow(hwnd, true);
            FlushMenuThemes();
        }
        catch (Exception)
        {
            // Older Windows without these exports: the menu stays light.
        }
    }

    private NotifyIconData Data() => new()
    {
        cbSize = Marshal.SizeOf<NotifyIconData>(),
        hWnd = _hwnd,
        uID = 1,
        uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage = WM_APP_TRAY,
        hIcon = _icon,
        szTip = "Anivox",
        szInfo = "",
        szInfoTitle = "",
    };

    private void Add()
    {
        var data = Data();
        _added = Shell_NotifyIcon(NIM_ADD, ref data) || Shell_NotifyIcon(NIM_MODIFY, ref data);
    }

    private IntPtr Proc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        if (msg == WM_APP_TRAY)
        {
            switch ((uint)(lParam.ToInt64() & 0xFFFF))
            {
                case WM_LBUTTONUP:
                    Toggle?.Invoke();
                    break;
                case WM_RBUTTONUP:
                case WM_CONTEXTMENU:
                    ShowMenu();
                    break;
            }
            return IntPtr.Zero;
        }

        if (msg == _taskbarCreated && _taskbarCreated != 0)
        {
            Add();
            return IntPtr.Zero;
        }

        return DefWindowProc(hwnd, msg, wParam, lParam);
    }

    private void ShowMenu()
    {
        var menu = CreatePopupMenu();
        AppendMenu(menu, MF_STRING, (UIntPtr)CmdOpen, "Открыть");
        AppendMenu(menu, MF_STRING | (MinimizeToTray ? MF_CHECKED : 0), (UIntPtr)CmdMinimize, "Сворачивать в трей");
        AppendMenu(menu, MF_SEPARATOR, UIntPtr.Zero, null);
        AppendMenu(menu, MF_STRING, (UIntPtr)CmdQuit, "Выход");
        GetCursorPos(out var point);
        // Without the foreground window the menu would not close on an outside click.
        SetForegroundWindow(_hwnd);
        var command = TrackPopupMenuEx(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            point.X, point.Y, _hwnd, IntPtr.Zero);
        PostMessage(_hwnd, WM_NULL, IntPtr.Zero, IntPtr.Zero);
        DestroyMenu(menu);
        switch (command)
        {
            case CmdOpen:
                Open?.Invoke();
                break;
            case CmdMinimize:
                ToggleMinimize?.Invoke();
                break;
            case CmdQuit:
                Quit?.Invoke();
                break;
        }
    }

    public void Dispose()
    {
        if (_added)
        {
            var data = Data();
            Shell_NotifyIcon(NIM_DELETE, ref data);
            _added = false;
        }
        if (_icon != IntPtr.Zero)
        {
            DestroyIcon(_icon);
        }
        DestroyWindow(_hwnd);
    }
}
