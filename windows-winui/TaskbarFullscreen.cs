using System;
using System.Runtime.InteropServices;

namespace Anivox.WinUI;

/// <summary>
/// Win32 helpers that make the switch to and from the FullScreen presenter invisible (see
/// <see cref="MainWindow"/>) and tell the taskbar that the window is fullscreen
/// (<c>ITaskbarList2::MarkFullscreenWindow</c>).
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

    private const int GwlpWndProc = -4;
    private const uint WmNcCalcSize = 0x0083;
    private const int SwShowMaximized = 3;

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    private static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);

    [DllImport("user32.dll", EntryPoint = "CallWindowProcW")]
    private static extern IntPtr CallWindowProc(IntPtr previous, IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetWindowPlacement(IntPtr hwnd, ref WindowPlacement placement);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowPlacement(IntPtr hwnd, ref WindowPlacement placement);

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left, Top, Right, Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct WindowPlacement
    {
        public int Length;
        public int Flags;
        public int ShowCmd;
        public int MinX, MinY;
        public int MaxX, MaxY;
        public NativeRect NormalPosition;
    }

    private delegate IntPtr WindowProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    /// <summary>Kept alive for as long as the window uses it.</summary>
    private static WindowProc? _windowProc;
    private static IntPtr _previousWindowProc;
    /// <summary>While set, the window frame takes no space: the client area is the whole window.</summary>
    private static bool _noFrame;
    /// <summary>The real normal rectangle while <see cref="Enter"/> replaced it with the monitor.</summary>
    private static NativeRect? _savedNormal;

    /// <summary>
    /// Prepares the switch to the FullScreen presenter. The presenter first restores a maximized
    /// window to its normal rectangle and then puts it over the monitor with its frame still on;
    /// with the normal rectangle replaced by the monitor and the frame taking no space, neither
    /// step changes what is on screen. Call <see cref="Entered"/> after the switch.
    /// </summary>
    public static void Enter(IntPtr hwnd, Windows.Graphics.RectInt32 monitor, Windows.Graphics.RectInt32 workArea)
    {
        HookWindowProc(hwnd);
        _noFrame = true;

        var placement = new WindowPlacement { Length = Marshal.SizeOf<WindowPlacement>() };
        if (_savedNormal is null && GetWindowPlacement(hwnd, ref placement) && placement.ShowCmd == SwShowMaximized)
        {
            _savedNormal = placement.NormalPosition;
            // The placement uses workspace coordinates (relative to the work area).
            var left = monitor.X - (workArea.X - monitor.X);
            var top = monitor.Y - (workArea.Y - monitor.Y);
            placement.Flags = 0;
            placement.NormalPosition = new NativeRect
            {
                Left = left,
                Top = top,
                Right = left + monitor.Width,
                Bottom = top + monitor.Height,
            };
            SetWindowPlacement(hwnd, ref placement);
        }
    }

    /// <summary>
    /// Finishes the switch to the FullScreen presenter. The taskbar is told only now: the shell
    /// looks at a window again when it visibly changes, and the switch no longer changes anything
    /// visible, so without this the taskbar stayed over the video.
    /// </summary>
    public static void Entered(IntPtr hwnd)
    {
        _noFrame = false;
        Mark(hwnd, true);
    }

    /// <summary>After the presenter switched back: the taskbar and the real normal rectangle return.</summary>
    public static void Leave(IntPtr hwnd)
    {
        Mark(hwnd, false);
        if (_savedNormal is not NativeRect normal)
        {
            return;
        }
        _savedNormal = null;
        var placement = new WindowPlacement { Length = Marshal.SizeOf<WindowPlacement>() };
        if (GetWindowPlacement(hwnd, ref placement) && placement.ShowCmd == SwShowMaximized)
        {
            // Only the rectangle "Restore" goes back to; the maximized window does not move.
            placement.Flags = 0;
            placement.NormalPosition = normal;
            SetWindowPlacement(hwnd, ref placement);
        }
    }

    private static void HookWindowProc(IntPtr hwnd)
    {
        if (_windowProc is not null)
        {
            return;
        }
        _windowProc = (window, msg, wParam, lParam) =>
            msg == WmNcCalcSize && wParam != IntPtr.Zero && _noFrame
                ? IntPtr.Zero
                : CallWindowProc(_previousWindowProc, window, msg, wParam, lParam);
        _previousWindowProc = SetWindowLongPtr(hwnd, GwlpWndProc, Marshal.GetFunctionPointerForDelegate(_windowProc));
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
