using System;
using System.Runtime.InteropServices;

namespace Anivox.WinUI;

/// <summary>
/// Tells the taskbar that a window is (no longer) fullscreen (<c>ITaskbarList2::MarkFullscreenWindow</c>),
/// so it steps behind a borderless window that covers the monitor.
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

    public static void Mark(IntPtr hwnd, bool fullscreen)
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
