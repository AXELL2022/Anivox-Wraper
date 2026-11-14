using System;
using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;

namespace Anivox.WinUI;

/// <summary>Temporarily turns off DWM window animations (minimize/maximize/restore).</summary>
internal static class WindowTransitions
{
    private const int DWMWA_TRANSITIONS_FORCEDISABLED = 3;
    private static DispatcherQueueTimer? _resume;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    private static void Set(IntPtr hwnd, bool disabled)
    {
        var value = disabled ? 1 : 0;
        DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, ref value, sizeof(int));
    }

    /// <summary>Disables animations now and re-enables them once the presenter change settles.</summary>
    public static void Suspend(IntPtr hwnd, DispatcherQueue queue)
    {
        Set(hwnd, true);
        if (_resume is null)
        {
            _resume = queue.CreateTimer();
            _resume.Interval = TimeSpan.FromMilliseconds(400);
            _resume.IsRepeating = false;
            _resume.Tick += (_, _) => Set(hwnd, false);
        }
        _resume.Stop();
        _resume.Start();
    }
}
