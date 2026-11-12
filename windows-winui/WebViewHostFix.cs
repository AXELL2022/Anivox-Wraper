using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Anivox.WinUI;

/// <summary>
/// Workarounds that make a windowed (child HWND) WebView2 usable inside a WinUI 3 window.
/// See https://github.com/microsoft/microsoft-ui-xaml/issues/10826: on Windows 11 Chromium's
/// "Intermediate D3D Window" sits above Chrome_RenderWidgetHostHWND with WS_EX_TRANSPARENT and
/// swallows every click, and WinUI's DesktopChildSiteBridge input window can end up above the
/// WebView2 HWND. Chromium may recreate the D3D window, so this is re-applied periodically.
/// </summary>
internal static class WebViewHostFix
{
    private const int GWL_EXSTYLE = -20;
    private const long WS_EX_TRANSPARENT = 0x20;
    private const uint GW_CHILD = 5;
    private const uint SWP_NOSIZE = 0x1, SWP_NOMOVE = 0x2, SWP_NOACTIVATE = 0x10, SWP_NOOWNERZORDER = 0x200;

    private delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumChildWindows(IntPtr parent, EnumProc proc, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr hwnd, StringBuilder name, int max);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string? cls, string? title);

    [DllImport("user32.dll")]
    private static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    private static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);

    [DllImport("user32.dll")]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    private static readonly EnumProc ClearTransparent = (hwnd, _) =>
    {
        var cls = new StringBuilder(64);
        GetClassName(hwnd, cls, cls.Capacity);
        if (cls.ToString() == "Intermediate D3D Window")
        {
            var ex = GetWindowLongPtr(hwnd, GWL_EXSTYLE).ToInt64();
            if ((ex & WS_EX_TRANSPARENT) != 0)
            {
                SetWindowLongPtr(hwnd, GWL_EXSTYLE, new IntPtr(ex & ~WS_EX_TRANSPARENT));
            }
        }
        return true;
    };

    public static void Apply(IntPtr window)
    {
        var host = FindWindowEx(window, IntPtr.Zero, "Chrome_WidgetWin_0", null);
        if (host == IntPtr.Zero)
        {
            return;
        }

        if (GetWindow(window, GW_CHILD) != host)
        {
            SetWindowPos(host, IntPtr.Zero /* HWND_TOP */, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }

        EnumChildWindows(host, ClearTransparent, IntPtr.Zero);
    }
}
