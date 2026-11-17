using System;
using System.Runtime.InteropServices;

namespace Anivox.WinUI;

/// <summary>
/// Win32 helpers for the fullscreen player: the window loses its caption and resize frame and
/// covers the monitor exactly (as tao, the Tauri window library, does), and the taskbar is told it
/// is fullscreen (<c>ITaskbarList2::MarkFullscreenWindow</c>). For a WinUI window this alone does
/// not convince the shell (it goes by the AppWindow presenter), so <see cref="MainWindow"/> also
/// switches to the FullScreen presenter once the window is already in place.
/// </summary>
internal static class TaskbarFullscreen
{
    [ComImport]
    [Guid("602D4995-B13A-429b-A66E-1935E44F4317")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface ITaskbarList2
    {
        void HrInit();
        void AddTab(IntPtr hwnd);
        void DeleteTab(IntPtr hwnd);
        void ActivateTab(IntPtr hwnd);
        void SetActiveAlt(IntPtr hwnd);
        void MarkFullscreenWindow(IntPtr hwnd, [MarshalAs(UnmanagedType.Bool)] bool fullscreen);
    }

    [ComImport]
    [Guid("56FDF344-FD6D-11d0-958A-006097C9A090")]
    private class TaskbarList
    {
    }

    private static ITaskbarList2? _taskbar;

    private const int GwlStyle = -16;
    private const long WsOverlappedWindow = 0x00CF0000;
    private const uint SwpNoZOrder = 0x0004;
    private const uint SwpNoActivate = 0x0010;
    private const uint SwpFrameChanged = 0x0020;
    private const uint SwpNoOwnerZOrder = 0x0200;

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    private static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int cx, int cy, uint flags);

    /// <summary>
    /// Removes the caption and resize frame and puts the window exactly over <paramref name="monitor"/>.
    /// The maximized state is kept, so nothing is restored or animated. Returns the old style for
    /// <see cref="Leave"/>.
    /// </summary>
    public static IntPtr Enter(IntPtr hwnd, Windows.Graphics.RectInt32 monitor)
    {
        var style = GetWindowLongPtr(hwnd, GwlStyle);
        SetWindowLongPtr(hwnd, GwlStyle, new IntPtr(style.ToInt64() & ~WsOverlappedWindow));
        Place(hwnd, monitor);
        Mark(hwnd, true);
        return style;
    }

    /// <summary>Restores the window style saved by <see cref="Enter"/> and the window rectangle.</summary>
    public static void Leave(IntPtr hwnd, IntPtr style, Windows.Graphics.RectInt32? rect)
    {
        Mark(hwnd, false);
        SetWindowLongPtr(hwnd, GwlStyle, style);
        if (rect is Windows.Graphics.RectInt32 bounds)
        {
            Place(hwnd, bounds);
        }
        else
        {
            SetWindowPos(hwnd, IntPtr.Zero, 0, 0, 0, 0,
                SwpNoZOrder | SwpNoActivate | SwpFrameChanged | SwpNoOwnerZOrder | 0x0001 | 0x0002);
        }
    }

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    private const int DwmwaTransitionsForceDisabled = 3;

    /// <summary>Turns DWM window transitions off for half a second (a presenter switch).</summary>
    public static void SuppressTransitions(IntPtr hwnd)
    {
        var off = 1;
        DwmSetWindowAttribute(hwnd, DwmwaTransitionsForceDisabled, ref off, sizeof(int));
        var timer = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread()?.CreateTimer();
        if (timer is null)
        {
            return;
        }
        timer.Interval = TimeSpan.FromMilliseconds(500);
        timer.IsRepeating = false;
        timer.Tick += (_, _) =>
        {
            var on = 0;
            DwmSetWindowAttribute(hwnd, DwmwaTransitionsForceDisabled, ref on, sizeof(int));
        };
        timer.Start();
    }

    private static void Place(IntPtr hwnd, Windows.Graphics.RectInt32 rect) =>
        SetWindowPos(hwnd, IntPtr.Zero, rect.X, rect.Y, rect.Width, rect.Height,
            SwpNoZOrder | SwpNoActivate | SwpFrameChanged | SwpNoOwnerZOrder);

    private static void Mark(IntPtr hwnd, bool fullscreen)
    {
        try
        {
            if (_taskbar is null)
            {
                _taskbar = (ITaskbarList2)new TaskbarList();
                _taskbar.HrInit();
            }
            _taskbar.MarkFullscreenWindow(hwnd, fullscreen);
        }
        catch (Exception)
        {
            // Explorer may be restarting; the window still covers the monitor.
        }
    }
}
