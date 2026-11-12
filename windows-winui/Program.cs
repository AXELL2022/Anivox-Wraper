using System;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;

namespace Anivox.WinUI;

/// <summary>
/// Single instance: a second launch hands its activation to the running process,
/// which shows and focuses its window (also when it is hidden in the tray).
/// </summary>
public static class Program
{
    private const string InstanceKey = "com.yasin.anivox.winui";

    [DllImport("ole32.dll")]
    private static extern int CoWaitForMultipleObjects(uint flags, uint timeout, uint count, IntPtr[] handles,
        out uint index);

    [DllImport("user32.dll")]
    private static extern bool AllowSetForegroundWindow(int processId);

    [STAThread]
    private static int Main()
    {
        WinRT.ComWrappersSupport.InitializeComWrappers();

        var instance = AppInstance.FindOrRegisterForKey(InstanceKey);
        if (!instance.IsCurrent)
        {
            Redirect(instance);
            return 0;
        }

        instance.Activated += (_, _) => App.Instance?.ShowFromActivation();

        Application.Start(p =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new App();
        });
        return 0;
    }

    /// <summary>
    /// Redirects on a worker thread while this STA thread keeps pumping COM calls
    /// (the pattern from the Windows App SDK single-instance sample).
    /// </summary>
    private static void Redirect(AppInstance target)
    {
        var args = AppInstance.GetCurrent().GetActivatedEventArgs();
        // The running instance may take the foreground only with this process' permission.
        AllowSetForegroundWindow(-1 /* ASFW_ANY */);
        using var done = new ManualResetEvent(false);
        Task.Run(() =>
        {
            try
            {
                target.RedirectActivationToAsync(args).AsTask().Wait(TimeSpan.FromSeconds(10));
            }
            finally
            {
                done.Set();
            }
        });
        const uint CWMO_DEFAULT = 0, Timeout = 15000;
        CoWaitForMultipleObjects(CWMO_DEFAULT, Timeout, 1, new[] { done.SafeWaitHandle.DangerousGetHandle() },
            out _);
    }
}

/// <summary>Raises a window above others after a tray click or a redirected launch.</summary>
internal static class WinActivate
{
    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hwnd, int command);

    public static void BringToFront(IntPtr hwnd)
    {
        const int SW_SHOW = 5;
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}
